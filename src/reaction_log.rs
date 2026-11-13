//! Reaction test results: every finished test, one row each in a small CSV of its own, as the
//! history CSV's columns are frozen. A row has the timestamp of the run's history row (which
//! carries the score), so the two join, and every scored trial, so a test can be looked at again
//! and compared with the ones before it.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::drill::kinds::reaction_test::{MIN_VALID, ReactionTest, Trial, summary, valid_ms};
use crate::drill::{DrillKind, DrillSet};
use crate::history::{RunRecorded, append_csv_row, csv_safe};
use crate::metrics::reaction::Summary;
use crate::settings::Settings;

const REACTION_PATH: &str = "aim_trainer_reaction.csv";
/// Persisted: never change the columns. `median_ms` is there for whoever opens the file; the
/// game works everything out from `trials_ms` (reaction times in trial order, `;` between them,
/// `lapse` for a trial without a click). All times are as measured, before `offset_ms` (the
/// latency offset setting at the time) is taken off.
const HEADER: &str = "unix_time,drill,median_ms,false_starts,frame_ms,offset_ms,trials_ms";
const LAPSE: &str = "lapse";

pub struct ReactionLogPlugin;

impl Plugin for ReactionLogPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ReactionLog::load(Path::new(REACTION_PATH)))
            .add_systems(Update, record_reaction_test.in_set(DrillSet::Outcome));
    }
}

/// One finished reaction test.
#[derive(Clone, Debug, PartialEq)]
pub struct ReactionEntry {
    /// The run's history timestamp.
    pub timestamp: u64,
    /// The run's history name.
    pub drill: String,
    /// The scored trials in order, as measured.
    pub trials: Vec<Trial>,
    /// False starts in scored trials.
    pub false_starts: u32,
    /// The run's mean frame time, ms: a trial is timed to within half of it.
    pub frame_ms: f32,
    /// The latency offset setting when the test was taken, ms.
    pub offset_ms: f32,
}

impl ReactionEntry {
    /// Median, quartiles, best, mean and SD of the valid trials, as measured; `None` without any.
    pub fn summary(&self) -> Option<Summary> {
        summary(&self.trials)
    }

    /// Whether the test has enough valid trials for its median to be compared with others.
    pub fn comparable(&self) -> bool {
        valid_ms(&self.trials).len() >= MIN_VALID
    }

    /// One CSV row (without the newline) in `HEADER`'s column order.
    fn row(&self) -> String {
        let median = self.summary().map_or(String::new(), |summary| format!("{:.1}", summary.median));
        let trials: Vec<String> = self
            .trials
            .iter()
            .map(|trial| match trial {
                Trial::Valid(ms) => format!("{ms:.1}"),
                Trial::Lapse => LAPSE.to_string(),
            })
            .collect();
        format!(
            "{},{},{median},{},{:.2},{:.0},{}",
            self.timestamp,
            csv_safe(&self.drill),
            self.false_starts,
            self.frame_ms,
            self.offset_ms,
            trials.join(";")
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let cols: Vec<&str> = line.split(',').collect();
        let [timestamp, drill, _median, false_starts, frame_ms, offset_ms, trials] = cols[..] else { return None };
        let trial = |text: &str| if text == LAPSE { Some(Trial::Lapse) } else { text.parse().ok().map(Trial::Valid) };
        Some(Self {
            timestamp: timestamp.parse().ok()?,
            drill: drill.to_string(),
            trials: trials.split(';').filter(|text| !text.is_empty()).map(trial).collect::<Option<_>>()?,
            false_starts: false_starts.parse().ok()?,
            frame_ms: frame_ms.parse().ok()?,
            offset_ms: offset_ms.parse().ok()?,
        })
    }
}

/// Every reaction test so far, oldest first.
#[derive(Resource)]
pub struct ReactionLog {
    entries: Vec<ReactionEntry>,
    /// The CSV file results are appended to.
    path: PathBuf,
}

impl ReactionLog {
    /// Reads the results kept in the CSV at `path` (empty if there is none yet).
    pub fn load(path: &Path) -> Self {
        let entries = fs::read_to_string(path)
            .map(|text| text.lines().skip(1).filter_map(ReactionEntry::parse).collect())
            .unwrap_or_default();
        Self { entries, path: path.to_path_buf() }
    }

    /// Appends a result to the CSV and the in-memory log.
    pub fn record(&mut self, entry: &ReactionEntry) {
        let row = entry.row();
        if let Err(err) = append_csv_row(&self.path, HEADER, &row) {
            error!("Failed to write {}: {err}", self.path.display());
        }
        self.entries.extend(ReactionEntry::parse(&row));
    }

    /// The result of the run with this history timestamp.
    pub fn entry(&self, timestamp: u64) -> Option<&ReactionEntry> {
        self.entries.iter().rev().find(|e| e.timestamp == timestamp)
    }

    /// Earlier tests of the same drill with a median worth comparing against, oldest first.
    fn earlier<'a>(&'a self, entry: &'a ReactionEntry) -> impl DoubleEndedIterator<Item = &'a ReactionEntry> {
        self.entries.iter().filter(move |e| e.timestamp < entry.timestamp && e.drill == entry.drill && e.comparable())
    }

    /// The latest earlier test to compare with.
    pub fn previous<'a>(&'a self, entry: &'a ReactionEntry) -> Option<&'a ReactionEntry> {
        self.earlier(entry).next_back()
    }

    /// The earlier test with the lowest median.
    pub fn best_before<'a>(&'a self, entry: &'a ReactionEntry) -> Option<&'a ReactionEntry> {
        let median = |e: &ReactionEntry| e.summary().map_or(f32::MAX, |summary| summary.median);
        self.earlier(entry).min_by(|a, b| median(a).total_cmp(&median(b)))
    }
}

/// Logs a finished reaction test with its history timestamp.
pub(crate) fn record_reaction_test(
    mut recorded: MessageReader<RunRecorded>,
    test: Res<ReactionTest>,
    settings: Res<Settings>,
    mut log: ResMut<ReactionLog>,
) {
    for recorded in recorded.read().filter(|r| r.run.stats.kind == DrillKind::ReactionTest) {
        log.record(&ReactionEntry {
            timestamp: recorded.timestamp,
            drill: recorded.run.stats.drill.clone(),
            trials: test.trials().to_vec(),
            false_starts: test.false_starts(),
            frame_ms: test.frame_ms(),
            offset_ms: settings.latency_offset_ms,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    /// A test of 30 trials around `median` ms (25 valid ones unless `lapses` says otherwise).
    fn entry(timestamp: u64, drill: &str, median: f32, lapses: usize) -> ReactionEntry {
        let trials =
            (0..30)
                .map(|i| {
                    if i < lapses {
                        Trial::Lapse
                    } else {
                        Trial::Valid(median + (i as f32 - 14.5 - lapses as f32 * 0.5))
                    }
                })
                .collect();
        ReactionEntry { timestamp, drill: drill.to_string(), trials, false_starts: 2, frame_ms: 5.62, offset_ms: 20.0 }
    }

    #[test]
    fn results_round_trip_through_the_file() {
        let dir = TempDir::new("reaction_log");
        let path = dir.join("reaction.csv");
        let mut log = ReactionLog::load(&path);
        assert!(log.entries.is_empty());
        let short = ReactionEntry {
            trials: vec![Trial::Valid(213.4), Trial::Lapse, Trial::Valid(198.2)],
            ..entry(100, "Reaction test", 0.0, 0)
        };
        let full = entry(200, "Reaction test", 220.0, 5);
        let empty = ReactionEntry { trials: Vec::new(), ..entry(300, "My, test", 0.0, 0) };
        for e in [&short, &full, &empty] {
            log.record(e);
        }
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().next(), Some(HEADER));
        assert_eq!(text.lines().nth(1), Some("100,Reaction test,205.8,2,5.62,20,213.4;lapse;198.2"));
        // A test without a single scored trial (the attempts ran out) has no median to write.
        assert_eq!(text.lines().nth(3), Some("300,My  test,,2,5.62,20,"));

        let loaded = ReactionLog::load(&path);
        assert_eq!(loaded.entries.len(), 3);
        assert_eq!(loaded.entry(100), Some(&short));
        assert_eq!(loaded.entry(200), Some(&full));
        assert_eq!((full.trials.len(), full.summary().unwrap().count), (30, 25), "lapses aren't reactions");
        assert_eq!(loaded.entry(300).unwrap().trials, []);
        assert!(loaded.entry(300).unwrap().summary().is_none());
        // What the running game keeps is what a restart reads back.
        assert_eq!(log.entries, loaded.entries);
    }

    #[test]
    fn compares_with_earlier_tests_that_have_enough_trials() {
        let dir = TempDir::new("reaction_compare");
        let mut log = ReactionLog::load(&dir.join("reaction.csv"));
        let fast = entry(100, "Reaction test", 205.0, 0);
        let slow = entry(200, "Reaction test", 231.0, 0);
        // 12 lapses leave 18 valid trials: too few to compare with, however fast.
        let rough = entry(300, "Reaction test", 150.0, 12);
        let other = entry(400, "Custom test", 180.0, 0);
        let latest = entry(500, "Reaction test", 212.0, 0);
        for e in [&fast, &slow, &rough, &other, &latest] {
            log.record(e);
        }
        assert!(!rough.comparable() && latest.comparable());
        assert_eq!(log.previous(&latest), Some(&slow));
        assert_eq!(log.best_before(&latest), Some(&fast));
        assert_eq!((log.previous(&fast), log.best_before(&fast)), (None, None));
        // Another drill of the same kind (a custom scenario) has its own results.
        assert_eq!(log.previous(&other), None);
    }

    /// A finished Reaction test is logged with its history timestamp, its trials and the offset
    /// set at the time; other drills aren't.
    #[test]
    fn finished_tests_are_logged() {
        use crate::drill::{DrillSpec, Run, RunFinished, RunPurpose};
        use crate::history::Comparison;

        let dir = TempDir::new("reaction_record");
        let path = dir.join("reaction.csv");
        let settings = Settings { latency_offset_ms: 15.0, ..Settings::default() };
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<RunRecorded>()
            .insert_resource(ReactionLog::load(&path))
            .insert_resource(settings.clone())
            .init_resource::<ReactionTest>()
            .add_systems(Update, record_reaction_test);
        let mut finish = |kind: DrillKind, timestamp: u64| {
            let drill = DrillSpec::builtin(kind, &settings);
            let run = Run::new(drill.clone(), 0.4, 60.0, RunPurpose::Practice, String::new());
            let finished =
                RunFinished { stats: run.stats(&settings), purpose: RunPurpose::Practice, drill, duration: 60.0 };
            app.world_mut().write_message(RunRecorded { run: finished, previous: Comparison::default(), timestamp });
            app.update();
        };
        finish(DrillKind::ReactionTest, 42);
        finish(DrillKind::Reaction, 43);
        let loaded = ReactionLog::load(&path);
        assert_eq!(loaded.entries.len(), 1);
        let entry = loaded.entry(42).expect("logged");
        assert_eq!((entry.drill.as_str(), entry.offset_ms, entry.false_starts), ("Reaction test", 15.0, 0));
    }
}
