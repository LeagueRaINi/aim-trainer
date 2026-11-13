//! A finished (or running) drill's statistics and how each [`ScoreModel`] turns them into a
//! score and the stats shown with it.

use super::{DrillConfig, DrillKind};
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
    /// A score the drill set itself ([`Run::rate`](super::Run::rate)), for score models that
    /// aren't computed from the counters above.
    pub rating: Option<f32>,
    /// Rounds that hit a target, in drills where the fire button is held
    /// ([`ScoreModel::held_fire`]); `shots` are the rounds fired there.
    pub rounds_hit: u32,
}

impl RunStats {
    /// Hits over everything that could have been a hit: shots fired plus targets that expired.
    /// With held fire a kill takes several rounds (or there are none), so there it is the rounds
    /// that hit over the rounds fired.
    pub fn accuracy(&self) -> f32 {
        let attempts = self.shots + self.expired;
        let hits = if self.kind.def().score.held_fire() { self.rounds_hit } else { self.hits };
        if attempts == 0 { 0.0 } else { hits as f32 / attempts as f32 }
    }

    /// Average time to kill: from appearance (or the previous kill, in Gridshot).
    pub fn avg_kill_ms(&self) -> f32 {
        if self.hits == 0 { 0.0 } else { self.ttk_total / self.hits as f32 * 1000.0 }
    }

    /// The share of the run the target was being hit: under the crosshair with the fire button
    /// held.
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
    /// The share of the run the target was being hit (fire held, crosshair on it), in 0.01%
    /// steps. Fire off the target costs nothing here; it shows in the accuracy.
    Tracking,
    /// 100 000 / average reaction ms, weighted by accuracy.
    Reaction,
    /// Accuracy-weighted kills (x100): the longer you last, the more you get.
    Survival,
    /// Kills per minute (x100), each earned by holding fire on a moving target for its time.
    Switch,
    /// Kills per minute (x100) times the share of the rounds fired that hit a target: held fire
    /// at targets with health. Killing faster raises the first factor, rounds sprayed between
    /// targets lower the second. `kills x 100 x rounds hit / rounds fired x 60 / seconds`.
    HeldFire,
    /// Bots shot with the tap rifle: kills per minute x kills per shot (x100). A head tap is one
    /// shot per kill, a body kill four, and every wasted shot lowers it. The same arithmetic as
    /// `Flick`; the stats are named for what they are here (a body hit is a hit, not a kill).
    Taps,
    /// The Reaction test: 100 000 / median reaction ms of its valid trials, which the drill sets
    /// through [`Run::rate`](super::Run::rate) (see [`crate::metrics::reaction::score`]).
    ReactionTest,
    /// Go / no-go: reaction speed x enemies hit x friendlies held, which the drill sets through
    /// [`Run::rate`](super::Run::rate) (see [`go_no_go::score`](super::kinds::go_no_go::score)).
    GoNoGo,
    /// The Calm aim test's own score ([`Run::rate`](super::Run::rate)): the mean of its four
    /// part scores, 0 to 100 in quarter points (see [`crate::metrics::calm`]).
    Calm,
}

/// A per-run value shown as a history column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryStat {
    OnTarget,
    Accuracy,
    KillMs,
}

impl ScoreModel {
    /// Whether the fire button is a gun that is held on targets (see
    /// [`held_fire`](super::held_fire)) rather than clicked at them.
    pub fn held_fire(self) -> bool {
        matches!(self, ScoreModel::Tracking | ScoreModel::Switch | ScoreModel::HeldFire)
    }

    pub fn score(self, stats: &RunStats) -> f32 {
        match self {
            ScoreModel::Tracking => (stats.on_target_ratio() * 10_000.0).round(),
            ScoreModel::Reaction if stats.hits == 0 => 0.0,
            ScoreModel::Reaction => (100_000.0 / stats.avg_kill_ms() * stats.accuracy()).round(),
            ScoreModel::Flick => (stats.hits as f32 * 100.0 * stats.accuracy() * 60.0 / stats.duration).round(),
            ScoreModel::Survival => (stats.hits as f32 * 100.0 * stats.accuracy()).round(),
            ScoreModel::Switch => (stats.hits as f32 * 100.0 * 60.0 / stats.duration).round(),
            ScoreModel::HeldFire => (stats.hits as f32 * 100.0 * stats.accuracy() * 60.0 / stats.duration).round(),
            ScoreModel::Taps => (stats.hits as f32 * 100.0 * stats.accuracy() * 60.0 / stats.duration).round(),
            ScoreModel::ReactionTest | ScoreModel::GoNoGo => stats.rating.unwrap_or(0.0),
            ScoreModel::Calm => stats.rating.unwrap_or(0.0),
        }
    }

    pub fn key_stats(self, stats: &RunStats) -> Vec<(&'static str, String)> {
        let best_ms = if stats.hits == 0 { 0.0 } else { stats.best_ttk * 1000.0 };
        match self {
            ScoreModel::Tracking => vec![
                ("Time hitting", format!("{:.1}%", stats.on_target_ratio() * 100.0)),
                ("Accuracy", format!("{:.1}%", stats.accuracy() * 100.0)),
                ("Rounds hit", format!("{} / {}", stats.rounds_hit, stats.shots)),
            ],
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
            },
            ScoreModel::Survival => vec![
                ("Survived", format!("{:.1} s", stats.duration)),
                ("Kills", stats.hits.to_string()),
                ("Lives lost", stats.expired.to_string()),
                ("Accuracy", format!("{:.1}%", stats.accuracy() * 100.0)),
                ("Avg kill", format!("{:.0} ms", stats.avg_kill_ms())),
            ],
            ScoreModel::Switch => vec![
                ("Kills", stats.hits.to_string()),
                ("Kills / s", format!("{:.2}", stats.hits as f32 / stats.duration)),
                ("Avg kill", format!("{:.0} ms", stats.avg_kill_ms())),
                ("Best kill", format!("{best_ms:.0} ms")),
                ("Accuracy", format!("{:.1}%", stats.accuracy() * 100.0)),
            ],
            ScoreModel::HeldFire => vec![
                ("Kills", stats.hits.to_string()),
                ("Accuracy", format!("{:.1}%", stats.accuracy() * 100.0)),
                ("Rounds hit", format!("{} / {}", stats.rounds_hit, stats.shots)),
                ("Avg kill", format!("{:.0} ms", stats.avg_kill_ms())),
                ("Best kill", format!("{best_ms:.0} ms")),
            ],
            ScoreModel::Taps => vec![
                ("Kills", stats.hits.to_string()),
                ("Shots", stats.shots.to_string()),
                ("Shots / kill", shots_per_kill(stats)),
                ("Avg kill", format!("{:.0} ms", stats.avg_kill_ms())),
                ("Kills / min", format!("{:.1}", stats.hits as f32 * 60.0 / stats.duration)),
            ],
            // A test's numbers are on its own results card, next to what they mean.
            ScoreModel::ReactionTest => Vec::new(),
            // Every click is a shot, so the shots that hit no enemy are the false alarms.
            ScoreModel::GoNoGo => vec![
                ("Avg reaction", format!("{:.0} ms", stats.avg_kill_ms())),
                ("Best", format!("{best_ms:.0} ms")),
                ("Enemies hit", format!("{} / {}", stats.hits, stats.hits + stats.expired)),
                ("Missed", stats.expired.to_string()),
                ("False alarms", stats.shots.saturating_sub(stats.hits).to_string()),
            ],
            // The parts and their numbers are the results screen's Calm aim card.
            ScoreModel::Calm => vec![
                ("Targets hit", stats.hits.to_string()),
                ("Shots", stats.shots.to_string()),
                ("Timed out", stats.expired.to_string()),
            ],
        }
    }

    /// The live stat line in the in-game HUD, for a run of `config`.
    pub fn hud_detail(self, stats: &RunStats, config: &DrillConfig) -> String {
        match self {
            ScoreModel::Tracking => {
                format!("Hitting {:.1}%   Acc {:.0}%", stats.on_target_ratio() * 100.0, stats.accuracy() * 100.0)
            },
            ScoreModel::Reaction => format!("Hits {}   Avg {:.0} ms", stats.hits, stats.avg_kill_ms()),
            ScoreModel::Flick => format!("Hits {}   Acc {:.0}%", stats.hits, stats.accuracy() * 100.0),
            ScoreModel::Survival => {
                format!(
                    "Kills {}   Lives {} / {}",
                    stats.hits,
                    config.lives.saturating_sub(stats.expired),
                    config.lives
                )
            },
            ScoreModel::Switch | ScoreModel::HeldFire => {
                format!("Kills {}   Acc {:.0}%", stats.hits, stats.accuracy() * 100.0)
            },
            ScoreModel::Taps => format!("Kills {}   Shots / kill {}", stats.hits, shots_per_kill(stats)),
            // Scored trials are hits (valid) or expired (lapses); false starts are shots without a hit.
            ScoreModel::ReactionTest => format!(
                "Valid {}   False starts {}   Lapses {}",
                stats.hits,
                stats.shots.saturating_sub(stats.hits),
                stats.expired
            ),
            ScoreModel::GoNoGo => format!(
                "Hits {}   Avg {:.0} ms   False alarms {}",
                stats.hits,
                stats.avg_kill_ms(),
                stats.shots.saturating_sub(stats.hits)
            ),
            ScoreModel::Calm => format!("Hits {}   Shots {}", stats.hits, stats.shots),
        }
    }

    /// The history run list's columns after the score, as (header, stat).
    pub fn history_columns(self) -> &'static [(&'static str, HistoryStat)] {
        match self {
            ScoreModel::Tracking => &[("Hitting", HistoryStat::OnTarget), ("Accuracy", HistoryStat::Accuracy)],
            ScoreModel::Reaction => &[("Accuracy", HistoryStat::Accuracy), ("Avg ms", HistoryStat::KillMs)],
            ScoreModel::Flick | ScoreModel::Survival => {
                &[("Accuracy", HistoryStat::Accuracy), ("ms / kill", HistoryStat::KillMs)]
            },
            ScoreModel::Switch | ScoreModel::HeldFire => {
                &[("Accuracy", HistoryStat::Accuracy), ("ms / kill", HistoryStat::KillMs)]
            },
            ScoreModel::Taps => &[("Kills / shot", HistoryStat::Accuracy), ("ms / kill", HistoryStat::KillMs)],
            // Accuracy here: valid trials over valid ones, false starts and lapses.
            ScoreModel::ReactionTest => &[("Valid", HistoryStat::Accuracy), ("Mean ms", HistoryStat::KillMs)],
            ScoreModel::GoNoGo => &[("Accuracy", HistoryStat::Accuracy), ("Avg ms", HistoryStat::KillMs)],
            ScoreModel::Calm => &[("Accuracy", HistoryStat::Accuracy)],
        }
    }
}

/// Shots fired per kill, for display: 1.0 when every kill was a single head tap.
fn shots_per_kill(stats: &RunStats) -> String {
    if stats.hits == 0 { "-".to_string() } else { format!("{:.1}", stats.shots as f32 / stats.hits as f32) }
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
            rating: None,
            rounds_hit: 0,
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
        // The fire off the target costs no score; it shows in the accuracy, which only counts
        // the rounds that were fired.
        tracking.shots = 200;
        tracking.rounds_hit = 150;
        assert_eq!(tracking.score(), 5000.0);
        assert_eq!(tracking.accuracy(), 0.75);
        assert_eq!(
            tracking.key_stats()[..2],
            [("Time hitting", "50.0%".to_string()), ("Accuracy", "75.0%".to_string())]
        );
        // Never firing is no accuracy at all, not a division by zero.
        assert_eq!(stats(DrillKind::SwitchTracking, 0, 0, 0, 30.0).accuracy(), 0.0);
    }

    /// Held fire: `hits` are kills, `shots` rounds fired, and the accuracy is the share of
    /// rounds that hit.
    fn held_fire(kills: u32, rounds_hit: u32, rounds: u32, duration: f32) -> RunStats {
        RunStats { rounds_hit, ..stats(DrillKind::TargetSwitch, kills, rounds, 0, duration) }
    }

    #[test]
    fn held_fire_scores_kills_per_minute_times_the_share_of_rounds_that_hit() {
        // 30 kills in 60 s with 240 of 400 rounds on target: 30 x 100 x 0.6.
        let run = held_fire(30, 240, 400, 60.0);
        assert!((run.accuracy() - 0.6).abs() < 1e-6);
        assert_eq!(run.score(), 1800.0);
        // The same pace and accuracy over a third of the time scores the same.
        assert_eq!(held_fire(10, 80, 133, 20.0).score(), (10.0f32 * 100.0 * (80.0 / 133.0) * 3.0).round());
        assert_eq!(held_fire(20, 160, 200, 20.0).score(), held_fire(60, 480, 600, 60.0).score());
        // More kills in the same time and with the same accuracy score more; so do the same
        // kills with fewer rounds wasted.
        assert!(held_fire(36, 288, 480, 60.0).score() > run.score());
        assert!(held_fire(30, 240, 300, 60.0).score() > run.score());
        // Holding fire all run long without ever finishing a bot, or never firing, scores nothing.
        assert_eq!(held_fire(0, 90, 600, 60.0).score(), 0.0);
        assert_eq!(held_fire(0, 0, 0, 60.0).score(), 0.0);
        // The kills don't count as hits twice: other models keep `hits / shots`.
        let flick = RunStats { rounds_hit: 7, ..stats(DrillKind::Gridshot, 40, 50, 0, 20.0) };
        assert!((flick.accuracy() - 0.8).abs() < 1e-6);
    }

    #[test]
    fn calm_aim_scores_what_the_test_rated() {
        let mut calm = stats(DrillKind::CalmAim, 18, 20, 1, 90.0);
        // A run that ended without a rating (it can't, but a score must still be a number).
        assert_eq!(calm.score(), 0.0);
        calm.rating = Some(76.75);
        assert_eq!(calm.score(), 76.75);
    }
}
