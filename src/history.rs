//! Run history: every finished drill is appended to a CSV and kept in memory for comparisons.
//! Records each [`RunFinished`] and announces it as [`RunRecorded`] for the systems reacting
//! to finished runs (replay saving, finders, routines, the results screen).

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::prelude::*;

use crate::{
    drill::{DrillKind, DrillSet, DrillSpec, RunFinished, RunPurpose, RunStats},
    settings::Settings,
};

const RESULTS_PATH: &str = "aim_trainer_results.csv";
const HEADER: &str = "unix_time,purpose,drill,sens,yaw,dpi,hfov,cm_per_360,duration_s,hits,shots,accuracy,avg_kill_ms,on_target_pct,score,kind";
/// How many recent practice runs the "recent average" covers.
pub const RECENT_RUNS: usize = 10;

pub struct HistoryPlugin;

impl Plugin for HistoryPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(History::load(Path::new(RESULTS_PATH)))
            .add_message::<RunRecorded>()
            .add_systems(Update, record_finished.in_set(DrillSet::Log));
    }
}

/// A finished run, after it was written to the history.
#[derive(Message, Clone, Debug)]
pub struct RunRecorded {
    pub run: RunFinished,
    /// Earlier practice runs of the same drill, not including this one.
    pub previous: Comparison,
    /// History timestamp, which also names the run's replay.
    pub timestamp: u64,
}

/// The most recent finished practice run, shown on the results screen.
#[derive(Resource)]
pub struct LastRun {
    pub stats: RunStats,
    /// Earlier practice runs of the same drill, not including this one.
    pub previous: Comparison,
    /// History timestamp, which also identifies the run's replay.
    pub timestamp: u64,
    /// What was played, so "Retry" replays the same (possibly custom) drill.
    pub drill: DrillSpec,
    pub duration: f32,
}

/// Writes every finished run to the history (routine steps as practice).
pub(crate) fn record_finished(
    mut finished: MessageReader<RunFinished>,
    mut history: ResMut<History>,
    settings: Res<Settings>,
    mut recorded: MessageWriter<RunRecorded>,
) {
    for run in finished.read() {
        let previous = history.compare(&run.stats.drill);
        let timestamp = history.record(&run.stats, run.purpose.logged_as(), &settings);
        recorded.write(RunRecorded { run: run.clone(), previous, timestamp });
    }
}

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub timestamp: u64,
    pub purpose: RunPurpose,
    /// Drill name (built-in or custom).
    pub drill: String,
    /// Kind of drill, which decides which stats are meaningful.
    pub kind: DrillKind,
    pub sens: f32,
    pub cm_per_360: f32,
    pub accuracy: f32,
    pub avg_kill_ms: f32,
    pub on_target_pct: f32,
    pub score: f32,
}

impl HistoryEntry {
    /// Parses a CSV row. Rows from before custom scenarios have 15 columns and no `kind`.
    fn parse(line: &str) -> Option<Self> {
        let cols: Vec<&str> = line.split(',').collect();
        let kind = match cols.len() {
            15 => DrillKind::from_name(cols[2])?,
            16 => DrillKind::from_name(cols[15])?,
            _ => return None,
        };
        Some(Self {
            timestamp: cols[0].parse().ok()?,
            purpose: RunPurpose::from_csv(cols[1])?,
            drill: cols[2].to_string(),
            kind,
            sens: cols[3].parse().ok()?,
            cm_per_360: cols[7].parse().ok()?,
            accuracy: cols[11].parse().ok()?,
            avg_kill_ms: cols[12].parse().ok()?,
            on_target_pct: cols[13].parse().ok()?,
            score: cols[14].parse().ok()?,
        })
    }
}

/// Drill names are user-chosen; keep them from breaking the CSV. Lookups go through this too, so
/// a name containing these characters still finds its stored runs.
pub fn csv_safe(name: &str) -> String {
    name.replace([',', '\n', '\r'], " ")
}

fn mean(values: &[f32]) -> Option<f32> {
    (!values.is_empty()).then(|| values.iter().sum::<f32>() / values.len() as f32)
}

/// Percent change from your first few runs to your latest few (up to 5 each, never overlapping),
/// so a single lucky or bad run doesn't dominate.
pub fn overall_change(scores: &[f32]) -> Option<f32> {
    let k = (scores.len() / 2).min(5);
    let first = mean(&scores[..k])?;
    let last = mean(&scores[scores.len() - k..])?;
    (first > 0.0).then(|| (last / first - 1.0) * 100.0)
}

/// Percent change of the average of the latest `RECENT_RUNS` scores versus the `RECENT_RUNS`
/// before them. `None` until there is at least one earlier run to compare against.
pub fn recent_change(scores: &[f32]) -> Option<f32> {
    let split = scores.len().checked_sub(RECENT_RUNS).filter(|&s| s > 0)?;
    let recent = mean(&scores[split..])?;
    let previous = mean(&scores[split.saturating_sub(RECENT_RUNS)..split])?;
    (previous > 0.0).then(|| (recent / previous - 1.0) * 100.0)
}

/// How a run stacks up against earlier practice runs of the same drill.
#[derive(Clone, Debug, Default)]
pub struct Comparison {
    pub best: Option<f32>,
    pub recent_avg: Option<f32>,
    pub recent_count: usize,
}

#[derive(Resource)]
pub struct History {
    entries: Vec<HistoryEntry>,
    /// The CSV file runs are appended to.
    path: PathBuf,
}

impl History {
    /// Reads the history kept in the CSV at `path` (empty if there is none yet).
    pub fn load(path: &Path) -> Self {
        let entries = fs::read_to_string(path)
            .map(|text| text.lines().skip(1).filter_map(HistoryEntry::parse).collect())
            .unwrap_or_default();
        Self { entries, path: path.to_path_buf() }
    }

    /// Practice runs of a drill (by name), oldest first. Finder trials are excluded since
    /// they're played at deliberately wrong sensitivities.
    pub fn practice_runs<'a>(&'a self, drill: &str) -> impl DoubleEndedIterator<Item = &'a HistoryEntry> + use<'a> {
        let drill = csv_safe(drill);
        self.entries
            .iter()
            .filter(move |e| e.drill == drill && e.purpose == RunPurpose::Practice)
    }

    /// Scores of the practice runs of a drill, oldest first.
    pub fn scores(&self, drill: &str) -> Vec<f32> {
        self.practice_runs(drill).map(|e| e.score).collect()
    }

    /// Names of every drill with practice runs, built-ins first, then others in order of
    /// their first run.
    pub fn drills(&self) -> Vec<String> {
        let mut names: Vec<String> = DrillKind::ALL.iter().map(|s| s.name().to_string()).collect();
        for entry in &self.entries {
            if entry.purpose == RunPurpose::Practice && !names.contains(&entry.drill) {
                names.push(entry.drill.clone());
            }
        }
        names
    }

    pub fn compare(&self, drill: &str) -> Comparison {
        let best = self.practice_runs(drill).map(|e| e.score).reduce(f32::max);
        let recent: Vec<f32> = self.practice_runs(drill).rev().take(RECENT_RUNS).map(|e| e.score).collect();
        let recent_avg = (!recent.is_empty()).then(|| recent.iter().sum::<f32>() / recent.len() as f32);
        Comparison { best, recent_avg, recent_count: recent.len() }
    }

    /// Appends a finished run to the CSV and the in-memory history. Returns the run's timestamp,
    /// which also names its replay file, so it is kept unique.
    pub fn record(&mut self, stats: &RunStats, purpose: RunPurpose, settings: &Settings) -> u64 {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let timestamp = self.entries.last().map_or(now, |last| now.max(last.timestamp.saturating_add(1)));
        let row = format_row(timestamp, stats, purpose, settings);
        if let Err(err) = append_row(&self.path, &row) {
            error!("Failed to write {}: {err}", self.path.display());
        }
        if let Some(entry) = HistoryEntry::parse(&row) {
            self.entries.push(entry);
        }
        timestamp
    }
}

/// One CSV row (without the newline) in `HEADER`'s column order.
fn format_row(timestamp: u64, stats: &RunStats, purpose: RunPurpose, settings: &Settings) -> String {
    format!(
        "{timestamp},{},{},{},{},{},{},{:.3},{:.2},{},{},{:.4},{:.1},{:.2},{},{}",
        purpose.csv_name(),
        csv_safe(&stats.drill),
        stats.sens,
        settings.yaw,
        settings.dpi,
        settings.hfov,
        stats.cm_per_360,
        stats.duration,
        stats.hits,
        stats.shots,
        stats.accuracy(),
        stats.avg_kill_ms(),
        stats.on_target_ratio() * 100.0,
        stats.score(),
        stats.kind.name(),
    )
}

fn append_row(path: &Path, row: &str) -> std::io::Result<()> {
    let needs_header = fs::metadata(path).map_or(true, |meta| meta.len() == 0);
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if needs_header {
        writeln!(file, "{HEADER}")?;
    }
    writeln!(file, "{row}")
}

/// `YYYY-MM-DD` (UTC) for a unix timestamp. Howard Hinnant's days-to-civil algorithm.
pub fn format_date(timestamp: u64) -> String {
    let days = (timestamp / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(purpose: &str, scenario: &str, score: f32) -> String {
        format!("1793394818,{purpose},{scenario},0.4,0.07,800,103,40.821,60.00,10,12,0.8333,500.0,0.00,{score}")
    }

    #[test]
    fn parses_old_and_new_rows_and_skips_garbage() {
        assert!(HistoryEntry::parse(&entry("Practice", "Gridshot", 123.0)).is_some());
        // Custom drill: name in column 3, kind in the extra 16th column.
        let custom = HistoryEntry::parse(&format!("{},Gridshot", entry("Practice", "My 5x5", 1.0))).unwrap();
        assert_eq!((custom.drill.as_str(), custom.kind), ("My 5x5", DrillKind::Gridshot));
        assert!(HistoryEntry::parse("not,a,row").is_none());
        // Old rows must name a built-in drill, since they carry no kind.
        assert!(HistoryEntry::parse(&entry("Practice", "Nope", 1.0)).is_none());
    }

    #[test]
    fn names_are_kept_csv_safe() {
        assert_eq!(csv_safe("a,b\nc"), "a b c");
    }

    #[test]
    fn comparison_ignores_finder_trials_and_other_drills() {
        let rows = [
            entry("Practice", "Gridshot", 100.0),
            entry("FinderTrial", "Gridshot", 999.0),
            entry("Practice", "Tracking", 500.0),
            entry("Practice", "Gridshot", 200.0),
        ];
        let history = History { entries: rows.iter().filter_map(|r| HistoryEntry::parse(r)).collect(), path: PathBuf::new() };
        let cmp = history.compare("Gridshot");
        assert_eq!(cmp.best, Some(200.0));
        assert_eq!(cmp.recent_avg, Some(150.0));
        assert_eq!(cmp.recent_count, 2);
        assert!(history.compare("Microflick").best.is_none());
    }

    #[test]
    fn recent_average_covers_only_latest_runs() {
        let rows: Vec<String> = (0..15).map(|i| entry("Practice", "Microflick", i as f32)).collect();
        let history = History { entries: rows.iter().filter_map(|r| HistoryEntry::parse(r)).collect(), path: PathBuf::new() };
        // Latest 10 scores are 5..=14.
        assert_eq!(history.compare("Microflick").recent_avg, Some(9.5));
    }

    #[test]
    fn recent_change_compares_latest_block_with_the_one_before() {
        assert_eq!(recent_change(&[100.0; RECENT_RUNS]), None);
        // 5 older runs at 100, then 10 at 110: +10%.
        let mut scores = vec![100.0; 5];
        scores.extend([110.0; RECENT_RUNS]);
        assert!((recent_change(&scores).unwrap() - 10.0).abs() < 1e-4);
    }

    #[test]
    fn overall_change_uses_averages_of_both_ends() {
        assert_eq!(overall_change(&[100.0]), None);
        assert_eq!(overall_change(&[100.0, 150.0]), Some(50.0));
        // First five average 100, last five average 120, the outlier in the middle is ignored.
        let scores = [90.0, 110.0, 100.0, 95.0, 105.0, 999.0, 120.0, 115.0, 125.0, 118.0, 122.0];
        assert!((overall_change(&scores).unwrap() - 20.0).abs() < 1e-4);
    }

    #[test]
    fn formats_dates() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_date(951_782_400), "2000-02-29");
        assert_eq!(format_date(1_793_394_818), "2026-10-30");
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;

    const FIXTURE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/history_rows.csv"));

    /// The runs `fixtures/history_rows.csv` was written from: one per kind, purposes alternating.
    fn fixture_runs() -> Vec<(u64, RunStats, RunPurpose)> {
        let settings = Settings::default();
        DrillKind::fixture_kinds()
            .iter()
            .copied()
            .enumerate()
            .map(|(i, kind)| {
                let f = i as f32;
                let sens = 0.4 + f * 0.05;
                let stats = RunStats {
                    kind,
                    drill: if i == 0 { "My, 5x5".into() } else { kind.name().into() },
                    sens,
                    cm_per_360: settings.cm_per_360(sens),
                    duration: 30.0 + f,
                    hits: 40 + i as u32,
                    shots: 50 + i as u32,
                    expired: i as u32,
                    ttk_total: 20.0 + f,
                    best_ttk: 0.2,
                    on_target: 12.5 + f,
                    landings: vec![],
                };
                let purpose = if i % 2 == 0 { RunPurpose::Practice } else { RunPurpose::FinderTrial };
                (1_793_394_818 + i as u64, stats, purpose)
            })
            .collect()
    }

    /// Pins the CSV row text: column order, number formats, purpose strings, drill names.
    #[test]
    fn row_format_fixture() {
        let rows: Vec<String> =
            fixture_runs().iter().map(|(t, stats, purpose)| format_row(*t, stats, *purpose, &Settings::default())).collect();
        assert_eq!(rows, FIXTURE.lines().collect::<Vec<_>>());
    }

    #[test]
    fn parses_every_kind_fixture() {
        let entries: Vec<HistoryEntry> = FIXTURE.lines().map(|line| HistoryEntry::parse(line).unwrap()).collect();
        let kinds: Vec<DrillKind> = entries.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, DrillKind::fixture_kinds());
        for (entry, (t, stats, purpose)) in entries.iter().zip(fixture_runs()) {
            assert_eq!((entry.timestamp, entry.purpose), (t, purpose));
            assert_eq!(entry.score, stats.score());
        }
        assert_eq!(entries[0].drill, "My  5x5");
        // Rows from before custom scenarios: 15 columns, the kind is the built-in drill name.
        for (line, kind) in FIXTURE.lines().zip(DrillKind::fixture_kinds().iter().copied()).skip(1) {
            let legacy = line.rsplit_once(',').unwrap().0;
            let entry = HistoryEntry::parse(legacy).unwrap();
            assert_eq!((entry.kind, entry.drill.as_str()), (kind, kind.name()));
        }
        for (name, kind) in [("Head level", DrillKind::HeadLevel), ("Peeks", DrillKind::Peek)] {
            assert_eq!(DrillKind::from_name(name), Some(kind));
        }
    }
}
