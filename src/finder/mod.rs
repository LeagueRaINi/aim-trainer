//! Finding a sensitivity. The design, with its assumptions and thresholds, is in [`limits`]:
//!
//! - [`limits`]: the calibration's result and the usable range it implies, with its pick;
//! - [`probe`]: the live measurements (sweeps, hold, follow); [`checkup`]: the calibration and
//!   the tired check as guided sequences of them; [`tired`]: the tired check's log and verdict;
//! - [`monitor`]: a summary of every flick-drill run and the evidence gate for suggestions;
//! - [`compare`]: the long comparison of two sensitivities over several days;
//! - [`classic`] and [`smart`]: the quick finders (A/B bisection, score model), a first look;
//! - [`stats`]: the statistics the gates share.
//!
//! The state machines and gates are pure; this module feeds them finished trials.

pub mod checkup;
pub mod classic;
pub mod compare;
pub mod gp;
pub mod limits;
pub mod monitor;
pub mod probe;
pub mod smart;
pub mod stats;
pub mod tired;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;

use self::checkup::Checkup;
use self::classic::Finder;
use self::compare::{Block, CompareSession, CompareStore};
use self::limits::CalibrationStore;
use self::monitor::MonitorLog;
use self::smart::{Segment, SmartFinder};
use self::tired::TiredLog;
use crate::AppState;
use crate::drill::{DrillSet, RunPurpose, begin_run};
use crate::history::RunRecorded;
use crate::persist::PersistPlugin;
use crate::settings::Settings;

pub struct FinderPlugin;

impl Plugin for FinderPlugin {
    fn build(&self, app: &mut App) {
        let (calibration, calibration_load) = CalibrationStore::load();
        let (comparison, comparison_load) = CompareStore::load();
        app.add_plugins((
            PersistPlugin { value: calibration, report: calibration_load },
            PersistPlugin { value: comparison, report: comparison_load },
        ))
        .insert_resource(MonitorLog::load(Path::new(monitor::MONITOR_PATH)))
        .insert_resource(TiredLog::load(Path::new(tired::TIRED_PATH)))
        .add_systems(
            Update,
            (
                finder_trial_finished,
                // The summary is made from the recording, which the replay handler takes away.
                monitor::record_monitor.before(crate::replay::save_finished_replay),
            )
                .in_set(DrillSet::Outcome),
        );
        probe::register(app);
    }
}

/// Seconds since the unix epoch: for dating saved results and telling which day it is. Never
/// used to time anything in a run.
pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// A finished finder trial (part) feeds whichever finder, checkup or comparison is running; the
/// smart finder goes straight on to the trial's next part, otherwise it's back to the finder
/// screen.
#[allow(clippy::too_many_arguments)]
pub(crate) fn finder_trial_finished(
    mut commands: Commands,
    mut recorded: MessageReader<RunRecorded>,
    settings: Res<Settings>,
    mut finder: Option<ResMut<Finder>>,
    mut smart: Option<ResMut<SmartFinder>>,
    mut checkup: Option<ResMut<Checkup>>,
    mut session: Option<ResMut<CompareSession>>,
    mut comparisons: Option<ResMut<CompareStore>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for recorded in recorded.read().filter(|r| r.run.purpose == RunPurpose::FinderTrial) {
        let stats = &recorded.run.stats;
        if let Some(finder) = finder.as_mut() {
            finder.record_trial(stats.score());
        }
        if let Some(smart) = smart.as_mut() {
            smart.record_segment(stats.kind, Segment::from_stats(stats));
            // Straight on to the trial's next part, if there is one.
            if let Some(next) = smart.segment_run(&settings) {
                begin_run(&mut commands, &mut next_state, next);
                continue;
            }
        }
        if let Some(checkup) = checkup.as_mut() {
            checkup.record_flicks(stats);
        }
        if let Some(session) = session.as_mut()
            && let CompareSession::Block(side) = **session
        {
            // A block with too few clicks to measure is simply played again.
            if let Some(block) = Block::from_stats(recorded.timestamp, side, stats)
                && let Some(comparison) = comparisons.as_mut().and_then(|store| store.active.as_mut())
            {
                comparison.blocks.push(block);
            }
            **session = CompareSession::Running;
        }
        next_state.set(AppState::Finder);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::math::Vec2;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;

    use super::checkup::{CheckupKind, Step};
    use super::compare::{LongComparison, Side};
    use super::probe::{ProbeResult, ProbeTask};
    use super::tired::Freshness;
    use super::*;
    use crate::drill::{RunFinished, RunStats};
    use crate::history::Comparison;
    use crate::metrics::throughput::Click;

    /// An app with just the handler for finished finder trials.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(AppState::Playing)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(10)))
            .insert_resource(Settings::default())
            .add_message::<RunRecorded>()
            .add_systems(Update, finder_trial_finished);
        app
    }

    /// Announces a finished run with `clicks` measured first clicks, 0.3° off the centre.
    fn finish(app: &mut App, run: crate::drill::Run, clicks: usize, timestamp: u64) {
        let click = Click {
            start: Vec2::ZERO,
            target: Vec2::new(9.0, 0.0),
            radius: 0.6,
            size: 0.6,
            aim: Vec2::new(9.3, 0.0),
            time: 0.5,
            hit: true,
            first: true,
        };
        let settings = Settings::default();
        let stats =
            RunStats { hits: clicks as u32, shots: clicks as u32, clicks: vec![click; clicks], ..run.stats(&settings) };
        let finished = RunFinished { stats, purpose: run.purpose, drill: run.drill.clone(), duration: run.duration };
        app.world_mut().write_message(RunRecorded { run: finished, previous: Comparison::default(), timestamp });
        app.update();
        app.update();
    }

    #[test]
    fn the_tired_checks_flick_part_completes_the_checkup() {
        let mut app = app();
        let mut checkup = Checkup::new(CheckupKind::Tired(Freshness::Fresh));
        checkup.record_probe(ProbeTask::Hold, ProbeResult::Steady(0.03));
        checkup.record_probe(ProbeTask::Follow, ProbeResult::Steady(0.1));
        assert_eq!(checkup.step(), Some(Step::Flicks));
        app.insert_resource(checkup);
        finish(&mut app, Checkup::flick_run(&Settings::default()), 20, 500);
        let checkup = app.world().resource::<Checkup>();
        assert_eq!(checkup.step(), None);
        assert!((checkup.flicks.unwrap().click_deg - 0.3).abs() < 1e-4);
        assert_eq!(*app.world().resource::<State<AppState>>().get(), AppState::Finder);
    }

    #[test]
    fn a_comparison_block_is_stored_under_the_side_that_was_played() {
        let mut app = app();
        let settings = Settings::default();
        let comparison = LongComparison::new(0, 40.0, 44.0);
        let run = comparison.block_run(Side::B, 1000, &settings);
        app.insert_resource(CompareStore { active: Some(comparison) }).insert_resource(CompareSession::Block(Side::B));
        finish(&mut app, run, 30, 1000);
        let blocks = &app.world().resource::<CompareStore>().active.as_ref().unwrap().blocks;
        assert_eq!(blocks.len(), 1);
        assert_eq!((blocks[0].side, blocks[0].unix_time, blocks[0].clicks), (Side::B, 1000, 30));
        assert_eq!(*app.world().resource::<CompareSession>(), CompareSession::Running);
        assert_eq!(*app.world().resource::<State<AppState>>().get(), AppState::Finder);
        // A block nobody clicked in is not stored; the same block is due again.
        let comparison = app.world().resource::<CompareStore>().active.clone().unwrap();
        let run = comparison.block_run(Side::A, 1200, &settings);
        app.insert_resource(CompareSession::Block(Side::A));
        finish(&mut app, run, 2, 1200);
        assert_eq!(app.world().resource::<CompareStore>().active.as_ref().unwrap().blocks.len(), 1);
    }
}
