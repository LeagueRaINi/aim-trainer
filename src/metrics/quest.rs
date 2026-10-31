//! QUEST+ (Watson 2017, J. Vision 17(3):10) for target size: a Bayesian adaptive procedure that
//! picks each target's angular radius from every outcome so far and estimates θ75, the radius
//! you hit 75% of the time within the drill's time limit.
//!
//! Model: P(hit | r) = (1 − λ)·Φ((ln r − ln θ) / σ) with no guessing and a lapse rate λ of 3%.
//! The posterior over θ (40 log-spaced values, 0.08°–2.5°) and the slope σ (8 log-spaced values,
//! 0.1–1.2 in ln-radius units) is kept exactly on that grid. Targets come from 41 log-spaced
//! radii, 0.15°–2.5°. θ75 = θ·e^(z·σ), where (1 − λ)·Φ(z) = 0.75.
//!
//! Next radius:
//! - [`QuestMode::Measure`]: the one minimising the expected entropy of the posterior after the
//!   outcome (QUEST+). The entropy is taken over θ75 (σ marginalised), the quantity reported:
//!   in simulation that puts θ75 within 10% after 60 targets for 90% of observers with σ = 0.25,
//!   versus 75% for the entropy of the joint (θ, σ) posterior.
//! - [`QuestMode::Train`]: the radius the posterior predicts you hit 75% of the time, so
//!   training stays at a fixed, hard-but-fair difficulty.

use serde::{Deserialize, Serialize};

use super::normal::normal_cdf;

/// Chance of missing a target that should be easy (a slip, a blink).
pub const LAPSE: f64 = 0.03;
/// The hit rate θ75 stands for, and the one Train mode aims at.
pub const TARGET_HIT: f64 = 0.75;
/// Smallest and largest target radius, degrees.
pub const RADIUS_RANGE: (f32, f32) = (0.15, 2.5);
/// Probability mass inside the reported credible interval.
pub const CREDIBLE: f32 = 0.9;
const RADII: usize = 41;
const THETA_RANGE: (f64, f64) = (0.08, 2.5);
const THETAS: usize = 40;
const SLOPE_RANGE: (f64, f64) = (0.1, 1.2);
const SLOPES: usize = 8;
/// Prior centred on an earlier θ75: SD of ln θ75 (±65%), plus a floor so a much better or worse
/// day is still found.
const PRIOR_SD: f64 = 0.5;
const PRIOR_FLOOR: f64 = 0.05;

/// How the next target's radius is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuestMode {
    /// The most informative radius: finds θ75 in the fewest targets.
    #[default]
    Measure,
    /// The radius you hit 75% of the time.
    Train,
}

impl QuestMode {
    pub const ALL: [QuestMode; 2] = [QuestMode::Measure, QuestMode::Train];

    pub fn name(self) -> &'static str {
        match self {
            QuestMode::Measure => "Measure",
            QuestMode::Train => "Train",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            QuestMode::Measure => "Finds the target size you hit 75% of the time in time.",
            QuestMode::Train => "Keeps targets at the size you hit 75% of the time.",
        }
    }
}

/// One grid point of the psychometric function.
#[derive(Clone, Copy, Debug)]
struct Param {
    ln_theta: f64,
    slope: f64,
    ln_theta75: f64,
}

impl Param {
    fn p_hit(&self, ln_radius: f64) -> f64 {
        (1.0 - LAPSE) * normal_cdf((ln_radius - self.ln_theta) / self.slope)
    }
}

/// What the posterior says after some targets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimate {
    /// Posterior mean of θ75 (in log space), degrees.
    pub theta75: f32,
    /// The [`CREDIBLE`] interval of θ75, degrees.
    pub low: f32,
    pub high: f32,
    /// Posterior mean slope σ, ln-radius units.
    pub slope: f32,
    pub trials: u32,
    pub hits: u32,
}

/// A running QUEST+ procedure.
#[derive(Clone, Debug)]
pub struct Quest {
    /// Target radii to choose from (Measure), degrees.
    radii: Vec<f64>,
    params: Vec<Param>,
    /// P(hit) for every (radius, param), radius-major.
    table: Vec<f64>,
    /// The θ75 bin of every param (bins as wide as the θ grid's step), for the entropy.
    bins: Vec<usize>,
    bin_count: usize,
    /// Params by ascending θ75, for the credible interval.
    by_theta75: Vec<usize>,
    posterior: Vec<f64>,
    trials: u32,
    hits: u32,
}

fn log_grid((low, high): (f64, f64), n: usize) -> Vec<f64> {
    let step = (high / low).ln() / (n - 1) as f64;
    (0..n).map(|i| low * (step * i as f64).exp()).collect()
}

/// z with (1 − λ)·Φ(z) = [`TARGET_HIT`].
fn target_z() -> f64 {
    let wanted = TARGET_HIT / (1.0 - LAPSE);
    let (mut low, mut high) = (-6.0, 6.0);
    for _ in 0..60 {
        let mid = 0.5 * (low + high);
        if normal_cdf(mid) < wanted { low = mid } else { high = mid }
    }
    0.5 * (low + high)
}

fn normalise(weights: &mut [f64]) {
    let total: f64 = weights.iter().sum();
    weights.iter_mut().for_each(|w| *w /= total);
}

/// `Σ m ln m` over masses, with 0 ln 0 = 0.
fn sum_m_ln_m(masses: &[f64]) -> f64 {
    masses.iter().filter(|&&m| m > 0.0).map(|&m| m * m.ln()).sum()
}

impl Quest {
    /// A fresh procedure. `prior` centres it on an earlier θ75 (degrees); `None` is uniform.
    pub fn new(prior: Option<f32>) -> Self {
        let z = target_z();
        let radii = log_grid((RADIUS_RANGE.0 as f64, RADIUS_RANGE.1 as f64), RADII);
        let slopes = log_grid(SLOPE_RANGE, SLOPES);
        let params: Vec<Param> = log_grid(THETA_RANGE, THETAS)
            .into_iter()
            .flat_map(|theta| {
                slopes.iter().map(move |&slope| Param { ln_theta: theta.ln(), slope, ln_theta75: theta.ln() + z * slope })
            })
            .collect();
        let table = radii.iter().flat_map(|r| params.iter().map(move |p| p.p_hit(r.ln()))).collect();
        let step = (THETA_RANGE.1 / THETA_RANGE.0).ln() / (THETAS - 1) as f64;
        let lowest = params.iter().map(|p| p.ln_theta75).fold(f64::INFINITY, f64::min);
        let bins: Vec<usize> = params.iter().map(|p| ((p.ln_theta75 - lowest) / step) as usize).collect();
        let bin_count = bins.iter().max().map_or(1, |b| b + 1);
        let mut by_theta75: Vec<usize> = (0..params.len()).collect();
        by_theta75.sort_by(|&a, &b| params[a].ln_theta75.total_cmp(&params[b].ln_theta75));
        let mut posterior: Vec<f64> = match prior.filter(|c| c.is_finite() && *c > 0.0) {
            None => vec![1.0; params.len()],
            Some(centre) => {
                let centre = (centre as f64).ln();
                params
                    .iter()
                    .map(|p| {
                        let d = (p.ln_theta75 - centre) / PRIOR_SD;
                        (-0.5 * d * d).exp() + PRIOR_FLOOR
                    })
                    .collect()
            }
        };
        normalise(&mut posterior);
        Self { radii, params, table, bins, bin_count, by_theta75, posterior, trials: 0, hits: 0 }
    }

    /// The next target's radius, degrees.
    pub fn next_radius(&self, mode: QuestMode) -> f32 {
        match mode {
            QuestMode::Measure => self.most_informative_radius(),
            QuestMode::Train => self.training_radius(),
        }
    }

    /// The candidate radius with the lowest [`expected_entropy`](Self::expected_entropy); the
    /// smallest one on ties.
    fn most_informative_radius(&self) -> f32 {
        let best = (0..self.radii.len())
            .map(|i| (i, self.expected_entropy(i)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0, |(i, _)| i);
        self.radii[best] as f32
    }

    /// Expected entropy (nats) of the θ75 posterior after a target of the `i`-th candidate radius:
    /// Σ over hit/miss of P(outcome) · H(posterior | outcome).
    fn expected_entropy(&self, i: usize) -> f64 {
        let row = &self.table[i * self.params.len()..(i + 1) * self.params.len()];
        let mut hit = vec![0.0; self.bin_count];
        let mut miss = vec![0.0; self.bin_count];
        for ((&weight, &p), &bin) in self.posterior.iter().zip(row).zip(&self.bins) {
            hit[bin] += weight * p;
            miss[bin] += weight * (1.0 - p);
        }
        // P(o)·H(post | o) = s ln s − Σ m ln m for the unnormalised masses m of outcome o (sum s).
        [hit, miss]
            .iter()
            .map(|masses| {
                let total: f64 = masses.iter().sum();
                if total > 0.0 { total * total.ln() - sum_m_ln_m(masses) } else { 0.0 }
            })
            .sum()
    }

    /// Entropy (nats) of the current θ75 posterior; the tests compare it with the expected one.
    #[cfg(test)]
    fn entropy(&self) -> f64 {
        let mut masses = vec![0.0; self.bin_count];
        for (&weight, &bin) in self.posterior.iter().zip(&self.bins) {
            masses[bin] += weight;
        }
        -sum_m_ln_m(&masses)
    }

    /// Posterior predictive chance of a hit at `radius` (degrees).
    pub fn predicted_hit(&self, radius: f32) -> f64 {
        let ln_radius = (radius as f64).ln();
        self.posterior.iter().zip(&self.params).map(|(w, p)| w * p.p_hit(ln_radius)).sum()
    }

    /// The radius whose predicted hit chance is [`TARGET_HIT`] (it rises with the radius),
    /// clamped to [`RADIUS_RANGE`].
    fn training_radius(&self) -> f32 {
        let (mut low, mut high) = (RADIUS_RANGE.0.ln(), RADIUS_RANGE.1.ln());
        if self.predicted_hit(RADIUS_RANGE.0) >= TARGET_HIT {
            return RADIUS_RANGE.0;
        }
        if self.predicted_hit(RADIUS_RANGE.1) <= TARGET_HIT {
            return RADIUS_RANGE.1;
        }
        for _ in 0..30 {
            let mid = 0.5 * (low + high);
            if self.predicted_hit(mid.exp()) < TARGET_HIT { low = mid } else { high = mid }
        }
        (0.5 * (low + high)).exp()
    }

    /// Bayes' rule for one target of `radius` degrees that was hit (or not) in time.
    pub fn update(&mut self, radius: f32, hit: bool) {
        let ln_radius = (radius as f64).ln();
        for (weight, p) in self.posterior.iter_mut().zip(&self.params) {
            let p_hit = p.p_hit(ln_radius);
            *weight *= if hit { p_hit } else { 1.0 - p_hit };
        }
        normalise(&mut self.posterior);
        self.trials += 1;
        self.hits += hit as u32;
    }

    pub fn estimate(&self) -> Estimate {
        let mean = |f: fn(&Param) -> f64| self.posterior.iter().zip(&self.params).map(|(w, p)| w * f(p)).sum::<f64>();
        let tail = (1.0 - CREDIBLE as f64) / 2.0;
        Estimate {
            theta75: mean(|p| p.ln_theta75).exp() as f32,
            low: self.theta75_quantile(tail),
            high: self.theta75_quantile(1.0 - tail),
            slope: mean(|p| p.slope) as f32,
            trials: self.trials,
            hits: self.hits,
        }
    }

    /// θ75 below which the posterior has mass `q`, degrees (interpolated between grid points).
    fn theta75_quantile(&self, q: f64) -> f32 {
        let mut below = 0.0;
        let mut previous = self.params[self.by_theta75[0]].ln_theta75;
        for &i in &self.by_theta75 {
            let (value, weight) = (self.params[i].ln_theta75, self.posterior[i]);
            if below + weight >= q {
                let t = if weight > 0.0 { (q - below) / weight } else { 1.0 };
                return (previous + t * (value - previous)).exp() as f32;
            }
            below += weight;
            previous = value;
        }
        previous.exp() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simulated player: hits a target of radius `r` with the model's probability.
    struct Observer {
        theta: f64,
        slope: f64,
        rng: fastrand::Rng,
    }

    impl Observer {
        fn new(theta: f64, slope: f64, seed: u64) -> Self {
            Self { theta, slope, rng: fastrand::Rng::with_seed(seed) }
        }

        fn p_hit(&self, radius: f32) -> f64 {
            Param { ln_theta: self.theta.ln(), slope: self.slope, ln_theta75: 0.0 }.p_hit((radius as f64).ln())
        }

        fn theta75(&self) -> f32 {
            (self.theta * (target_z() * self.slope).exp()) as f32
        }

        fn respond(&mut self, radius: f32) -> bool {
            self.rng.f64() < self.p_hit(radius)
        }
    }

    /// Runs `trials` targets; returns the procedure and the radii shown.
    fn simulate(observer: &mut Observer, mode: QuestMode, trials: usize) -> (Quest, Vec<f32>) {
        let mut quest = Quest::new(None);
        let mut shown = Vec::with_capacity(trials);
        for _ in 0..trials {
            let radius = quest.next_radius(mode);
            let hit = observer.respond(radius);
            quest.update(radius, hit);
            shown.push(radius);
        }
        (quest, shown)
    }

    fn relative_error(estimate: f32, truth: f32) -> f32 {
        (estimate / truth - 1.0).abs()
    }

    #[test]
    fn target_z_gives_the_75_percent_point() {
        assert!((((1.0 - LAPSE) * normal_cdf(target_z())) - TARGET_HIT).abs() < 1e-9);
    }

    #[test]
    fn posterior_stays_normalised() {
        for prior in [None, Some(0.6)] {
            let mut quest = Quest::new(prior);
            assert!((quest.posterior.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            let mut rng = fastrand::Rng::with_seed(7);
            for _ in 0..200 {
                let radius = RADIUS_RANGE.0 + rng.f32() * (RADIUS_RANGE.1 - RADIUS_RANGE.0);
                quest.update(radius, rng.bool());
            }
            assert!((quest.posterior.iter().sum::<f64>() - 1.0).abs() < 1e-9);
            assert!(quest.posterior.iter().all(|w| w.is_finite() && *w >= 0.0));
            let estimate = quest.estimate();
            assert!(estimate.low <= estimate.theta75 && estimate.theta75 <= estimate.high, "{estimate:?}");
        }
    }

    #[test]
    fn prior_centres_on_the_last_result() {
        let uniform = Quest::new(None).estimate();
        let centred = Quest::new(Some(0.9)).estimate();
        assert!(relative_error(centred.theta75, 0.9) < 0.05, "{centred:?}");
        assert!(centred.high / centred.low < uniform.high / uniform.low);
        // A broken stored value falls back to uniform.
        assert_eq!(Quest::new(Some(f32::NAN)).estimate(), uniform);
    }

    /// Several players with a fairly steep psychometric function (σ = 0.15: from 25% to 75%
    /// hits over a 1.23x change in radius), across the range: θ75 within 10% after 60 targets.
    /// It is a random experiment (about 1 player in 40 lands outside 10%, see the next test), so
    /// one miss is allowed, and none beyond 15%.
    #[test]
    fn measure_finds_theta75_within_10_percent_in_60_targets() {
        let mut errors = Vec::new();
        for theta in [0.25, 0.5, 1.0] {
            for seed in 1..=6 {
                let mut observer = Observer::new(theta, 0.15, seed);
                let (quest, _) = simulate(&mut observer, QuestMode::Measure, 60);
                let (estimate, truth) = (quest.estimate(), observer.theta75());
                let error = relative_error(estimate.theta75, truth);
                assert!(error < 0.15, "theta {theta} seed {seed}: {estimate:?} vs {truth}");
                errors.push(error);
            }
        }
        let outside = errors.iter().filter(|&&e| e >= 0.10).count();
        println!("errors: {errors:.3?}");
        assert!(outside <= 1, "{outside} of {} outside 10%", errors.len());
    }

    /// Many simulated players: unbiased, mostly within 10% after 60 targets (nearly always for
    /// steep players, usually for shallower ones), and the 90% interval covers the truth about
    /// 90% of the time.
    #[test]
    fn measure_statistics_over_many_players() {
        for (slope, min_within) in [(0.15, 0.93), (0.25, 0.8), (0.4, 0.6)] {
            let (mut within, mut covered, mut log_error, mut squared, mut players) = (0, 0, 0.0, 0.0, 0);
            for theta in [0.25, 0.5, 1.0, 1.5] {
                for seed in 0..30 {
                    let mut observer = Observer::new(theta, slope, 1000 + seed);
                    let (quest, _) = simulate(&mut observer, QuestMode::Measure, 60);
                    let (estimate, truth) = (quest.estimate(), observer.theta75());
                    let error = relative_error(estimate.theta75, truth);
                    within += (error < 0.10) as u32;
                    covered += (estimate.low <= truth && truth <= estimate.high) as u32;
                    log_error += (estimate.theta75 / truth).ln();
                    squared += error * error;
                    players += 1;
                }
            }
            let n = players as f32;
            let (within, covered, bias, rms) = (within as f32 / n, covered as f32 / n, log_error / n, (squared / n).sqrt());
            println!(
                "slope {slope}: {players} players, within 10% {within:.2}, rms {rms:.3}, bias {bias:+.3}, {CREDIBLE} interval covers {covered:.2}"
            );
            assert!(within >= min_within, "slope {slope}: {within}");
            assert!(bias.abs() < 0.03, "slope {slope}: {bias}");
            assert!(covered >= 0.8, "slope {slope}: {covered}");
        }
    }

    #[test]
    fn entropy_rule_picks_informative_radii() {
        let mut observer = Observer::new(0.5, 0.2, 3);
        for trials in [0, 25] {
            let (quest, _) = simulate(&mut observer, QuestMode::Measure, trials);
            let chosen = quest.next_radius(QuestMode::Measure);
            let index = quest.radii.iter().position(|&r| r as f32 == chosen).unwrap();
            let now = quest.entropy();
            // Expected information gain (nats) of a target of each radius.
            let gain = |i: usize| now - quest.expected_entropy(i);
            let ends = gain(0).max(gain(quest.radii.len() - 1));
            println!("after {trials}: chosen {chosen:.2}° gains {:.3}, ends {ends:.3}", gain(index));
            assert!((0..quest.radii.len()).all(|i| gain(i) <= gain(index)), "the rule takes the largest gain");
            assert!(gain(index) > ends && gain(index) > 0.0);
            // The chosen radius is one whose outcome is uncertain.
            let p = quest.predicted_hit(chosen);
            assert!((0.15..0.95).contains(&p), "{p}");
            if trials > 0 {
                // Once roughly known, sure hits (huge targets) and sure misses (tiny ones) teach
                // much less, and the chosen radius is near the player's θ75.
                assert!(gain(index) > 2.0 * ends, "{} vs {ends}", gain(index));
                let ratio = chosen / observer.theta75();
                assert!((0.6..1.6).contains(&ratio), "{chosen} vs {}", observer.theta75());
            }
        }
    }

    #[test]
    fn train_targets_75_percent_under_the_posterior() {
        let mut observer = Observer::new(0.6, 0.25, 11);
        let (quest, _) = simulate(&mut observer, QuestMode::Measure, 20);
        let radius = quest.next_radius(QuestMode::Train);
        assert!((quest.predicted_hit(radius) - TARGET_HIT).abs() < 1e-3);
        // Clamped at the ends when the 75% point is outside the range.
        let mut hopeless = Quest::new(None);
        (0..30).for_each(|_| hopeless.update(RADIUS_RANGE.1, false));
        assert_eq!(hopeless.next_radius(QuestMode::Train), RADIUS_RANGE.1);
    }

    /// Train mode: after a short search the radii stay close to the player's true θ75, so the
    /// player really hits about 75% of the targets.
    #[test]
    fn train_settles_near_the_true_75_percent_point() {
        for (theta, slope) in [(0.4, 0.2), (0.8, 0.25)] {
            let players = 40;
            let (mut rates, mut errors) = (Vec::new(), Vec::new());
            for seed in 0..players {
                let mut observer = Observer::new(theta, slope, 50 + seed);
                let (_, shown) = simulate(&mut observer, QuestMode::Train, 80);
                // Where it settled: the geometric mean radius of the last 20 targets.
                let last = &shown[60..];
                let settled = (last.iter().map(|r| r.ln()).sum::<f32>() / last.len() as f32).exp();
                errors.push(relative_error(settled, observer.theta75()));
                rates.push(shown[20..].iter().map(|&r| observer.p_hit(r)).sum::<f64>() / (shown.len() - 20) as f64);
            }
            let mean_rate = rates.iter().sum::<f64>() / rates.len() as f64;
            let within = errors.iter().filter(|&&e| e < 0.15).count() as f32 / players as f32;
            errors.sort_by(f32::total_cmp);
            let median = errors[errors.len() / 2];
            println!(
                "theta {theta} slope {slope}: settled within 15% {within:.2}, median error {median:.3}, true hit rate after 20 targets {mean_rate:.3}"
            );
            assert!(within >= 0.9, "{within}");
            assert!(median < 0.08, "{median}");
            assert!((mean_rate - TARGET_HIT).abs() < 0.03, "{mean_rate}");
        }
    }
}
