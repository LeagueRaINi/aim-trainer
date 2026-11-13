//! The continuous monitor: a small summary of every practice run of a flick drill, kept in
//! `aim_trainer_monitor.csv` (only the latest replays are kept, so the flicks themselves are
//! gone later), and the evidence gate that decides whether those summaries justify suggesting
//! one small step of sensitivity. Silence is the default. The reasoning behind every threshold
//! is in [`limits`](super::limits).

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use super::limits::Limits;
use super::stats::{Summary, Welch, daily_means, day_of, t_critical};
use crate::analysis::{self, Flick};
use crate::drill::{DrillKind, RunPurpose, RunStats};
use crate::history::{RunRecorded, append_csv_row, csv_safe};
use crate::metrics::median;
use crate::replay::Recorder;

pub const MONITOR_PATH: &str = "aim_trainer_monitor.csv";
const HEADER: &str = "unix_time,drill,kind,sens,cm_per_360,flicks,landing,bias,click_deg,corrections,misses,score";

/// Runs, measured flicks and separate days the current sensitivity needs before anything is
/// said about it.
pub const MIN_RUNS: usize = 20;
pub const MIN_FLICKS: u32 = 500;
pub const MIN_DAYS: usize = 4;
/// The size of a suggested step: 5% of the sensitivity.
pub const STEP: f32 = 0.05;
/// Only the latest runs at the current sensitivity are looked at: a rolling picture, so a
/// habit trained away months ago stops counting.
const WINDOW: usize = 60;
/// Sensitivities whose cm/360 differ by less than this factor are the same one (the log keeps
/// 3 decimals).
const SAME_SENS: f32 = 1.002;
/// Flicks with a value (landing, click) a run needs for that value to count.
const MIN_RUN_FLICKS: usize = 8;
/// How many standard errors of the player's own run-to-run scatter a pattern's average must
/// stand above half of the effect that matters (and never less than the 1% critical value for
/// the number of runs): the estimate must reach the threshold, and noise around a small real
/// bias must not be able to carry it there.
const CLEAR_T: f32 = 4.0;
/// Share of the days at this sensitivity that must show the pattern.
const DAY_SHARE: f32 = 0.8;
/// First flicks travelling this much too far (median landing ratio above 1 by it) are an
/// overshoot worth acting on.
const OVERSHOOT: f32 = 0.05;
/// First flicks falling this much short are an undershoot worth acting on. Larger than
/// [`OVERSHOOT`]: a first flick that stops a little short and is finished by a small
/// correction is how practised aiming normally looks, not a sign of a slow sensitivity.
const UNDERSHOOT: f32 = 0.12;
/// First clicks landing this many target radii past the centre along the flick, on average,
/// are an overshoot too. Only this side counts: clicking as the crosshair enters the target,
/// short of its centre, is efficient aiming, not a sign of anything.
const PAST_CENTRE: f32 = 0.15;
/// Corrections per kill from which the end of a flick counts as shaky, when the first flick
/// itself is not short (see [`SHORT`]): the aim arrives and then wobbles. More than one
/// corrective movement for every kill; a practised player's logs show 0.0 to 0.3 on Gridshot,
/// Microflick and Head level.
const SHAKY: f32 = 1.0;
/// First flicks short by more than this explain corrections on their own.
const SHORT: f32 = 0.04;
/// Runs further apart than this belong to different sessions, seconds.
const SESSION_GAP: u64 = 20 * 60;
/// A session must span this long and have this many runs in each half for its fade to count.
const FADE_SPAN: u64 = 15 * 60;
const FADE_HALF_RUNS: usize = 3;
/// Sessions with a measured fade needed, and the fade that matters: click error 10% larger in
/// the late half (as a difference of logarithms).
const FADE_SESSIONS: usize = 6;
const FADE: f32 = 0.0953;
/// Runs of a drill needed for its average click error to be a reference.
const DRILL_RUNS: usize = 3;
/// Runs at the previous sensitivity needed to check a change against, and runs right after a
/// change that are left out as adaptation.
const REVIEW_RUNS: usize = 10;
const ADAPT_RUNS: usize = 5;

/// What one finished run of a flick drill measured. Values a run didn't measure (too few
/// flicks) are `None`, and an empty field in the file.
#[derive(Clone, Debug, PartialEq)]
pub struct RunSummary {
    /// The run's history timestamp. Time into the session is worked out from these.
    pub unix_time: u64,
    /// The run's history name.
    pub drill: String,
    pub kind: DrillKind,
    pub sens: f32,
    pub cm_per_360: f32,
    /// Flicks found in the run.
    pub flicks: u32,
    /// Median first-flick landing ratio (1 = on target, above = overshoot).
    pub landing: Option<f32>,
    /// Mean first-click position along the flick, target radii (above 0 = past the centre).
    pub bias: Option<f32>,
    /// Mean distance of the first clicks from the target centre, degrees.
    pub click_deg: Option<f32>,
    /// Corrective movements per kill.
    pub corrections: Option<f32>,
    /// Missed shots per flick.
    pub misses: f32,
    pub score: f32,
}

impl RunSummary {
    /// Summarises a run's flicks; `None` for a run without any.
    pub fn measure(unix_time: u64, stats: &RunStats, flicks: &[Flick]) -> Option<Self> {
        if flicks.is_empty() {
            return None;
        }
        let enough = |values: &[f32]| values.len() >= MIN_RUN_FLICKS;
        let mean = |values: &[f32]| values.iter().sum::<f32>() / values.len() as f32;
        let mut landings: Vec<f32> = flicks.iter().filter_map(|f| f.landing).collect();
        let offsets: Vec<Vec2> = flicks.iter().filter_map(|f| f.click?.task_offset()).collect();
        let radii: Vec<f32> = flicks.iter().filter_map(|f| Some(f.click?.normalized()?.x)).collect();
        let distances: Vec<f32> = offsets.iter().map(|offset| offset.length()).collect();
        let corrections: Vec<f32> = flicks.iter().filter(|f| f.ttk.is_some()).map(|f| f.corrections as f32).collect();
        Some(Self {
            unix_time,
            drill: csv_safe(&stats.drill),
            kind: stats.kind,
            sens: stats.sens,
            cm_per_360: stats.cm_per_360,
            flicks: flicks.len() as u32,
            landing: if enough(&landings) { median(&mut landings) } else { None },
            bias: enough(&radii).then(|| mean(&radii)),
            click_deg: enough(&distances).then(|| mean(&distances)),
            corrections: enough(&corrections).then(|| mean(&corrections)),
            misses: flicks.iter().map(|f| f.misses as f32).sum::<f32>() / flicks.len() as f32,
            score: stats.score(),
        })
    }

    fn row(&self) -> String {
        let optional = |value: Option<f32>| value.map_or(String::new(), |v| format!("{v:.4}"));
        format!(
            "{},{},{},{},{:.3},{},{},{},{},{},{:.4},{}",
            self.unix_time,
            self.drill,
            self.kind.name(),
            self.sens,
            self.cm_per_360,
            self.flicks,
            optional(self.landing),
            optional(self.bias),
            optional(self.click_deg),
            optional(self.corrections),
            self.misses,
            self.score
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let cols: Vec<&str> = line.split(',').collect();
        let [unix_time, drill, kind, sens, cm_per_360, flicks, landing, bias, click_deg, corrections, misses, score] =
            cols[..]
        else {
            return None;
        };
        let number = |text: &str| text.parse::<f32>().ok().filter(|v| v.is_finite());
        // An empty field is a value the run didn't measure; anything else must be a number.
        let optional = |text: &str| if text.is_empty() { Some(None) } else { number(text).map(Some) };
        Some(Self {
            unix_time: unix_time.parse().ok()?,
            drill: drill.to_string(),
            kind: DrillKind::from_name(kind)?,
            sens: number(sens)?,
            cm_per_360: number(cm_per_360).filter(|v| *v > 0.0)?,
            flicks: flicks.parse().ok()?,
            landing: optional(landing)?,
            bias: optional(bias)?,
            click_deg: optional(click_deg)?,
            corrections: optional(corrections)?,
            misses: number(misses)?,
            score: number(score)?,
        })
    }
}

/// Every run summary so far, oldest first.
#[derive(Resource, Default)]
pub struct MonitorLog {
    entries: Vec<RunSummary>,
    /// The CSV file summaries are appended to; `None` for a log that only lives in memory.
    path: Option<PathBuf>,
}

impl MonitorLog {
    /// Reads the summaries kept in the CSV at `path` (empty if there is none yet).
    pub fn load(path: &Path) -> Self {
        let entries = fs::read_to_string(path)
            .map(|text| text.lines().skip(1).filter_map(RunSummary::parse).collect())
            .unwrap_or_default();
        Self { entries, path: Some(path.to_path_buf()) }
    }

    /// A log of these summaries that is never written anywhere: for staged screenshots.
    pub fn in_memory(entries: Vec<RunSummary>) -> Self {
        Self { entries, path: None }
    }

    pub fn record(&mut self, summary: &RunSummary) {
        let row = summary.row();
        if let Some(path) = &self.path
            && let Err(err) = append_csv_row(path, HEADER, &row)
        {
            error!("Failed to write {}: {err}", path.display());
        }
        self.entries.extend(RunSummary::parse(&row));
    }

    pub fn entries(&self) -> &[RunSummary] {
        &self.entries
    }
}

/// Summarises every finished practice run of a flick drill from its recording, before the
/// replay handler takes the frames.
pub(crate) fn record_monitor(
    mut recorded: MessageReader<RunRecorded>,
    recorder: Res<Recorder>,
    mut log: ResMut<MonitorLog>,
) {
    for recorded in recorded.read().filter(|r| r.run.purpose.logged_as() == RunPurpose::Practice) {
        let stats = &recorded.run.stats;
        if !stats.kind.def().flick_analysis {
            continue;
        }
        let flicks = analysis::flicks_in(stats.kind, recorder.frames());
        if let Some(summary) = RunSummary::measure(recorded.timestamp, stats, &flicks) {
            log.record(&summary);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The evidence gate
// ---------------------------------------------------------------------------------------------

/// How much data the current sensitivity has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub runs: usize,
    pub flicks: u32,
    pub days: usize,
}

impl Progress {
    fn of(runs: &[RunSummary]) -> Self {
        let mut days: Vec<u64> = runs.iter().map(|r| day_of(r.unix_time)).collect();
        days.sort_unstable();
        days.dedup();
        Self { runs: runs.len(), flicks: runs.iter().map(|r| r.flicks).sum(), days: days.len() }
    }

    /// Whether the minimum of runs, flicks and days is there.
    pub fn enough(&self) -> bool {
        self.missing() == Progress::default()
    }

    /// What is still missing of each.
    pub fn missing(&self) -> Progress {
        Progress {
            runs: MIN_RUNS.saturating_sub(self.runs),
            flicks: MIN_FLICKS.saturating_sub(self.flicks),
            days: MIN_DAYS.saturating_sub(self.days),
        }
    }
}

/// A stable pattern in the runs at the current sensitivity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// First flicks travel past the target.
    Overshoot,
    /// First clicks land past the target's centre.
    PastCentre,
    /// First flicks fall well short of it.
    Undershoot,
    /// Flicks arrive, then need many corrections.
    Shaky,
    /// Click error grows in the late half of sessions.
    Fade,
}

impl Pattern {
    /// Whether the pattern asks for a slower sensitivity (more cm/360) or a faster one.
    pub fn slower(self) -> bool {
        self != Pattern::Undershoot
    }
}

/// A pattern that passed the gate, with the numbers to say so.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finding {
    pub pattern: Pattern,
    /// The average the pattern rests on: landing ratio minus 1 (overshoot, undershoot), click
    /// position in target radii (past the centre), corrections per kill (shaky), late click
    /// error over early (fade).
    pub value: f32,
    /// Runs (sessions, for a fade) and days it was seen on.
    pub samples: usize,
    pub days: usize,
}

/// Why a pattern leads to no step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldBy {
    /// There is no calibration yet, so no range a step could be kept inside.
    NoRange,
    /// The calibration's limits cross: there is no range.
    EmptyRange,
    /// The current sensitivity is already outside the range: the range's own advice comes first.
    Outside,
    /// The step would cross the range's limit on that side.
    Limit,
}

/// How a change of sensitivity turned out, comparing the runs before it with the runs since.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Clicks land clearly closer (or scores are clearly higher) and nothing got clearly worse.
    Helped,
    /// Clicks land clearly further off, or scores are clearly lower with no gain in precision.
    Hurt,
    /// Clearly more precise and clearly lower scores: a trade only the player can judge.
    Mixed,
    /// No clear difference.
    Unclear,
}

/// The check of the latest change of sensitivity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Review {
    /// The sensitivity before the change, cm/360.
    pub from_cm: f32,
    pub outcome: Outcome,
    /// Click error since the change over before it (0.9 = 10% closer to the centre).
    pub click_error: f32,
    /// Score since the change over before it.
    pub score: f32,
}

/// What the monitor has to say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Advice {
    /// Not enough data at this sensitivity yet.
    Collecting,
    /// Enough data and no stable pattern: nothing to change.
    Quiet,
    /// A stable pattern and a step inside the range: try this sensitivity.
    Step { finding: Finding, to_cm: f32 },
    /// A stable pattern, but no step can be suggested.
    Held { finding: Finding, by: HeldBy },
    /// The latest change clearly did not help: go back.
    Revert { to_cm: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Status {
    pub progress: Progress,
    pub advice: Advice,
    /// How the latest change turned out, once both sides have enough runs.
    pub review: Option<Review>,
    /// The sensitivity played before the current one, if the log has it.
    pub previous_cm: Option<f32>,
}

fn same_sens(a: f32, b: f32) -> bool {
    a <= b * SAME_SENS && b <= a * SAME_SENS
}

/// The trailing runs at `cm_per_360` (the block at the current sensitivity, unbroken by any
/// other) and the block right before it.
fn blocks(runs: &[RunSummary], cm_per_360: f32) -> (&[RunSummary], &[RunSummary]) {
    let block_start = |end: usize| {
        let Some(last) = end.checked_sub(1) else { return 0 };
        let cm = runs[last].cm_per_360;
        runs[..end].iter().rposition(|r| !same_sens(r.cm_per_360, cm)).map_or(0, |i| i + 1)
    };
    let current_from = match runs.last() {
        Some(last) if same_sens(last.cm_per_360, cm_per_360) => block_start(runs.len()),
        _ => runs.len(),
    };
    let previous_from = block_start(current_from);
    (&runs[current_from..], &runs[previous_from..current_from])
}

/// Whether `values` (with their timestamps) show a pattern: on average at least `effect` above
/// 0, clear of their own scatter (see [`CLEAR_T`]), and present on the days they were played.
fn gate(values: &[(u64, f32)], effect: f32, min_samples: usize) -> Option<(Summary, usize)> {
    let summary = Summary::of(&values.iter().map(|v| v.1).collect::<Vec<_>>())?;
    let days = daily_means(values.iter().copied());
    if summary.n < min_samples || days.len() < MIN_DAYS || summary.mean < effect {
        return None;
    }
    let clear = summary.t_against(effect * 0.5)? >= CLEAR_T.max(t_critical((summary.n - 1) as f32));
    // Every one of the latest days, and most of all days.
    let showing = days.iter().filter(|day| day.1 > 0.0).count();
    let lately = days[days.len() - MIN_DAYS..].iter().all(|day| day.1 > 0.0);
    (clear && lately && showing as f32 >= DAY_SHARE * days.len() as f32).then_some((summary, days.len()))
}

/// Each session's fade: the mean of its late half minus the mean of its early half of click
/// error, as logarithms relative to the drill's own average (drills differ in target size).
fn fades(runs: &[RunSummary]) -> Vec<(u64, f32)> {
    let relative: Vec<Option<f32>> = runs
        .iter()
        .map(|run| {
            let own: Vec<f32> = runs.iter().filter(|r| r.drill == run.drill).filter_map(|r| r.click_deg).collect();
            let average = Summary::of(&own).filter(|s| s.n >= DRILL_RUNS && s.mean > 0.0)?.mean;
            run.click_deg.filter(|c| *c > 0.0).map(|c| (c / average).ln())
        })
        .collect();
    let mut fades = Vec::new();
    let mut start = 0;
    for end in 1..=runs.len() {
        if end < runs.len() && runs[end].unix_time - runs[end - 1].unix_time <= SESSION_GAP {
            continue;
        }
        let (first, last) = (runs[start].unix_time, runs[end - 1].unix_time);
        let middle = first + (last - first) / 2;
        let half = |late: bool| -> Vec<f32> {
            (start..end).filter(|&i| (runs[i].unix_time > middle) == late).filter_map(|i| relative[i]).collect()
        };
        let (early, late) = (half(false), half(true));
        if last - first >= FADE_SPAN && early.len().min(late.len()) >= FADE_HALF_RUNS {
            let mean = |values: &[f32]| values.iter().sum::<f32>() / values.len() as f32;
            fades.push((first, mean(&late) - mean(&early)));
        }
        start = end;
    }
    fades
}

/// The patterns that pass the gate in the runs at the current sensitivity.
fn findings(runs: &[RunSummary]) -> Vec<Finding> {
    let with = |value: fn(&RunSummary) -> Option<f32>| -> Vec<(u64, f32)> {
        runs.iter().filter_map(|r| Some((r.unix_time, value(r)?))).collect()
    };
    let landings = with(|r| r.landing.map(|l| l - 1.0));
    let mut found = Vec::new();
    let mut add = |pattern, gated: Option<(Summary, usize)>, value: fn(f32) -> f32| {
        if let Some((summary, days)) = gated {
            found.push(Finding { pattern, value: value(summary.mean), samples: summary.n, days });
        }
    };
    add(Pattern::Overshoot, gate(&landings, OVERSHOOT, MIN_RUNS), |mean| mean);
    add(Pattern::PastCentre, gate(&with(|r| r.bias), PAST_CENTRE, MIN_RUNS), |mean| mean);
    let short: Vec<(u64, f32)> = landings.iter().map(|&(t, l)| (t, -l)).collect();
    add(Pattern::Undershoot, gate(&short, UNDERSHOOT, MIN_RUNS), |mean| -mean);
    // Corrections only count as shaky when the first flick isn't short: a short flick needs
    // its corrections, and a faster sensitivity, not a slower one, would save them.
    let landing = Summary::of(&landings.iter().map(|l| l.1).collect::<Vec<_>>());
    if landing.is_none_or(|l| l.mean >= -SHORT) {
        let excess = with(|r| r.corrections.map(|c| c - SHAKY));
        add(Pattern::Shaky, gate(&excess, 0.0, MIN_RUNS), |mean| mean + SHAKY);
    }
    add(Pattern::Fade, gate(&fades(runs), FADE, FADE_SESSIONS), f32::exp);
    found
}

/// The difference since a change, with days as the unit (runs of one day share that day's
/// form, so they are not independent evidence). Only drills with [`DRILL_RUNS`] runs on both
/// sides count; every run's value is taken as a logarithm relative to its drill's average over
/// both sides, so drills with different targets can be put together and a different mix of
/// drills before and after doesn't look like a change. Returns Welch's comparison of the daily
/// averages after with those before; `None` without two such days on each side.
fn change(before: &[RunSummary], after: &[RunSummary], value: fn(&RunSummary) -> Option<f32>) -> Option<Welch> {
    let log = |run: &RunSummary| value(run).filter(|v| *v > 0.0).map(f32::ln);
    let reference = |drill: &str| -> Option<f32> {
        let of = |runs: &[RunSummary]| runs.iter().filter(|r| r.drill == drill).filter_map(log).collect::<Vec<_>>();
        let (old, new) = (of(before), of(after));
        (old.len().min(new.len()) >= DRILL_RUNS)
            .then(|| (old.iter().sum::<f32>() + new.iter().sum::<f32>()) / (old.len() + new.len()) as f32)
    };
    let days = |runs: &[RunSummary]| -> Vec<f32> {
        let relative = runs.iter().filter_map(|run| Some((run.unix_time, log(run)? - reference(&run.drill)?)));
        daily_means(relative).into_iter().map(|day| day.1).collect()
    };
    Welch::of(&days(before), &days(after))
}

/// Checks the change from `before` to `after` (see [`Outcome`]), leaving out the first
/// [`ADAPT_RUNS`] runs after it. A difference is clear at the gates' 1% level.
fn review(before: &[RunSummary], after: &[RunSummary]) -> Option<Review> {
    if before.len() < REVIEW_RUNS {
        return None;
    }
    let settled = after.get(ADAPT_RUNS..)?;
    let error = change(before, settled, |r| r.click_deg)?;
    let score = change(before, settled, |r| Some(r.score))?;
    let (precise, sloppy) = (error.clear() && error.difference < 0.0, error.clear() && error.difference > 0.0);
    let (higher, lower) = (score.clear() && score.difference > 0.0, score.clear() && score.difference < 0.0);
    let outcome = match (precise, sloppy, higher, lower) {
        (true, _, _, true) => Outcome::Mixed,
        (true, ..) | (false, false, true, _) => Outcome::Helped,
        (_, true, ..) | (false, _, _, true) => Outcome::Hurt,
        _ => Outcome::Unclear,
    };
    Some(Review {
        from_cm: before[0].cm_per_360,
        outcome,
        click_error: error.difference.exp(),
        score: score.difference.exp(),
    })
}

/// One step for `finding` from `current` cm/360, kept inside the range.
fn step(finding: Finding, current: f32, limits: Option<&Limits>) -> Advice {
    let held = |by| Advice::Held { finding, by };
    let Some(limits) = limits else { return held(HeldBy::NoRange) };
    let Some((fast, slow)) = limits.range() else { return held(HeldBy::EmptyRange) };
    if current < fast || current > slow {
        return held(HeldBy::Outside);
    }
    let wanted = if finding.pattern.slower() { current * (1.0 + STEP) } else { current / (1.0 + STEP) };
    let to_cm = wanted.clamp(fast, slow);
    // A step cut down to under a percent by the limit is no step.
    if (to_cm / current).ln().abs() < 0.01 { held(HeldBy::Limit) } else { Advice::Step { finding, to_cm } }
}

/// What the monitor says for the sensitivity `cm_per_360`, from the whole log.
pub fn status(log: &[RunSummary], cm_per_360: f32, limits: Option<&Limits>) -> Status {
    let (current, previous) = blocks(log, cm_per_360);
    let progress = Progress::of(current);
    let previous_cm = previous.first().map(|r| r.cm_per_360);
    if !progress.enough() {
        return Status { progress, advice: Advice::Collecting, review: None, previous_cm };
    }
    let review = review(previous, current);
    let window = &current[current.len().saturating_sub(WINDOW)..];
    let found = findings(window);
    let in_range = |cm: f32| limits.and_then(Limits::range).is_none_or(|(fast, slow)| (fast..=slow).contains(&cm));
    let advice = match (review, found.first()) {
        (Some(Review { outcome: Outcome::Hurt, from_cm, .. }), _) if in_range(from_cm) => {
            Advice::Revert { to_cm: from_cm }
        },
        // Patterns that ask for opposite steps cancel: no suggestion on mixed evidence.
        (_, Some(_)) if found.iter().any(|f| f.pattern.slower()) && found.iter().any(|f| !f.pattern.slower()) => {
            Advice::Quiet
        },
        (_, Some(&finding)) => step(finding, cm_per_360, limits),
        (_, None) => Advice::Quiet,
    };
    Status { progress, advice, review, previous_cm }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::finder::limits::tests::calibration;
    use crate::finder::stats::SECS_PER_DAY;
    use crate::finder::stats::tests::normal;
    use crate::test_util::TempDir;

    /// A Microflick run summary at 40 cm/360 with neutral values.
    pub(crate) fn run(unix_time: u64) -> RunSummary {
        RunSummary {
            unix_time,
            drill: "Microflick".to_string(),
            kind: DrillKind::Microflick,
            sens: 0.408,
            cm_per_360: 40.0,
            flicks: 40,
            landing: Some(1.0),
            bias: Some(0.0),
            click_deg: Some(0.4),
            corrections: Some(0.8),
            misses: 0.1,
            score: 9000.0,
        }
    }

    /// A synthetic player's habits; every value scatters from run to run and from day to day.
    #[derive(Clone, Copy)]
    pub(crate) struct Player {
        /// True median landing ratio minus 1.
        pub landing: f32,
        /// True mean click position along the flick, target radii.
        pub bias: f32,
        pub corrections: f32,
        /// Click error in the late half of a session over the early half.
        pub fade: f32,
        pub click_deg: f32,
        pub score: f32,
    }

    impl Player {
        /// No pattern: flicks land on target, clicks a little short of the centre, few
        /// corrections (about what a practised player's logs show).
        pub(crate) const STEADY: Player =
            Player { landing: 0.0, bias: -0.08, corrections: 0.3, fade: 1.0, click_deg: 0.4, score: 9000.0 };

        /// One day's session of `runs` runs, two minutes apart, starting at `start`.
        pub(crate) fn session(&self, start: u64, runs: usize, cm: f32, rng: &mut fastrand::Rng) -> Vec<RunSummary> {
            // The day's form shifts everything a little, on top of each run's own scatter.
            let day = (0.02 * normal(rng), 0.08 * normal(rng), 0.06 * normal(rng), 0.04 * normal(rng));
            (0..runs)
                .map(|i| {
                    let late = if i >= runs / 2 { self.fade } else { 1.0 };
                    RunSummary {
                        cm_per_360: cm,
                        landing: Some(1.0 + self.landing + day.0 + 0.06 * normal(rng)),
                        bias: Some(self.bias + day.3 + 0.08 * normal(rng)),
                        corrections: Some((self.corrections + day.1 + 0.15 * normal(rng)).max(0.0)),
                        click_deg: Some(self.click_deg * late * (day.2 + 0.15 * normal(rng)).exp()),
                        score: self.score * (0.5 * day.2 + 0.08 * normal(rng)).exp(),
                        ..run(start + i as u64 * 120)
                    }
                })
                .collect()
        }

        /// `days` days of `runs` runs each, from day `first_day`.
        pub(crate) fn play(
            &self,
            first_day: u64,
            days: u64,
            runs: usize,
            cm: f32,
            rng: &mut fastrand::Rng,
        ) -> Vec<RunSummary> {
            (first_day..first_day + days)
                .flat_map(|day| self.session(day * SECS_PER_DAY + 70_000, runs, cm, rng))
                .collect()
        }
    }

    fn limits() -> Limits {
        // Range 26.9 to 58 cm/360.
        Limits::new(&calibration(29.0, 7.5, 0.12), None)
    }

    #[test]
    fn nothing_is_said_before_the_minimum_of_runs_flicks_and_days() {
        let mut rng = fastrand::Rng::with_seed(1);
        // A glaring overshoot of 30%.
        let player = Player { landing: 0.3, ..Player::STEADY };
        let limits = limits();
        // 30 runs on 3 days: enough runs and flicks, one day short.
        let log = player.play(100, 3, 10, 40.0, &mut rng);
        let short = status(&log, 40.0, Some(&limits));
        assert_eq!(short.advice, Advice::Collecting);
        assert_eq!(short.progress, Progress { runs: 30, flicks: 1200, days: 3 });
        assert_eq!(short.progress.missing(), Progress { runs: 0, flicks: 0, days: 1 });
        // 4 days of 4 runs: enough days, short of runs.
        let log = player.play(100, 4, 4, 40.0, &mut rng);
        let short = status(&log, 40.0, Some(&limits));
        assert_eq!(short.advice, Advice::Collecting);
        assert_eq!(short.progress.missing(), Progress { runs: 4, flicks: 0, days: 0 });
        // Runs with few flicks: 4 days, 20 runs, 300 flicks.
        let sparse: Vec<RunSummary> =
            player.play(100, 4, 5, 40.0, &mut rng).into_iter().map(|r| RunSummary { flicks: 15, ..r }).collect();
        assert_eq!(status(&sparse, 40.0, Some(&limits)).progress.missing().flicks, 200);
        // Evaluated after every single run of a longer history: never before the minimum.
        let log = player.play(100, 8, 5, 40.0, &mut rng);
        for played in 0..=log.len() {
            let so_far = status(&log[..played], 40.0, Some(&limits));
            if !so_far.progress.enough() {
                assert_eq!(so_far.advice, Advice::Collecting, "after {played} runs");
            }
        }
    }

    #[test]
    fn a_persistent_overshoot_gets_one_step_slower_after_a_realistic_amount_of_play() {
        let limits = limits();
        let mut first_said = Vec::new();
        for seed in 0..200 {
            let mut rng = fastrand::Rng::with_seed(seed);
            // First flicks 10% long, six runs a day.
            let log = Player { landing: 0.10, ..Player::STEADY }.play(100, 12, 6, 40.0, &mut rng);
            let said = (1..=log.len()).find_map(|played| match status(&log[..played], 40.0, Some(&limits)).advice {
                Advice::Step { finding, to_cm } => Some((played, finding, to_cm)),
                Advice::Collecting | Advice::Quiet => None,
                other => panic!("seed {seed}: {other:?}"),
            });
            let Some((played, finding, to_cm)) = said else { continue };
            assert!(played >= MIN_RUNS, "seed {seed}: said after {played} runs");
            assert_eq!(finding.pattern, Pattern::Overshoot);
            assert!(finding.value > OVERSHOOT && finding.days >= MIN_DAYS);
            // One 5% step slower.
            assert!((to_cm - 42.0).abs() < 1e-3, "{to_cm}");
            first_said.push(played);
        }
        // Nearly every such player hears it within 12 days, half of them within a week.
        assert!(first_said.len() >= 190, "only {} of 200", first_said.len());
        first_said.sort_unstable();
        assert!(first_said[first_said.len() / 2] <= 42, "median {} runs", first_said[first_said.len() / 2]);
    }

    /// The false-suggestion rate: of 1000 players with no pattern at all (and of 1000 with a
    /// small one below the thresholds), each playing 5 runs a day for 60 days with the monitor
    /// looking after every run, fewer than 1 in 100 ever see a suggestion.
    #[test]
    fn a_noisy_player_without_a_pattern_almost_never_gets_a_suggestion() {
        let limits = limits();
        for (player, allowed) in [
            (Player::STEADY, 10),
            // Small real habits, all below what matters: first flicks 2% long, clicks a little
            // past the centre, 0.7 corrections per kill.
            (Player { landing: 0.02, bias: 0.05, corrections: 0.7, ..Player::STEADY }, 10),
        ] {
            let mut suggested = 0;
            for seed in 0..1000 {
                let mut rng = fastrand::Rng::with_seed(seed);
                let log = player.play(100, 60, 5, 40.0, &mut rng);
                let spoke = (MIN_RUNS..=log.len()).any(|played| {
                    !matches!(status(&log[..played], 40.0, Some(&limits)).advice, Advice::Quiet | Advice::Collecting)
                });
                suggested += usize::from(spoke);
            }
            assert!(suggested <= allowed, "{suggested} of 1000 players got a suggestion");
        }
    }

    #[test]
    fn undershoot_shaky_corrections_and_fading_each_get_their_step() {
        let limits = limits();
        let expect = |player: Player, runs: usize, pattern: Pattern, to_cm: f32| {
            let mut hits = 0;
            for seed in 0..40 {
                let mut rng = fastrand::Rng::with_seed(seed);
                let log = player.play(100, 14, runs, 40.0, &mut rng);
                match status(&log, 40.0, Some(&limits)).advice {
                    Advice::Step { finding, to_cm: step } => {
                        assert_eq!(finding.pattern, pattern, "seed {seed}");
                        assert!((step - to_cm).abs() < 1e-3, "{step}");
                        hits += 1;
                    },
                    Advice::Quiet => {},
                    other => panic!("seed {seed}: {other:?}"),
                }
            }
            assert!(hits >= 36, "{pattern:?}: only {hits} of 40");
        };
        // First flicks 20% short: faster.
        expect(Player { landing: -0.20, ..Player::STEADY }, 6, Pattern::Undershoot, 40.0 / 1.05);
        // Clicks a third of a radius past the centre: slower.
        expect(Player { bias: 0.33, ..Player::STEADY }, 6, Pattern::PastCentre, 42.0);
        // On target, then 1.6 corrections per kill: slower.
        expect(Player { corrections: 1.6, ..Player::STEADY }, 6, Pattern::Shaky, 42.0);
        // Click error 50% larger in the late half of 20-minute sessions: slower.
        expect(Player { fade: 1.5, ..Player::STEADY }, 10, Pattern::Fade, 42.0);
    }

    #[test]
    fn corrections_after_short_flicks_are_not_shaky_and_mixed_evidence_is_silence() {
        let limits = limits();
        let mut rng = fastrand::Rng::with_seed(3);
        // 8% short (not an undershoot worth acting on) with many corrections: they are the
        // approach, not a wobble.
        let log = Player { landing: -0.08, corrections: 1.6, ..Player::STEADY }.play(100, 14, 6, 40.0, &mut rng);
        assert_eq!(status(&log, 40.0, Some(&limits)).advice, Advice::Quiet);
        // 20% short and fading late: one asks for faster, the other for slower.
        let log = Player { landing: -0.20, fade: 1.6, ..Player::STEADY }.play(100, 14, 10, 40.0, &mut rng);
        let found = findings(&log[log.len() - WINDOW..]);
        assert_eq!(found.iter().map(|f| f.pattern).collect::<Vec<_>>(), vec![Pattern::Undershoot, Pattern::Fade]);
        assert_eq!(status(&log, 40.0, Some(&limits)).advice, Advice::Quiet);
    }

    #[test]
    fn a_pattern_that_stopped_showing_is_not_suggested() {
        let limits = limits();
        let mut rng = fastrand::Rng::with_seed(5);
        // Ten days of overshoot, then the habit is gone for the latest four days.
        let mut log = Player { landing: 0.12, ..Player::STEADY }.play(100, 6, 5, 40.0, &mut rng);
        assert!(matches!(status(&log, 40.0, Some(&limits)).advice, Advice::Step { .. }));
        log.extend(Player { landing: -0.01, ..Player::STEADY }.play(106, 4, 5, 40.0, &mut rng));
        assert_eq!(status(&log, 40.0, Some(&limits)).advice, Advice::Quiet);
    }

    #[test]
    fn steps_never_leave_the_measured_range() {
        let mut rng = fastrand::Rng::with_seed(2);
        let overshooter = Player { landing: 0.12, ..Player::STEADY };
        let finding = |advice: Advice| match advice {
            Advice::Held { finding, by } => (finding.pattern, by),
            other => panic!("{other:?}"),
        };
        // No calibration: the pattern is named, no step is offered.
        let log = overshooter.play(100, 8, 6, 40.0, &mut rng);
        assert_eq!(finding(status(&log, 40.0, None).advice), (Pattern::Overshoot, HeldBy::NoRange));
        // Limits that cross.
        let empty = Limits::new(&calibration(20.0, 7.0, 0.25), None);
        assert_eq!(finding(status(&log, 40.0, Some(&empty)).advice), (Pattern::Overshoot, HeldBy::EmptyRange));
        // At the pad's limit (58 cm/360) a slower step has no room.
        let limits = limits();
        let at_limit = overshooter.play(100, 8, 6, 57.9, &mut rng);
        assert_eq!(finding(status(&at_limit, 57.9, Some(&limits)).advice), (Pattern::Overshoot, HeldBy::Limit));
        // Close to it the step is cut to the limit.
        let near = overshooter.play(100, 8, 6, 56.5, &mut rng);
        let Advice::Step { to_cm, .. } = status(&near, 56.5, Some(&limits)).advice else { panic!() };
        assert_eq!(to_cm, 58.0);
        // Outside the range the range's advice comes first.
        let outside = overshooter.play(100, 8, 6, 20.0, &mut rng);
        assert_eq!(finding(status(&outside, 20.0, Some(&limits)).advice), (Pattern::Overshoot, HeldBy::Outside));
        // An undershooter at the fast limit can't go faster either.
        let fast = Player { landing: -0.2, ..Player::STEADY }.play(100, 8, 6, 27.0, &mut rng);
        assert_eq!(finding(status(&fast, 27.0, Some(&limits)).advice), (Pattern::Undershoot, HeldBy::Limit));
    }

    #[test]
    fn a_new_sensitivity_starts_from_nothing_and_is_then_checked_against_the_old() {
        let limits = limits();
        let mut rng = fastrand::Rng::with_seed(4);
        let before = Player { landing: 0.12, ..Player::STEADY };
        let mut log = before.play(100, 8, 6, 40.0, &mut rng);
        assert!(matches!(status(&log, 40.0, Some(&limits)).advice, Advice::Step { .. }));
        // The setting was changed but nothing played yet.
        let fresh = status(&log, 42.0, Some(&limits));
        assert_eq!(
            (fresh.advice, fresh.progress, fresh.previous_cm),
            (Advice::Collecting, Progress::default(), Some(40.0))
        );
        // Still overshooting at the new sensitivity: silent until it has its own minimum.
        let after = before.play(108, 3, 6, 42.0, &mut rng);
        log.extend(after);
        let status_3_days = status(&log, 42.0, Some(&limits));
        assert_eq!((status_3_days.advice, status_3_days.review), (Advice::Collecting, None));
        assert_eq!(status_3_days.progress.runs, 18);
        // With its minimum it is compared with the block before (no clear difference here), and
        // the pattern, still there, gets its next step.
        log.extend(before.play(111, 3, 6, 42.0, &mut rng));
        let later = status(&log, 42.0, Some(&limits));
        let review = later.review.expect("both sides have enough runs");
        assert_eq!((review.from_cm, review.outcome), (40.0, Outcome::Unclear));
        assert!(matches!(later.advice, Advice::Step { to_cm, .. } if (to_cm - 44.1).abs() < 1e-3));
        // Going back to the old sensitivity starts a new block there, too.
        assert_eq!(status(&log, 40.0, Some(&limits)).progress, Progress::default());
    }

    /// Plays 8 days at 40 cm/360, then 6 at 42 as `after`, and returns the status.
    fn changed(after: Player, seed: u64) -> Status {
        let mut rng = fastrand::Rng::with_seed(seed);
        let mut log = Player::STEADY.play(100, 8, 6, 40.0, &mut rng);
        log.extend(after.play(108, 6, 6, 42.0, &mut rng));
        status(&log, 42.0, Some(&limits()))
    }

    #[test]
    fn a_change_is_judged_by_click_error_and_score() {
        // Clicks 30% closer, score unchanged: it helped, nothing more to do.
        let helped = changed(Player { click_deg: 0.28, ..Player::STEADY }, 1);
        let review = helped.review.unwrap();
        assert_eq!((review.outcome, helped.advice), (Outcome::Helped, Advice::Quiet));
        assert!((review.click_error - 0.7).abs() < 0.08 && (review.score - 1.0).abs() < 0.08, "{review:?}");
        // Clicks 40% further off: go back.
        let hurt = changed(Player { click_deg: 0.56, ..Player::STEADY }, 2);
        assert_eq!(hurt.review.unwrap().outcome, Outcome::Hurt);
        assert_eq!(hurt.advice, Advice::Revert { to_cm: 40.0 });
        // Score 20% lower with the same precision: go back.
        let slower = changed(Player { score: 7200.0, ..Player::STEADY }, 3);
        assert_eq!((slower.review.unwrap().outcome, slower.advice), (Outcome::Hurt, Advice::Revert { to_cm: 40.0 }));
        // More precise but lower scores: said as it is, no revert.
        let mixed = changed(Player { click_deg: 0.28, score: 7200.0, ..Player::STEADY }, 4);
        assert_eq!((mixed.review.unwrap().outcome, mixed.advice), (Outcome::Mixed, Advice::Quiet));
        // No real difference: called either way in about 2 of 100 changes (two tests at 1%).
        let called = (0..500).filter(|&seed| changed(Player::STEADY, seed).review.unwrap().outcome != Outcome::Unclear);
        let called = called.count();
        assert!(called <= 20, "{called} of 500 changes without a difference were called");
    }

    #[test]
    fn a_change_is_only_compared_within_the_same_drills() {
        let mut rng = fastrand::Rng::with_seed(6);
        let rename = |runs: Vec<RunSummary>, drill: &str, factor: f32| -> Vec<RunSummary> {
            runs.into_iter()
                .map(|r| RunSummary { drill: drill.to_string(), click_deg: r.click_deg.map(|c| c * factor), ..r })
                .collect()
        };
        // Before: Gridshot only, with large targets and click errors. After: Microflick only.
        let mut log = rename(Player::STEADY.play(100, 8, 6, 40.0, &mut rng), "Gridshot", 3.0);
        log.extend(Player::STEADY.play(108, 6, 6, 42.0, &mut rng));
        let apart = status(&log, 42.0, Some(&limits()));
        assert_eq!((apart.review, apart.advice), (None, Advice::Quiet));
        // Both drills on both sides, Gridshot's errors three times Microflick's: no difference.
        let mut rng = fastrand::Rng::with_seed(7);
        let mut log = Vec::new();
        for (first_day, cm) in [(100, 40.0), (108, 42.0)] {
            let mut days = Player::STEADY.play(first_day, 8, 6, cm, &mut rng);
            let gridshot = rename(days.split_off(24), "Gridshot", 3.0);
            log.extend(days);
            log.extend(gridshot);
        }
        log.sort_by_key(|r| r.unix_time);
        // Sorting by time must keep the two sensitivities apart for this to be one change.
        assert!(log.windows(2).filter(|w| w[0].cm_per_360 != w[1].cm_per_360).count() == 1);
        assert_eq!(status(&log, 42.0, Some(&limits())).review.unwrap().outcome, Outcome::Unclear);
    }

    #[test]
    fn summaries_round_trip_through_the_file() {
        let dir = TempDir::new("monitor_log");
        let path = dir.join("monitor.csv");
        let mut log = MonitorLog::load(&path);
        let full = RunSummary { landing: Some(1.0625), bias: Some(-0.125), ..run(100) };
        let sparse = RunSummary { landing: None, bias: None, click_deg: None, corrections: None, ..run(200) };
        log.record(&full);
        log.record(&sparse);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().next(), Some(HEADER));
        assert_eq!(
            text.lines().nth(1),
            Some("100,Microflick,Microflick,0.408,40.000,40,1.0625,-0.1250,0.4000,0.8000,0.1000,9000")
        );
        assert_eq!(text.lines().nth(2), Some("200,Microflick,Microflick,0.408,40.000,40,,,,,0.1000,9000"));
        assert_eq!(MonitorLog::load(&path).entries(), [full, sparse]);
        // A staged log writes nothing; broken rows are skipped.
        let mut staged = MonitorLog::in_memory(vec![run(1)]);
        staged.record(&run(2));
        assert_eq!(staged.entries().len(), 2);
        fs::write(
            &path,
            format!(
                "{HEADER}\n1,Microflick,Nope,0.4,40,40,,,,,0.1,9000\n2,Microflick,Microflick,0.4,40,40,x,,,,0.1,9\n"
            ),
        )
        .unwrap();
        assert!(MonitorLog::load(&path).entries().is_empty());
    }

    #[test]
    fn a_run_is_summarised_from_its_flicks() {
        use bevy::math::Vec2;

        use crate::metrics::throughput::Click;
        let flick = |landing: f32, corrections: u32, off: f32, killed: bool| Flick {
            kind: DrillKind::Microflick,
            angle: 0.0,
            distance: 10.0,
            width: 2.0,
            landing: Some(landing),
            off_axis: Some(0.0),
            bow: None,
            corrections,
            ttk: killed.then_some(0.5),
            misses: u32::from(!killed),
            click: Some(Click {
                start: Vec2::ZERO,
                target: Vec2::new(-10.0, 0.0),
                radius: 1.0,
                size: 1.0,
                aim: Vec2::new(-10.0 - off, 0.0),
                time: 0.4,
                hit: killed,
                first: true,
            }),
        };
        let stats = RunStats {
            kind: DrillKind::Microflick,
            drill: "Micro, small".to_string(),
            sens: 0.4,
            cm_per_360: 40.82,
            duration: 30.0,
            hits: 9,
            shots: 10,
            expired: 0,
            ttk_total: 4.5,
            best_ttk: 0.4,
            on_target: 0.0,
            landings: Vec::new(),
            clicks: Vec::new(),
            rating: None,
            rounds_hit: 0,
        };
        // Nine kills (landing 1.1, one correction, 0.2° past the centre) and one expiry.
        let mut flicks: Vec<Flick> = (0..9).map(|_| flick(1.1, 1, 0.2, true)).collect();
        flicks.push(flick(0.5, 7, -0.6, false));
        let summary = RunSummary::measure(77, &stats, &flicks).unwrap();
        assert_eq!((summary.unix_time, summary.drill.as_str(), summary.flicks), (77, "Micro  small", 10));
        assert_eq!(summary.landing, Some(1.1));
        assert!((summary.bias.unwrap() - (9.0 * 0.2 - 0.6) / 10.0).abs() < 1e-5);
        assert!((summary.click_deg.unwrap() - (9.0 * 0.2 + 0.6) / 10.0).abs() < 1e-5);
        // Corrections count for kills only.
        assert_eq!(summary.corrections, Some(1.0));
        assert!((summary.misses - 0.1).abs() < 1e-6);
        assert_eq!(summary.score, stats.score());
        // Seven flicks are too few for any of the values; no flicks, no summary.
        let few = RunSummary::measure(78, &stats, &flicks[..7]).unwrap();
        assert_eq!((few.landing, few.bias, few.click_deg, few.corrections), (None, None, None, None));
        assert_eq!(RunSummary::measure(79, &stats, &[]), None);
    }

    /// Practice runs of flick drills (routine steps included) are summarised from the recorder's
    /// frames; finder trials and other drills are not.
    #[test]
    fn only_practice_runs_of_flick_drills_are_logged() {
        use crate::drill::{DrillSpec, Run, RunFinished};
        use crate::history::Comparison;
        use crate::settings::Settings;

        // Ten flicks between two dots 20 degrees apart, each killed after 0.3 s.
        let mut recorder = Recorder::default();
        let (dt, radius) = (0.005, 0.2);
        let at = |yaw: f32| Quat::from_rotation_y(yaw.to_radians()) * Vec3::NEG_Z * 10.0;
        let (mut t, mut yaw) = (0.0, 0.0f32);
        recorder.sample(t, (0.0, 0.0), std::iter::empty());
        for flick in 0..10 {
            let (from, to) = (yaw, if flick % 2 == 0 { 10.0 } else { -10.0 });
            for frame in 1..=60 {
                t += dt;
                let u = frame as f32 / 60.0;
                yaw = from + (to - from) * u * u * u * (10.0 - 15.0 * u + 6.0 * u * u);
                let killed = frame == 60;
                if killed {
                    recorder.mark_shot(true);
                }
                let target = (!killed).then_some((at(to), radius));
                recorder.sample(t, (yaw.to_radians(), 0.0), target.into_iter());
            }
        }

        let dir = TempDir::new("monitor_record");
        let path = dir.join("monitor.csv");
        let settings = Settings::default();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<RunRecorded>()
            .insert_resource(recorder)
            .insert_resource(MonitorLog::load(&path))
            .add_systems(Update, record_monitor);
        let mut finish = |kind: DrillKind, purpose: RunPurpose, timestamp: u64| {
            let drill = DrillSpec::builtin(kind, &settings);
            let run = Run::new(drill.clone(), 0.4, 30.0, purpose, String::new());
            let finished = RunFinished { stats: run.stats(&settings), purpose, drill, duration: 30.0 };
            app.world_mut().write_message(RunRecorded { run: finished, previous: Comparison::default(), timestamp });
            app.update();
            app.world().resource::<MonitorLog>().entries().iter().map(|e| e.unix_time).collect::<Vec<_>>()
        };
        assert_eq!(finish(DrillKind::Microflick, RunPurpose::FinderTrial, 1), Vec::<u64>::new());
        assert_eq!(finish(DrillKind::Tracking, RunPurpose::Practice, 2), Vec::<u64>::new());
        assert_eq!(finish(DrillKind::Microflick, RunPurpose::Practice, 3), [3]);
        assert_eq!(finish(DrillKind::Gridshot, RunPurpose::Routine, 4), [3, 4]);
        let logged = MonitorLog::load(&path);
        let entry = &logged.entries()[0];
        assert_eq!((entry.flicks, entry.drill.as_str(), entry.kind), (10, "Microflick", DrillKind::Microflick));
        assert!((entry.cm_per_360 - settings.cm_per_360(0.4)).abs() < 1e-2);
        // Clean flicks: they land on the target, without corrections or misses.
        assert!((entry.landing.unwrap() - 1.0).abs() < 0.03, "{entry:?}");
        assert_eq!((entry.corrections, entry.misses), (Some(0.0), 0.0));
    }
}
