//! The tired check's log and its verdict: does control drop when the player is tired, and by
//! how much. Every saved check is one row of `aim_trainer_tired.csv`; see
//! [`limits`](super::limits) for the battery and the reasoning.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use super::stats::Welch;
use crate::history::append_csv_row;

pub const TIRED_PATH: &str = "aim_trainer_tired.csv";
const HEADER: &str = "unix_time,state,cm_per_360,hold_mm,follow_mm,click_mm,clicks,accuracy";
/// Checks of each kind (fresh, tired) needed before the two are compared.
pub const MIN_CHECKS: usize = 3;
/// Checks count for the current sensitivity while theirs is within this factor of it: the
/// battery's flicks are set in degrees, so a very different sensitivity is a different task for
/// the hand.
const SAME_SENS: f32 = 1.1;

/// Whether a check was run rested or worn out, as the player marked it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    /// After a match or a long session.
    Tired,
}

impl Freshness {
    /// Persisted: the CSV's `state` column.
    pub fn csv_name(self) -> &'static str {
        match self {
            Freshness::Fresh => "fresh",
            Freshness::Tired => "tired",
        }
    }

    fn from_csv(name: &str) -> Option<Self> {
        [Freshness::Fresh, Freshness::Tired].into_iter().find(|f| f.csv_name() == name)
    }
}

/// One saved tired check. Lengths are millimetres of hand travel.
#[derive(Clone, Debug, PartialEq)]
pub struct TiredEntry {
    pub unix_time: u64,
    pub state: Freshness,
    /// The sensitivity it was run at.
    pub cm_per_360: f32,
    /// Hand noise holding on a still target (per-axis SD).
    pub hold_mm: f32,
    /// Hand noise across the path following a slow target (SD).
    pub follow_mm: f32,
    /// Mean distance of the precise flicks' first clicks from the target centre.
    pub click_mm: f32,
    /// First clicks measured.
    pub clicks: u32,
    /// Hits per shot of the flick part.
    pub accuracy: f32,
}

impl TiredEntry {
    /// The check's one number, mm: the geometric mean of the hand's noise (the larger of hold
    /// and follow, as in the calibration) and its click error. Both are errors of fine control
    /// in hand space; their product keeps either from dominating by its scale.
    pub fn control_error(&self) -> f32 {
        (self.hold_mm.max(self.follow_mm) * self.click_mm).sqrt()
    }

    fn row(&self) -> String {
        format!(
            "{},{},{:.3},{:.4},{:.4},{:.4},{},{:.4}",
            self.unix_time,
            self.state.csv_name(),
            self.cm_per_360,
            self.hold_mm,
            self.follow_mm,
            self.click_mm,
            self.clicks,
            self.accuracy
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let cols: Vec<&str> = line.split(',').collect();
        let [unix_time, state, cm_per_360, hold_mm, follow_mm, click_mm, clicks, accuracy] = cols[..] else {
            return None;
        };
        let length = |text: &str| text.parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0);
        Some(Self {
            unix_time: unix_time.parse().ok()?,
            state: Freshness::from_csv(state)?,
            cm_per_360: length(cm_per_360).filter(|v| *v > 0.0)?,
            hold_mm: length(hold_mm)?,
            follow_mm: length(follow_mm)?,
            click_mm: length(click_mm)?,
            clicks: clicks.parse().ok()?,
            accuracy: length(accuracy)?,
        })
    }
}

/// What the checks so far say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TiredVerdict {
    /// Not enough checks of both kinds yet: how many there are.
    Collecting { fresh: usize, tired: usize },
    /// Enough checks, and no clear drop: `change` is tired control error over fresh (1.1 = 10%
    /// more), which may be noise.
    Steady { fresh: usize, tired: usize, change: f32 },
    /// Control error is clearly larger when tired, by this factor.
    Drops { fresh: usize, tired: usize, loss: f32 },
}

impl TiredVerdict {
    /// The factor the range's fast limit moves by: only a clear drop moves it.
    pub fn loss(&self) -> Option<f32> {
        match *self {
            TiredVerdict::Drops { loss, .. } => Some(loss),
            _ => None,
        }
    }
}

/// Compares the tired checks with the fresh ones on the logarithm of their control error (so the
/// difference is a ratio): a drop counts only when there are [`MIN_CHECKS`] of each and the
/// difference is clear of the scatter between checks (Welch's t at the gates' 1% level).
pub fn verdict<'a>(entries: impl Iterator<Item = &'a TiredEntry>) -> TiredVerdict {
    let (mut fresh, mut tired) = (Vec::new(), Vec::new());
    for entry in entries {
        let error = entry.control_error();
        if error > 0.0 {
            match entry.state {
                Freshness::Fresh => fresh.push(error.ln()),
                Freshness::Tired => tired.push(error.ln()),
            }
        }
    }
    let counts = (fresh.len(), tired.len());
    let Some(welch) = Welch::of(&fresh, &tired).filter(|_| counts.0.min(counts.1) >= MIN_CHECKS) else {
        return TiredVerdict::Collecting { fresh: counts.0, tired: counts.1 };
    };
    let change = welch.difference.exp();
    if welch.difference > 0.0 && welch.clear() {
        TiredVerdict::Drops { fresh: counts.0, tired: counts.1, loss: change }
    } else {
        TiredVerdict::Steady { fresh: counts.0, tired: counts.1, change }
    }
}

/// Every saved tired check, oldest first.
#[derive(Resource, Default)]
pub struct TiredLog {
    entries: Vec<TiredEntry>,
    /// The CSV file checks are appended to; `None` for a log that only lives in memory.
    path: Option<PathBuf>,
}

impl TiredLog {
    /// Reads the checks kept in the CSV at `path` (empty if there is none yet).
    pub fn load(path: &Path) -> Self {
        let entries = fs::read_to_string(path)
            .map(|text| text.lines().skip(1).filter_map(TiredEntry::parse).collect())
            .unwrap_or_default();
        Self { entries, path: Some(path.to_path_buf()) }
    }

    /// A log of these checks that is never written anywhere: for staged screenshots.
    pub fn in_memory(entries: Vec<TiredEntry>) -> Self {
        Self { entries, path: None }
    }

    /// Appends a check to the CSV and the in-memory log.
    pub fn record(&mut self, entry: &TiredEntry) {
        let row = entry.row();
        if let Some(path) = &self.path
            && let Err(err) = append_csv_row(path, HEADER, &row)
        {
            error!("Failed to write {}: {err}", path.display());
        }
        self.entries.extend(TiredEntry::parse(&row));
    }

    /// The checks run at (about) this sensitivity.
    pub fn at(&self, cm_per_360: f32) -> impl Iterator<Item = &TiredEntry> {
        self.entries
            .iter()
            .filter(move |e| e.cm_per_360 <= cm_per_360 * SAME_SENS && e.cm_per_360 >= cm_per_360 / SAME_SENS)
    }

    /// What the checks at this sensitivity say.
    pub fn verdict(&self, cm_per_360: f32) -> TiredVerdict {
        verdict(self.at(cm_per_360))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::finder::stats::tests::normal;
    use crate::test_util::TempDir;

    /// A check with this control error (noise and click error equal), at 40 cm/360.
    pub(crate) fn check(unix_time: u64, state: Freshness, error_mm: f32) -> TiredEntry {
        TiredEntry {
            unix_time,
            state,
            cm_per_360: 40.0,
            hold_mm: error_mm * 0.3,
            follow_mm: error_mm,
            click_mm: error_mm,
            clicks: 24,
            accuracy: 0.9,
        }
    }

    #[test]
    fn control_error_is_the_geometric_mean_of_noise_and_click_error() {
        let entry = TiredEntry { hold_mm: 0.02, follow_mm: 0.09, click_mm: 0.36, ..check(1, Freshness::Fresh, 0.0) };
        assert!((entry.control_error() - 0.18).abs() < 1e-6);
        // The larger of hold and follow is the noise.
        let entry = TiredEntry { hold_mm: 0.16, ..entry };
        assert!((entry.control_error() - 0.24).abs() < 1e-6);
    }

    #[test]
    fn nothing_is_said_before_three_checks_of_each() {
        let mut entries = vec![
            check(1, Freshness::Fresh, 0.10),
            check(2, Freshness::Fresh, 0.11),
            check(3, Freshness::Fresh, 0.10),
            check(4, Freshness::Tired, 0.30),
            check(5, Freshness::Tired, 0.31),
        ];
        // Tired is three times worse, but twice is not enough.
        assert_eq!(verdict(entries.iter()), TiredVerdict::Collecting { fresh: 3, tired: 2 });
        entries.push(check(6, Freshness::Tired, 0.29));
        let TiredVerdict::Drops { loss, fresh: 3, tired: 3 } = verdict(entries.iter()) else {
            panic!("{:?}", verdict(entries.iter()))
        };
        assert!((loss - 0.2999 / 0.1032).abs() < 0.02, "{loss}");
        assert_eq!(verdict(entries.iter()).loss(), Some(loss));
    }

    #[test]
    fn a_difference_inside_the_scatter_moves_nothing() {
        // 10% worse on average with checks scattering by 25%.
        let entries: Vec<TiredEntry> = [0.10, 0.14, 0.08, 0.12]
            .into_iter()
            .map(|e| (Freshness::Fresh, e))
            .chain([0.13, 0.09, 0.15, 0.11].into_iter().map(|e| (Freshness::Tired, e)))
            .enumerate()
            .map(|(i, (state, error))| check(i as u64, state, error))
            .collect();
        let result = verdict(entries.iter());
        let TiredVerdict::Steady { change, .. } = result else { panic!("{result:?}") };
        assert!(change > 1.0 && change < 1.2, "{change}");
        assert_eq!(result.loss(), None);
        // Clearly better when tired is not a drop either.
        let better: Vec<TiredEntry> = (0..8)
            .map(|i| {
                if i < 4 { check(i, Freshness::Fresh, 0.2) } else { check(i, Freshness::Tired, 0.1 + i as f32 * 1e-3) }
            })
            .collect();
        assert!(matches!(verdict(better.iter()), TiredVerdict::Steady { change, .. } if change < 0.6));
    }

    /// A player whose control is the same tired and fresh, with checks scattering by 20%, is
    /// told of a drop in at most 1 of 50 histories of 8 checks of each kind; one who really
    /// loses 50% is told within those 8 in most. (Smaller losses need more checks, or checks
    /// that scatter less.)
    #[test]
    fn false_and_true_drop_rates() {
        let mut rng = fastrand::Rng::with_seed(11);
        let mut history = |loss: f32, checks: usize| -> bool {
            let mut entries = Vec::new();
            for i in 0..checks {
                for (state, factor) in [(Freshness::Fresh, 1.0), (Freshness::Tired, loss)] {
                    let error = 0.12 * factor * (0.2 * normal(&mut rng)).exp();
                    entries.push(check(i as u64, state, error));
                }
                // Looked at after every pair of checks, as the page does.
                if verdict(entries.iter()).loss().is_some() {
                    return true;
                }
            }
            false
        };
        const PLAYERS: usize = 2000;
        let false_drops = (0..PLAYERS).filter(|_| history(1.0, 8)).count();
        assert!(false_drops <= PLAYERS / 50, "{false_drops} of {PLAYERS} steady players were told of a drop");
        let found = (0..PLAYERS).filter(|_| history(1.5, 8)).count();
        assert!(found >= PLAYERS * 7 / 10, "only {found} of {PLAYERS} real drops were found");
    }

    #[test]
    fn checks_round_trip_and_are_compared_per_sensitivity() {
        let dir = TempDir::new("tired_log");
        let path = dir.join("tired.csv");
        let mut log = TiredLog::load(&path);
        let first = check(100, Freshness::Fresh, 0.1);
        let other = TiredEntry { cm_per_360: 60.0, ..check(200, Freshness::Tired, 0.2) };
        let near = TiredEntry { cm_per_360: 42.0, ..check(300, Freshness::Tired, 0.2) };
        for entry in [&first, &other, &near] {
            log.record(entry);
        }
        // As stored: four decimals.
        let stored = log.entries.clone();
        assert!((stored[1].hold_mm - other.hold_mm).abs() < 1e-4 && stored[1].cm_per_360 == 60.0);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().next(), Some(HEADER));
        assert_eq!(text.lines().nth(1), Some("100,fresh,40.000,0.0300,0.1000,0.1000,24,0.9000"));
        let loaded = TiredLog::load(&path);
        assert_eq!(loaded.entries, stored);
        // 42 cm/360 counts for 40; 60 doesn't.
        assert_eq!(loaded.at(40.0).map(|e| e.unix_time).collect::<Vec<_>>(), vec![100, 300]);
        assert_eq!(loaded.verdict(40.0), TiredVerdict::Collecting { fresh: 1, tired: 1 });
        assert_eq!(loaded.verdict(60.0), TiredVerdict::Collecting { fresh: 0, tired: 1 });
        // Garbage rows are skipped; a staged log writes nothing.
        fs::write(&path, format!("{HEADER}\n1,sleepy,40,0,0,0,1,1\n2,fresh,40,0.1,0.1,NaN,1,1\n")).unwrap();
        assert!(TiredLog::load(&path).entries.is_empty());
        let mut staged = TiredLog::in_memory(Vec::new());
        staged.record(&check(1, Freshness::Fresh, 0.1));
        assert_eq!(staged.entries.len(), 1);
    }
}
