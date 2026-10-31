//! Adaptive-mode results: the θ75 estimate of every finished adaptive run, one row each in a
//! small CSV of its own, as the history CSV's columns are frozen. A row has the timestamp of the
//! run's history row (which also names its replay), so the two join; the history row itself
//! carries the score under the adaptive drill name (e.g. `Microflick · adaptive · measure`).
//! Each task's latest θ75 becomes the prior of its next run.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::drill::DrillSet;
use crate::drill::adaptive::{AdaptiveRun, quest_task};
use crate::history::{RunRecorded, append_csv_row, csv_safe};
use crate::metrics::quest::{Estimate, QuestMode};

const ADAPTIVE_PATH: &str = "aim_trainer_adaptive.csv";
const HEADER: &str = "unix_time,drill,mode,time_limit_s,targets,hits,theta75_deg,low_deg,high_deg,slope";

pub struct AdaptiveLogPlugin;

impl Plugin for AdaptiveLogPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(AdaptiveLog::load(Path::new(ADAPTIVE_PATH)))
            .add_systems(Startup, seed_priors)
            .add_systems(Update, record_adaptive.in_set(DrillSet::Outcome));
    }
}

/// One finished adaptive run.
#[derive(Clone, Debug, PartialEq)]
pub struct AdaptiveEntry {
    /// The run's history timestamp.
    pub timestamp: u64,
    /// The run's history name.
    pub drill: String,
    pub mode: QuestMode,
    /// Seconds each target had to be hit in.
    pub time_limit: f32,
    pub estimate: Estimate,
}

impl AdaptiveEntry {
    /// One CSV row (without the newline) in `HEADER`'s column order.
    fn row(&self) -> String {
        let e = &self.estimate;
        format!(
            "{},{},{},{:.2},{},{},{:.4},{:.4},{:.4},{:.4}",
            self.timestamp,
            csv_safe(&self.drill),
            self.mode.name(),
            self.time_limit,
            e.trials,
            e.hits,
            e.theta75,
            e.low,
            e.high,
            e.slope
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let cols: Vec<&str> = line.split(',').collect();
        let [timestamp, drill, mode, time_limit, trials, hits, theta75, low, high, slope] = cols[..] else {
            return None;
        };
        Some(Self {
            timestamp: timestamp.parse().ok()?,
            drill: drill.to_string(),
            mode: QuestMode::ALL.into_iter().find(|m| m.name() == mode)?,
            time_limit: time_limit.parse().ok()?,
            estimate: Estimate {
                theta75: theta75.parse().ok()?,
                low: low.parse().ok()?,
                high: high.parse().ok()?,
                slope: slope.parse().ok()?,
                trials: trials.parse().ok()?,
                hits: hits.parse().ok()?,
            },
        })
    }
}

/// Every adaptive result so far, oldest first.
#[derive(Resource)]
pub struct AdaptiveLog {
    entries: Vec<AdaptiveEntry>,
    /// The CSV file results are appended to.
    path: PathBuf,
}

impl AdaptiveLog {
    /// Reads the results kept in the CSV at `path` (empty if there is none yet).
    pub fn load(path: &Path) -> Self {
        let entries = fs::read_to_string(path)
            .map(|text| text.lines().skip(1).filter_map(AdaptiveEntry::parse).collect())
            .unwrap_or_default();
        Self { entries, path: path.to_path_buf() }
    }

    /// Appends a result to the CSV and the in-memory log.
    pub fn record(&mut self, entry: &AdaptiveEntry) {
        let row = entry.row();
        if let Err(err) = append_csv_row(&self.path, HEADER, &row) {
            error!("Failed to write {}: {err}", self.path.display());
        }
        self.entries.extend(AdaptiveEntry::parse(&row));
    }

    /// The result of the run with this history timestamp.
    pub fn entry(&self, timestamp: u64) -> Option<&AdaptiveEntry> {
        self.entries.iter().rev().find(|e| e.timestamp == timestamp)
    }

    /// The latest earlier result of the same task (either mode), to compare against.
    pub fn previous(&self, entry: &AdaptiveEntry) -> Option<&AdaptiveEntry> {
        let task = quest_task(&entry.drill);
        self.entries.iter().rev().find(|e| e.timestamp < entry.timestamp && quest_task(&e.drill) == task)
    }
}

/// Each task's latest θ75 becomes the prior of its next run.
fn seed_priors(log: Res<AdaptiveLog>, mut adaptive: ResMut<AdaptiveRun>) {
    for entry in &log.entries {
        adaptive.priors.insert(quest_task(&entry.drill).to_string(), entry.estimate.theta75);
    }
}

/// Logs a finished adaptive run's final estimate.
pub(crate) fn record_adaptive(
    mut recorded: MessageReader<RunRecorded>,
    mut adaptive: ResMut<AdaptiveRun>,
    mut log: ResMut<AdaptiveLog>,
) {
    for recorded in recorded.read() {
        let config = &recorded.run.drill.config;
        let (Some(mode), Some(estimate)) = (config.adaptive(), adaptive.estimate()) else { continue };
        if estimate.trials == 0 {
            continue;
        }
        let entry = AdaptiveEntry {
            timestamp: recorded.timestamp,
            drill: recorded.run.stats.drill.clone(),
            mode,
            time_limit: config.time_limit(),
            estimate,
        };
        adaptive.priors.insert(quest_task(&entry.drill).to_string(), estimate.theta75);
        log.record(&entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("aim_trainer_{name}_{}_{nanos}.csv", std::process::id()))
    }

    fn entry(timestamp: u64, drill: &str, mode: QuestMode, theta75: f32) -> AdaptiveEntry {
        AdaptiveEntry {
            timestamp,
            drill: drill.to_string(),
            mode,
            time_limit: 1.5,
            estimate: Estimate { theta75, low: 0.5, high: 1.25, slope: 0.25, trials: 70, hits: 50 },
        }
    }

    #[test]
    fn results_round_trip_through_the_file() {
        let path = temp_path("adaptive_log");
        let mut log = AdaptiveLog::load(&path);
        assert!(log.entries.is_empty());
        let first = entry(100, "Microflick · adaptive · measure", QuestMode::Measure, 0.62);
        let second = entry(200, "Microflick · adaptive · train", QuestMode::Train, 0.55);
        let other = entry(300, "Weak spot: up, 15-25° · adaptive", QuestMode::Train, 0.8);
        for e in [&first, &second, &other] {
            log.record(e);
        }
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().next(), Some(HEADER));
        assert_eq!(
            text.lines().nth(1),
            Some("100,Microflick · adaptive · measure,Measure,1.50,70,50,0.6200,0.5000,1.2500,0.2500")
        );

        let loaded = AdaptiveLog::load(&path);
        assert_eq!(loaded.entries.len(), 3);
        assert_eq!(loaded.entry(100), Some(&first));
        // Names are made CSV-safe, like the history's.
        assert_eq!(loaded.entry(300).unwrap().drill, "Weak spot: up  15-25° · adaptive");
        // Train compares against the last Measure of the same task, and vice versa.
        assert_eq!(loaded.previous(&second), Some(&first));
        assert_eq!(loaded.previous(&first), None);
        assert_eq!(loaded.previous(loaded.entry(300).unwrap()), None);
        let _ = fs::remove_file(&path);
    }

    /// A finished adaptive run is logged with its history timestamp and becomes the task's
    /// prior; a fixed-size run isn't logged.
    #[test]
    fn finished_adaptive_runs_are_logged() {
        use crate::drill::{DrillKind, DrillSpec, Run, RunFinished, RunPurpose};
        use crate::history::Comparison;
        use crate::settings::Settings;

        let path = temp_path("adaptive_record");
        let settings = Settings::default();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<RunRecorded>()
            .insert_resource(AdaptiveLog::load(&path))
            .init_resource::<AdaptiveRun>()
            .add_systems(Update, record_adaptive);
        let finish = |app: &mut App, drill: DrillSpec, timestamp: u64| {
            let run = Run::new(drill.clone(), 0.4, 90.0, RunPurpose::Practice, String::new());
            let mut adaptive = app.world_mut().resource_mut::<AdaptiveRun>();
            adaptive.begin(&run);
            for hit in [true, true, false] {
                adaptive.next_target();
                adaptive.outcome(hit, true);
            }
            let finished =
                RunFinished { stats: run.stats(&settings), purpose: RunPurpose::Practice, drill, duration: 90.0 };
            app.world_mut().write_message(RunRecorded { run: finished, previous: Comparison::default(), timestamp });
            app.update();
        };
        finish(&mut app, DrillSpec::adaptive(DrillKind::Microflick, QuestMode::Measure, &settings), 42);
        finish(&mut app, DrillSpec::builtin(DrillKind::Microflick, &settings), 43);

        let log = app.world().resource::<AdaptiveLog>();
        let entry = log.entry(42).expect("logged");
        assert_eq!(
            (entry.drill.as_str(), entry.mode, entry.time_limit),
            ("Microflick · adaptive · measure", QuestMode::Measure, 1.5)
        );
        assert_eq!((entry.estimate.trials, entry.estimate.hits), (3, 2));
        assert!(log.entry(43).is_none());
        let prior = app.world().resource::<AdaptiveRun>().priors["Microflick · adaptive"];
        assert!((prior - entry.estimate.theta75).abs() < 1e-3);
        assert_eq!(AdaptiveLog::load(&path).entries.len(), 1);
        let _ = fs::remove_file(&path);
    }
}
