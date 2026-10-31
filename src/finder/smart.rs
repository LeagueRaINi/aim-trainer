//! Smart sensitivity finder: models performance as a function of sensitivity instead of
//! bisecting on gut feel alone.
//!
//! 1. Probe: blind trials at 5 log-spaced sensitivities from 0.6x to 1.6x your current one, in
//!    random order. Each trial is a battery of drills at the same sensitivity, picked by the
//!    finder's focus (see [`FinderFocus`]): the best sensitivity depends on the task, so it's
//!    found for the kind of aiming you care about. With several drills the order alternates
//!    between trials to balance order effects, and the first `WARM_IN` seconds of each part
//!    aren't scored (you adapt quickly right after a sens change).
//! 2. After every trial you rate how it felt (1-5). Each trial's objective is the mean, over its
//!    drills, of its result relative to that drill's average result, nudged by the feel rating.
//!    A flick drill's result is its effective throughput (TPe, see
//!    [`throughput`](crate::metrics::throughput)) when the part measured one, else its score:
//!    TPe barely moves when you happen to play one trial faster and sloppier and the next one
//!    slower and more careful, which a score does. Tracking uses its score.
//! 3. Fit `objective = a*x^2 + b*x + c + d*trial` by least squares, where `x = ln(sens / start)`.
//!    The `d*trial` term absorbs warm-up drift (later trials tend to score higher no matter the
//!    sensitivity), which would otherwise bias the result toward whatever was tested last.
//! 4. Refine: more trials around the fitted peak, then refit.
//! 5. Recommend: performance is flat over a broad plateau around the peak (NVIDIA, CHI 2022),
//!    and short trials are noisy, so the plateau is every sensitivity whose predicted result is
//!    within the trial noise (at least 3%) of the peak. Inside it, prefer the sensitivity where
//!    your aim is on target: where first flicks land on target (median ratio of first-submovement
//!    travel to target distance) and where clicks land on the target centre along the flick
//!    (mean click bias, in target radii), each fitted as a line in `x` and used only if that
//!    trend is statistically clear; the average of the two when both are. Otherwise keep the
//!    plateau sensitivity closest to your current one: no change unless the data shows one helps.

use bevy::prelude::*;

use crate::{
    drill::{DrillKind, DrillSpec, Run, RunPurpose, RunStats},
    metrics::throughput,
    settings::{FinderFocus, Settings},
};

/// Relative range probed around the starting sensitivity.
const PROBE_LOW: f32 = 0.6;
const PROBE_HIGH: f32 = 1.6;
pub const PROBES: usize = 5;
pub const REFINEMENTS: usize = 4;
/// How much one feel step (on a 1-5 scale) shifts a trial's objective, relative to its score.
const FEEL_WEIGHT: f32 = 0.06;
/// Offsets (in ln-space, ~ +-6%) applied to refinement trials so the peak region gets sampled
/// on both sides instead of only at the current estimate.
const REFINE_OFFSETS: [f32; REFINEMENTS] = [0.0, -0.06, 0.06, 0.0];
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
/// Smallest objective tolerance of the performance plateau (relative to the peak); noisier
/// data widens it to the trial noise.
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

/// Quadratic fit in ln-space plus a linear trial-order term.
#[derive(Clone, Copy, Debug)]
pub struct Fit {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
}

impl Fit {
    /// Predicted objective at relative log-sens `x`, at a given trial index.
    fn predict(&self, x: f32, trial: f32) -> f32 {
        self.a * x * x + self.b * x + self.c + self.d * trial
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
    /// Probe sensitivities still to play (popped from the back).
    probes: Vec<f32>,
    pub trials: Vec<SmartTrial>,
    pending: Pending,
    pub step: SmartStep,
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
        let (lo, hi) = (PROBE_LOW.ln(), PROBE_HIGH.ln());
        let mut probes: Vec<f32> = (0..PROBES)
            .map(|i| start_sens * (lo + (hi - lo) * i as f32 / (PROBES - 1) as f32).exp())
            .collect();
        fastrand::shuffle(&mut probes);
        Self { start_sens, drills: focus_drills(focus), probes, trials: Vec::new(), pending: Pending::default(), step: SmartStep::Trial }
    }

    pub fn total_trials(&self) -> usize {
        PROBES + REFINEMENTS
    }

    /// Sensitivity for the trial in progress. Deterministic, so it's the same when the trial
    /// starts and when its result is recorded.
    pub fn current_sens(&self) -> f32 {
        if let Some(&probe) = self.probes.last() {
            return probe;
        }
        let refinement = self.trials.len().saturating_sub(PROBES).min(REFINEMENTS - 1);
        let x = self.optimum_x() + REFINE_OFFSETS[refinement];
        self.start_sens * x.clamp(PROBE_LOW.ln(), PROBE_HIGH.ln()).exp()
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
            "Finder trial {}/{} - part {}/{}: {}",
            self.trials.len() + 1,
            self.total_trials(),
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

    /// Stores the finished trial with its feel rating (1-5).
    pub fn rate(&mut self, feel: u8) {
        if self.step != SmartStep::Rate {
            return;
        }
        let Pending { scores, throughputs, landings, biases, .. } = std::mem::take(&mut self.pending);
        let sens = self.current_sens();
        self.probes.pop();
        let bias = (biases.len() >= MIN_CLICKS).then(|| biases.iter().sum::<f32>() / biases.len() as f32);
        let landing = median(landings);
        self.trials.push(SmartTrial { sens, scores, throughputs, landing, bias, feel: feel.clamp(1, 5) });
        self.step = if self.trials.len() >= self.total_trials() { SmartStep::Done } else { SmartStep::Trial };
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
        let throughput_means: Vec<f32> =
            (0..self.drills.len()).map(|d| mean(self.trials.iter().filter_map(|t| t.throughputs[d]).collect())).collect();
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

    /// Least-squares fit, once there are enough trials to determine all four terms.
    pub fn fit(&self) -> Option<Fit> {
        if self.trials.len() < 5 {
            return None;
        }
        let rows: Vec<[f32; 4]> = self
            .trials
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let x = self.x(t.sens);
                [x * x, x, 1.0, i as f32]
            })
            .collect();
        let [a, b, c, d] = least_squares(&rows, &self.objectives())?;
        Some(Fit { a, b, c, d })
    }

    /// Best relative log-sens for performance: the fitted peak if the curve has one inside the
    /// probed range, otherwise the best single trial.
    fn optimum_x(&self) -> f32 {
        let (lo, hi) = (PROBE_LOW.ln(), PROBE_HIGH.ln());
        if let Some(fit) = self.fit()
            && fit.a < -1e-4
        {
            return (-fit.b / (2.0 * fit.a)).clamp(lo, hi);
        }
        let objectives = self.objectives();
        self.trials
            .iter()
            .zip(&objectives)
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0.0, |(t, _)| self.x(t.sens))
    }

    /// Sensitivity with the best predicted performance.
    pub fn peak_sens(&self) -> f32 {
        self.start_sens * self.optimum_x().exp()
    }

    /// Typical scatter of a trial's objective around the fitted curve (residual standard
    /// deviation). Differences smaller than this are indistinguishable from luck.
    fn noise(&self) -> f32 {
        let Some(fit) = self.fit() else { return 0.0 };
        let objectives = self.objectives();
        let rss: f32 = self
            .trials
            .iter()
            .zip(&objectives)
            .enumerate()
            .map(|(i, (t, y))| (y - fit.predict(self.x(t.sens), i as f32)).powi(2))
            .sum();
        // Four fitted terms.
        (rss / (self.trials.len() as f32 - 4.0).max(1.0)).sqrt()
    }

    /// Sensitivities whose predicted performance can't be told apart from the peak: within
    /// `PLATEAU` of it, or within the trial noise when the data is noisier than that. Just the
    /// best trial before there is a fit.
    pub fn plateau(&self) -> (f32, f32) {
        if let Some(range) = self.good_range(PLATEAU.max(self.noise())) {
            return range;
        }
        let best = self.peak_sens();
        (best, best)
    }

    /// The plateau spans (almost) the whole probed range: within it, sensitivity made no
    /// measurable difference.
    pub fn is_flat(&self) -> bool {
        let (lo, hi) = self.plateau();
        lo <= self.start_sens * PROBE_LOW * 1.02 && hi >= self.start_sens * PROBE_HIGH * 0.98
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
        self.landing_x().map(|x| self.start_sens * x.exp())
    }

    /// Sensitivity whose clicks are predicted to land on the target centre along the flick.
    pub fn bias_sens(&self) -> Option<f32> {
        self.bias_x().map(|x| self.start_sens * x.exp())
    }

    /// Sensitivity where your aim is on target (see [`Self::recommendation`]).
    pub fn precision_sens(&self) -> Option<f32> {
        self.precision_x().map(|x| self.start_sens * x.exp())
    }

    /// Predicted first-flick landing ratio at the starting sensitivity.
    pub fn landing_at_start(&self) -> Option<f32> {
        self.landing_fit().map(|line| line.intercept)
    }

    /// Predicted click bias (target radii) at the starting sensitivity.
    pub fn bias_at_start(&self) -> Option<f32> {
        self.bias_fit().map(|line| line.intercept)
    }

    /// Whether landing or click-bias data picked the recommendation inside the plateau.
    pub fn uses_precision(&self) -> bool {
        self.precision_x().is_some()
    }

    /// Inside the plateau, the sensitivity where your aim is clearly on target (first flicks
    /// land on target, clicks land on the centre); otherwise the plateau sensitivity closest to
    /// the one you started with. Changing sensitivity costs re-adaptation, so it's only worth it
    /// when the data shows a real difference: if your current sensitivity is already in the
    /// plateau, it is the recommendation.
    pub fn recommendation(&self) -> f32 {
        let (lo, hi) = self.plateau();
        let target = self.precision_x().unwrap_or(0.0);
        self.start_sens * target.clamp(self.x(lo), self.x(hi)).exp()
    }

    /// Fitted curve across the probed range as (sens, predicted objective), evaluated at the
    /// average trial index so warm-up drift is factored out.
    pub fn curve(&self, points: usize) -> Vec<(f32, f32)> {
        let Some(fit) = self.fit() else { return Vec::new() };
        let mid_trial = (self.trials.len().saturating_sub(1)) as f32 * 0.5;
        let (lo, hi) = (PROBE_LOW.ln(), PROBE_HIGH.ln());
        (0..points)
            .map(|i| {
                let x = lo + (hi - lo) * i as f32 / (points - 1).max(1) as f32;
                (self.start_sens * x.exp(), fit.predict(x, mid_trial))
            })
            .collect()
    }

    /// Sensitivities whose predicted objective is within `tolerance` of the peak: the "anything
    /// in here is fine" band.
    fn good_range(&self, tolerance: f32) -> Option<(f32, f32)> {
        let curve = self.curve(200);
        let best = curve.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        let inside: Vec<f32> = curve.iter().filter(|p| p.1 >= best - tolerance).map(|p| p.0).collect();
        Some((*inside.first()?, *inside.last()?))
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

/// Solves the normal equations for an n x 4 design matrix. `None` if the system is singular.
fn least_squares(rows: &[[f32; 4]], y: &[f32]) -> Option<[f32; 4]> {
    // Build [A^T A | A^T y] in f64 for stability.
    let mut m = [[0.0f64; 5]; 4];
    for (row, &target) in rows.iter().zip(y) {
        for i in 0..4 {
            for j in 0..4 {
                m[i][j] += f64::from(row[i]) * f64::from(row[j]);
            }
            m[i][4] += f64::from(row[i]) * f64::from(target);
        }
    }
    // Gaussian elimination with partial pivoting.
    for col in 0..4 {
        let pivot = (col..4).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[pivot][col].abs() < 1e-9 {
            return None;
        }
        m.swap(col, pivot);
        for r in 0..4 {
            if r != col {
                let factor = m[r][col] / m[col][col];
                for c in col..5 {
                    m[r][c] -= factor * m[col][c];
                }
            }
        }
    }
    Some(std::array::from_fn(|i| (m[i][4] / m[i][i]) as f32))
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

    /// Plays a finder to completion against a synthetic player whose score peaks at `best` in
    /// both drills (Tracking on a 9x larger scale) and who improves by `drift` per trial.
    fn simulate(best: f32, drift: f32) -> SmartFinder {
        let mut finder = SmartFinder::new(1.0, FinderFocus::Balanced);
        while finder.step != SmartStep::Done {
            let i = finder.trials.len() as f32;
            let x = (finder.current_sens() / best).ln();
            let score = 1000.0 * (1.0 - 0.8 * x * x) + drift * i;
            play(&mut finder, &[score, 9.0 * score], &[], 3);
        }
        finder
    }

    /// The relative log-sens of the 9 trials of [`finished_trials`], in play order: they cover
    /// the probed range, shuffled so trial index and sensitivity aren't collinear.
    fn trial_xs() -> impl Iterator<Item = f32> {
        let (lo, hi) = (PROBE_LOW.ln(), PROBE_HIGH.ln());
        [4, 0, 7, 2, 8, 1, 5, 3, 6].into_iter().map(move |i| lo + (hi - lo) * i as f32 / 8.0)
    }

    /// A finished finder with the given focus and one trial per [`trial_xs`] (trial index, x).
    fn finished_trials(focus: FinderFocus, mut trial: impl FnMut(usize, f32) -> SmartTrial) -> SmartFinder {
        let mut finder = SmartFinder::new(1.0, focus);
        finder.trials = trial_xs().enumerate().map(|(i, x)| trial(i, x)).collect();
        finder.probes.clear();
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
    fn probes_cover_the_range_once_each() {
        let mut finder = SmartFinder::new(0.5, FinderFocus::Balanced);
        let mut seen = Vec::new();
        for _ in 0..PROBES {
            seen.push(finder.current_sens());
            play(&mut finder, &[100.0, 100.0], &[], 3);
        }
        seen.sort_by(f32::total_cmp);
        assert!((seen[0] - 0.3).abs() < 1e-4 && (seen[PROBES - 1] - 0.8).abs() < 1e-4);
        assert_eq!(finder.trials.len(), PROBES);
    }

    #[test]
    fn trials_play_both_drills_in_alternating_order() {
        let settings = Settings::default();
        let mut finder = SmartFinder::new(1.0, FinderFocus::Balanced);
        finder.begin_trial();
        let first = finder.segment_run(&settings).unwrap();
        assert_eq!(first.kind(), DrillKind::Microflick);
        assert_eq!(first.title, "Finder trial 1/9 - part 1/2: Microflick");
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
        assert_eq!(flicking.segment_run(&settings).unwrap().title, "Finder trial 1/9 - part 1/2: Gridshot");
        play(&mut flicking, &[30_000.0, 9_000.0], &[], 3);
        assert_eq!(flicking.segment_order(), [DrillKind::Microflick, DrillKind::Gridshot]);
        // Tracking only: one part per trial, rated right after it.
        let mut tracking = SmartFinder::new(1.0, FinderFocus::Tracking);
        assert_eq!(tracking.segment_run(&settings).unwrap().title, "Finder trial 1/9 - part 1/1: Tracking");
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
        assert!(finder.fit().unwrap().a < 0.0);
        assert!((finder.peak_sens() - 1.2).abs() < 0.02, "{}", finder.peak_sens());
        assert_eq!(finder.trials.len(), PROBES + REFINEMENTS);
    }

    #[test]
    fn keeps_the_current_sens_when_it_is_already_near_best() {
        // Peak 10% above the start: the start still scores within 3% of it, so don't change.
        let finder = simulate(1.1, 0.0);
        let (lo, hi) = finder.plateau();
        assert!(lo < 1.0 && hi > 1.1, "{lo} {hi}");
        assert_eq!(finder.recommendation(), 1.0);
    }

    #[test]
    fn moves_only_as_far_as_the_best_range_when_the_start_is_clearly_worse() {
        let finder = simulate(1.45, 0.0);
        let (lo, hi) = finder.plateau();
        assert!(lo > 1.0, "{lo} {hi}");
        assert!((finder.recommendation() - lo).abs() < 1e-4, "{}", finder.recommendation());
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
    }

    #[test]
    fn drills_are_normalised_so_neither_dominates() {
        // Microflick peaks at 0.2 below the start, Tracking 0.2 above, on a 10x larger scale.
        // Raw scores would put the peak near +0.16; normalised, the drills weigh equally.
        let finder = finished(|x| {
            ([100.0 * (1.0 - 0.8 * (x + 0.2).powi(2)), 1000.0 * (1.0 - 0.8 * (x - 0.2).powi(2))], None)
        });
        assert!(finder.fit().unwrap().a < 0.0);
        assert!(finder.peak_sens().ln().abs() < 0.03, "{}", finder.peak_sens());
    }

    #[test]
    fn recommends_the_plateau_sens_where_flicks_land_on_target() {
        // Landing on target at x = 0.15, inside the plateau.
        let finder = finished(|x| (flat_top(x), Some(1.0 + 0.5 * (x - 0.15))));
        let (lo, hi) = finder.plateau();
        assert!(lo < 0.8 && hi > 1.25, "{lo} {hi}");
        assert!((finder.peak_sens() - 1.0).abs() < 0.01, "{}", finder.peak_sens());
        assert!((finder.recommendation().ln() - 0.15).abs() < 0.01, "{}", finder.recommendation());
        assert!((finder.landing_sens().unwrap().ln() - 0.15).abs() < 0.01);
        assert!((finder.landing_at_start().unwrap() - 0.925).abs() < 0.01);
        assert!(finder.uses_precision());

        // Landing on target far above the plateau: the plateau's top edge is as close as it gets.
        let finder = finished(|x| (flat_top(x), Some(1.0 + 0.5 * (x - 0.6))));
        assert!((finder.recommendation() - finder.plateau().1).abs() < 1e-4);
        assert!(finder.landing_sens().unwrap() > finder.recommendation());
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
        let mut finder = SmartFinder::new(1.0, FinderFocus::Balanced);
        while finder.step != SmartStep::Done {
            let sens = finder.current_sens();
            // Scores identical everywhere, but high sens feels much better.
            play(&mut finder, &[1000.0, 5000.0], &[], if sens > 1.2 { 5 } else { 2 });
        }
        assert!(finder.recommendation() > 1.2);
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
        assert!((throughput.peak_sens().ln() - 0.2).abs() < 0.01, "{}", throughput.peak_sens());
        assert!(throughput.noise() < 1e-3);
        // On scores alone the playing style drowns the curve.
        let score = play(false);
        assert!(score.noise() > 0.1, "{}", score.noise());
        assert!((score.peak_sens().ln() - 0.2).abs() > 0.05, "{}", score.peak_sens());
    }

    #[test]
    fn click_bias_breaks_the_tie_inside_the_plateau() {
        // No landing data; clicks land on the centre at x = 0.15 (past it above, short below).
        let biased = finished_trials(FinderFocus::Balanced, |_, x| SmartTrial { bias: Some(0.8 * (x - 0.15)), ..trial(x, &flat_top(x)) });
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
        let noisy = finished_trials(FinderFocus::Balanced, |i, x| SmartTrial { bias: Some(0.3 * jitter[i]), ..trial(x, &flat_top(x)) });
        assert!(!noisy.uses_precision());
        assert_eq!(noisy.recommendation(), 1.0);
    }

    #[test]
    fn solves_exact_linear_system() {
        let rows = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]];
        let y = [1.0, 2.0, 3.0, 4.0, 10.0];
        let solution = least_squares(&rows, &y).unwrap();
        for (got, want) in solution.iter().zip([1.0, 2.0, 3.0, 4.0]) {
            assert!((got - want).abs() < 1e-4);
        }
        assert!(least_squares(&[[0.0; 4]; 5], &[1.0; 5]).is_none());
    }
}
