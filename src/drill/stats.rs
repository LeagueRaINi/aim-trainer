//! A finished (or running) drill's statistics and how each [`ScoreModel`] turns them into a
//! score and the stats shown with it.

use super::DrillKind;
use crate::metrics::throughput::{self, Click};

#[derive(Clone, Debug)]
pub struct RunStats {
    /// Kind of drill, which decides scoring.
    pub kind: DrillKind,
    /// Drill name: the built-in name or a custom scenario's name. History is grouped by it.
    pub drill: String,
    pub sens: f32,
    pub cm_per_360: f32,
    /// Counted seconds: live time after the warm-in.
    pub duration: f32,
    pub hits: u32,
    pub shots: u32,
    pub expired: u32,
    pub ttk_total: f32,
    pub best_ttk: f32,
    pub on_target: f32,
    /// First-flick landing ratios (1 = landed on target); Microflick only.
    pub landings: Vec<f32>,
    /// Every counted click at a flick target, in click-killing drills (see
    /// [`throughput`](crate::metrics::throughput)).
    pub clicks: Vec<Click>,
}

impl RunStats {
    /// Hits over everything that could have been a hit: shots fired plus targets that expired.
    pub fn accuracy(&self) -> f32 {
        let attempts = self.shots + self.expired;
        if attempts == 0 { 0.0 } else { self.hits as f32 / attempts as f32 }
    }

    /// Average time to kill: from appearance (or the previous kill, in Gridshot).
    pub fn avg_kill_ms(&self) -> f32 {
        if self.hits == 0 { 0.0 } else { self.ttk_total / self.hits as f32 * 1000.0 }
    }

    pub fn on_target_ratio(&self) -> f32 {
        self.on_target / self.duration
    }

    /// Single number used to compare runs, independent of drill length.
    pub fn score(&self) -> f32 {
        self.kind.def().score.score(self)
    }

    /// The secondary stats shown next to the score, as (caption, value).
    pub fn key_stats(&self) -> Vec<(&'static str, String)> {
        self.kind.def().score.key_stats(self)
    }

    /// Effective throughput, bits/s; `None` without enough flicks of a kind.
    pub fn throughput(&self) -> Option<f32> {
        throughput::throughput(&self.clicks)
    }

    /// Mean click position along the flick, in target radii: > 0 past the centre.
    pub fn click_bias(&self) -> Option<f32> {
        throughput::bias(&self.clicks)
    }

    /// Mean sideways click distance from the centre, in target radii.
    pub fn click_off_axis(&self) -> Option<f32> {
        throughput::off_axis(&self.clicks)
    }

    /// Fraction of flicks whose first click was within half a radius of the centre.
    pub fn centre_hits(&self) -> Option<f32> {
        throughput::centre_hits(&self.clicks)
    }
}

/// How a drill is scored. Persisted scores are compared across years of history: never change
/// a formula.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreModel {
    /// Accuracy-weighted kills per minute (x100).
    Flick,
    /// Time on target in 0.01% steps.
    Tracking,
    /// 100 000 / average reaction ms, weighted by accuracy.
    Reaction,
}

/// A per-run value shown as a history column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryStat {
    OnTarget,
    Accuracy,
    KillMs,
}

impl ScoreModel {
    pub fn score(self, stats: &RunStats) -> f32 {
        match self {
            ScoreModel::Tracking => (stats.on_target_ratio() * 10_000.0).round(),
            ScoreModel::Reaction if stats.hits == 0 => 0.0,
            ScoreModel::Reaction => (100_000.0 / stats.avg_kill_ms() * stats.accuracy()).round(),
            ScoreModel::Flick => (stats.hits as f32 * 100.0 * stats.accuracy() * 60.0 / stats.duration).round(),
        }
    }

    pub fn key_stats(self, stats: &RunStats) -> Vec<(&'static str, String)> {
        let best_ms = if stats.hits == 0 { 0.0 } else { stats.best_ttk * 1000.0 };
        match self {
            ScoreModel::Tracking => vec![("On target", format!("{:.1}%", stats.on_target_ratio() * 100.0))],
            ScoreModel::Reaction => vec![
                ("Avg reaction", format!("{:.0} ms", stats.avg_kill_ms())),
                ("Best", format!("{best_ms:.0} ms")),
                ("Hits", stats.hits.to_string()),
                ("Early / late", (stats.shots + stats.expired - stats.hits).to_string()),
                ("Accuracy", format!("{:.1}%", stats.accuracy() * 100.0)),
            ],
            ScoreModel::Flick => {
                let mut list = vec![
                    ("Hits", format!("{} / {}", stats.hits, stats.shots)),
                    ("Accuracy", format!("{:.1}%", stats.accuracy() * 100.0)),
                    ("Avg kill", format!("{:.0} ms", stats.avg_kill_ms())),
                    ("Best kill", format!("{best_ms:.0} ms")),
                    ("Kills / s", format!("{:.2}", stats.hits as f32 / stats.duration)),
                ];
                if stats.expired > 0 {
                    list.insert(1, ("Expired", stats.expired.to_string()));
                }
                list
            }
        }
    }

    /// The live stat line in the in-game HUD.
    pub fn hud_detail(self, stats: &RunStats) -> String {
        match self {
            ScoreModel::Tracking => format!("On target {:.1}%", stats.on_target_ratio() * 100.0),
            ScoreModel::Reaction => format!("Hits {}   Avg {:.0} ms", stats.hits, stats.avg_kill_ms()),
            ScoreModel::Flick => format!("Hits {}   Acc {:.0}%", stats.hits, stats.accuracy() * 100.0),
        }
    }

    /// The history run list's columns after the score, as (header, stat).
    pub fn history_columns(self) -> &'static [(&'static str, HistoryStat)] {
        match self {
            ScoreModel::Tracking => &[("On target", HistoryStat::OnTarget)],
            ScoreModel::Reaction => &[("Accuracy", HistoryStat::Accuracy), ("Avg ms", HistoryStat::KillMs)],
            ScoreModel::Flick => &[("Accuracy", HistoryStat::Accuracy), ("ms / kill", HistoryStat::KillMs)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(kind: DrillKind, hits: u32, shots: u32, expired: u32, duration: f32) -> RunStats {
        RunStats {
            kind,
            drill: kind.name().to_string(),
            sens: 0.4,
            cm_per_360: 40.8,
            duration,
            hits,
            shots,
            expired,
            ttk_total: hits as f32 * 0.25,
            best_ttk: 0.2,
            on_target: 0.0,
            landings: Vec::new(),
            clicks: Vec::new(),
        }
    }

    #[test]
    fn flick_score_does_not_depend_on_drill_length() {
        // Same pace and accuracy: a 20 s finder trial must score like a 60 s practice run.
        let short = stats(DrillKind::Gridshot, 40, 50, 0, 20.0);
        let long = stats(DrillKind::Gridshot, 120, 150, 0, 60.0);
        assert_eq!(short.score(), long.score());
    }

    #[test]
    fn expired_targets_count_as_misses() {
        let clean = stats(DrillKind::Microflick, 50, 50, 0, 60.0);
        let decayed = stats(DrillKind::Microflick, 50, 50, 10, 60.0);
        assert!((decayed.accuracy() - 50.0 / 60.0).abs() < 1e-6);
        assert!(decayed.score() < clean.score());
    }

    #[test]
    fn reaction_score_rewards_faster_and_more_accurate() {
        let mut fast = stats(DrillKind::Reaction, 20, 20, 0, 30.0);
        fast.ttk_total = 20.0 * 0.2;
        let mut slow = fast.clone();
        slow.ttk_total = 20.0 * 0.3;
        let mut sloppy = fast.clone();
        sloppy.shots = 25; // five early clicks
        assert_eq!(fast.score(), 500.0);
        assert!(slow.score() < fast.score());
        assert!(sloppy.score() < fast.score());
        assert_eq!(stats(DrillKind::Reaction, 0, 3, 0, 30.0).score(), 0.0);
    }

    #[test]
    fn tracking_scores_time_on_target() {
        let mut tracking = stats(DrillKind::Tracking, 0, 0, 0, 30.0);
        tracking.on_target = 15.0;
        assert_eq!(tracking.score(), 5000.0);
        assert_eq!(tracking.key_stats(), vec![("On target", "50.0%".to_string())]);
    }
}
