//! The benchmark: a fixed set of scenarios with pinned settings, played as one routine; its log
//! (`aim_trainer_benchmark.csv`) and the index that compares a benchmark with the player's first.
//! The design (scenarios and why, the index formula, the file format) is in the
//! [module docs of `training`](super).

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use super::AimKind;
use crate::drill::{DrillConfig, DrillKind, DrillSpec, RunPurpose, RunStats};
use crate::history::{RunRecorded, append_csv_row};
use crate::routine::{Routine, RoutineOrigin, RoutineStep};

pub(super) const BENCHMARK_PATH: &str = "aim_trainer_benchmark.csv";
/// Persisted: never rename or reorder.
const HEADER: &str = "benchmark,scenario,run,score,throughput,on_target,cm_per_360";
/// Seconds each scenario lasts.
pub const SCENARIO_SECS: f32 = 60.0;
/// Changes within this many percent are called steady: single runs of the same drill commonly
/// differ by that much.
pub const STEADY_PCT: f32 = 5.0;
/// A scenario's ratio to its first score is clamped to this range before it enters an index.
pub const RATIO_LIMITS: (f32, f32) = (0.25, 4.0);
/// Days between benchmarks.
pub const INTERVAL_DAYS: i64 = 7;

/// A comparable physical measure a scenario reports besides its score.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
    /// Effective throughput of the clicks, bits/s.
    Throughput,
    /// Share of the time on target.
    OnTarget,
    None,
}

/// One scenario of the benchmark.
pub struct Scenario {
    /// Persisted: the history name and the log's `scenario` column. Never change it.
    pub name: &'static str,
    pub drill: DrillKind,
    pub aim: AimKind,
    pub measure: Measure,
    /// Sets the drill's own parameters on top of the common pinned values (see [`pinned`]).
    tune: fn(&mut DrillConfig),
}

/// The scenarios in the order they are played: kinds alternate, clicking twice per kind (see the
/// module docs for why each is here). Persisted through the names: append-only would change what
/// the index means, so a different set needs a new benchmark.
pub const SCENARIOS: [Scenario; 8] = [
    Scenario {
        name: "Smooth tracking · benchmark",
        drill: DrillKind::SmoothTracking,
        aim: AimKind::SmoothTracking,
        measure: Measure::OnTarget,
        tune: |c| (c.sweep_speed, c.lane) = (2.5, 5.0),
    },
    Scenario {
        name: "Microflick · benchmark",
        drill: DrillKind::Microflick,
        aim: AimKind::StaticClicking,
        measure: Measure::Throughput,
        tune: |c| (c.yaw_range, c.pitch_range, c.min_flick) = (30.0, 12.0, 8.0),
    },
    Scenario {
        name: "Strafing bot · benchmark",
        drill: DrillKind::StrafingBot,
        aim: AimKind::DynamicClicking,
        measure: Measure::None,
        tune: |c| {
            (c.strafe_min, c.strafe_max, c.turn_min, c.turn_max) = (2.7, 5.4, 0.4, 1.2);
            (c.hold_min, c.hold_max, c.lane) = (0.3, 0.7, 4.0);
        },
    },
    Scenario {
        name: "Tracking · benchmark",
        drill: DrillKind::Tracking,
        aim: AimKind::ReactiveTracking,
        measure: Measure::OnTarget,
        tune: |c| (c.strafe_min, c.strafe_max, c.turn_min, c.turn_max, c.lane) = (1.5, 4.0, 0.4, 1.4, 5.0),
    },
    Scenario {
        name: "Wall flicks · benchmark",
        drill: DrillKind::WallFlicks,
        aim: AimKind::PrecisionSwitching,
        measure: Measure::Throughput,
        tune: |c| (c.grid_active, c.yaw_range, c.pitch_range, c.min_flick) = (4, 20.0, 12.0, 6.0),
    },
    Scenario {
        name: "Head taps · benchmark",
        drill: DrillKind::HeadTaps,
        aim: AimKind::StaticClicking,
        measure: Measure::None,
        tune: |c| (c.grid_active, c.yaw_range, c.min_flick) = (2, 25.0, 4.0),
    },
    Scenario {
        name: "Target switching · benchmark",
        drill: DrillKind::TargetSwitch,
        aim: AimKind::SpeedSwitching,
        measure: Measure::None,
        tune: |c| {
            (c.track_time, c.grid_active, c.yaw_range, c.pitch_range) = (0.8, 4, 30.0, 12.0);
            (c.strafe_min, c.strafe_max, c.turn_min, c.turn_max) = (1.0, 2.5, 0.6, 1.5);
        },
    },
    Scenario {
        name: "Peeks · benchmark",
        drill: DrillKind::Peek,
        aim: AimKind::DynamicClicking,
        measure: Measure::None,
        tune: |c| (c.peek_speed_min, c.peek_speed_max, c.hold_min, c.hold_max) = (2.5, 4.5, 0.35, 0.8),
    },
];

impl Scenario {
    /// What this scenario plays: every parameter pinned, whatever the run settings say.
    pub fn spec(&self) -> DrillSpec {
        DrillSpec { name: self.name.to_string(), config: pinned(self, DrillConfig::defaults(self.drill)) }
    }

    /// The physical measure of a finished run of this scenario: (throughput bits/s, share on
    /// target).
    fn measures(&self, stats: &RunStats) -> (Option<f32>, Option<f32>) {
        match self.measure {
            Measure::Throughput => (stats.throughput(), None),
            Measure::OnTarget => (None, Some(stats.on_target_ratio())),
            Measure::None => (None, None),
        }
    }
}

/// `base` with every parameter set: the values the drills had by default when the benchmark was
/// designed, then the scenario's own. Every field is assigned, so neither the run settings nor a
/// later change of a drill's defaults reach a benchmark.
pub fn pinned(scenario: &Scenario, base: DrillConfig) -> DrillConfig {
    let mut c = base;
    c.kind = scenario.drill;
    (c.size, c.lifetime, c.adaptive) = (1.0, 0.0, None);
    (c.grid_cols, c.grid_rows, c.grid_spacing, c.grid_active) = (3, 3, 7.0, 3);
    (c.yaw_range, c.pitch_range, c.min_flick, c.max_flick) = (30.0, 12.0, 8.0, 0.0);
    (c.flick_direction, c.flick_spread) = (None, 22.5);
    (c.strafe_min, c.strafe_max, c.turn_min, c.turn_max, c.lane) = (1.5, 4.0, 0.4, 1.4, 5.0);
    (c.peek_speed_min, c.peek_speed_max, c.hold_min, c.hold_max) = (2.5, 4.5, 0.35, 0.8);
    (c.delay_min, c.delay_max) = (0.9, 2.6);
    (c.lives, c.approach_speed, c.spawn_interval) = (3, 2.0, 1.2);
    (c.track_time, c.sweep_speed) = (0.5, 2.5);
    (scenario.tune)(&mut c);
    c
}

/// The benchmark as a routine, ready to start.
pub fn routine() -> Routine {
    let steps = SCENARIOS.iter().map(|s| RoutineStep { drill: s.spec(), secs: SCENARIO_SECS }).collect();
    let blurb = format!(
        "{} scenarios of {SCENARIO_SECS:.0} s with fixed settings. Take short breaks. Ending early records nothing as a benchmark.",
        SCENARIOS.len()
    );
    Routine::new("Benchmark".to_string(), blurb, steps).with_origin(RoutineOrigin::Benchmark)
}

/// One scenario's result in a benchmark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScenarioResult {
    /// History timestamp of the run (names its replay).
    pub run: u64,
    pub score: f32,
    /// Effective throughput, bits/s, where measured.
    pub throughput: Option<f32>,
    /// Share of the time on target, where measured.
    pub on_target: Option<f32>,
}

/// A completed benchmark: every scenario, in [`SCENARIOS`] order.
#[derive(Clone, Debug, PartialEq)]
pub struct Benchmark {
    /// History timestamp of the last scenario: the benchmark's id and date.
    pub timestamp: u64,
    pub cm_per_360: f32,
    pub results: Vec<ScenarioResult>,
}

impl Benchmark {
    /// Its CSV rows (without the final newline), one per scenario.
    fn rows(&self) -> String {
        let optional = |value: Option<f32>, decimals: usize| value.map_or(String::new(), |v| format!("{v:.decimals$}"));
        SCENARIOS
            .iter()
            .zip(&self.results)
            .map(|(scenario, result)| {
                format!(
                    "{},{},{},{},{},{},{:.3}",
                    self.timestamp,
                    scenario.name,
                    result.run,
                    result.score,
                    optional(result.throughput, 2),
                    optional(result.on_target, 4),
                    self.cm_per_360
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The benchmarks in a log file's text; one missing a scenario (or with a damaged row) is
    /// left out.
    fn parse_all(text: &str) -> Vec<Benchmark> {
        let mut benchmarks = Vec::new();
        let mut rows: Vec<(u64, &str, ScenarioResult, f32)> = Vec::new();
        let mut flush = |rows: &mut Vec<(u64, &str, ScenarioResult, f32)>| {
            let complete = rows.len() == SCENARIOS.len()
                && rows.iter().zip(&SCENARIOS).all(|((id, name, ..), s)| *name == s.name && *id == rows[0].0);
            if complete {
                benchmarks.push(Benchmark {
                    timestamp: rows[0].0,
                    cm_per_360: rows[0].3,
                    results: rows.iter().map(|row| row.2).collect(),
                });
            }
            rows.clear();
        };
        for line in text.lines().skip(1) {
            let row = parse_row(line);
            if rows.first().is_some_and(|first| row.is_none_or(|row| row.0 != first.0)) {
                flush(&mut rows);
            }
            match row {
                Some(row) => rows.push(row),
                None => rows.clear(),
            }
        }
        flush(&mut rows);
        benchmarks
    }
}

/// One log row: (benchmark id, scenario name, result, cm/360).
fn parse_row(line: &str) -> Option<(u64, &str, ScenarioResult, f32)> {
    let [benchmark, scenario, run, score, throughput, on_target, cm] = line.split(',').collect::<Vec<_>>()[..] else {
        return None;
    };
    let number = |text: &str| text.parse::<f32>().ok().filter(|v| v.is_finite());
    let optional = |text: &str| if text.is_empty() { Some(None) } else { number(text).map(Some) };
    let result = ScenarioResult {
        run: run.parse().ok()?,
        score: number(score)?,
        throughput: optional(throughput)?,
        on_target: optional(on_target)?,
    };
    Some((benchmark.parse().ok()?, scenario, result, number(cm)?))
}

/// Every completed benchmark, oldest first.
#[derive(Resource)]
pub struct BenchmarkLog {
    entries: Vec<Benchmark>,
    path: PathBuf,
}

impl BenchmarkLog {
    pub fn load(path: &Path) -> Self {
        let entries = fs::read_to_string(path).map(|text| Benchmark::parse_all(&text)).unwrap_or_default();
        Self { entries, path: path.to_path_buf() }
    }

    /// Appends a benchmark to the file (all its rows in one write) and the log.
    pub fn record(&mut self, benchmark: &Benchmark) {
        if let Err(err) = append_csv_row(&self.path, HEADER, &benchmark.rows()) {
            error!("Failed to write {}: {err}", self.path.display());
        }
        self.entries.push(benchmark.clone());
    }

    /// Adds a benchmark to the in-memory log only: for screens staged with made-up numbers.
    pub fn stage(&mut self, benchmark: Benchmark) {
        self.entries.push(benchmark);
    }

    pub fn entries(&self) -> &[Benchmark] {
        &self.entries
    }

    pub fn latest(&self) -> Option<&Benchmark> {
        self.entries.last()
    }
}

/// `ln(score / reference)` with the ratio clamped to [`RATIO_LIMITS`]; `None` without a usable
/// reference.
fn log_ratio(score: f32, reference: f32) -> Option<f32> {
    (reference > 0.0 && score.is_finite()).then(|| (score / reference).clamp(RATIO_LIMITS.0, RATIO_LIMITS.1).ln())
}

/// Mean of the values after dropping the highest and the lowest (with at least five of them).
fn trimmed_mean(mut values: Vec<f32>) -> Option<f32> {
    values.sort_by(f32::total_cmp);
    let kept = if values.len() >= 5 { &values[1..values.len() - 1] } else { &values[..] };
    (!kept.is_empty()).then(|| kept.iter().sum::<f32>() / kept.len() as f32)
}

/// The benchmark index of `benchmark` against the player's `first` (module docs): 100 for the
/// first itself.
pub fn index(first: &Benchmark, benchmark: &Benchmark) -> f32 {
    let logs = benchmark.results.iter().zip(&first.results).filter_map(|(now, then)| log_ratio(now.score, then.score));
    trimmed_mean(logs.collect()).map_or(100.0, |mean| 100.0 * mean.exp())
}

/// One kind's index: the geometric mean of its scenarios' ratios to the first benchmark, x100.
pub fn kind_index(first: &Benchmark, benchmark: &Benchmark, aim: AimKind) -> Option<f32> {
    let logs: Vec<f32> = SCENARIOS
        .iter()
        .zip(benchmark.results.iter().zip(&first.results))
        .filter(|(scenario, _)| scenario.aim == aim)
        .filter_map(|(_, (now, then))| log_ratio(now.score, then.score))
        .collect();
    (!logs.is_empty()).then(|| 100.0 * (logs.iter().sum::<f32>() / logs.len() as f32).exp())
}

/// The kinds of aim in the benchmark, in [`AimKind::ALL`] order.
pub fn kinds() -> Vec<AimKind> {
    AimKind::ALL.into_iter().filter(|aim| SCENARIOS.iter().any(|s| s.aim == *aim)).collect()
}

/// How each kind moved from benchmark `at - 1` to `at` of `log`, in percent, biggest gain first.
/// Empty for the first benchmark.
pub fn kind_moves(log: &[Benchmark], at: usize) -> Vec<(AimKind, f32)> {
    let (Some(first), Some(before), Some(now)) = (log.first(), at.checked_sub(1).and_then(|i| log.get(i)), log.get(at))
    else {
        return Vec::new();
    };
    let mut moves: Vec<(AimKind, f32)> = kinds()
        .into_iter()
        .filter_map(|aim| Some((aim, (kind_index(first, now, aim)? / kind_index(first, before, aim)? - 1.0) * 100.0)))
        .collect();
    moves.sort_by(|a, b| b.1.total_cmp(&a.1));
    moves
}

/// The kind that improved least since the first benchmark, with its index and the overall one;
/// `None` before a second benchmark (there is nothing to have improved on yet).
pub fn least_improved(log: &[Benchmark]) -> Option<(AimKind, f32, f32)> {
    let (first, latest) = (log.first()?, log.last()?);
    if log.len() < 2 {
        return None;
    }
    kinds()
        .into_iter()
        .filter_map(|aim| Some((aim, kind_index(first, latest, aim)?)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(aim, kind)| (aim, kind, index(first, latest)))
}

/// Each scenario's best score in the benchmarks before `at`.
pub fn bests_before(log: &[Benchmark], at: usize) -> Vec<Option<f32>> {
    (0..SCENARIOS.len()).map(|i| log[..at.min(log.len())].iter().map(|b| b.results[i].score).reduce(f32::max)).collect()
}

/// Which way a benchmark went, for its takeaway.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trend {
    Baseline,
    Up,
    Steady,
    Down,
}

/// The takeaway of benchmark `at` of `log` in plain words: (trend, title, text).
pub fn takeaway(log: &[Benchmark], at: usize) -> (Trend, String, String) {
    let Some(first) = log.first() else {
        return (Trend::Baseline, String::new(), String::new());
    };
    if at == 0 {
        let text = "Every later benchmark is compared with this one. Benchmark again in a week, at the same time of day and with the same warm-up.";
        return (Trend::Baseline, "Baseline set: index 100".into(), text.into());
    }
    let (now, before) = (index(first, &log[at]), index(first, &log[at - 1]));
    let change = (now / before - 1.0) * 100.0;
    let moves = kind_moves(log, at);
    // Whole percent, as the moves are shown.
    let gain = moves.first().filter(|m| m.1.round() >= STEADY_PCT);
    let drop = moves.last().filter(|m| m.1.round() <= -STEADY_PCT);
    let named = |(aim, pct): &(AimKind, f32)| format!("{} ({pct:+.0}%)", aim.name().to_lowercase());
    let detail = match (gain, drop) {
        (Some(g), Some(d)) => format!("Biggest gain: {}. Biggest drop: {}.", named(g), named(d)),
        (Some(g), None) => format!("Biggest gain: {}; no kind dropped more than {STEADY_PCT:.0}%.", named(g)),
        (None, Some(d)) => format!("Biggest drop: {}; no kind gained more than {STEADY_PCT:.0}%.", named(d)),
        (None, None) => format!("No kind moved more than {STEADY_PCT:.0}% either way."),
    };
    let (trend, title, advice) = if change >= STEADY_PCT {
        (Trend::Up, format!("Up {change:.0}% since the last benchmark"), "")
    } else if change <= -STEADY_PCT {
        let advice = " One benchmark can be off (sleep, warm-up, time of day): trust the trend over several.";
        (Trend::Down, format!("Down {:.0}% since the last benchmark", -change), advice)
    } else {
        (Trend::Steady, "About the same as last time".to_string(), "")
    };
    (trend, title, format!("{detail}{advice}"))
}

/// The benchmark the routine just finished, if it is one, played to the end in order: every
/// scenario has a result (none skipped) and `runs` holds their history timestamps.
pub fn completed(routine: &Routine, runs: &[u64]) -> Option<Benchmark> {
    let complete = routine.origin == RoutineOrigin::Benchmark
        && routine.finished()
        && routine.results.len() == SCENARIOS.len()
        && runs.len() == SCENARIOS.len()
        && routine.results.iter().zip(&SCENARIOS).all(|(result, scenario)| result.stats.drill == scenario.name);
    if !complete {
        return None;
    }
    let results = SCENARIOS
        .iter()
        .zip(routine.results.iter().zip(runs))
        .map(|(scenario, (result, &run))| {
            let (throughput, on_target) = scenario.measures(&result.stats);
            ScenarioResult { run, score: result.stats.score(), throughput, on_target }
        })
        .collect();
    let last = &routine.results.last()?.stats;
    Some(Benchmark { timestamp: *runs.last()?, cm_per_360: last.cm_per_360, results })
}

/// The history timestamps of the benchmark routine's runs so far, by result.
#[derive(Resource, Default)]
pub struct BenchmarkSession {
    runs: Vec<u64>,
}

/// Notes each benchmark step's history timestamp and, once the last scenario is played, logs
/// the benchmark. Runs after the routine has recorded the step.
pub(crate) fn record_benchmark_step(
    mut recorded: MessageReader<RunRecorded>,
    routine: Option<Res<Routine>>,
    mut session: ResMut<BenchmarkSession>,
    mut log: ResMut<BenchmarkLog>,
) {
    for recorded in recorded.read().filter(|r| r.run.purpose == RunPurpose::Routine) {
        let Some(routine) = routine.as_ref().filter(|r| r.origin == RoutineOrigin::Benchmark) else { continue };
        // The step just recorded is the routine's latest result; the first starts a new session.
        let index = routine.results.len().saturating_sub(1);
        session.runs.truncate(index);
        session.runs.push(recorded.timestamp);
        if let Some(benchmark) = completed(routine, &session.runs) {
            log.record(&benchmark);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::state::app::StatesPlugin;

    use super::*;
    use crate::AppState;
    use crate::drill::{RunFinished, VARIANT_SEPARATOR};
    use crate::history::Comparison;
    use crate::settings::Settings;
    use crate::test_util::TempDir;

    fn benchmark(timestamp: u64, scores: [f32; 8]) -> Benchmark {
        let results = scores
            .iter()
            .enumerate()
            .map(|(i, &score)| ScenarioResult {
                run: timestamp - 8 + i as u64,
                score,
                throughput: None,
                on_target: None,
            })
            .collect();
        Benchmark { timestamp, cm_per_360: 40.0, results }
    }

    #[test]
    fn names_are_permanent_variants_of_their_drills() {
        for (i, scenario) in SCENARIOS.iter().enumerate() {
            assert_eq!(scenario.name, format!("{}{VARIANT_SEPARATOR}benchmark", scenario.drill.name()));
            assert_eq!(AimKind::of(scenario.drill), Some(scenario.aim), "{}", scenario.name);
            assert!(SCENARIOS[..i].iter().all(|other| other.name != scenario.name));
            // Never the name of a practice run.
            assert_eq!(DrillKind::from_name(scenario.name), None);
        }
        // Every kind of aim is measured.
        assert_eq!(kinds(), AimKind::ALL);
    }

    /// The configs and lengths a benchmark is played with are the same whatever the player's
    /// run settings are, and every parameter is pinned: a config built from scrambled values
    /// comes out the same as one built from the drill's defaults.
    #[test]
    fn pinned_configs_ignore_the_players_settings() {
        let mut odd = Settings::default();
        (odd.target_scale, odd.targets.lifetime, odd.run_secs, odd.sens) = (0.5, 0.9, 20.0, 1.7);
        let runs = |settings: &Settings| -> Vec<(DrillSpec, f32)> {
            routine()
                .steps
                .iter()
                .map(|step| {
                    let run = crate::drill::Run::new(
                        step.drill.clone(),
                        settings.sens,
                        step.secs,
                        RunPurpose::Routine,
                        String::new(),
                    );
                    (run.drill.clone(), run.duration)
                })
                .collect()
        };
        assert_eq!(runs(&Settings::default()), runs(&odd));
        for scenario in &SCENARIOS {
            let spec = scenario.spec();
            let mut sanitized = spec.config.clone();
            sanitized.sanitize();
            assert_eq!(sanitized, spec.config, "{} is outside the drill's limits", scenario.name);
            let mut scrambled = DrillConfig::defaults(DrillKind::Survival);
            scrambled.size = 3.0;
            scrambled.lifetime = 0.7;
            (scrambled.grid_cols, scrambled.grid_rows, scrambled.grid_spacing, scrambled.grid_active) = (9, 7, 3.0, 9);
            (scrambled.yaw_range, scrambled.pitch_range, scrambled.min_flick, scrambled.max_flick) =
                (70.0, 30.0, 2.0, 50.0);
            (scrambled.flick_direction, scrambled.flick_spread) = (Some(90.0), 10.0);
            (scrambled.strafe_min, scrambled.strafe_max, scrambled.turn_min, scrambled.turn_max, scrambled.lane) =
                (0.2, 9.0, 0.1, 3.0, 1.0);
            (scrambled.peek_speed_min, scrambled.peek_speed_max, scrambled.hold_min, scrambled.hold_max) =
                (1.0, 9.0, 0.1, 2.0);
            (scrambled.delay_min, scrambled.delay_max) = (0.2, 5.0);
            (scrambled.lives, scrambled.approach_speed, scrambled.spawn_interval) = (9, 7.0, 0.3);
            (scrambled.track_time, scrambled.sweep_speed) = (2.0, 9.0);
            scrambled.adaptive = Some(crate::metrics::quest::QuestMode::Train);
            assert_eq!(pinned(scenario, scrambled), spec.config, "{} leaves a parameter unpinned", scenario.name);
        }
    }

    #[test]
    fn index_is_100_first_and_trims_one_outlier() {
        let first = benchmark(100, [100.0; 8]);
        assert_eq!(index(&first, &first), 100.0);
        // Seven scenarios up 10% and one up 200%: the outlier is dropped, and so is one of the
        // others, leaving 110. (The plain mean of the ratios would be 133.75.)
        let second = benchmark(200, [110.0, 110.0, 300.0, 110.0, 110.0, 110.0, 110.0, 110.0]);
        assert!((index(&first, &second) - 110.0).abs() < 1e-3, "{}", index(&first, &second));
        // Ratios are geometric: one scenario at double and one at half cancel out.
        let mixed = benchmark(300, [200.0, 50.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0]);
        assert!((index(&first, &mixed) - 100.0).abs() < 1e-3);
        // A collapsed scenario (score 0) is clamped and then trimmed, not -infinity.
        let zero = benchmark(400, [0.0, 120.0, 120.0, 120.0, 120.0, 120.0, 120.0, 120.0]);
        assert!((index(&first, &zero) - 120.0).abs() < 1e-3);
        // A scenario the first benchmark scored 0 in has no ratio.
        let blank = benchmark(500, [0.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0]);
        assert!((index(&blank, &second) - 110.0).abs() < 1e-3);
    }

    #[test]
    fn kinds_move_and_the_least_improved_is_found() {
        let first = benchmark(100, [100.0; 8]);
        // Scenario order: smooth, static, dynamic, reactive, precision, static, speed, dynamic.
        let second = benchmark(200, [100.0, 120.0, 100.0, 90.0, 100.0, 120.0, 100.0, 100.0]);
        let log = [first.clone(), second];
        assert_eq!(least_improved(&log[..1]), None);
        let (aim, kind, _) = least_improved(&log).unwrap();
        assert_eq!(aim, AimKind::ReactiveTracking);
        assert!((kind - 90.0).abs() < 1e-3);
        let moves = kind_moves(&log, 1);
        assert_eq!(moves.first().map(|m| m.0), Some(AimKind::StaticClicking));
        assert!((moves[0].1 - 20.0).abs() < 1e-3);
        assert_eq!(moves.last().map(|m| m.0), Some(AimKind::ReactiveTracking));
        assert!(kind_moves(&log, 0).is_empty());
        assert_eq!(bests_before(&log, 1)[1], Some(100.0));
        assert_eq!(bests_before(&log, 0)[1], None);
    }

    #[test]
    fn log_round_trips_and_skips_incomplete_benchmarks() {
        let dir = TempDir::new("benchmark_log");
        let path = dir.join("benchmark.csv");
        let mut log = BenchmarkLog::load(&path);
        let mut first = benchmark(1000, [5200.0, 6100.0, 330.0, 6500.0, 7400.0, 280.0, 5100.0, 4900.0]);
        first.results[1].throughput = Some(4.25);
        first.results[0].on_target = Some(0.6123);
        let second = benchmark(2000, [5300.0; 8]);
        log.record(&first);
        log.record(&second);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().next(), Some(HEADER));
        assert_eq!(text.lines().nth(2), Some("1000,Microflick · benchmark,993,6100,4.25,,40.000"));
        assert_eq!(BenchmarkLog::load(&path).entries(), &[first.clone(), second.clone()]);
        // A benchmark cut short (a crash mid-write, a hand edit) or damaged is left out; the
        // complete ones around it still load.
        let partial: String = text.lines().skip(1).take(3).map(|l| format!("{l}\n")).collect();
        let damaged = text.lines().nth(12).unwrap().replace("5300", "x");
        let mut lines: Vec<String> = text.lines().map(String::from).collect();
        lines[12] = damaged;
        fs::write(&path, format!("{}\n{partial}", lines.join("\n"))).unwrap();
        assert_eq!(BenchmarkLog::load(&path).entries(), &[first]);
    }

    fn stats(name: &str, kind: DrillKind) -> RunStats {
        RunStats {
            kind,
            drill: name.to_string(),
            sens: 0.4,
            cm_per_360: 40.8,
            duration: 60.0,
            hits: 40,
            shots: 50,
            expired: 0,
            ttk_total: 20.0,
            best_ttk: 0.3,
            on_target: 30.0,
            landings: Vec::new(),
            clicks: Vec::new(),
            rating: None,
            rounds_hit: 0,
        }
    }

    /// Plays benchmark steps through the routine and benchmark systems: `skip` skips that step
    /// on the break screen instead. Returns how many benchmarks were logged.
    fn play(skip: Option<usize>, stop_after: usize) -> usize {
        let dir = TempDir::new("benchmark_record");
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(AppState::Routine)
            .insert_resource(routine())
            .insert_resource(BenchmarkLog::load(&dir.join("benchmark.csv")))
            .init_resource::<BenchmarkSession>()
            .add_message::<RunRecorded>()
            .add_systems(Update, (crate::routine::routine_step_finished, record_benchmark_step).chain());
        for (index, scenario) in SCENARIOS.iter().enumerate().take(stop_after) {
            if Some(index) == skip {
                app.world_mut().resource_mut::<Routine>().skip();
                continue;
            }
            let run = RunFinished {
                stats: stats(scenario.name, scenario.drill),
                purpose: RunPurpose::Routine,
                drill: scenario.spec(),
                duration: SCENARIO_SECS,
            };
            let timestamp = 1000 + index as u64;
            app.world_mut().write_message(RunRecorded { run, previous: Comparison::default(), timestamp });
            app.update();
        }
        let logged = app.world().resource::<BenchmarkLog>().entries().len();
        assert_eq!(BenchmarkLog::load(&dir.join("benchmark.csv")).entries().len(), logged);
        logged
    }

    #[test]
    fn only_a_benchmark_played_to_the_end_is_logged() {
        assert_eq!(play(None, SCENARIOS.len()), 1);
        // Ended part-way, or with a step skipped: nothing.
        assert_eq!(play(None, SCENARIOS.len() - 1), 0);
        assert_eq!(play(Some(3), SCENARIOS.len()), 0);
        assert_eq!(play(Some(SCENARIOS.len() - 1), SCENARIOS.len()), 0);
    }
}
