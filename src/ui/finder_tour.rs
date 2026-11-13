//! The screenshot tour's scenes of the finder: its page in the states that take days of play
//! to reach, the calibration's steps, the tired check's result and the long comparison. All
//! data is made up and only ever in memory: nothing staged here is written to the player's
//! files, and [`FinderStagehand::reset`] puts the real data back.

use std::path::Path;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::menu::MenuTab;
use super::menu::finder::FinderView;
use super::screens::compare::ComparePick;
use crate::AppState;
use crate::drill::{DrillKind, RunStats};
use crate::finder::checkup::{Checkup, CheckupKind};
use crate::finder::compare::{Block, CompareSession, CompareStore, DAY_BLOCKS, LongComparison, Side};
use crate::finder::limits::{Calibration, CalibrationStore};
use crate::finder::monitor::{MONITOR_PATH, MonitorLog, RunSummary};
use crate::finder::probe::{Probe, ProbePhase, ProbeResult, ProbeTask, Reach};
use crate::finder::stats::SECS_PER_DAY;
use crate::finder::tired::{Freshness, TIRED_PATH, TiredEntry, TiredLog};
use crate::finder::unix_now;
use crate::metrics::throughput::Click;
use crate::settings::Settings;

/// One staged scene of the finder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinderScene {
    /// The quick finders' settings.
    PageQuick,
    /// Calibrated, the current sensitivity inside the range; the monitor still collecting.
    PageRange,
    /// The monitor suggests a step; the tired check has moved the fast limit; a comparison runs.
    PageSuggestion,
    /// The current sensitivity is too fast for the hand: the range's pick.
    PagePick,
    /// Limits that cross.
    PageEmpty,
    CalibrationIntro,
    SweepReady,
    SweepLive,
    /// The card before the hold, with both sweeps measured.
    HoldCard,
    FollowLive,
    ProbePaused,
    CalibrationResult,
    /// The tired check's card before its flick part.
    TiredFlicks,
    TiredResult,
    CompareSetup,
    CompareDay,
    CompareVerdict,
}

/// Every scene with its screenshot's name and the seconds to wait before taking it: long
/// enough for buttons to arm, short for live measurements, which go on meanwhile.
pub const SCENES: [(FinderScene, &str, f32); 17] = [
    (FinderScene::PageQuick, "menu_finder_quick", 1.0),
    (FinderScene::PageRange, "menu_finder_range", 1.0),
    (FinderScene::PageSuggestion, "menu_finder_suggestion", 1.0),
    (FinderScene::PagePick, "menu_finder_pick", 1.0),
    (FinderScene::PageEmpty, "menu_finder_empty", 1.0),
    (FinderScene::CalibrationIntro, "calibration_intro", 1.3),
    (FinderScene::SweepReady, "calibration_sweep_ready", 0.5),
    (FinderScene::SweepLive, "calibration_sweep_live", 0.5),
    (FinderScene::HoldCard, "calibration_hold_card", 1.3),
    (FinderScene::FollowLive, "calibration_follow_live", 0.4),
    (FinderScene::ProbePaused, "calibration_paused", 0.5),
    (FinderScene::CalibrationResult, "calibration_result", 1.3),
    (FinderScene::TiredFlicks, "tired_check_flicks_card", 1.3),
    (FinderScene::TiredResult, "tired_check_result", 1.3),
    (FinderScene::CompareSetup, "compare_setup", 1.3),
    (FinderScene::CompareDay, "compare_day", 1.3),
    (FinderScene::CompareVerdict, "compare_verdict", 1.3),
];

/// What the finder's scenes act on.
#[derive(SystemParam)]
pub struct FinderStagehand<'w> {
    view: ResMut<'w, FinderView>,
    pick: ResMut<'w, ComparePick>,
    calibration: ResMut<'w, CalibrationStore>,
    comparison: ResMut<'w, CompareStore>,
    monitor: ResMut<'w, MonitorLog>,
    tired: ResMut<'w, TiredLog>,
}

impl FinderStagehand<'_> {
    /// Sets a scene up. The stores are changed without marking them changed, so the made-up
    /// data never reaches the files.
    pub fn set(
        &mut self,
        scene: FinderScene,
        settings: &Settings,
        commands: &mut Commands,
        next_state: &mut NextState<AppState>,
        tab: &mut ResMut<MenuTab>,
    ) {
        let cm = settings.cm_per_360(settings.sens);
        let now = unix_now();
        commands.remove_resource::<Checkup>();
        commands.remove_resource::<Probe>();
        commands.remove_resource::<CompareSession>();
        // (sweep cm, follow noise mm) of the staged calibration, the monitor's and the tired
        // check's data, and the comparison.
        let (calibration, overshoot, tired_drop, comparison) = match scene {
            FinderScene::PageRange => (Some((29.0, 0.11)), false, false, None),
            FinderScene::PageSuggestion | FinderScene::CompareSetup => {
                (Some((29.0, 0.11)), true, true, Some(staged_comparison(cm, now, 3, 2, 0.97)))
            },
            FinderScene::PagePick => (Some((29.0, 0.21)), false, false, None),
            FinderScene::PageEmpty => (Some((19.0, 0.21)), false, false, None),
            FinderScene::CompareDay => (Some((29.0, 0.11)), true, true, Some(staged_comparison(cm, now, 3, 2, 0.97))),
            FinderScene::CompareVerdict => {
                (Some((29.0, 0.11)), true, true, Some(staged_comparison(cm, now, 6, 0, 0.84)))
            },
            FinderScene::TiredResult | FinderScene::TiredFlicks => (Some((29.0, 0.11)), false, true, None),
            _ => (None, false, false, None),
        };
        self.calibration.bypass_change_detection().latest = calibration.map(|(sweep_cm, follow_mm)| Calibration {
            unix_time: now - 3 * SECS_PER_DAY,
            dpi: settings.dpi,
            cm_per_360: cm,
            sweep_cm,
            wrist_cm: 7.5,
            hold_mm: 0.03,
            follow_mm,
        });
        *self.monitor = MonitorLog::in_memory(staged_runs(settings, now, overshoot));
        *self.tired = TiredLog::in_memory(staged_checks(cm, now, tired_drop));
        self.comparison.bypass_change_detection().active = comparison.filter(|_| scene != FinderScene::CompareSetup);
        *self.view = if scene == FinderScene::PageQuick { FinderView::Quick } else { FinderView::Overview };
        self.pick.0 = 0;

        let probe = |task: ProbeTask| Probe::new(task, settings);
        let measured = |steps: usize| {
            let mut checkup = Checkup::new(CheckupKind::Calibration);
            for (task, result) in [
                (ProbeTask::Sweep(Reach::Arm), ProbeResult::Sweep(29.2)),
                (ProbeTask::Sweep(Reach::Wrist), ProbeResult::Sweep(7.4)),
                (ProbeTask::Hold, ProbeResult::Steady(0.03)),
                (ProbeTask::Follow, ProbeResult::Steady(0.11)),
            ]
            .into_iter()
            .take(steps)
            {
                checkup.record_probe(task, result);
            }
            checkup
        };
        match scene {
            FinderScene::PageQuick
            | FinderScene::PageRange
            | FinderScene::PageSuggestion
            | FinderScene::PagePick
            | FinderScene::PageEmpty => {
                // The menu is entered anew, which builds the page from the staged data; a new
                // assignment of the open tab would also rebuild it under this frame's systems.
                tab.set_if_neq(MenuTab::Finder);
                next_state.set(AppState::Menu);
                return;
            },
            FinderScene::CalibrationIntro => commands.insert_resource(measured(0)),
            FinderScene::SweepReady => {
                commands.insert_resource(measured(0));
                commands.insert_resource(probe(ProbeTask::Sweep(Reach::Arm)));
            },
            FinderScene::SweepLive => {
                commands.insert_resource(measured(0));
                let mut probe = probe(ProbeTask::Sweep(Reach::Arm));
                go_live(&mut probe);
                // To the right edge, two and a half sweeps, at 800 counts a step.
                for (counts, steps) in [(4000.0, 20), (-9200.0, 46), (9000.0, 45), (-4400.0, 22)] {
                    for _ in 0..steps {
                        probe.advance(0.01, Vec2::new(counts / steps as f32, 0.0));
                    }
                }
                commands.insert_resource(probe);
            },
            FinderScene::HoldCard => commands.insert_resource(measured(2)),
            FinderScene::FollowLive | FinderScene::ProbePaused => {
                commands.insert_resource(measured(3));
                let mut probe = probe(ProbeTask::Follow);
                go_live(&mut probe);
                // Seven seconds in, a little off the target's line.
                let deg_per_count = settings.deg_per_count(settings.sens);
                for _ in 0..700 {
                    let wanted = probe.target() + Vec2::new(0.08, 0.1);
                    let counts = -(wanted - probe.aim()) / deg_per_count;
                    probe.advance(0.01, counts);
                }
                if scene == FinderScene::ProbePaused {
                    probe.pause();
                }
                commands.insert_resource(probe);
            },
            FinderScene::CalibrationResult => commands.insert_resource(measured(4)),
            FinderScene::TiredFlicks | FinderScene::TiredResult => {
                let mut checkup = Checkup::new(CheckupKind::Tired(Freshness::Tired));
                checkup.record_probe(ProbeTask::Hold, ProbeResult::Steady(0.04));
                checkup.record_probe(ProbeTask::Follow, ProbeResult::Steady(0.15));
                if scene == FinderScene::TiredResult {
                    checkup.record_flicks(&staged_flicks(settings));
                }
                commands.insert_resource(checkup);
            },
            FinderScene::CompareSetup => commands.insert_resource(CompareSession::Setup),
            FinderScene::CompareDay | FinderScene::CompareVerdict => commands.insert_resource(CompareSession::Running),
        }
        next_state.set(AppState::Finder);
    }

    /// Ends whatever scene is up and loads the real data again.
    pub fn reset(&mut self, commands: &mut Commands) {
        commands.remove_resource::<Checkup>();
        commands.remove_resource::<Probe>();
        commands.remove_resource::<CompareSession>();
        *self.calibration.bypass_change_detection() = CalibrationStore::load().0;
        *self.comparison.bypass_change_detection() = CompareStore::load().0;
        *self.monitor = MonitorLog::load(Path::new(MONITOR_PATH));
        *self.tired = TiredLog::load(Path::new(TIRED_PATH));
        *self.view = FinderView::Overview;
    }
}

/// Takes a probe through its ready gate (and countdown) until it is live.
fn go_live(probe: &mut Probe) {
    for _ in 0..1000 {
        if probe.phase() == ProbePhase::Live {
            return;
        }
        probe.start();
        probe.advance(0.01, Vec2::ZERO);
    }
}

/// Run summaries at the current sensitivity: nine runs on two days without a pattern, or 30
/// runs on six days with first flicks 9% long.
fn staged_runs(settings: &Settings, now: u64, overshoot: bool) -> Vec<RunSummary> {
    let mut rng = fastrand::Rng::with_seed(7);
    let (days, per_day, landing) = if overshoot { (6, 5, 1.09) } else { (2, 4, 1.0) };
    let mut runs = Vec::new();
    for day in (0..days).rev() {
        for index in 0..per_day + usize::from(!overshoot && day == 0) {
            runs.push(RunSummary {
                unix_time: now - day * SECS_PER_DAY - 3600 + index as u64 * 150,
                drill: DrillKind::Microflick.name().to_string(),
                kind: DrillKind::Microflick,
                sens: settings.sens,
                cm_per_360: settings.cm_per_360(settings.sens),
                flicks: 34 + rng.u32(0..8),
                landing: Some(landing + 0.06 * (rng.f32() - 0.5)),
                bias: Some(0.1),
                click_deg: Some(0.4 + 0.1 * (rng.f32() - 0.5)),
                corrections: Some(0.8 + 0.3 * (rng.f32() - 0.5)),
                misses: 0.12,
                score: 9000.0 + 800.0 * (rng.f32() - 0.5),
            });
        }
    }
    runs
}

/// Saved tired checks: two fresh and one tired, or four of each with the tired ones 30% worse.
fn staged_checks(cm: f32, now: u64, drop: bool) -> Vec<TiredEntry> {
    let check = |days_ago: u64, state: Freshness, error: f32| TiredEntry {
        unix_time: now - days_ago * SECS_PER_DAY,
        state,
        cm_per_360: cm,
        hold_mm: 0.03,
        follow_mm: error * 0.6,
        click_mm: error / 0.6,
        clicks: 26,
        accuracy: 0.9,
    };
    let (fresh, tired): (&[f32], &[f32]) =
        if drop { (&[0.19, 0.2, 0.21, 0.2], &[0.25, 0.27, 0.26, 0.26]) } else { (&[0.19, 0.21], &[0.24]) };
    let fresh = fresh.iter().enumerate().map(|(i, &error)| check(8 - 2 * i as u64, Freshness::Fresh, error));
    let tired = tired.iter().enumerate().map(|(i, &error)| check(7 - 2 * i as u64, Freshness::Tired, error));
    fresh.chain(tired).collect()
}

/// A comparison of the current sensitivity with one 5% slower: `days` complete days and
/// `today` blocks of today, with B's click error `ratio` times A's.
fn staged_comparison(cm: f32, now: u64, days: u64, today: usize, ratio: f32) -> LongComparison {
    let mut rng = fastrand::Rng::with_seed(3);
    let mut comparison = LongComparison::new(now - (days + 1) * SECS_PER_DAY, cm, cm * 1.05);
    let mut play = |comparison: &mut LongComparison, at: u64, blocks: usize| {
        for index in 0..blocks {
            let time = at + index as u64 * 150;
            let Some(side) = comparison.next_side(time) else { break };
            let factor = if side == Side::B { ratio } else { 1.0 };
            comparison.blocks.push(Block {
                unix_time: time,
                side,
                click_deg: 0.42 * factor * (1.0 + 0.06 * (rng.f32() - 0.5)),
                accuracy: 0.86 + 0.03 * (rng.f32() - 0.5) + if side == Side::B { 0.5 * (1.0 - ratio) } else { 0.0 },
                clicks: 48,
            });
        }
    };
    // Whole UTC days, so every staged day is complete whatever the time is now.
    let midnight = now - now % SECS_PER_DAY;
    for day in (1..=days).rev() {
        play(&mut comparison, midnight - day * SECS_PER_DAY + SECS_PER_DAY / 2, DAY_BLOCKS);
    }
    play(&mut comparison, midnight, today);
    comparison
}

/// The stats of a tired check's flick part: 24 first clicks a third of a degree off.
fn staged_flicks(settings: &Settings) -> RunStats {
    let click = |index: usize| {
        let angle = index as f32 * 2.4;
        Click {
            start: Vec2::ZERO,
            target: Vec2::new(9.0, 2.0),
            radius: 0.57,
            size: 0.57,
            aim: Vec2::new(9.0, 2.0) + Vec2::from_angle(angle) * (0.2 + 0.03 * (index % 9) as f32),
            time: 0.6,
            hit: !index.is_multiple_of(8),
            first: true,
        }
    };
    RunStats {
        kind: DrillKind::Microflick,
        drill: "Tired check".to_string(),
        sens: settings.sens,
        cm_per_360: settings.cm_per_360(settings.sens),
        duration: 30.0,
        hits: 21,
        shots: 24,
        expired: 0,
        ttk_total: 21.0 * 0.9,
        best_ttk: 0.5,
        on_target: 0.0,
        landings: Vec::new(),
        clicks: (0..24).map(click).collect(),
        rating: None,
        rounds_hit: 0,
    }
}
