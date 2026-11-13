//! Calm aim results: what every finished Calm aim run measured, one row each in a small CSV of
//! its own, as the history CSV's columns are frozen. A row has the timestamp of the run's
//! history row (which also names its replay), so the two join; the history row carries the
//! score. The results screen reads a run's breakdown from here and compares it with the one
//! before, also after a restart.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::drill::kinds::calm_aim::CalmRun;
use crate::drill::{DrillKind, DrillSet};
use crate::history::{RunRecorded, append_csv_row, csv_safe};
use crate::metrics::calm::{CalmResult, Flicks, Part, Settling, Smoothness, Stillness};

const CALM_PATH: &str = "aim_trainer_calm.csv";
/// Persisted: never rename or reorder. The five scores are there for people reading the file;
/// loading works them out again from the measures after them.
const HEADER: &str = "unix_time,drill,score,smoothness,stillness,flick,settle,\
smooth_jitter_deg,smooth_with_target,smooth_error_deg,smooth_on_target,\
still_waits,still_wait_s,still_drift_deg,still_travel_deg,still_early_shots,still_targets,still_moves,still_expired,\
flick_kills,flick_expired,flick_shots,flick_moving_shots,flick_overshot,flick_corrections,\
settle_targets,settle_expired,settle_s,settle_exits";

pub struct CalmLogPlugin;

impl Plugin for CalmLogPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CalmLog::load(Path::new(CALM_PATH)))
            .add_systems(Update, record_calm.in_set(DrillSet::Outcome));
    }
}

/// One finished Calm aim run.
#[derive(Clone, Debug, PartialEq)]
pub struct CalmEntry {
    /// The run's history timestamp.
    pub timestamp: u64,
    /// The run's history name.
    pub drill: String,
    pub result: CalmResult,
}

impl CalmEntry {
    /// One CSV row (without the newline) in `HEADER`'s column order.
    fn row(&self) -> String {
        let CalmResult { smoothness: smooth, stillness: still, flicks, settling } = &self.result;
        let scores = Part::ALL.map(|part| self.result.score(part).to_string()).join(",");
        format!(
            "{},{},{},{scores},{:.4},{:.4},{:.4},{:.4},{},{:.2},{:.4},{:.3},{},{},{},{},{},{},{},{},{},{},{},{},{:.4},{}",
            self.timestamp,
            csv_safe(&self.drill),
            self.result.overall(),
            smooth.jitter,
            smooth.with_target,
            smooth.error,
            smooth.on_target,
            still.waits,
            still.wait_secs,
            still.drift,
            still.travel,
            still.early_shots,
            still.targets,
            still.moves,
            still.expired,
            flicks.kills,
            flicks.expired,
            flicks.shots,
            flicks.moving_shots,
            flicks.overshot,
            flicks.corrections,
            settling.targets,
            settling.expired,
            settling.secs,
            settling.exits
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let cols: Vec<&str> = line.split(',').collect();
        let [
            timestamp,
            drill,
            _score,
            _smoothness,
            _stillness,
            _flick,
            _settle,
            jitter,
            with_target,
            error,
            on_target,
            waits,
            wait_secs,
            drift,
            travel,
            early_shots,
            still_targets,
            moves,
            still_expired,
            kills,
            flick_expired,
            shots,
            moving_shots,
            overshot,
            corrections,
            settle_targets,
            settle_expired,
            settle_secs,
            exits,
        ] = cols[..]
        else {
            return None;
        };
        // A measure that isn't a number would make every score built on it one too.
        let measure = |text: &str| text.parse::<f32>().ok().filter(|value| value.is_finite());
        let count = |text: &str| text.parse::<u32>().ok();
        Some(Self {
            timestamp: timestamp.parse().ok()?,
            drill: drill.to_string(),
            result: CalmResult {
                smoothness: Smoothness {
                    jitter: measure(jitter)?,
                    with_target: measure(with_target)?,
                    error: measure(error)?,
                    on_target: measure(on_target)?,
                },
                stillness: Stillness {
                    waits: count(waits)?,
                    wait_secs: measure(wait_secs)?,
                    drift: measure(drift)?,
                    travel: measure(travel)?,
                    early_shots: count(early_shots)?,
                    targets: count(still_targets)?,
                    moves: count(moves)?,
                    expired: count(still_expired)?,
                },
                flicks: Flicks {
                    kills: count(kills)?,
                    expired: count(flick_expired)?,
                    shots: count(shots)?,
                    moving_shots: count(moving_shots)?,
                    overshot: count(overshot)?,
                    corrections: count(corrections)?,
                },
                settling: Settling {
                    targets: count(settle_targets)?,
                    expired: count(settle_expired)?,
                    secs: measure(settle_secs)?,
                    exits: count(exits)?,
                },
            },
        })
    }
}

/// Every Calm aim result so far, oldest first.
#[derive(Resource)]
pub struct CalmLog {
    entries: Vec<CalmEntry>,
    /// The CSV file results are appended to.
    path: PathBuf,
}

impl CalmLog {
    /// Reads the results kept in the CSV at `path` (empty if there is none yet).
    pub fn load(path: &Path) -> Self {
        let entries = fs::read_to_string(path)
            .map(|text| text.lines().skip(1).filter_map(CalmEntry::parse).collect())
            .unwrap_or_default();
        Self { entries, path: path.to_path_buf() }
    }

    /// Appends a result to the CSV and the in-memory log, as it reads back from the file.
    pub fn record(&mut self, entry: &CalmEntry) {
        let row = entry.row();
        if let Err(err) = append_csv_row(&self.path, HEADER, &row) {
            error!("Failed to write {}: {err}", self.path.display());
        }
        self.entries.extend(CalmEntry::parse(&row));
    }

    /// Adds a result to the in-memory log only: for screens staged with made-up numbers.
    pub fn stage(&mut self, entry: CalmEntry) {
        self.entries.push(entry);
    }

    /// The result of the run with this history timestamp.
    pub fn entry(&self, timestamp: u64) -> Option<&CalmEntry> {
        self.entries.iter().rev().find(|e| e.timestamp == timestamp)
    }

    /// The latest earlier result of the same drill (the test itself, or the same custom
    /// scenario of it), to compare against.
    pub fn previous(&self, entry: &CalmEntry) -> Option<&CalmEntry> {
        self.entries.iter().rev().find(|e| e.timestamp < entry.timestamp && e.drill == entry.drill)
    }
}

/// Logs a finished Calm aim run's result.
pub(crate) fn record_calm(mut recorded: MessageReader<RunRecorded>, calm: Res<CalmRun>, mut log: ResMut<CalmLog>) {
    for recorded in recorded.read().filter(|r| r.run.stats.kind == DrillKind::CalmAim) {
        let Some(&result) = calm.result() else { continue };
        log.record(&CalmEntry { timestamp: recorded.timestamp, drill: recorded.run.stats.drill.clone(), result });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::calm::tests::{calm, tense};
    use crate::test_util::TempDir;

    #[test]
    fn results_round_trip_through_the_file() {
        let dir = TempDir::new("calm_log");
        let path = dir.join("calm.csv");
        let mut log = CalmLog::load(&path);
        assert!(log.entries.is_empty());
        let mixed = CalmResult { settling: tense().settling, ..calm() };
        let first = CalmEntry { timestamp: 100, drill: "Calm aim".into(), result: tense() };
        let custom = CalmEntry { timestamp: 150, drill: "Calm, big".into(), result: calm() };
        let second = CalmEntry { timestamp: 200, drill: "Calm aim".into(), result: mixed };
        for entry in [&first, &custom, &second] {
            log.record(entry);
        }
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().next(), Some(HEADER));
        assert_eq!(
            text.lines().nth(3),
            Some(
                "200,Calm aim,75,100,100,100,0,0.0300,1.0000,0.2000,0.9500,5,12.00,0.0500,1.000,0,5,5,0,12,0,12,0,1,1,8,1,0.4500,9"
            )
        );

        let loaded = CalmLog::load(&path);
        assert_eq!(loaded.entries.len(), 3);
        for entry in [&first, &second] {
            assert_eq!(loaded.entry(entry.timestamp), Some(entry));
            // What the file holds scores the same as what was played.
            assert_eq!(loaded.entry(entry.timestamp).unwrap().result.overall(), entry.result.overall());
        }
        // Names are made CSV-safe, like the history's.
        assert_eq!(loaded.entry(150).unwrap().drill, "Calm  big");
        // Compared with the last result of the same drill only.
        assert_eq!(loaded.previous(&second), Some(&first));
        assert_eq!(loaded.previous(&first), None);
        assert_eq!(loaded.previous(loaded.entry(150).unwrap()), None);
        // A damaged row is skipped, not misread.
        fs::write(&path, format!("{text}300,Calm aim,75,100\n300,Calm aim,nan,{}\n", ",x".repeat(26))).unwrap();
        assert_eq!(CalmLog::load(&path).entries.len(), 3);
    }
}
