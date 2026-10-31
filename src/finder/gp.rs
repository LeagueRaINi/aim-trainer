//! Gaussian-process regression for the smart finder, in one input dimension with a linear
//! trial-order term.
//!
//! Observations are `y_i = c + f(x_i) + d * (t_i - mean t) + noise`, where `f ~ GP(0, k)` with
//! a squared-exponential kernel `k(x, x') = s^2 exp(-(x - x')^2 / (2 l^2))`, the constant `c`
//! and the drift `d` have broad Gaussian priors (so they're integrated out in closed form, as
//! extra kernel terms), and the noise is `N(0, n^2)`. The hyperparameters `(l, s, n)` come from
//! a small grid, each point weighted by its marginal likelihood times a weak log-normal prior
//! (so a handful of trials can't settle on a degenerate fit, like zero noise through every
//! point), and the posterior is the weighted mixture.
//!
//! Predictions are of `c + f(x)` at the average trial index: the drift is factored out.

use crate::metrics::normal::{normal_cdf, normal_pdf};

/// Prior standard deviation of the constant around the mean of the data (objectives are
/// relative, around 1: this is effectively flat).
const CONSTANT_SD: f64 = 0.3;
/// Prior standard deviation of the drift per trial (a relative objective gaining 5% per trial
/// is already an extreme warm-up).
const DRIFT_SD: f64 = 0.05;
/// Length scales in ln-sens. ~0.3 is a broad plateau (NVIDIA, CHI 2022).
const LENGTHS: [f64; 4] = [0.2, 0.3, 0.45, 0.7];
const SIGNALS: [f64; 7] = [0.005, 0.01, 0.02, 0.04, 0.08, 0.16, 0.32];
const NOISES: [f64; 7] = [0.01, 0.02, 0.03, 0.045, 0.07, 0.11, 0.16];
/// Log-normal hyperprior centres and spreads (in natural-log units).
const LENGTH_PRIOR: (f64, f64) = (0.3, 0.5);
const SIGNAL_PRIOR: (f64, f64) = (0.03, 1.0);
const NOISE_PRIOR: (f64, f64) = (0.04, 0.7);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hyper {
    /// Length scale in ln-sens.
    pub length: f64,
    /// Signal standard deviation of `f`.
    pub signal: f64,
    /// Standard deviation of one trial's noise.
    pub noise: f64,
}

impl Hyper {
    fn shape(&self, a: f64, b: f64) -> f64 {
        self.signal * self.signal * (-(a - b).powi(2) / (2.0 * self.length * self.length)).exp()
    }

    fn log_prior(&self) -> f64 {
        let term = |value: f64, (centre, spread): (f64, f64)| -0.5 * ((value / centre).ln() / spread).powi(2);
        term(self.length, LENGTH_PRIOR) + term(self.signal, SIGNAL_PRIOR) + term(self.noise, NOISE_PRIOR)
    }
}

/// A conditioned GP.
#[derive(Clone, Debug)]
pub struct Gp {
    pub hyper: Hyper,
    xs: Vec<f64>,
    /// Trial indices, centred on their mean.
    ts: Vec<f64>,
    /// Prior mean (the mean of the data).
    offset: f64,
    /// Lower Cholesky factor of the data covariance, row-major n x n.
    chol: Vec<f64>,
    /// `K^-1 (y - offset)`.
    alpha: Vec<f64>,
    /// Log marginal likelihood of the data.
    pub log_evidence: f64,
}

impl Gp {
    /// Conditions the GP with fixed hyperparameters.
    pub fn condition(hyper: Hyper, xs: &[f64], trials: &[f64], ys: &[f64]) -> Option<Self> {
        let n = xs.len();
        if n == 0 || trials.len() != n || ys.len() != n {
            return None;
        }
        let mean_t = trials.iter().sum::<f64>() / n as f64;
        let ts: Vec<f64> = trials.iter().map(|t| t - mean_t).collect();
        let offset = ys.iter().sum::<f64>() / n as f64;
        let mut chol = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                chol[i * n + j] =
                    hyper.shape(xs[i], xs[j]) + CONSTANT_SD * CONSTANT_SD + DRIFT_SD * DRIFT_SD * ts[i] * ts[j];
            }
            chol[i * n + i] += hyper.noise * hyper.noise;
        }
        if !cholesky(&mut chol, n) {
            return None;
        }
        let centred: Vec<f64> = ys.iter().map(|y| y - offset).collect();
        let half = solve_lower(&chol, n, &centred);
        let log_det: f64 = (0..n).map(|i| chol[i * n + i].ln()).sum::<f64>() * 2.0;
        let log_evidence = -0.5 * half.iter().map(|v| v * v).sum::<f64>()
            - 0.5 * log_det
            - 0.5 * n as f64 * (2.0 * std::f64::consts::PI).ln();
        let alpha = solve_upper(&chol, n, &half);
        Some(Self { hyper, xs: xs.to_vec(), ts, offset, chol, alpha, log_evidence })
    }

    /// Prior covariance of the drift-free prediction at `x` with observation `i`.
    fn cross(&self, x: f64, i: usize) -> f64 {
        self.hyper.shape(x, self.xs[i]) + CONSTANT_SD * CONSTANT_SD
    }

    fn prior_variance(&self) -> f64 {
        self.hyper.signal * self.hyper.signal + CONSTANT_SD * CONSTANT_SD
    }

    /// `L^-1 k(x)`, the whitened cross-covariance.
    fn whitened(&self, x: f64) -> Vec<f64> {
        let k: Vec<f64> = (0..self.xs.len()).map(|i| self.cross(x, i)).collect();
        solve_lower(&self.chol, self.xs.len(), &k)
    }

    /// Posterior mean and variance of `c + f(x)` at the average trial.
    pub fn predict(&self, x: f64) -> (f64, f64) {
        let mean = self.offset + (0..self.xs.len()).map(|i| self.cross(x, i) * self.alpha[i]).sum::<f64>();
        let v = self.whitened(x);
        let variance = (self.prior_variance() - v.iter().map(|v| v * v).sum::<f64>()).max(0.0);
        (mean, variance)
    }

    /// Posterior mean of the drift per trial.
    pub fn drift(&self) -> f64 {
        DRIFT_SD * DRIFT_SD * self.ts.iter().zip(&self.alpha).map(|(t, a)| t * a).sum::<f64>()
    }

    /// Observation `i`'s trial offset from the average trial.
    pub fn trial_offset(&self, i: usize) -> f64 {
        self.ts[i]
    }

    /// Posterior mean vector and row-major covariance matrix of `c + f` over `grid`.
    pub fn joint(&self, grid: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let g = grid.len();
        let means = grid.iter().map(|&x| self.predict(x).0).collect();
        let whitened: Vec<Vec<f64>> = grid.iter().map(|&x| self.whitened(x)).collect();
        let mut cov = vec![0.0; g * g];
        for a in 0..g {
            for b in 0..=a {
                let explained: f64 = whitened[a].iter().zip(&whitened[b]).map(|(p, q)| p * q).sum();
                let value = self.hyper.shape(grid[a], grid[b]) + CONSTANT_SD * CONSTANT_SD - explained;
                cov[a * g + b] = value;
                cov[b * g + a] = value;
            }
        }
        (means, cov)
    }

    /// Adds `samples` joint posterior draws over `grid` to `counts`: per grid point, how often
    /// it's within `tolerance` of the draw's best value and how often it is the best.
    fn sample(&self, grid: &[f64], tolerance: f64, samples: usize, rng: &mut fastrand::Rng, counts: &mut NearBest) {
        let g = grid.len();
        let (means, mut cov) = self.joint(grid);
        let scale = (0..g).map(|i| cov[i * g + i]).fold(0.0, f64::max).max(1e-12);
        // Grid points are strongly correlated: add jitter until the factorisation succeeds.
        let mut jitter = 1e-10 * scale;
        let base = cov.clone();
        while !cholesky(&mut cov, g) {
            cov.copy_from_slice(&base);
            for i in 0..g {
                cov[i * g + i] += jitter;
            }
            jitter *= 10.0;
        }
        let mut z = vec![0.0; g];
        let mut draw = vec![0.0; g];
        for _ in 0..samples {
            for value in &mut z {
                *value = standard_normal(rng);
            }
            for i in 0..g {
                draw[i] = means[i] + (0..=i).map(|j| cov[i * g + j] * z[j]).sum::<f64>();
            }
            let (best_index, best) =
                draw.iter().enumerate().fold((0, f64::MIN), |acc, (i, &v)| if v > acc.1 { (i, v) } else { acc });
            counts.peak[best_index] += 1.0;
            for (count, &value) in counts.near.iter_mut().zip(&draw) {
                if value >= best - tolerance {
                    *count += 1.0;
                }
            }
        }
    }
}

/// Hyperparameter uncertainty: every grid point's GP weighted by its posterior probability
/// (marginal likelihood times hyperprior), negligible ones dropped. A few trials rarely pin the
/// noise and the signal down; averaging over them keeps the model from being confidently wrong.
#[derive(Clone, Debug)]
pub struct Mixture {
    /// (weight, GP), weights summing to 1, heaviest first.
    components: Vec<(f64, Gp)>,
}

/// Components lighter than this are dropped.
const MIN_WEIGHT: f64 = 1e-3;

impl Mixture {
    /// `None` without data or if every grid point is numerically singular.
    pub fn fit(xs: &[f64], trials: &[f64], ys: &[f64]) -> Option<Self> {
        let mut scored = Vec::new();
        for &length in &LENGTHS {
            for &signal in &SIGNALS {
                for &noise in &NOISES {
                    let hyper = Hyper { length, signal, noise };
                    if let Some(gp) = Gp::condition(hyper, xs, trials, ys) {
                        scored.push((gp.log_evidence + hyper.log_prior(), gp));
                    }
                }
            }
        }
        let top = scored.iter().map(|(score, _)| *score).fold(f64::MIN, f64::max);
        let total: f64 = scored.iter().map(|(score, _)| (score - top).exp()).sum();
        let mut components: Vec<(f64, Gp)> = scored
            .into_iter()
            .map(|(score, gp)| ((score - top).exp() / total, gp))
            .filter(|(weight, _)| *weight >= MIN_WEIGHT)
            .collect();
        // Stable: equal weights keep grid order.
        components.sort_by(|a, b| b.0.total_cmp(&a.0));
        let kept: f64 = components.iter().map(|(weight, _)| weight).sum();
        for (weight, _) in &mut components {
            *weight /= kept;
        }
        (!components.is_empty()).then_some(Self { components })
    }

    fn weighted(&self, value: impl Fn(&Gp) -> f64) -> f64 {
        self.components.iter().map(|(weight, gp)| weight * value(gp)).sum()
    }

    /// Posterior mean and variance of `c + f(x)` at the average trial.
    pub fn predict(&self, x: f64) -> (f64, f64) {
        let parts: Vec<(f64, (f64, f64))> =
            self.components.iter().map(|(weight, gp)| (*weight, gp.predict(x))).collect();
        let mean: f64 = parts.iter().map(|(w, (m, _))| w * m).sum();
        let second: f64 = parts.iter().map(|(w, (m, v))| w * (v + m * m)).sum();
        (mean, (second - mean * mean).max(0.0))
    }

    /// Expected improvement over `best` at `x`, averaged over the components.
    pub fn expected_improvement(&self, x: f64, best: f64) -> f64 {
        self.weighted(|gp| {
            let (mean, variance) = gp.predict(x);
            expected_improvement(mean, variance.sqrt(), best)
        })
    }

    /// Posterior mean of the drift per trial.
    pub fn drift(&self) -> f64 {
        self.weighted(Gp::drift)
    }

    /// Posterior mean of the trial noise (standard deviation).
    pub fn noise(&self) -> f64 {
        self.weighted(|gp| gp.hyper.noise)
    }

    /// Observation `i`'s trial offset from the average trial.
    pub fn trial_offset(&self, i: usize) -> f64 {
        self.components[0].1.trial_offset(i)
    }

    /// Joint posterior draws over `grid` (split between the components by weight) decide, per
    /// grid point, the probability that it is within `tolerance` of the best value (`near`) and
    /// that it is the best (`peak`). Uses a fixed-seed generator, so the result only depends on
    /// the data.
    pub fn near_best(&self, grid: &[f64], tolerance: f64, samples: usize, seed: u64) -> NearBest {
        let mut rng = fastrand::Rng::with_seed(seed);
        let mut counts = NearBest { near: vec![0.0; grid.len()], peak: vec![0.0; grid.len()] };
        let mut drawn = 0;
        for (weight, gp) in &self.components {
            let share = ((weight * samples as f64).round() as usize).max(1);
            gp.sample(grid, tolerance, share, &mut rng, &mut counts);
            drawn += share;
        }
        let n = drawn as f64;
        NearBest {
            near: counts.near.into_iter().map(|c| c / n).collect(),
            peak: counts.peak.into_iter().map(|c| c / n).collect(),
        }
    }
}

/// Per grid point: probability of being within the tolerance of the best, and of being the
/// best.
#[derive(Clone, Debug, PartialEq)]
pub struct NearBest {
    pub near: Vec<f64>,
    pub peak: Vec<f64>,
}

/// Expected improvement over `best` of a normal posterior with `mean` and `sd`.
pub fn expected_improvement(mean: f64, sd: f64, best: f64) -> f64 {
    let gain = mean - best;
    if sd < 1e-12 {
        return gain.max(0.0);
    }
    let z = gain / sd;
    gain * normal_cdf(z) + sd * normal_pdf(z)
}

/// Box-Muller.
fn standard_normal(rng: &mut fastrand::Rng) -> f64 {
    let u = 1.0 - rng.f64();
    let v = rng.f64();
    (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}

/// In-place lower Cholesky factor of a row-major symmetric n x n matrix (upper part zeroed).
/// `false` if it isn't numerically positive definite.
fn cholesky(a: &mut [f64], n: usize) -> bool {
    for j in 0..n {
        let diagonal = a[j * n + j] - (0..j).map(|k| a[j * n + k] * a[j * n + k]).sum::<f64>();
        if diagonal <= 0.0 || !diagonal.is_finite() {
            return false;
        }
        let root = diagonal.sqrt();
        a[j * n + j] = root;
        for i in j + 1..n {
            let value = a[i * n + j] - (0..j).map(|k| a[i * n + k] * a[j * n + k]).sum::<f64>();
            a[i * n + j] = value / root;
        }
        for k in j + 1..n {
            a[j * n + k] = 0.0;
        }
    }
    true
}

/// Solves `L v = b`.
fn solve_lower(l: &[f64], n: usize, b: &[f64]) -> Vec<f64> {
    let mut v = vec![0.0; n];
    for i in 0..n {
        v[i] = (b[i] - (0..i).map(|k| l[i * n + k] * v[k]).sum::<f64>()) / l[i * n + i];
    }
    v
}

/// Solves `L^T v = b`.
fn solve_upper(l: &[f64], n: usize, b: &[f64]) -> Vec<f64> {
    let mut v = vec![0.0; n];
    for i in (0..n).rev() {
        v[i] = (b[i] - (i + 1..n).map(|k| l[k * n + i] * v[k]).sum::<f64>()) / l[i * n + i];
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cholesky_solves_a_known_system() {
        // A = [[4, 2], [2, 3]] = L L^T with L = [[2, 0], [1, sqrt 2]].
        let mut a = vec![4.0, 2.0, 2.0, 3.0];
        assert!(cholesky(&mut a, 2));
        assert!(
            (a[0] - 2.0).abs() < 1e-12
                && (a[2] - 1.0).abs() < 1e-12
                && (a[3] - 2f64.sqrt()).abs() < 1e-12
                && a[1] == 0.0
        );
        // A x = [8, 7] -> x = [1.25, 1.5].
        let x = solve_upper(&a, 2, &solve_lower(&a, 2, &[8.0, 7.0]));
        assert!((x[0] - 1.25).abs() < 1e-12 && (x[1] - 1.5).abs() < 1e-12, "{x:?}");
        assert!(!cholesky(&mut [1.0, 2.0, 2.0, 1.0], 2));
    }

    #[test]
    fn interpolates_precise_data_and_is_uncertain_away_from_it() {
        let xs: Vec<f64> = (0..=8).map(|i| -0.4 + 0.1 * f64::from(i)).collect();
        let ys: Vec<f64> = xs.iter().map(|x| 1.0 - x * x).collect();
        let hyper = Hyper { length: 0.25, signal: 0.1, noise: 0.001 };
        // Shuffled trial order, so the drift isn't confounded with x.
        let gp = Gp::condition(hyper, &xs, &[4.0, 0.0, 7.0, 2.0, 8.0, 1.0, 5.0, 3.0, 6.0], &ys).unwrap();
        let far = gp.predict(1.5).1.sqrt();
        for (&x, &y) in xs.iter().zip(&ys) {
            let (mean, variance) = gp.predict(x);
            // What's left is the drift's uncertainty: one trial per sens can't pin it exactly.
            assert!((mean - y).abs() < 0.01 && variance.sqrt() < far / 3.0, "{x}: {mean} {variance} {far}");
        }
        assert!(gp.drift().abs() < 1e-3, "{}", gp.drift());
    }

    #[test]
    fn separates_a_drift_from_the_curve() {
        // Same curve, plus 0.03 per trial, sampled in shuffled order.
        let xs = [0.0, -0.4, 0.4, 0.2, -0.2, 0.1, -0.1, 0.3];
        let ts: Vec<f64> = (0..xs.len()).map(|t| t as f64).collect();
        let ys: Vec<f64> = xs.iter().zip(&ts).map(|(x, t)| 1.0 - x * x + 0.03 * t).collect();
        let gp = Mixture::fit(&xs, &ts, &ys).unwrap();
        assert!((gp.drift() - 0.03).abs() < 5e-3, "{}", gp.drift());
        let best = (-50..=50)
            .map(|i| f64::from(i) / 100.0)
            .max_by(|a, b| gp.predict(*a).0.total_cmp(&gp.predict(*b).0))
            .unwrap();
        assert!(best.abs() <= 0.03, "{best}");
    }

    #[test]
    fn expected_improvement_rewards_mean_and_uncertainty() {
        assert_eq!(expected_improvement(1.0, 0.0, 0.5), 0.5);
        assert_eq!(expected_improvement(0.4, 0.0, 0.5), 0.0);
        assert!(expected_improvement(0.5, 0.2, 0.5) > expected_improvement(0.5, 0.1, 0.5));
        assert!(expected_improvement(0.6, 0.1, 0.5) > expected_improvement(0.5, 0.1, 0.5));
        assert!((expected_improvement(0.5, 0.1, 0.5) - 0.1 * normal_pdf(0.0)).abs() < 1e-12);
    }

    #[test]
    fn near_best_samples_find_a_clear_peak() {
        let grid: Vec<f64> = (0..=8).map(|i| -0.4 + 0.1 * f64::from(i)).collect();
        let ys: Vec<f64> = grid.iter().map(|x| 1.0 - 2.0 * (x - 0.2).powi(2)).collect();
        let trials = [4.0, 0.0, 7.0, 2.0, 8.0, 1.0, 5.0, 3.0, 6.0];
        let gp = Gp::condition(Hyper { length: 0.25, signal: 0.2, noise: 0.005 }, &grid, &trials, &ys).unwrap();
        let gp = Mixture { components: vec![(1.0, gp)] };
        let result = gp.near_best(&grid, 0.03, 400, 1);
        assert_eq!(result, gp.near_best(&grid, 0.03, 400, 1), "deterministic");
        // 2 (x - 0.2)^2 <= 0.03 for |x - 0.2| <= 0.12: grid points 0.1, 0.2, 0.3.
        for (i, (&x, &p)) in grid.iter().zip(&result.near).enumerate() {
            let inside = (x - 0.2).abs() < 0.11;
            assert!(if inside { p > 0.8 } else { p < 0.2 }, "{i} {x}: {p}");
        }
        assert!(result.peak[6] > 0.9, "{:?}", result.peak);
        assert!((result.peak.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }
}
