//! Smart sensitivity finder: models performance as a function of sensitivity instead of
//! bisecting on gut feel alone.
//!
//! 1. Trials: each trial is a blind battery of drills at one sensitivity, picked by the finder's
//!    focus (see [`FinderFocus`]): the best sensitivity depends on the task, so it's found for
//!    the kind of aiming you care about. With several drills the order alternates between
//!    trials to balance order effects, and the first `WARM_IN` seconds of each part aren't
//!    scored (you adapt quickly right after a sens change).
//! 2. After every trial you rate how it felt (1-5). Each trial's objective is the mean, over its
//!    drills, of its result relative to that drill's average result, nudged by the feel rating.
//!    A flick drill's result is its effective throughput (TPe, see
//!    [`throughput`](crate::metrics::throughput)) when the part measured one, else its score:
//!    TPe barely moves when you happen to play one trial faster and sloppier and the next one
//!    slower and more careful, which a score does. Tracking uses its score.
//! 3. Model: `objective = f(x) + d*trial + noise` with `x = ln(sens / start)` over 0.6x to 1.6x,
//!    `f` a Gaussian process (see [`gp`](super::gp)). The `d*trial` term absorbs warm-up drift
//!    (later trials tend to score higher no matter the sensitivity), which would otherwise bias
//!    the result toward whatever was tested last; everything below uses the drift-corrected
//!    posterior.
//! 4. Bayesian optimisation: the first 3 trials are the low end, your sens and the high end, in
//!    random order; every later one goes where the expected improvement over the best so far is
//!    largest, at least `MIN_SPACING` from every tested sens.
//! 5. Best range: performance is flat over a broad plateau around the peak (NVIDIA, CHI 2022),
//!    so the best range is every sensitivity that is at least as likely as not to be within
//!    `PLATEAU` of the best, with its confidence (the probability that the best is inside it).
//!    The finder stops once that range has settled for 2 trials in a row (from trial
//!    `MIN_TRIALS`), or after `MAX_TRIALS`.
//! 6. Recommend: inside the best range, prefer the sensitivity where your aim is on target: where
//!    first flicks land on target (median ratio of first-submovement travel to target distance)
//!    and where clicks land on the target centre along the flick (mean click bias, in target
//!    radii), each fitted as a line in `x` and used only if that trend is statistically clear;
//!    the average of the two when both are. Otherwise keep your current sensitivity if it's in
//!    the best range: no change unless the data shows one helps. When the preferred sensitivity
//!    is outside the range, move to the closest one that is probably (`CONFIDENT`) within
//!    `PLATEAU` of the best, rather than to the range's coin-flip edge.

use bevy::prelude::*;

use super::gp::Mixture;
use crate::drill::{DrillKind, DrillSpec, Run, RunPurpose, RunStats};
use crate::metrics::throughput;
use crate::settings::{FinderFocus, Settings};

/// Relative range searched around the starting sensitivity.
const PROBE_LOW: f32 = 0.6;
const PROBE_HIGH: f32 = 1.6;
/// Trials before the finder may stop.
pub const MIN_TRIALS: usize = 6;
/// The finder always stops after this many trials.
pub const MAX_TRIALS: usize = 12;
/// Space-filling opening trials (low end, start, high end) before the model picks.
pub const OPENING: usize = 3;
/// Closest (ln-sens) a model-picked trial may come to an already tested sensitivity.
const MIN_SPACING: f32 = 0.04;
/// Best-range edges moving less than this (ln-sens) between trials count as settled.
const SETTLED: f32 = 0.03;
/// Grid the best range and the chart are evaluated on.
const GRID: usize = 61;
/// Candidate grid for the next trial.
const CANDIDATES: usize = 200;
/// Joint posterior draws for the best range.
const SAMPLES: usize = 600;
const SAMPLE_SEED: u64 = 0x5EED_F1DE;
/// Posterior probability of being within `PLATEAU` of the best needed to be in the best range.
const NEAR_BEST: f32 = 0.5;
/// Posterior probability of being within `PLATEAU` of the best a sensitivity needs to be moved
/// to.
const CONFIDENT: f32 = 0.7;
/// Two-sided 90% normal quantile, for the chart's band.
const BAND_Z: f32 = 1.644_853_6;
/// How much one feel step (on a 1-5 scale) shifts a trial's objective, relative to its score.
const FEEL_WEIGHT: f32 = 0.06;
/// Unscored seconds at the start of every part.
pub const WARM_IN: f32 = 3.0;
const MIN_LANDINGS: usize = 3;
/// Measured clicks a trial needs for its click bias.
const MIN_CLICKS: usize = 8;
/// Trials with landing (or click bias) data needed before it may influence the recommendation.
const MIN_TREND_TRIALS: usize = 4;
/// How clearly landings (or click biases) must grow with sensitivity (slope t statistic) to be
/// trusted.
const MIN_TREND_T: f32 = 2.5;
/// Objective tolerance of the best range, relative to the peak: smaller differences aren't
/// worth re-adapting to a new sensitivity for.
pub const PLATEAU: f32 = 0.03;
/// On-target sensitivities further than this from the start are extrapolation.
const MAX_TREND_FACTOR: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartStep {
    /// Waiting for the player to start (or continue) the next trial.
    Trial,
    /// Trial finished; waiting for a feel rating.
    Rate,
    Done,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SmartTrial {
    pub sens: f32,
    /// Score per drill, in `SmartFinder::drills` order.
    pub scores: Vec<f32>,
    /// Effective throughput (bits/s) per drill, in the same order, where the part measured one.
    pub throughputs: Vec<Option<f32>>,
    /// Median first-flick landing ratio (1 = on target), if there were enough flicks.
    pub landing: Option<f32>,
    /// Mean click bias along the flick (target radii, > 0 past the centre), if there were
    /// enough measured clicks.
    pub bias: Option<f32>,
    /// 1 (awful) to 5 (great).
    pub feel: u8,
}

/// What a finished part contributes to its trial.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Segment {
    pub score: f32,
    pub throughput: Option<f32>,
    /// First-flick landing ratios.
    pub landings: Vec<f32>,
    /// Measured clicks' offsets along the flick, in target radii.
    pub biases: Vec<f32>,
}

impl Segment {
    pub fn from_stats(stats: &RunStats) -> Self {
        Self {
            score: stats.score(),
            throughput: stats.throughput(),
            landings: stats.landings.clone(),
            biases: throughput::measured(&stats.clicks).map(|(_, offset)| offset.x).collect(),
        }
    }
}

/// The fitted model after the latest trial, all in relative log-sens `x`.
#[derive(Clone, Debug)]
struct Model {
    /// Evaluation grid across the searched range.
    grid: Vec<f32>,
    /// Drift-corrected posterior mean and standard deviation of the objective on the grid.
    mean: Vec<f32>,
    sd: Vec<f32>,
    /// Probability of being within `PLATEAU` of the best, on the grid.
    near: Vec<f32>,
    /// Best range (inclusive).
    range: (f32, f32),
    /// Posterior probability that the best sensitivity is inside the range.
    confidence: f32,
    /// Highest posterior mean.
    peak: f32,
    /// Fitted gain per trial (warm-up drift).
    drift: f32,
    /// Fitted trial-to-trial noise (standard deviation).
    noise: f32,
    /// Each trial's share of the drift (`drift * (trial - average trial)`).
    drift_shares: Vec<f32>,
    /// Where the next trial goes: the largest expected improvement.
    next: f32,
}

impl Model {
    fn fit(xs: &[f32], ys: &[f32]) -> Option<Self> {
        let (lo, hi) = (PROBE_LOW.ln(), PROBE_HIGH.ln());
        let xs64: Vec<f64> = xs.iter().map(|&x| f64::from(x)).collect();
        let ts: Vec<f64> = (0..xs.len()).map(|i| i as f64).collect();
        let ys64: Vec<f64> = ys.iter().map(|&y| f64::from(y)).collect();
        let gp = Mixture::fit(&xs64, &ts, &ys64)?;
        let along = |points: usize| -> Vec<f64> {
            (0..points).map(|i| f64::from(lo) + f64::from(hi - lo) * i as f64 / (points - 1) as f64).collect()
        };

        let grid = along(GRID);
        let near_best = gp.near_best(&grid, f64::from(PLATEAU), SAMPLES, SAMPLE_SEED);
        let (mean, sd): (Vec<f32>, Vec<f32>) = grid
            .iter()
            .map(|&x| {
                let (mean, variance) = gp.predict(x);
                (mean as f32, variance.sqrt() as f32)
            })
            .unzip();
        let inside: Vec<usize> = (0..GRID).filter(|&i| near_best.near[i] as f32 >= NEAR_BEST).collect();
        let (first, last) = match (inside.first(), inside.last()) {
            (Some(&first), Some(&last)) => (first, last),
            // Nothing even odds: the single most likely point.
            _ => {
                let best = (0..GRID).max_by(|&a, &b| near_best.near[a].total_cmp(&near_best.near[b])).unwrap_or(0);
                (best, best)
            },
        };
        let confidence = near_best.peak[first..=last].iter().sum::<f64>() as f32;

        // Next trial: expected improvement over the best predicted tested sensitivity, among
        // candidates not too close to a tested one. Ties keep the lowest candidate.
        let candidates = along(CANDIDATES);
        let predictions: Vec<(f64, f64)> = candidates.iter().map(|&x| gp.predict(x)).collect();
        let peak_index =
            (0..CANDIDATES).fold(0, |best, i| if predictions[i].0 > predictions[best].0 { i } else { best });
        let incumbent = xs64.iter().map(|&x| gp.predict(x).0).fold(f64::MIN, f64::max);
        let open: Vec<usize> =
            (0..CANDIDATES).filter(|&i| xs.iter().all(|&x| (candidates[i] as f32 - x).abs() >= MIN_SPACING)).collect();
        let pick = |score: &dyn Fn(usize) -> f64| {
            open.iter().copied().fold(None, |best: Option<(usize, f64)>, i| {
                let value = score(i);
                if best.is_none_or(|(_, b)| value > b) { Some((i, value)) } else { best }
            })
        };
        let ei = |i: usize| gp.expected_improvement(candidates[i], incumbent);
        let next = match pick(&ei) {
            Some((i, value)) if value > 1e-12 => i,
            // No expected gain anywhere (a confident model): measure where it's least sure.
            Some(_) => pick(&|i: usize| predictions[i].1).map_or(peak_index, |(i, _)| i),
            None => peak_index,
        };

        let drift = gp.drift();
        Some(Self {
            grid: grid.iter().map(|&x| x as f32).collect(),
            mean,
            sd,
            near: near_best.near.iter().map(|&p| p as f32).collect(),
            range: (grid[first] as f32, grid[last] as f32),
            confidence,
            peak: candidates[peak_index] as f32,
            drift: drift as f32,
            noise: gp.noise() as f32,
            drift_shares: (0..xs.len()).map(|i| (drift * gp.trial_offset(i)) as f32).collect(),
            next: candidates[next] as f32,
        })
    }
}

/// Parts of the trial in progress played so far.
#[derive(Clone, Debug, Default)]
struct Pending {
    scores: Vec<f32>,
    throughputs: Vec<Option<f32>>,
    played: usize,
    landings: Vec<f32>,
    biases: Vec<f32>,
}

#[derive(Resource, Clone, Debug)]
pub struct SmartFinder {
    pub start_sens: f32,
    /// The drills every trial plays, in canonical order (trial scores are stored in this
    /// order); chosen by the finder's focus.
    pub drills: &'static [DrillKind],
    /// Opening trials still to play, as relative log-sens (popped from the back).
    opening: Vec<f32>,
    pub trials: Vec<SmartTrial>,
    pending: Pending,
    pub step: SmartStep,
    /// Model after the latest trial, once there are `OPENING` trials.
    model: Option<Model>,
    /// Best range (relative log-sens) after each modelled trial, for the stopping rule.
    ranges: Vec<(f32, f32)>,
}

/// The drills each trial plays for a focus.
pub fn focus_drills(focus: FinderFocus) -> &'static [DrillKind] {
    match focus {
        FinderFocus::Balanced => &[DrillKind::Microflick, DrillKind::Tracking],
        FinderFocus::Flicking => &[DrillKind::Gridshot, DrillKind::Microflick],
        FinderFocus::Tracking => &[DrillKind::Tracking],
    }
}

impl SmartFinder {
    pub fn new(start_sens: f32, focus: FinderFocus) -> Self {
        Self::with_rng(start_sens, focus, &mut fastrand::Rng::new())
    }

    /// A finder whose opening order comes from `rng`.
    pub fn with_rng(start_sens: f32, focus: FinderFocus, rng: &mut fastrand::Rng) -> Self {
        let mut opening = vec![PROBE_LOW.ln(), 0.0, PROBE_HIGH.ln()];
        rng.shuffle(&mut opening);
        Self {
            start_sens,
            drills: focus_drills(focus),
            opening,
            trials: Vec::new(),
            pending: Pending::default(),
            step: SmartStep::Trial,
            model: None,
            ranges: Vec::new(),
        }
    }

    /// Relative log-sens of the trial in progress.
    fn current_x(&self) -> f32 {
        match (self.opening.last(), &self.model) {
            (Some(&x), _) => x,
            (None, Some(model)) => model.next,
            (None, None) => 0.0,
        }
    }

    /// Sensitivity for the trial in progress. Deterministic, so it's the same when the trial
    /// starts and when its result is recorded.
    pub fn current_sens(&self) -> f32 {
        self.start_sens * self.current_x().exp()
    }

    /// The current trial's drills in play order. With several drills the order reverses on
    /// every other trial, to balance order effects.
    pub fn segment_order(&self) -> Vec<DrillKind> {
        let mut order = self.drills.to_vec();
        if self.trials.len() % 2 == 1 {
            order.reverse();
        }
        order
    }

    /// Starts the current trial from its first part, dropping any parts of an aborted attempt.
    pub fn begin_trial(&mut self) {
        if self.step == SmartStep::Trial {
            self.pending = Pending::default();
        }
    }

    /// The run for the current trial's next part, if it has one left.
    pub fn segment_run(&self, settings: &Settings) -> Option<Run> {
        if self.step != SmartStep::Trial {
            return None;
        }
        let part = self.pending.played;
        let drill = *self.segment_order().get(part)?;
        let title = format!(
            "Finder trial {} (up to {MAX_TRIALS}) - part {}/{}: {}",
            self.trials.len() + 1,
            part + 1,
            self.drills.len(),
            drill.name(),
        );
        let run = Run::new(
            DrillSpec::builtin(drill, settings),
            self.current_sens(),
            settings.finder_trial_secs,
            RunPurpose::FinderTrial,
            title,
        );
        Some(run.with_warm_in(WARM_IN))
    }

    /// Stores a finished part; after the last one the trial waits for its feel rating.
    pub fn record_segment(&mut self, drill: DrillKind, segment: Segment) {
        let Some(index) = self.drills.iter().position(|&d| d == drill) else { return };
        if self.step != SmartStep::Trial {
            return;
        }
        let pending = &mut self.pending;
        pending.scores.resize(self.drills.len(), 0.0);
        pending.throughputs.resize(self.drills.len(), None);
        pending.scores[index] = segment.score;
        pending.throughputs[index] = segment.throughput;
        pending.landings.extend(segment.landings);
        pending.biases.extend(segment.biases);
        pending.played += 1;
        if pending.played >= self.drills.len() {
            self.step = SmartStep::Rate;
        }
    }

    /// Stores the finished trial with its feel rating (1-5), refits the model and either picks
    /// the next trial or stops.
    pub fn rate(&mut self, feel: u8) {
        if self.step != SmartStep::Rate {
            return;
        }
        let Pending { scores, throughputs, landings, biases, .. } = std::mem::take(&mut self.pending);
        let sens = self.current_sens();
        self.opening.pop();
        let bias = (biases.len() >= MIN_CLICKS).then(|| biases.iter().sum::<f32>() / biases.len() as f32);
        let landing = median(landings);
        self.trials.push(SmartTrial { sens, scores, throughputs, landing, bias, feel: feel.clamp(1, 5) });
        self.refit();
        self.step = if self.finished() { SmartStep::Done } else { SmartStep::Trial };
    }

    /// Refits the model to all trials so far (once the opening is complete).
    fn refit(&mut self) {
        if self.trials.len() < OPENING {
            return;
        }
        let xs: Vec<f32> = self.trials.iter().map(|t| self.x(t.sens)).collect();
        self.model = Model::fit(&xs, &self.objectives());
        if let Some(model) = &self.model {
            self.ranges.push(model.range);
        }
    }

    /// Whether the search is over: `MAX_TRIALS` played, or at least `MIN_TRIALS` with a best
    /// range that hasn't moved for the last 2 trials.
    fn finished(&self) -> bool {
        let settled = |pair: &[(f32, f32)]| {
            let (a, b) = (pair[0], pair[1]);
            (a.0 - b.0).abs() < SETTLED && (a.1 - b.1).abs() < SETTLED
        };
        let n = self.trials.len();
        n >= MAX_TRIALS
            || (n >= MIN_TRIALS
                && self.ranges.len() >= 3
                && self.ranges[self.ranges.len() - 3..].windows(2).all(settled))
    }

    /// Throws away a finished trial (e.g. a mis-click or distraction) so all its parts can be
    /// replayed.
    pub fn redo(&mut self) {
        if self.step == SmartStep::Rate {
            self.pending = Pending::default();
            self.step = SmartStep::Trial;
        }
    }

    fn x(&self, sens: f32) -> f32 {
        (sens / self.start_sens).ln()
    }

    fn sens(&self, x: f32) -> f32 {
        self.start_sens * x.exp()
    }

    /// Per-trial objective: the mean over drills of the result relative to that drill's mean
    /// result (so no drill dominates through its scale), plus the feel adjustment. A drill's
    /// result is its throughput where the part measured one, else its score.
    pub fn objectives(&self) -> Vec<f32> {
        let mean = |values: Vec<f32>| {
            let n = values.len().max(1) as f32;
            values.into_iter().sum::<f32>() / n
        };
        let score_means: Vec<f32> =
            (0..self.drills.len()).map(|d| mean(self.trials.iter().map(|t| t.scores[d]).collect())).collect();
        let throughput_means: Vec<f32> = (0..self.drills.len())
            .map(|d| mean(self.trials.iter().filter_map(|t| t.throughputs[d]).collect()))
            .collect();
        let relative = |value: f32, mean: f32| if mean > 0.0 { value / mean } else { 1.0 };
        self.trials
            .iter()
            .map(|t| {
                let results: f32 = (0..self.drills.len())
                    .map(|d| match t.throughputs[d] {
                        Some(tp) => relative(tp, throughput_means[d]),
                        None => relative(t.scores[d], score_means[d]),
                    })
                    .sum();
                results / self.drills.len() as f32 + FEEL_WEIGHT * (f32::from(t.feel) - 3.0)
            })
            .collect()
    }

    /// Objectives with each trial's share of the warm-up drift removed (as if every trial had
    /// been played at the average point of the session).
    pub fn corrected_objectives(&self) -> Vec<f32> {
        let objectives = self.objectives();
        match &self.model {
            Some(model) => objectives.iter().zip(&model.drift_shares).map(|(y, share)| y - share).collect(),
            None => objectives,
        }
    }

    /// Relative log-sens with the best predicted performance; the best trial before there's a
    /// model.
    fn optimum_x(&self) -> f32 {
        if let Some(model) = &self.model {
            return model.peak;
        }
        let objectives = self.objectives();
        self.trials.iter().zip(&objectives).max_by(|a, b| a.1.total_cmp(b.1)).map_or(0.0, |(t, _)| self.x(t.sens))
    }

    /// Sensitivity with the best predicted performance.
    pub fn peak_sens(&self) -> f32 {
        self.sens(self.optimum_x())
    }

    /// Best range in relative log-sens; just the best trial before there's a model.
    fn range_x(&self) -> (f32, f32) {
        match &self.model {
            Some(model) => model.range,
            None => (self.optimum_x(), self.optimum_x()),
        }
    }

    /// Sensitivities at least as likely as not to perform within `PLATEAU` of the best.
    pub fn best_range(&self) -> (f32, f32) {
        let (lo, hi) = self.range_x();
        (self.sens(lo), self.sens(hi))
    }

    /// Posterior probability that the best sensitivity is inside [`Self::best_range`].
    pub fn confidence(&self) -> Option<f32> {
        self.model.as_ref().map(|model| model.confidence)
    }

    /// Fitted trial-to-trial noise of the objective (standard deviation).
    pub fn noise(&self) -> Option<f32> {
        self.model.as_ref().map(|model| model.noise)
    }

    /// Fitted warm-up drift of the objective per trial.
    pub fn drift(&self) -> Option<f32> {
        self.model.as_ref().map(|model| model.drift)
    }

    /// The best range spans (almost) the whole searched range: within it, sensitivity made no
    /// measurable difference.
    pub fn is_flat(&self) -> bool {
        let (lo, hi) = self.range_x();
        self.model.is_some() && lo <= PROBE_LOW.ln() + 0.02 && hi >= PROBE_HIGH.ln() - 0.02
    }

    /// Drift-corrected posterior across the searched range as (sens, mean, lower, upper), the
    /// bounds of the 90% credible band.
    pub fn band(&self) -> Vec<(f32, f32, f32, f32)> {
        let Some(model) = &self.model else { return Vec::new() };
        (0..model.grid.len())
            .map(|i| {
                let (mean, spread) = (model.mean[i], BAND_Z * model.sd[i]);
                (self.sens(model.grid[i]), mean, mean - spread, mean + spread)
            })
            .collect()
    }

    /// Least-squares line `y = y0 + y1 * x` through the trials with a value, with the slope's t
    /// statistic.
    fn trend(&self, value: impl Fn(&SmartTrial) -> Option<f32>) -> Option<Line> {
        let points: Vec<(f32, f32)> = self.trials.iter().filter_map(|t| Some((self.x(t.sens), value(t)?))).collect();
        Line::fit(&points)
    }

    /// Line through the trials' median first-flick landing ratios.
    fn landing_fit(&self) -> Option<Line> {
        self.trend(|t| t.landing)
    }

    /// Line through the trials' mean click biases.
    fn bias_fit(&self) -> Option<Line> {
        self.trend(|t| t.bias)
    }

    /// Relative log-sens where first flicks are predicted to land exactly on target.
    fn landing_x(&self) -> Option<f32> {
        self.landing_fit()?.on_target(1.0)
    }

    /// Relative log-sens where clicks are predicted to land on the target centre along the
    /// flick.
    fn bias_x(&self) -> Option<f32> {
        self.bias_fit()?.on_target(0.0)
    }

    /// Relative log-sens where your aim is on target: the landing and click-bias crossings,
    /// averaged when both are clear.
    fn precision_x(&self) -> Option<f32> {
        match (self.landing_x(), self.bias_x()) {
            (Some(landing), Some(bias)) => Some((landing + bias) * 0.5),
            (landing, bias) => landing.or(bias),
        }
    }

    /// Sensitivity whose first flicks are predicted to land on target.
    pub fn landing_sens(&self) -> Option<f32> {
        self.landing_x().map(|x| self.sens(x))
    }

    /// Sensitivity whose clicks are predicted to land on the target centre along the flick.
    pub fn bias_sens(&self) -> Option<f32> {
        self.bias_x().map(|x| self.sens(x))
    }

    /// Sensitivity where your aim is on target (see [`Self::recommendation`]).
    pub fn precision_sens(&self) -> Option<f32> {
        self.precision_x().map(|x| self.sens(x))
    }

    /// Predicted first-flick landing ratio at the starting sensitivity.
    pub fn landing_at_start(&self) -> Option<f32> {
        self.landing_fit().map(|line| line.intercept)
    }

    /// Predicted click bias (target radii) at the starting sensitivity.
    pub fn bias_at_start(&self) -> Option<f32> {
        self.bias_fit().map(|line| line.intercept)
    }

    /// Whether landing or click-bias data picked the recommendation inside the best range.
    pub fn uses_precision(&self) -> bool {
        self.precision_x().is_some()
    }

    /// Inside the best range, the sensitivity where your aim is clearly on target (first flicks
    /// land on target, clicks land on the centre); otherwise the sensitivity you started with.
    /// Changing sensitivity costs re-adaptation, so it's only worth it when the data shows a
    /// real difference: if your current sensitivity is already in the best range, it is the
    /// recommendation. When that target is outside the range, the closest sensitivity that is
    /// probably (`CONFIDENT`) within `PLATEAU` of the best: the range's edge itself is a coin
    /// flip.
    pub fn recommendation(&self) -> f32 {
        let (lo, hi) = self.range_x();
        let target = self.precision_x().unwrap_or(0.0);
        if (lo..=hi).contains(&target) {
            return self.sens(target);
        }
        let confident = self.model.as_ref().and_then(|model| {
            (0..model.grid.len())
                .filter(|&i| model.near[i] >= CONFIDENT)
                .map(|i| model.grid[i])
                .min_by(|a, b| (a - target).abs().total_cmp(&(b - target).abs()))
        });
        self.sens(confident.unwrap_or(target.clamp(lo, hi)))
    }
}

/// A least-squares line `y = intercept + slope * x` with the slope's t statistic (slope /
/// standard error; infinite for a perfect line).
#[derive(Clone, Copy, Debug)]
struct Line {
    intercept: f32,
    slope: f32,
    t: f32,
}

impl Line {
    /// `None` with fewer than `MIN_TREND_TRIALS` points or no spread in `x`.
    fn fit(points: &[(f32, f32)]) -> Option<Self> {
        if points.len() < MIN_TREND_TRIALS {
            return None;
        }
        let n = points.len() as f32;
        let mean_x = points.iter().map(|p| p.0).sum::<f32>() / n;
        let mean_y = points.iter().map(|p| p.1).sum::<f32>() / n;
        let sxx: f32 = points.iter().map(|p| (p.0 - mean_x).powi(2)).sum();
        let sxy: f32 = points.iter().map(|p| (p.0 - mean_x) * (p.1 - mean_y)).sum();
        if sxx < 1e-6 {
            return None;
        }
        let slope = sxy / sxx;
        let intercept = mean_y - slope * mean_x;
        let rss: f32 = points.iter().map(|p| (p.1 - intercept - slope * p.0).powi(2)).sum();
        let se = (rss / (n - 2.0) / sxx).sqrt();
        let t = if se > 1e-9 { slope / se } else { f32::INFINITY.copysign(slope) };
        Some(Self { intercept, slope, t })
    }

    /// Relative log-sens where the line reaches `level` (on target). Only when the value clearly
    /// grows with sensitivity, as landings and click biases should (faster aim travels
    /// further): a slope that could be noise would put the crossing anywhere. Also bounded to
    /// `MAX_TREND_FACTOR` around the start.
    fn on_target(&self, level: f32) -> Option<f32> {
        let x = (level - self.intercept) / self.slope;
        (self.t >= MIN_TREND_T && x.abs() <= MAX_TREND_FACTOR.ln()).then_some(x)
    }
}

/// Median of a trial's landing ratios, if it has at least `MIN_LANDINGS` of them.
fn median(mut values: Vec<f32>) -> Option<f32> {
    if values.len() < MIN_LANDINGS {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) { (values[mid - 1] + values[mid]) * 0.5 } else { values[mid] })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A part with just a score.
    fn scored(score: f32) -> Segment {
        Segment { score, ..Segment::default() }
    }

    /// Plays all parts of the current trial with per-drill scores (in `finder.drills` order)
    /// and Microflick landings, then rates it.
    fn play(finder: &mut SmartFinder, scores: &[f32], landings: &[f32], feel: u8) {
        finder.begin_trial();
        for drill in finder.segment_order() {
            let index = finder.drills.iter().position(|&d| d == drill).unwrap();
            let landings = if drill == DrillKind::Microflick { landings.to_vec() } else { Vec::new() };
            finder.record_segment(drill, Segment { landings, ..scored(scores[index]) });
        }
        finder.rate(feel);
    }

    /// A synthetic player: relative skill `1 - curvature * (x - peak)^2` in ln-sens, plus
    /// `drift` per trial and normal noise with standard deviation `noise`.
    #[derive(Clone, Copy)]
    struct Player {
        peak: f32,
        curvature: f32,
        noise: f32,
        drift: f32,
    }

    impl Player {
        fn skill(&self, x: f32) -> f32 {
            1.0 - self.curvature * (x - self.peak).powi(2)
        }

        /// Whether `x` performs within `PLATEAU` of the peak.
        fn in_plateau(&self, x: f32) -> bool {
            self.skill(x) >= 1.0 - PLATEAU
        }

        /// Plays a Balanced finder to completion (both drills on the same curve, Tracking on a
        /// 9x larger scale); `seed` decides the opening order and the noise.
        fn run(&self, seed: u64) -> SmartFinder {
            let mut rng = fastrand::Rng::with_seed(seed);
            let mut finder = SmartFinder::with_rng(1.0, FinderFocus::Balanced, &mut rng);
            while finder.step != SmartStep::Done {
                let x = finder.current_sens().ln();
                let noise = self.noise * normal(&mut rng);
                let score = 1000.0 * (self.skill(x) + self.drift * finder.trials.len() as f32 + noise);
                play(&mut finder, &[score, 9.0 * score], &[], 3);
            }
            finder
        }
    }

    /// Box-Muller.
    fn normal(rng: &mut fastrand::Rng) -> f32 {
        let (u, v) = (1.0 - rng.f32(), rng.f32());
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }

    /// Plays a finder to completion against a noiseless player peaking at `best` who improves
    /// by `drift` (on a ~1000 scale) per trial.
    fn simulate(best: f32, drift: f32) -> SmartFinder {
        Player { peak: best.ln(), curvature: 0.8, noise: 0.0, drift: drift / 1000.0 }.run(1)
    }

    /// The relative log-sens of the 9 trials of [`finished_trials`], in play order: they cover
    /// the searched range, shuffled so trial index and sensitivity aren't collinear.
    fn trial_xs() -> impl Iterator<Item = f32> {
        let (lo, hi) = (PROBE_LOW.ln(), PROBE_HIGH.ln());
        [4, 0, 7, 2, 8, 1, 5, 3, 6].into_iter().map(move |i| lo + (hi - lo) * i as f32 / 8.0)
    }

    /// A finished finder with the given focus and one trial per [`trial_xs`] (trial index, x).
    fn finished_trials(focus: FinderFocus, mut trial: impl FnMut(usize, f32) -> SmartTrial) -> SmartFinder {
        let mut finder = SmartFinder::new(1.0, focus);
        finder.trials = trial_xs().enumerate().map(|(i, x)| trial(i, x)).collect();
        finder.opening.clear();
        finder.refit();
        finder.step = SmartStep::Done;
        finder
    }

    /// A trial at relative log-sens `x` with just scores.
    fn trial(x: f32, scores: &[f32]) -> SmartTrial {
        SmartTrial {
            sens: x.exp(),
            scores: scores.to_vec(),
            throughputs: vec![None; scores.len()],
            landing: None,
            bias: None,
            feel: 3,
        }
    }

    /// A finished Balanced finder with scores and landings given per `x`.
    fn finished(mut scores_and_landing: impl FnMut(f32) -> ([f32; 2], Option<f32>)) -> SmartFinder {
        finished_trials(FinderFocus::Balanced, |_, x| {
            let (scores, landing) = scores_and_landing(x);
            SmartTrial { landing, ..trial(x, &scores) }
        })
    }

    /// Broad performance plateau centred on the start sens, equal in both drills.
    fn flat_top(x: f32) -> [f32; 2] {
        let score = 1000.0 * (1.0 - 0.3 * x * x);
        [score, 5.0 * score]
    }

    #[test]
    fn the_opening_covers_both_ends_and_the_start_once_each() {
        let mut finder = SmartFinder::new(0.5, FinderFocus::Balanced);
        let mut seen = Vec::new();
        for _ in 0..OPENING {
            seen.push(finder.current_sens());
            assert!(finder.confidence().is_none());
            play(&mut finder, &[100.0, 100.0], &[], 3);
        }
        seen.sort_by(f32::total_cmp);
        assert!((seen[0] - 0.3).abs() < 1e-4 && seen[1] == 0.5 && (seen[2] - 0.8).abs() < 1e-4, "{seen:?}");
        assert_eq!(finder.trials.len(), OPENING);
        assert!(finder.confidence().is_some());
        // The order is shuffled.
        let orders: std::collections::HashSet<_> = (0..20)
            .map(|seed| {
                SmartFinder::with_rng(1.0, FinderFocus::Balanced, &mut fastrand::Rng::with_seed(seed))
                    .opening
                    .iter()
                    .map(|x| x.to_bits())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(orders.len() > 1);
    }

    #[test]
    fn trials_play_both_drills_in_alternating_order() {
        let settings = Settings::default();
        let mut finder = SmartFinder::new(1.0, FinderFocus::Balanced);
        finder.begin_trial();
        let first = finder.segment_run(&settings).unwrap();
        assert_eq!(first.kind(), DrillKind::Microflick);
        assert_eq!(first.title, "Finder trial 1 (up to 12) - part 1/2: Microflick");
        let landings = vec![0.8, 1.3, 0.9, 1.0];
        let biases = vec![0.2; MIN_CLICKS];
        finder.record_segment(DrillKind::Microflick, Segment { score: 500.0, throughput: Some(4.5), landings, biases });
        assert_eq!(finder.step, SmartStep::Trial);
        let second = finder.segment_run(&settings).unwrap();
        assert_eq!((second.kind(), second.sens), (DrillKind::Tracking, first.sens));
        finder.record_segment(DrillKind::Tracking, scored(4000.0));
        assert_eq!(finder.step, SmartStep::Rate);
        assert!(finder.segment_run(&settings).is_none());
        finder.rate(4);
        assert_eq!(finder.trials[0].scores, [500.0, 4000.0]);
        assert_eq!(finder.trials[0].throughputs, [Some(4.5), None]);
        assert_eq!(finder.trials[0].landing, Some(0.95));
        assert!((finder.trials[0].bias.unwrap() - 0.2).abs() < 1e-6);
        assert_eq!(finder.segment_order(), [DrillKind::Tracking, DrillKind::Microflick]);
        // Too few flicks for a landing estimate.
        play(&mut finder, &[500.0, 4000.0], &[1.0, 1.1], 3);
        assert_eq!(finder.trials[1].landing, None);
        assert_eq!(finder.trials[1].bias, None);
    }

    #[test]
    fn the_focus_picks_the_drills() {
        let settings = Settings::default();
        let mut flicking = SmartFinder::new(1.0, FinderFocus::Flicking);
        assert_eq!(flicking.segment_run(&settings).unwrap().title, "Finder trial 1 (up to 12) - part 1/2: Gridshot");
        play(&mut flicking, &[30_000.0, 9_000.0], &[], 3);
        assert_eq!(flicking.segment_order(), [DrillKind::Microflick, DrillKind::Gridshot]);
        assert_eq!(flicking.segment_run(&settings).unwrap().title, "Finder trial 2 (up to 12) - part 1/2: Microflick");
        // Tracking only: one part per trial, rated right after it.
        let mut tracking = SmartFinder::new(1.0, FinderFocus::Tracking);
        assert_eq!(tracking.segment_run(&settings).unwrap().title, "Finder trial 1 (up to 12) - part 1/1: Tracking");
        tracking.record_segment(DrillKind::Tracking, scored(5000.0));
        assert_eq!(tracking.step, SmartStep::Rate);
        // A drill outside the focus is ignored.
        let mut other = SmartFinder::new(1.0, FinderFocus::Tracking);
        other.record_segment(DrillKind::Gridshot, scored(5000.0));
        assert_eq!(other.step, SmartStep::Trial);
    }

    #[test]
    fn finds_the_peak_of_a_noiseless_player() {
        let finder = simulate(1.2, 0.0);
        assert!((finder.peak_sens() - 1.2).abs() < 0.03, "{}", finder.peak_sens());
        assert!((MIN_TRIALS..=MAX_TRIALS).contains(&finder.trials.len()));
        let (lo, hi) = finder.best_range();
        assert!(lo < 1.2 && hi > 1.2 && finder.confidence().unwrap() > 0.8, "{lo} {hi} {:?}", finder.confidence());
    }

    #[test]
    fn keeps_the_current_sens_when_it_is_already_near_best() {
        // Peak 10% above the start: the start still scores within 3% of it, so don't change.
        let finder = simulate(1.1, 0.0);
        let (lo, hi) = finder.best_range();
        assert!(lo < 1.0 && hi > 1.1, "{lo} {hi}");
        assert_eq!(finder.recommendation(), 1.0);
    }

    #[test]
    fn moves_only_as_far_as_the_best_range_when_the_start_is_clearly_worse() {
        // The closest sens that's probably near the best: just inside the range's low edge.
        let finder = simulate(1.45, 0.0);
        let (lo, hi) = finder.best_range();
        let recommended = finder.recommendation();
        assert!(lo > 1.0, "{lo} {hi}");
        assert!(recommended >= lo && recommended < lo * 1.06 && recommended < finder.peak_sens(), "{lo} {recommended}");
    }

    #[test]
    fn noisy_flat_results_keep_the_current_sens() {
        // Scores and landings that don't depend on sensitivity, scattered by +-12% / +-8%:
        // nothing here justifies a change, however the noise happens to fall.
        let jitter = [0.9, -0.6, 1.0, -1.0, 0.3, -0.2, 0.7, -0.9, 0.1];
        let mut k = 0;
        let finder = finished(|_| {
            let j = jitter[k % jitter.len()];
            k += 1;
            ([1000.0 * (1.0 + 0.12 * j), 5000.0 * (1.0 - 0.12 * j)], Some(1.0 + 0.08 * j))
        });
        assert!(finder.is_flat());
        assert!(finder.landing_sens().is_none());
        assert_eq!(finder.recommendation(), 1.0);
    }

    #[test]
    fn warm_up_drift_does_not_bias_the_result() {
        // Strong warm-up: +40 points per trial on a ~1000-point scale.
        let finder = simulate(0.9, 40.0);
        assert!((finder.peak_sens() - 0.9).abs() < 0.03, "{}", finder.peak_sens());
        assert!((finder.drift().unwrap() - 0.04).abs() < 0.01, "{:?}", finder.drift());
    }

    #[test]
    fn drills_are_normalised_so_neither_dominates() {
        // Microflick peaks at 0.2 below the start, Tracking 0.2 above, on a 10x larger scale.
        // Raw scores would put the peak near +0.16; normalised, the drills weigh equally.
        let finder =
            finished(|x| ([100.0 * (1.0 - 0.8 * (x + 0.2).powi(2)), 1000.0 * (1.0 - 0.8 * (x - 0.2).powi(2))], None));
        assert!(finder.peak_sens().ln().abs() < 0.03, "{}", finder.peak_sens());
    }

    #[test]
    fn recommends_the_in_range_sens_where_flicks_land_on_target() {
        // Landing on target at x = 0.15, inside the best range.
        let finder = finished(|x| (flat_top(x), Some(1.0 + 0.5 * (x - 0.15))));
        let (lo, hi) = finder.best_range();
        assert!(lo < 0.8 && hi > 1.25, "{lo} {hi}");
        assert!((finder.peak_sens() - 1.0).abs() < 0.02, "{}", finder.peak_sens());
        assert!((finder.recommendation().ln() - 0.15).abs() < 0.01, "{}", finder.recommendation());
        assert!((finder.landing_sens().unwrap().ln() - 0.15).abs() < 0.01);
        assert!((finder.landing_at_start().unwrap() - 0.925).abs() < 0.01);
        assert!(finder.uses_precision());

        // Landing on target far above the best range: as far up as is still probably near the
        // best.
        let finder = finished(|x| (flat_top(x), Some(1.0 + 0.5 * (x - 0.6))));
        let (lo, hi) = finder.best_range();
        let recommended = finder.recommendation();
        assert!(recommended > 1.2 && recommended <= hi, "{lo} {hi} {recommended}");
        assert!(finder.landing_sens().unwrap() > hi);
    }

    #[test]
    fn falls_back_to_the_start_without_usable_landing_data() {
        let none = finished(|x| (flat_top(x), None));
        assert!(!none.uses_precision() && none.landing_at_start().is_none());
        assert_eq!(none.recommendation(), 1.0);
        // Landings that shrink with sensitivity make no sense; they're ignored.
        let backwards = finished(|x| (flat_top(x), Some(1.0 - 0.5 * (x - 0.15))));
        assert!(backwards.landing_sens().is_none());
        assert_eq!(backwards.recommendation(), 1.0);
    }

    #[test]
    fn feel_ratings_shift_the_recommendation() {
        let mut finder = SmartFinder::with_rng(1.0, FinderFocus::Balanced, &mut fastrand::Rng::with_seed(3));
        while finder.step != SmartStep::Done {
            let sens = finder.current_sens();
            // Scores identical everywhere, but high sens feels much better.
            play(&mut finder, &[1000.0, 5000.0], &[], if sens > 1.2 { 5 } else { 2 });
        }
        assert!(finder.recommendation() > 1.2, "{}", finder.recommendation());
    }

    #[test]
    fn redo_discards_the_whole_trial() {
        let mut finder = SmartFinder::new(1.0, FinderFocus::Balanced);
        let sens = finder.current_sens();
        finder.record_segment(DrillKind::Microflick, Segment { landings: vec![1.0; 5], ..scored(10.0) });
        finder.record_segment(DrillKind::Tracking, scored(10.0));
        finder.redo();
        assert_eq!(finder.step, SmartStep::Trial);
        assert!(finder.trials.is_empty());
        assert_eq!(finder.current_sens(), sens);
        finder.rate(3); // nothing to rate: ignored
        assert!(finder.trials.is_empty());
        // An aborted attempt restarts from the first part.
        let settings = Settings::default();
        finder.record_segment(DrillKind::Microflick, scored(10.0));
        assert_eq!(finder.segment_run(&settings).unwrap().kind(), DrillKind::Tracking);
        finder.begin_trial();
        assert_eq!(finder.segment_run(&settings).unwrap().kind(), DrillKind::Microflick);
    }

    #[test]
    fn redo_of_a_model_picked_trial_replays_the_same_sens() {
        let mut finder = SmartFinder::with_rng(1.0, FinderFocus::Tracking, &mut fastrand::Rng::with_seed(2));
        for _ in 0..OPENING {
            let x = finder.current_sens().ln();
            play(&mut finder, &[1000.0 * (1.0 - x * x)], &[], 3);
        }
        let sens = finder.current_sens();
        finder.record_segment(DrillKind::Tracking, scored(10.0));
        finder.redo();
        assert_eq!(finder.current_sens(), sens);
        assert_eq!(finder.trials.len(), OPENING);
    }

    #[test]
    fn the_objective_uses_throughput_for_flick_drills() {
        // Balanced: Microflick by its throughput, Tracking by its score. Microflick scores swing
        // wildly; its throughput alternates 4 / 6 bits/s.
        let mut finder = finished_trials(FinderFocus::Balanced, |i, x| SmartTrial {
            throughputs: vec![Some(if i % 2 == 0 { 4.0 } else { 6.0 }), None],
            ..trial(x, &[if i % 2 == 0 { 900.0 } else { 100.0 }, 5000.0])
        });
        let mean = (5.0 * 4.0 + 4.0 * 6.0) / 9.0;
        let objectives = finder.objectives();
        assert!((objectives[0] - (4.0 / mean + 1.0) / 2.0).abs() < 1e-5, "{objectives:?}");
        assert!((objectives[1] - (6.0 / mean + 1.0) / 2.0).abs() < 1e-5, "{objectives:?}");
        // A part without a throughput (too few flicks) falls back to its score.
        finder.trials[1].throughputs[0] = None;
        let score_mean = (5.0 * 900.0 + 4.0 * 100.0) / 9.0;
        let objectives = finder.objectives();
        assert!((objectives[1] - (100.0 / score_mean + 1.0) / 2.0).abs() < 1e-5, "{objectives:?}");
        let mean = (5.0 * 4.0 + 3.0 * 6.0) / 8.0;
        assert!((objectives[0] - (4.0 / mean + 1.0) / 2.0).abs() < 1e-5, "{objectives:?}");
    }

    #[test]
    fn reckless_and_careful_trials_do_not_move_the_peak() {
        // Flicking: skill peaks at x = 0.2. Each part is played either recklessly or carefully,
        // which swings its score by up to +-35% but leaves its throughput on the skill curve.
        let style = [1.35, 0.7, 1.0, 1.3, 0.65, 1.35, 0.75, 1.25, 0.7];
        let skill = |x: f32| 1.0 - 0.8 * (x - 0.2).powi(2);
        let play = |with_throughput: bool| {
            finished_trials(FinderFocus::Flicking, |i, x| {
                let scores = [30_000.0 * skill(x) * style[i], 9_000.0 * skill(x) * style[(i + 4) % 9]];
                let measured = |tp: f32| with_throughput.then_some(tp * skill(x));
                SmartTrial { throughputs: vec![measured(4.0), measured(5.0)], ..trial(x, &scores) }
            })
        };
        let throughput = play(true);
        assert!((throughput.peak_sens().ln() - 0.2).abs() < 0.02, "{}", throughput.peak_sens());
        assert!(throughput.noise().unwrap() < 0.025, "{:?}", throughput.noise());
        let (lo, hi) = throughput.best_range();
        // 0.8 (x - 0.2)^2 <= 0.03 for x in 0.2 -+ 0.19.
        assert!((lo.ln() - 0.01).abs() < 0.06 && (hi.ln() - 0.39).abs() < 0.06, "{} {}", lo.ln(), hi.ln());
        // On scores alone the playing style drowns the curve: the model sees mostly noise and
        // is far less sure where the best is.
        let score = play(false);
        assert!(score.noise().unwrap() >= 0.08, "{:?}", score.noise());
        assert!(score.confidence().unwrap() < throughput.confidence().unwrap());
    }

    #[test]
    fn click_bias_breaks_the_tie_inside_the_best_range() {
        // No landing data; clicks land on the centre at x = 0.15 (past it above, short below).
        let biased = finished_trials(FinderFocus::Balanced, |_, x| SmartTrial {
            bias: Some(0.8 * (x - 0.15)),
            ..trial(x, &flat_top(x))
        });
        assert!(biased.uses_precision());
        assert!((biased.recommendation().ln() - 0.15).abs() < 0.01, "{}", biased.recommendation());
        assert!((biased.bias_sens().unwrap().ln() - 0.15).abs() < 0.01);
        assert!((biased.bias_at_start().unwrap() + 0.12).abs() < 1e-3);
        assert_eq!(biased.landing_sens(), None);

        // Both clear: landings on target at x = 0.05, clicks centred at 0.25; they're averaged.
        let both = finished_trials(FinderFocus::Balanced, |_, x| SmartTrial {
            landing: Some(1.0 + 0.5 * (x - 0.05)),
            bias: Some(0.8 * (x - 0.25)),
            ..trial(x, &flat_top(x))
        });
        assert!((both.recommendation().ln() - 0.15).abs() < 0.01, "{}", both.recommendation());
        assert!((both.precision_sens().unwrap().ln() - 0.15).abs() < 0.01);

        // Biases without a clear trend are ignored, like landings.
        let jitter = [0.9, -0.6, 1.0, -1.0, 0.3, -0.2, 0.7, -0.9, 0.1];
        let noisy = finished_trials(FinderFocus::Balanced, |i, x| SmartTrial {
            bias: Some(0.3 * jitter[i]),
            ..trial(x, &flat_top(x))
        });
        assert!(!noisy.uses_precision());
        assert_eq!(noisy.recommendation(), 1.0);
    }

    #[test]
    fn expected_improvement_picks_informative_trials() {
        // Opening: the high end clearly best, the low end clearly worst. The next trial goes
        // above the start, toward the promising end, and never onto a tested sens.
        let mut finder = SmartFinder::with_rng(1.0, FinderFocus::Tracking, &mut fastrand::Rng::with_seed(0));
        let skill = |x: f32| 1.0 + 0.3 * x;
        for _ in 0..OPENING {
            let x = finder.current_sens().ln();
            play(&mut finder, &[1000.0 * skill(x)], &[], 3);
        }
        let next = finder.current_sens().ln();
        assert!(next > 0.15, "{next}");
        // Peak in the middle of the opening: the next trial refines next to it, not at the ends.
        let mut finder = SmartFinder::with_rng(1.0, FinderFocus::Tracking, &mut fastrand::Rng::with_seed(0));
        for _ in 0..OPENING {
            let x = finder.current_sens().ln();
            play(&mut finder, &[1000.0 * (1.0 - 0.8 * x * x)], &[], 3);
        }
        let next = finder.current_sens().ln();
        assert!(next.abs() >= MIN_SPACING - 1e-6 && next.abs() < 0.3, "{next}");
        // Over whole runs, every model-picked trial keeps its distance from the earlier ones.
        for seed in 0..10 {
            let finder = Player { peak: 0.1, curvature: 1.0, noise: 0.03, drift: 0.0 }.run(seed);
            let xs: Vec<f32> = finder.trials.iter().map(|t| t.sens.ln()).collect();
            for i in OPENING..xs.len() {
                assert!(xs[..i].iter().all(|&x| (x - xs[i]).abs() >= MIN_SPACING - 1e-6), "seed {seed}: {xs:?}");
            }
        }
    }

    /// Plays `seeds` finders, against `player(seed)` each: the fraction that `pass`, and the mean
    /// trial count.
    fn over_seeds(
        seeds: u64,
        player: impl Fn(u64) -> Player,
        pass: impl Fn(&Player, &SmartFinder) -> bool,
    ) -> (f32, f32) {
        let (mut passed, mut trials) = (0, 0);
        for seed in 0..seeds {
            let player = player(seed);
            let finder = player.run(seed);
            passed += usize::from(pass(&player, &finder));
            trials += finder.trials.len();
        }
        (passed as f32 / seeds as f32, trials as f32 / seeds as f32)
    }

    /// The recommendation performs within `PLATEAU` of the player's true best.
    fn in_plateau(player: &Player, finder: &SmartFinder) -> bool {
        player.in_plateau(finder.recommendation().ln())
    }

    #[test]
    fn converges_inside_the_true_plateau_of_noisy_players() {
        // Peaks spread over the inner range (about half of them with the start outside the
        // plateau), 3% trial noise, mild warm-up.
        let player = |seed| {
            let peak = -0.35 + 0.65 * fastrand::Rng::with_seed(1000 + seed).f32();
            Player { peak, curvature: 1.0, noise: 0.03, drift: 0.005 }
        };
        let (rate, trials) = over_seeds(100, player, in_plateau);
        println!("noisy players: {:.0}% inside the plateau, {trials:.1} trials on average", rate * 100.0);
        assert!(rate >= 0.9, "{rate}");
    }

    #[test]
    fn flat_noisy_players_keep_their_sens() {
        // 5% trial noise, more than the 3% that counts as a difference.
        let player = |_| Player { peak: 0.0, curvature: 0.0, noise: 0.05, drift: 0.0 };
        let (rate, trials) = over_seeds(100, player, |_, finder| finder.recommendation() == 1.0);
        println!("flat players: {:.0}% kept, {trials:.1} trials on average", rate * 100.0);
        assert!(rate >= 0.9, "{rate}");
    }

    #[test]
    fn strong_drift_does_not_bias_the_peak() {
        // +4% per trial (a 12-trial session gains 44%) on top of 2% noise.
        let mut errors = Vec::new();
        for seed in 0..50 {
            let player = Player { peak: 0.9f32.ln(), curvature: 1.0, noise: 0.02, drift: 0.04 };
            errors.push(player.run(seed).peak_sens().ln() - player.peak);
        }
        let bias = errors.iter().sum::<f32>() / errors.len() as f32;
        let worst = errors.iter().fold(0.0f32, |w, e| w.max(e.abs()));
        println!("drift: mean peak error {bias:+.3}, worst {worst:.3} (ln-sens)");
        assert!(bias.abs() < 0.03 && worst < 0.1, "{bias} {worst}");
    }

    #[test]
    fn stops_early_on_easy_cases() {
        // A clear peak and little noise: done before the old fixed schedule's 9 trials, whether
        // the start is clearly worse (peak 28% above it) or inside the plateau (peak 5% above it).
        for peak in [0.25, 0.05] {
            let player = |_| Player { peak, curvature: 1.5, noise: 0.01, drift: 0.0 };
            let (rate, trials) = over_seeds(50, player, in_plateau);
            println!(
                "easy players (peak {peak}): {:.0}% inside the plateau, {trials:.1} trials on average",
                rate * 100.0
            );
            assert!(rate >= 0.95 && trials < 8.0, "{rate} {trials}");
        }
    }
}
