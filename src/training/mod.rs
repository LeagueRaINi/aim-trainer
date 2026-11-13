//! Structured training on top of the drills: a fixed **benchmark** that measures progress under
//! the same conditions every time ([`benchmark`]), and a **weekly plan** that says what to train
//! on which day and why ([`plan`]). Both are played through the routine machinery
//! ([`crate::routine`]: its break screen between steps, its summary at the end).
//!
//! # Kinds of aim
//!
//! [`AimKind`] is Voltaic's split of aim into six kinds that train differently: static clicking,
//! dynamic clicking, smooth tracking, reactive tracking, speed switching and precision switching.
//! Every drill that trains aim belongs to one ([`AimKind::of`]); the eye-training drills, Survival
//! and the tests belong to none and are never scheduled. There are no population norms for this
//! trainer's scores, so nothing here ranks a player against others: every comparison is with the
//! player's own earlier results.
//!
//! # Benchmark
//!
//! Eight scenarios, 60 s each, in a fixed order that alternates kinds (about ten minutes with the
//! self-paced breaks). Every drill parameter is pinned in [`benchmark::pinned`] (length, target
//! size, decay, and each of the drill's own parameters), independent of the player's run settings
//! and of later retuning of a drill's defaults; only the sensitivity is the player's own, and it
//! is logged with each benchmark so a change can be pointed out. Each scenario's history name is
//! `<drill> · benchmark`: permanent, distinct from every practice name (and, as a variant of a
//! built-in name, unavailable to custom scenarios), so benchmark scores never mix with practice.
//!
//! | # | Scenario | Kind | Why it is in the set |
//! |---|---|---|---|
//! | 1 | Smooth tracking | Smooth tracking | a gentle start; the purest velocity-matching task (no randomness to react to) |
//! | 2 | Microflick | Static clicking | single small dot: flick precision; its clicks give effective throughput |
//! | 3 | Strafing bot | Dynamic clicking | a Valorant-speed strafing head, tap timed on a mover |
//! | 4 | Tracking | Reactive tracking | ADAD strafes with inertia: correcting after a change of direction |
//! | 5 | Wall flicks | Precision switching | four small dots: flick, stop, click; throughput again |
//! | 6 | Head taps | Static clicking | Valorant's first-shot skill: head at head height, rifle that must settle |
//! | 7 | Target switching | Speed switching | bots with health under held fire: kill, then switch (multi-kills) |
//! | 8 | Peeks | Dynamic clicking | holding an angle against random swings |
//!
//! Clicking gets two scenarios per kind because a Valorant player's aim is mostly clicking; each
//! other kind gets one so it is still seen.
//!
//! **Physical measures.** Raw scores of different scenarios are not comparable, so the result
//! shows, where it exists, a measure that is: effective throughput (ISO 9241-411, bits/s,
//! [`crate::metrics::throughput`]) for the two static click scenarios with recorded clicks
//! (Microflick, Wall flicks), and the share of time on target for the two tracking scenarios.
//!
//! **Index.** For each scenario the ratio of its score to the same scenario's score in the
//! player's *first* benchmark, clamped to [1/4, 4] so no single ratio can be infinite or near
//! zero. The index is `100 × exp(trimmed mean of ln(ratio))`: the geometric mean of the ratios
//! after dropping the single highest and the single lowest (with at least five ratios). Why: the
//! geometric mean treats +10% and −10% on any scenario alike whatever its score scale (Tracking
//! scores run to 10 000, tap drills to a few hundred), and trimming one ratio at each end means
//! one fluke run (a scenario that tripled, or one played half-asleep) cannot move the index on
//! its own. The first benchmark is 100 by definition. Example: seven scenarios up 10% and one up
//! 200% give 110 (the plain mean of the ratios would say 134). A scenario whose first score was 0
//! has no ratio and is left out. **Kinds** are indexed the same way over their one or two
//! scenarios, untrimmed. How a kind *moved* is its change since the previous benchmark; the
//! result orders the kinds by it and names the biggest gain and loss, calling anything within
//! ±[`benchmark::STEADY_PCT`]% steady (single runs commonly differ by that much).
//!
//! **Log.** `aim_trainer_benchmark.csv`, header `benchmark,scenario,run,score,throughput,on_target,cm_per_360`:
//! one row per scenario of a completed benchmark, all rows of a benchmark written in one append.
//! `benchmark` is the history timestamp of its last scenario (the benchmark's id), `run` the
//! scenario run's own history timestamp (it names its replay), `throughput` in bits/s and
//! `on_target` as a fraction, empty where not measured. Only a benchmark with every scenario
//! played in order is written ([`benchmark::completed`]): ending early, or skipping a step,
//! records nothing as a benchmark (the runs stay in the history like any routine step). On load,
//! a benchmark missing a scenario is ignored.
//!
//! # Weekly plan
//!
//! [`plan::generate`] is a pure function of [`plan::PlanInputs`]: the local date, the player's
//! choices (training days a week, focus), when each drill was last played (the history), the
//! benchmark log, the weak spot Insights finds ([`crate::analysis`]), and the latest Calm aim
//! result. The same inputs always give the same plan.
//!
//! **The week** runs Monday to Sunday. The player trains on `days` days (2 to 6): one is the
//! benchmark day, the others hold a training session each; the rest are rest days, at least one.
//! The benchmark sits on the weekday of the latest benchmark (same day of the week every time),
//! else of last week's plan, else today. Training days are spread out from it: the `k`-th of `n`
//! sessions sits `floor(k × 7 / (n + 1))` days after it (k = 1..n), so sessions are spaced as
//! evenly as a week allows and the day before a benchmark is always a rest day.
//!
//! **A session** is [`plan::SESSION_BLOCKS`] blocks of [`plan::BLOCK_SECS`] (15 minutes of play,
//! about 18 with breaks). How many blocks each kind gets: with the Valorant focus the clicking
//! kinds share 60% (6 blocks) and the other four 40% (one each) so they don't decay; the tracking
//! focus mirrors that for the tracking kinds; all-round spreads evenly (largest-remainder
//! rounding). Then the kind that improved least since the first benchmark (with two benchmarks or
//! more) gets one block more, taken from the kind with the most. The focus share therefore stays
//! within 60% ± 10 points (5 to 7 of 10 blocks). Within a kind, in this order:
//! - the Insights weak spot, if there is one, is a static-clicking block of constrained adaptive
//!   Microflick, exactly what the Insights page's Train button plays;
//! - the weakest part of the latest Calm aim result gets a block of the drill that trains it
//!   (Smoothness: Smooth tracking, Stillness: Peeks, Composed flick: Microflick, Settle: Wall flicks);
//! - the remaining blocks go to the kind's drills not played for the longest (never played
//!   first), at most once each per session where the kind has more than one drill; across the
//!   week's sessions the choice rotates as if each session had been played on its day.
//!
//! Drills that offer adaptive mode (Microflick, Head level) are played in adaptive Train mode
//! (QUEST+ holds hits at 75%). The blocks are then ordered so that consecutive blocks are of
//! different kinds (greedy: the kind with the most blocks left that differs from the previous
//! one), except that a drill the player has never played gets two blocks in a row: a task one
//! cannot do yet is better practised in a block. Every block carries one reason
//! ([`plan::Why`]), the most specific that applies: weak spot, Calm aim, new, least improved kind,
//! not played for a week or more, the focus weighting.
//!
//! With no data the same rules give a default week: no weak spot, no Calm block, the focus
//! split, the drills in their default order, and the page says the plan adapts once there is a
//! benchmark.
//!
//! **Persistence.** `aim_trainer_plan.ron` ([`plan::PlanStore`]): the player's choices, this
//! week's plan with what it was built from ([`plan::Basis`]) and which sessions are done, and
//! last week's plan for "what changed". The plan is generated when the Plan page is opened and
//! there is none for this week, the choices changed, or a newer benchmark exists; days already
//! past and sessions already done keep what they were. A session counts as done once its routine
//! reaches the end with at least half its blocks played; a benchmark day is done when the week
//! has a benchmark in the log. A missed day stays missed: nothing is moved to later days.
//!
//! **Days** are local calendar days (the system's time zone, [`utc_offset_secs`]), so a session
//! played late in the evening counts for that evening's day.

pub mod benchmark;
pub mod plan;

use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::drill::{DrillKind, DrillSet};
use crate::persist::{self, PersistPlugin};

pub struct TrainingPlugin;

impl Plugin for TrainingPlugin {
    fn build(&self, app: &mut App) {
        let (mut store, report): (plan::PlanStore, _) = persist::load_ron(Path::new(plan::PLAN_PATH));
        store.repair();
        app.add_plugins(PersistPlugin { value: store, report })
            .insert_resource(benchmark::BenchmarkLog::load(Path::new(benchmark::BENCHMARK_PATH)))
            .init_resource::<benchmark::BenchmarkSession>()
            .add_systems(
                Update,
                benchmark::record_benchmark_step.in_set(DrillSet::Outcome).after(crate::routine::routine_step_finished),
            )
            .add_systems(OnEnter(AppState::Routine), plan::finish_session);
    }
}

/// The six kinds of aim (Voltaic's bins). Persisted in the plan file: never rename.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AimKind {
    StaticClicking,
    DynamicClicking,
    SmoothTracking,
    ReactiveTracking,
    SpeedSwitching,
    PrecisionSwitching,
}

impl AimKind {
    pub const ALL: [AimKind; 6] = [
        AimKind::StaticClicking,
        AimKind::DynamicClicking,
        AimKind::SmoothTracking,
        AimKind::ReactiveTracking,
        AimKind::SpeedSwitching,
        AimKind::PrecisionSwitching,
    ];

    pub fn name(self) -> &'static str {
        match self {
            AimKind::StaticClicking => "Static clicking",
            AimKind::DynamicClicking => "Dynamic clicking",
            AimKind::SmoothTracking => "Smooth tracking",
            AimKind::ReactiveTracking => "Reactive tracking",
            AimKind::SpeedSwitching => "Speed switching",
            AimKind::PrecisionSwitching => "Precision switching",
        }
    }

    /// The kind a drill trains; `None` for drills that aren't aim training (eye drills, the
    /// tests) or don't fit a timed block (Survival ends when the lives are gone).
    pub fn of(kind: DrillKind) -> Option<AimKind> {
        Some(match kind {
            DrillKind::Gridshot
            | DrillKind::Microflick
            | DrillKind::HeadLevel
            | DrillKind::HeadTaps
            | DrillKind::CounterStrafe => AimKind::StaticClicking,
            DrillKind::Peek | DrillKind::MovingGridshot | DrillKind::StrafingBot => AimKind::DynamicClicking,
            DrillKind::SmoothTracking => AimKind::SmoothTracking,
            DrillKind::Tracking => AimKind::ReactiveTracking,
            DrillKind::SwitchTracking | DrillKind::Pokeball | DrillKind::TargetSwitch => AimKind::SpeedSwitching,
            DrillKind::WallFlicks => AimKind::PrecisionSwitching,
            _ => return None,
        })
    }

    /// The drills a plan picks from for this kind, Valorant-like ones first (the order breaks
    /// ties between drills played equally long ago).
    pub fn drills(self) -> &'static [DrillKind] {
        match self {
            AimKind::StaticClicking => &[
                DrillKind::HeadLevel,
                DrillKind::Microflick,
                DrillKind::HeadTaps,
                DrillKind::CounterStrafe,
                DrillKind::Gridshot,
            ],
            AimKind::DynamicClicking => &[DrillKind::StrafingBot, DrillKind::Peek, DrillKind::MovingGridshot],
            AimKind::SmoothTracking => &[DrillKind::SmoothTracking],
            AimKind::ReactiveTracking => &[DrillKind::Tracking],
            AimKind::SpeedSwitching => &[DrillKind::TargetSwitch, DrillKind::SwitchTracking, DrillKind::Pokeball],
            AimKind::PrecisionSwitching => &[DrillKind::WallFlicks],
        }
    }
}

/// Seconds in a day.
pub const DAY_SECS: i64 = 86_400;

/// The local calendar day (days since 1970-01-01 in the local time zone) of a unix time, for a
/// time zone `offset` seconds ahead of UTC.
pub fn local_day(unix_time: u64, offset: i64) -> i64 {
    (unix_time as i64 + offset).div_euclid(DAY_SECS)
}

/// Day of the week of a local day: 0 is Monday (1970-01-01 was a Thursday).
pub fn weekday(day: i64) -> i64 {
    (day + 3).rem_euclid(7)
}

/// The Monday of the week a local day is in.
pub fn monday(day: i64) -> i64 {
    day - weekday(day)
}

/// Short weekday name.
pub fn weekday_name(day: i64) -> &'static str {
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][weekday(day) as usize]
}

/// `Fri 13 Nov`: a local day as people read it.
pub fn day_label(day: i64) -> String {
    let date = crate::history::format_date((day * DAY_SECS) as u64);
    let month = date[5..7].parse::<usize>().unwrap_or(1);
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {} {}", weekday_name(day), date[8..10].trim_start_matches('0'), MONTHS[month - 1])
}

/// Seconds the system's time zone is ahead of UTC right now (daylight saving included). UTC
/// where the system can't say: off Windows `time` refuses to ask while other threads run.
pub fn utc_offset_secs() -> i64 {
    time::UtcOffset::current_local_offset().map_or(0, |offset| i64::from(offset.whole_seconds()))
}

/// Today's local day.
pub fn today() -> i64 {
    local_day(crate::finder::unix_now(), utc_offset_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_weeks_and_weekdays() {
        // 2026-11-13 was a Friday; its week began on Monday the 9th.
        let friday = 1_794_528_000 / DAY_SECS;
        assert_eq!(crate::history::format_date((friday * DAY_SECS) as u64), "2026-11-13");
        assert_eq!((weekday(friday), weekday_name(friday)), (4, "Fri"));
        assert_eq!(day_label(monday(friday)), "Mon 9 Nov");
        assert_eq!(monday(monday(friday) + 6), monday(friday));
        // 23:30 UTC is already the next day two hours east, still the same day five hours west.
        let late = (friday * DAY_SECS + 23 * 3600 + 1800) as u64;
        assert_eq!(local_day(late, 2 * 3600), friday + 1);
        assert_eq!(local_day(late, -5 * 3600), friday);
    }

    #[test]
    fn every_scheduled_drill_belongs_to_its_kind() {
        for aim in AimKind::ALL {
            assert!(!aim.drills().is_empty());
            for &drill in aim.drills() {
                assert_eq!(AimKind::of(drill), Some(aim), "{drill:?}");
                assert_ne!(drill.def().category, crate::drill::def::Category::Tests);
            }
        }
        // Every drill with a kind is in that kind's list, so a plan can pick it.
        for drill in DrillKind::ALL {
            if let Some(aim) = AimKind::of(drill) {
                assert!(aim.drills().contains(&drill), "{drill:?}");
            }
        }
    }
}
