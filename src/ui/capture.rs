//! Built-in screenshots and benchmark. F12 saves the window to `screenshots/<unix ms>.png` at
//! any time.
//!
//! `aim_trainer --tour <dir>` walks through every menu page (some in more than one state), a few
//! drills (their ready card, a moment of play, the pause menu), a results screen of a run with
//! hits, the routine and finder screens and the replay HUD, saves a PNG of each to `<dir>`, then
//! quits: a repeatable way to look at the whole UI, e.g. after a layout change. Screens that
//! take a whole session to reach (a finder result) are staged with made-up numbers.
//!
//! `aim_trainer --bench` measures frame times on two menu pages and in a few drills (a few
//! seconds each, after a warmup, with no input), logs each scenario's frame rate and frame-time
//! percentiles (`bench <scenario>: …` lines in the log), then quits.
//!
//! Both open the window without taking focus, so they can run while the PC is in use.
//! `--size <width>x<height>` sets the starting window size (windowed mode). Run them in a scratch
//! directory, since settings and history are read from the working directory and the tour's
//! runs are recorded like any practice run.

use std::path::PathBuf;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::winit::WinitSettings;

use super::finder_tour::{FinderScene, FinderStagehand, SCENES};
use super::menu::MenuTab;
use super::menu::custom::CustomSelection;
use super::menu::history::HistoryView;
use super::menu::play::PlaySelection;
use crate::AppState;
use crate::calm_log::{CalmEntry, CalmLog};
use crate::camera::{Look, PlayerCamera};
use crate::custom::CustomContent;
use crate::drill::kinds::go_no_go::GoNoGo;
use crate::drill::kinds::reaction_test::{ReactionTest, Trial};
use crate::drill::rifle::{MoveStats, TapRun, TapStats};
use crate::drill::{
    DrillConfig, DrillKind, DrillSpec, FacesEye, KeepRunsOnFocusLoss, Run, RunPurpose, RunStats, begin_run, range,
};
use crate::finder::classic::{Finder, FinderStep};
use crate::finder::smart::{Segment, SmartFinder, SmartStep};
use crate::history::{Comparison, LastRun};
use crate::metrics::calm::{CalmResult, Flicks, Part, Settling, Smoothness, Stillness};
use crate::metrics::reaction;
use crate::reaction_log::{ReactionEntry, ReactionLog};
use crate::replay::{Replay, ReplayDir, watch};
use crate::routine::{ROUTINES, Routine, start_routine};
use crate::settings::Settings;
use crate::training::benchmark::{self, Benchmark, BenchmarkLog, Measure, SCENARIOS, ScenarioResult};
use crate::training::plan::{Focus, PlanInputs, PlanStore, WeakSpot, Weakest};
use crate::training::{self, AimKind};

pub struct CapturePlugin;

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, screenshot_on_f12);
        let Some(script) = script(&std::env::args().collect::<Vec<_>>()) else { return };
        app.insert_resource(script)
            .insert_resource(KeepRunsOnFocusLoss)
            // The window has no focus, which would otherwise drop the app to 60 Hz updates.
            .insert_resource(WinitSettings::continuous())
            // After input is read, so simulated key presses count as this frame's.
            .add_systems(PreUpdate, run_tour.after(InputSystems));
    }
}

/// The script the command line asks for: `--tour <dir>` or `--bench`. A `--tour` without a
/// directory is reported and ignored.
fn script(args: &[String]) -> Option<Tour> {
    if let Some(dir) = tour_dir(args.iter().cloned()) {
        return Some(Tour::new(dir));
    }
    if args.iter().any(|arg| arg == "--tour") {
        error!("--tour needs a directory to save the screenshots in: `--tour <dir>`");
    }
    args.iter().any(|arg| arg == "--bench").then(Tour::bench)
}

/// Scripted runs (`--tour <dir>`, `--bench`) open the window without focus, at `--size` if
/// given.
pub fn script_window(window: &mut Window) {
    let args: Vec<String> = std::env::args().collect();
    let scripted = tour_dir(args.iter().cloned()).is_some() || args.iter().any(|arg| arg == "--bench");
    if scripted {
        window.focused = false;
    }
    if let Some((width, height)) = window_size(args.into_iter()) {
        window.resolution.set(width as f32, height as f32);
    }
}

/// The directory after `--tour`, if given.
fn tour_dir(mut args: impl Iterator<Item = String>) -> Option<PathBuf> {
    args.find(|arg| arg == "--tour")?;
    args.next().map(PathBuf::from)
}

/// The `<width>x<height>` after `--size`, if given and valid.
fn window_size(mut args: impl Iterator<Item = String>) -> Option<(u32, u32)> {
    args.find(|arg| arg == "--size")?;
    let size = args.next()?;
    let (width, height) = size.split_once('x')?;
    Some((width.parse().ok().filter(|&w| w > 0)?, height.parse().ok().filter(|&h| h > 0)?))
}

fn screenshot_on_f12(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if !keys.just_pressed(KeyCode::F12) {
        return;
    }
    let millis =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_millis());
    let dir = PathBuf::from("screenshots");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        warn!("couldn't create {}: {err}", dir.display());
        return;
    }
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(dir.join(format!("{millis}.png"))));
}

/// One thing the tour does, after the previous step's wait.
enum Step {
    Tab(MenuTab),
    /// A practice run of the drill, lasting this many seconds.
    Drill(DrillKind, f32),
    Press(KeyCode),
    /// Saves the window to this file.
    Shot(PathBuf),
    Wait(f32),
    /// Records frame times for this many seconds, then logs their statistics under the name.
    Measure(String, f32),
    /// Brings up a screen that takes more than a key press to reach.
    Stage(Stage),
    /// Aims at the target nearest the crosshair, a little off its centre, and clicks.
    Shoot,
    Exit,
    /// Holds the fire button for this many seconds, following the target nearest the crosshair.
    Fire(f32),
    /// Presses a key and keeps it down until it is [released](Step::Release).
    Hold(KeyCode),
    Release(KeyCode),
    /// Aims this many target radii off the centre of the target nearest the crosshair, and clicks.
    ShootAt(Vec2),
    /// Puts the crosshair on the centre of the target nearest to it.
    Aim,
    /// Presses the fire button and keeps it down (`true`), or lets it go.
    Trigger(bool),
    /// Turns the view to this (yaw, pitch) in degrees.
    Look(f32, f32),
}

/// The screens the tour sets up itself.
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    /// The Play page with this drill or routine picked.
    Play(PlaySelection),
    /// The History page of this drill.
    History(&'static str),
    /// The Custom page with a scenario open in the editor.
    CustomEditor,
    RoutineBreak,
    RoutineSummary,
    SmartTrial,
    SmartRating,
    SmartResult,
    ClassicChoice,
    ClassicResult,
    /// The newest replay, playing.
    Replay,
    /// Back to the menu from any of the above.
    Menu,
    /// The Custom page with a new scenario of this drill open in the editor.
    CustomScenario(DrillKind),
    /// The History page of this built-in drill as the current settings name it.
    HistoryOf(DrillKind),
    /// The results screen of a bot drill, with its breakdown card.
    TapResults(DrillKind),
    /// The Reaction test's dot, now, in a test that is being played.
    ReactionCue,
    /// A Go / no-go cue, now, in a run that is being played.
    GoNoGoCue {
        friendly: bool,
    },
    /// The results screen of a Reaction test taken after an earlier one, with this latency offset.
    ReactionResults {
        offset_ms: f32,
    },
    /// A scene of the finder: its page with staged data, a calibration step, a check's result.
    Finder(FinderScene),
    /// The results of a Calm aim test, with an earlier one to compare with.
    CalmResult,
    /// The Benchmark page with four weekly benchmarks (staged unless there are some).
    Benchmarks,
    /// A benchmark's break screen after two scenarios.
    BenchmarkBreak,
    /// The result of a benchmark played to the end (the latest staged one).
    BenchmarkResult,
    /// The Plan page with a week built on the staged benchmarks, a weak spot and a Calm aim
    /// result, its sessions before today done (and today's, if `done_today`).
    Plan {
        done_today: bool,
    },
}

#[derive(Resource)]
struct Tour {
    steps: std::vec::IntoIter<Step>,
    /// Seconds until the next step.
    wait: f32,
    /// A key pressed by the last step, released before the next.
    held: Option<KeyCode>,
    /// Whether the last step pressed the mouse button, and how many shots were fired so far.
    clicking: bool,
    shots: usize,
    /// The scenario being measured and its frame times so far, seconds.
    measuring: Option<(String, Vec<f32>)>,
    /// Seconds the fire button is still held for (`Step::Fire`).
    firing: f32,
}

/// Long enough for a page (and the preview camera) to settle.
const SETTLE: f32 = 0.8;

/// From the pause menu (Escape during a drill) back to the screen the drill was started from.
fn leave_paused() -> [Step; 2] {
    [Step::Press(KeyCode::KeyQ), Step::Wait(SETTLE)]
}

impl Tour {
    fn new(dir: PathBuf) -> Self {
        let shot = |name: String| Step::Shot(dir.join(format!("{name}.png")));
        let mut steps = vec![Step::Wait(2.0)];
        for tab in MenuTab::ALL {
            steps.extend([Step::Tab(tab), Step::Wait(SETTLE), shot(format!("menu_{}", tab.name().to_lowercase()))]);
        }
        // Pages whose look depends on what is picked.
        // Long enough for buttons that arm after a delay to do so.
        let stage = |stage: Stage, name: &str| [Step::Stage(stage), Step::Wait(1.3), shot(name.into())];
        steps.extend(stage(Stage::Play(PlaySelection::Drill(DrillKind::Microflick)), "menu_play_adaptive"));
        steps.extend(stage(Stage::Play(PlaySelection::Routine(0)), "menu_play_routine"));
        steps.extend(stage(Stage::Play(PlaySelection::Drill(DrillKind::Survival)), "menu_play_endless"));
        steps.push(Step::Stage(Stage::Play(PlaySelection::default())));
        steps.extend(stage(Stage::History("Reaction"), "menu_history_other"));
        steps.extend(stage(Stage::History("Peeks"), "menu_history_peeks"));
        steps.extend(stage(Stage::CustomEditor, "menu_custom_editor"));
        // Screens outside the menu.
        steps.extend(stage(Stage::RoutineBreak, "routine_break"));
        steps.extend(stage(Stage::RoutineSummary, "routine_summary"));
        steps.extend(stage(Stage::SmartTrial, "finder_smart_trial"));
        steps.extend(stage(Stage::SmartRating, "finder_smart_rating"));
        steps.extend(stage(Stage::SmartResult, "finder_smart_result"));
        steps.extend(stage(Stage::ClassicChoice, "finder_classic_choice"));
        steps.extend(stage(Stage::ClassicResult, "finder_classic_result"));
        steps.extend(stage(Stage::Replay, "replay"));
        steps.extend([Step::Stage(Stage::Menu), Step::Tab(MenuTab::Play), Step::Wait(SETTLE)]);
        // The finder: its page in the states that take days to reach, and its procedures.
        for (scene, name, wait) in SCENES {
            steps.extend([Step::Stage(Stage::Finder(scene)), Step::Wait(wait), shot(name.into())]);
        }
        steps.extend([Step::Stage(Stage::Menu), Step::Tab(MenuTab::Play), Step::Wait(SETTLE)]);
        let drills = [
            DrillKind::Gridshot,
            DrillKind::MovingGridshot,
            DrillKind::Survival,
            DrillKind::SwitchTracking,
            DrillKind::Peek,
            DrillKind::Tracking,
            DrillKind::WallFlicks,
            DrillKind::Pokeball,
            DrillKind::TargetSwitch,
            DrillKind::SmoothTracking,
        ];
        for (index, kind) in drills.into_iter().enumerate() {
            let name = file_name(kind);
            steps.extend([
                Step::Drill(kind, 60.0),
                Step::Wait(SETTLE),
                shot(format!("ready_{name}")),
                Step::Press(KeyCode::Space),
                // Countdown, then a moment of play.
                Step::Wait(3.0),
                shot(format!("drill_{name}")),
                Step::Press(KeyCode::Escape),
                Step::Wait(SETTLE),
            ]);
            // The pause menu and the countdown back into the run are the same over every drill.
            if index == 0 {
                steps.extend([
                    shot("pause".into()),
                    Step::Press(KeyCode::Escape),
                    Step::Wait(0.3),
                    shot("resume".into()),
                    Step::Press(KeyCode::Escape),
                    Step::Wait(SETTLE),
                ]);
            }
            steps.extend(leave_paused());
        }
        // The wall and target-switching drills: their Play page cards, a custom scenario, and a
        // short run of each played to its results (clicks at the wall, held fire at the bots),
        // then those runs in the history and the newest as a replay.
        const CLICK_SECS: f32 = 8.0;
        steps.extend([Step::Stage(Stage::Menu), Step::Wait(SETTLE)]);
        for kind in [DrillKind::WallFlicks, DrillKind::Pokeball, DrillKind::TargetSwitch] {
            steps.extend(stage(Stage::Play(PlaySelection::Drill(kind)), &format!("menu_play_{}", file_name(kind))));
        }
        steps.push(Step::Stage(Stage::Play(PlaySelection::default())));
        steps.extend(stage(Stage::CustomScenario(DrillKind::TargetSwitch), "menu_custom_target_switching"));
        steps.extend(stage(Stage::CustomScenario(DrillKind::Pokeball), "menu_custom_pokeball"));
        steps.extend([Step::Drill(DrillKind::WallFlicks, CLICK_SECS), Step::Wait(SETTLE), Step::Press(KeyCode::Space)]);
        steps.push(Step::Wait(2.0));
        for _ in 0..16 {
            steps.extend([Step::Shoot, Step::Wait(0.35)]);
        }
        steps.extend([Step::Wait(CLICK_SECS - 16.0 * 0.35 + 1.5), shot("results_wall_flicks".into())]);
        steps.extend([
            Step::Drill(DrillKind::TargetSwitch, CLICK_SECS),
            Step::Wait(SETTLE),
            Step::Press(KeyCode::Space),
            Step::Wait(2.0),
            Step::Fire(2.0),
            shot("drill_target_switching_fire".into()),
            Step::Fire(3.0),
            Step::Wait(CLICK_SECS - 5.0 + 1.5),
            shot("results_target_switching".into()),
            Step::Stage(Stage::Menu),
            Step::Wait(SETTLE),
        ]);
        steps.extend(stage(Stage::HistoryOf(DrillKind::TargetSwitch), "menu_history_target_switching"));
        steps.extend(stage(Stage::HistoryOf(DrillKind::WallFlicks), "menu_history_wall_flicks"));
        steps.extend(stage(Stage::Replay, "replay_target_switching"));
        steps.extend([Step::Stage(Stage::Menu), Step::Tab(MenuTab::Play), Step::Wait(SETTLE)]);
        // The bot drills: the ready card, then a moment of play with two quick shots at the body
        // (a bot's body is a few head radii below its head), for the health bar and the spread
        // bar of a rifle that hasn't settled.
        const BODY: Vec2 = Vec2::new(0.0, -3.5);
        for kind in [DrillKind::HeadTaps, DrillKind::StrafingBot] {
            let name = file_name(kind);
            steps.extend([
                Step::Drill(kind, 60.0),
                Step::Wait(SETTLE),
                shot(format!("ready_{name}")),
                Step::Press(KeyCode::Space),
                Step::Wait(2.5),
                Step::ShootAt(BODY),
                Step::Wait(0.15),
                Step::ShootAt(BODY),
                Step::Wait(0.05),
                shot(format!("drill_{name}")),
                Step::Press(KeyCode::Escape),
                Step::Wait(SETTLE),
            ]);
            steps.extend(leave_paused());
        }
        // Counter-strafe: in cover, strafed out with the bot in view, and on the move (the
        // spread bar again).
        let name = file_name(DrillKind::CounterStrafe);
        steps.extend([
            Step::Drill(DrillKind::CounterStrafe, 60.0),
            Step::Wait(SETTLE),
            shot(format!("ready_{name}")),
            Step::Press(KeyCode::Space),
            Step::Wait(2.2),
            shot(format!("drill_{name}_cover")),
            Step::Hold(KeyCode::KeyD),
            Step::Wait(0.5),
            Step::Release(KeyCode::KeyD),
            Step::Wait(0.4),
            shot(format!("drill_{name}_peek")),
            Step::Hold(KeyCode::KeyA),
            Step::Wait(0.12),
            shot(format!("drill_{name}_moving")),
            Step::Release(KeyCode::KeyA),
            Step::Press(KeyCode::Escape),
            Step::Wait(SETTLE),
        ]);
        steps.extend(leave_paused());
        // Their tiles on the Play page and their results breakdown.
        steps.extend(stage(Stage::Play(PlaySelection::Drill(DrillKind::CounterStrafe)), "menu_play_bots"));
        steps.extend(stage(Stage::TapResults(DrillKind::HeadTaps), "results_head_taps"));
        steps.extend(stage(Stage::TapResults(DrillKind::CounterStrafe), "results_counter-strafe"));
        // A short Counter-strafe run played to the end (out to the right, a shot at the head, back
        // through cover and out to the left, another shot), then its replay: the bot and the pillar
        // as the strafing player saw them.
        const HEAD: Vec2 = Vec2::new(0.1, 0.1);
        steps.extend([
            Step::Drill(DrillKind::CounterStrafe, 6.0),
            Step::Wait(SETTLE),
            Step::Press(KeyCode::Space),
            Step::Wait(2.0),
            Step::Hold(KeyCode::KeyD),
            Step::Wait(0.5),
            Step::Release(KeyCode::KeyD),
            Step::Wait(0.5),
            Step::ShootAt(HEAD),
            Step::Wait(0.5),
            Step::Hold(KeyCode::KeyA),
            Step::Wait(0.9),
            Step::Release(KeyCode::KeyA),
            Step::Wait(0.4),
            Step::ShootAt(HEAD),
            Step::Wait(4.5),
            shot(format!("results_{name}_run")),
        ]);
        steps.extend(stage(Stage::Replay, "replay_counter-strafe"));
        steps.extend([Step::Stage(Stage::Menu), Step::Wait(SETTLE)]);
        // The Reaction test: its tile, a wait, a false start, the dot and an answer, then its
        // results without and with a latency offset.
        let test = "reaction_test";
        steps.extend(stage(Stage::Play(PlaySelection::Drill(DrillKind::ReactionTest)), "menu_play_reaction_test"));
        steps.extend([
            Step::Drill(DrillKind::ReactionTest, 60.0),
            Step::Wait(SETTLE),
            shot(format!("ready_{test}")),
            Step::Press(KeyCode::Space),
            // The countdown, then into the first wait (which lasts at least 1.5 s).
            Step::Wait(2.0),
            shot(format!("{test}_wait")),
            Step::Shoot,
            Step::Wait(0.3),
            shot(format!("{test}_false_start")),
            Step::Stage(Stage::ReactionCue),
            Step::Wait(0.2),
            shot(format!("{test}_dot")),
            Step::Shoot,
            Step::Wait(0.3),
            shot(format!("{test}_answered")),
            Step::Press(KeyCode::Escape),
            Step::Wait(SETTLE),
        ]);
        steps.extend(leave_paused());
        steps.extend(stage(Stage::ReactionResults { offset_ms: 0.0 }, "results_reaction_test"));
        steps.push(Step::Stage(Stage::Menu));
        steps.extend(stage(Stage::ReactionResults { offset_ms: 20.0 }, "results_reaction_test_corrected"));
        steps.push(Step::Stage(Stage::Menu));
        // Go / no-go: an enemy, a friendly, and the friendly shot.
        let drill = "go_no_go";
        steps.extend(stage(Stage::Play(PlaySelection::Drill(DrillKind::GoNoGo)), "menu_play_go_no_go"));
        steps.extend([
            Step::Drill(DrillKind::GoNoGo, 60.0),
            Step::Wait(SETTLE),
            shot(format!("ready_{drill}")),
            Step::Press(KeyCode::Space),
            // The countdown, then into the first wait.
            Step::Wait(1.8),
            Step::Stage(Stage::GoNoGoCue { friendly: false }),
            Step::Wait(0.1),
            shot(format!("{drill}_enemy")),
            // Its time runs out: a miss.
            Step::Wait(0.4),
            Step::Stage(Stage::GoNoGoCue { friendly: true }),
            Step::Wait(0.1),
            shot(format!("{drill}_friendly")),
            Step::Shoot,
            Step::Wait(0.3),
            shot(format!("{drill}_false_alarm")),
            Step::Press(KeyCode::Escape),
            Step::Wait(SETTLE),
        ]);
        steps.extend(leave_paused());
        steps.push(Step::Stage(Stage::Play(PlaySelection::default())));
        steps.extend(calm_aim_steps(&shot));
        steps.extend(tracking_result_steps(&shot));
        steps.extend(stage(Stage::CalmResult, "results_calm_aim"));
        steps.extend([Step::Stage(Stage::Menu), Step::Wait(SETTLE)]);
        // The benchmark (with staged results: a page with several, a break, a result) and the
        // weekly plan built on them, with the week partly done and with today's session done.
        steps.extend(stage(Stage::Benchmarks, "menu_benchmark_progress"));
        steps.extend(stage(Stage::BenchmarkBreak, "routine_benchmark_break"));
        steps.extend(stage(Stage::BenchmarkResult, "benchmark_result"));
        steps.extend([Step::Stage(Stage::Menu), Step::Wait(SETTLE)]);
        steps.extend(stage(Stage::Plan { done_today: false }, "menu_plan_week"));
        steps.extend(stage(Stage::Plan { done_today: true }, "menu_plan_done"));
        steps.extend([Step::Stage(Stage::Menu), Step::Wait(SETTLE)]);
        // A short run played to the end, with hits and a few misses, for the results screen.
        const RESULT_SECS: f32 = 10.0;
        const SHOT_EVERY: f32 = 0.3;
        steps.extend([Step::Drill(DrillKind::Gridshot, RESULT_SECS), Step::Wait(SETTLE), Step::Press(KeyCode::Space)]);
        // The countdown, then shots for most of the run.
        steps.push(Step::Wait(2.0));
        for _ in 0..(RESULT_SECS * 0.8 / SHOT_EVERY) as usize {
            steps.extend([Step::Shoot, Step::Wait(SHOT_EVERY)]);
        }
        steps.extend([Step::Wait(RESULT_SECS * 0.2 + 2.0), shot("results".into()), Step::Exit]);
        Self::from_steps(steps)
    }

    /// The `--bench` script: each scenario warms up, then is measured for a few seconds.
    fn bench() -> Self {
        const WARMUP: f32 = 2.0;
        const MEASURE: f32 = 5.0;
        let mut steps = vec![Step::Wait(WARMUP)];
        for tab in [MenuTab::Play, MenuTab::Video] {
            steps.extend([
                Step::Tab(tab),
                Step::Wait(WARMUP),
                Step::Measure(format!("menu_{}", tab.name().to_lowercase()), MEASURE),
            ]);
        }
        steps.push(Step::Tab(MenuTab::Play));
        for kind in [DrillKind::Gridshot, DrillKind::Tracking, DrillKind::SwitchTracking] {
            steps.extend([
                Step::Drill(kind, 60.0),
                Step::Wait(SETTLE),
                Step::Press(KeyCode::Space),
                // Countdown, then play.
                Step::Wait(WARMUP + 1.5),
                Step::Measure(file_name(kind), MEASURE),
                Step::Press(KeyCode::Escape),
                Step::Wait(SETTLE),
            ]);
            steps.extend(leave_paused());
        }
        steps.push(Step::Exit);
        Self::from_steps(steps)
    }

    fn from_steps(steps: Vec<Step>) -> Self {
        Self {
            steps: steps.into_iter(),
            wait: 0.0,
            held: None,
            clicking: false,
            shots: 0,
            measuring: None,
            firing: 0.0,
        }
    }
}

/// Calm aim from its ready card through its four parts: each part's banner, then a moment of
/// the part itself with the crosshair where the part wants it. Waits follow the test's plan
/// (parts start 0, 21, 46 and 69 s into it, each after a 3 s banner); a screenshot takes
/// [`SHOT_SECS`] of it.
fn calm_aim_steps(shot: &dyn Fn(String) -> Step) -> Vec<Step> {
    vec![
        Step::Drill(DrillKind::CalmAim, 60.0),
        Step::Wait(SETTLE),
        shot("ready_calm_aim".into()),
        Step::Press(KeyCode::Space),
        // The countdown, then 1 s into the test.
        Step::Wait(2.5),
        shot("calm_1_banner".into()),
        // 5 s in: on the dot as it glides.
        Step::Wait(3.5),
        Step::Aim,
        Step::Wait(0.05),
        shot("calm_1_smoothness".into()),
        Step::Wait(16.4),
        shot("calm_2_banner".into()),
        // Resting on the mark from the start of the part: its target is up from 27.8 s at the
        // latest to 28.8 s at the earliest.
        Step::Look(4.0, 0.0),
        Step::Wait(5.5),
        shot("calm_2_stillness".into()),
        Step::Wait(18.5),
        shot("calm_3_banner".into()),
        Step::Wait(2.2),
        shot("calm_3_flick".into()),
        Step::Wait(19.8),
        shot("calm_4_banner".into()),
        // Half-way through the stay on the first target.
        Step::Wait(1.8),
        Step::Aim,
        Step::Wait(0.25),
        shot("calm_4_settle".into()),
        Step::Press(KeyCode::Escape),
        Step::Wait(SETTLE),
        Step::Press(KeyCode::KeyQ),
        Step::Wait(SETTLE),
    ]
}

/// A short Tracking run played to its end by a crosshair that hops onto the dot ten times a
/// second, for the results card on how its changes of direction were followed.
fn tracking_result_steps(shot: &dyn Fn(String) -> Step) -> Vec<Step> {
    const SECS: f32 = 12.0;
    const HOP_EVERY: f32 = 0.1;
    let mut steps = vec![Step::Drill(DrillKind::Tracking, SECS), Step::Wait(SETTLE), Step::Press(KeyCode::Space)];
    // The countdown, then hops for the whole run with the fire button held.
    steps.extend([Step::Wait(1.6), Step::Trigger(true)]);
    for _ in 0..(SECS / HOP_EVERY) as usize {
        steps.extend([Step::Aim, Step::Wait(HOP_EVERY)]);
    }
    steps.push(Step::Trigger(false));
    steps.extend([Step::Wait(2.0), shot("results_tracking".into()), Step::Stage(Stage::Menu), Step::Wait(SETTLE)]);
    steps
}

/// Two Calm aim results for the staged results screen: an earlier, tenser test and the one shown.
fn staged_calm_results() -> [CalmResult; 2] {
    let earlier = CalmResult {
        smoothness: Smoothness { jitter: 0.17, with_target: 0.97, error: 0.62, on_target: 0.66 },
        stillness: Stillness {
            waits: 5,
            wait_secs: 12.4,
            drift: 0.42,
            travel: 9.3,
            early_shots: 1,
            targets: 5,
            moves: 8,
            expired: 0,
        },
        flicks: Flicks { kills: 12, expired: 1, shots: 15, moving_shots: 5, overshot: 5, corrections: 7 },
        settling: Settling { targets: 9, expired: 0, secs: 0.21, exits: 5 },
    };
    let shown = CalmResult {
        smoothness: Smoothness { jitter: 0.11, with_target: 0.99, error: 0.48, on_target: 0.78 },
        stillness: Stillness {
            waits: 5,
            wait_secs: 12.9,
            drift: 0.19,
            travel: 4.1,
            early_shots: 0,
            targets: 5,
            moves: 6,
            expired: 0,
        },
        flicks: Flicks { kills: 13, expired: 0, shots: 14, moving_shots: 2, overshot: 4, corrections: 5 },
        settling: Settling { targets: 10, expired: 0, secs: 0.24, exits: 6 },
    };
    [earlier, shown]
}

fn file_name(kind: DrillKind) -> String {
    kind.name().to_lowercase().replace(' ', "_")
}

/// Frame-time statistics of a measured scenario.
#[derive(Debug, PartialEq)]
struct FrameStats {
    frames: usize,
    fps: f32,
    /// Frame times, milliseconds.
    p50: f32,
    p99: f32,
    max: f32,
}

impl FrameStats {
    /// `None` without frames. Sorts `secs`.
    fn new(secs: &mut [f32]) -> Option<Self> {
        secs.sort_by(f32::total_cmp);
        let max = *secs.last()?;
        let frames = secs.len();
        let percentile = |p: f32| secs[((frames - 1) as f32 * p).round() as usize] * 1000.0;
        Some(Self {
            frames,
            fps: frames as f32 / secs.iter().sum::<f32>(),
            p50: percentile(0.5),
            p99: percentile(0.99),
            max: max * 1000.0,
        })
    }
}

/// Time a screenshot gets to be read back and saved before the tour moves on.
const SHOT_SECS: f32 = 0.5;

/// A drill's targets: not the Video page's preview targets, which live on their own layer.
type DrillTarget = (With<FacesEye>, Without<RenderLayers>, Without<PlayerCamera>);

/// What the tour's steps act on.
#[derive(SystemParam)]
struct Stagehand<'w, 's> {
    commands: Commands<'w, 's>,
    settings: Res<'w, Settings>,
    next_state: ResMut<'w, NextState<AppState>>,
    tab: ResMut<'w, MenuTab>,
    picked: ResMut<'w, PlaySelection>,
    history_view: ResMut<'w, HistoryView>,
    custom: ResMut<'w, CustomContent>,
    editing: ResMut<'w, CustomSelection>,
    replays: Res<'w, ReplayDir>,
    calm_log: ResMut<'w, CalmLog>,
    look: ResMut<'w, Look>,
    camera: Single<'w, 's, &'static Transform, With<PlayerCamera>>,
    targets: Query<'w, 's, &'static Transform, DrillTarget>,
    reaction_test: ResMut<'w, ReactionTest>,
    go_no_go: ResMut<'w, GoNoGo>,
    reaction_log: ResMut<'w, ReactionLog>,
    finder: FinderStagehand<'w>,
    benchmarks: ResMut<'w, BenchmarkLog>,
    plan: ResMut<'w, PlanStore>,
}

impl Stagehand<'_, '_> {
    fn play(&mut self, kind: DrillKind, secs: f32) {
        let drill = DrillSpec::builtin(kind, &self.settings);
        let run = Run::new(drill, self.settings.sens, secs, RunPurpose::Practice, kind.name().to_string());
        begin_run(&mut self.commands, &mut self.next_state, run);
    }

    /// Four weekly benchmarks in the log (in memory only), unless it has some.
    fn stage_benchmarks(&mut self) {
        if self.benchmarks.entries().is_empty() {
            for staged in staged_benchmarks(crate::finder::unix_now()) {
                self.benchmarks.stage(staged);
            }
        }
    }

    fn set(&mut self, stage: Stage) {
        let sens = self.settings.sens;
        match stage {
            Stage::Play(picked) => {
                *self.picked = picked;
                // Not a new assignment when Play is already open: that would rebuild the page in
                // the frame its detail card is being refilled for the new pick.
                self.tab.set_if_neq(MenuTab::Play);
            },
            Stage::History(drill) => {
                self.history_view.show(drill);
                // As for Play: no rebuild of a page that is already open.
                self.tab.set_if_neq(MenuTab::History);
            },
            Stage::CustomEditor => {
                let config = DrillConfig::builtin(DrillKind::Gridshot, &self.settings);
                let id = self.custom.upsert_scenario("Tour scenario", 45.0, config);
                self.editing.edit_scenario(id);
                // As for Play: no rebuild of a page that is already open.
                self.tab.set_if_neq(MenuTab::Custom);
            },
            Stage::RoutineBreak => {
                let mut routine = Routine::from_plan(&ROUTINES[0], &self.settings);
                routine.record(staged_stats(routine.steps[0].drill.config.kind, 31, 36), staged_comparison());
                start_routine(&mut self.commands, &mut self.next_state, routine);
            },
            Stage::RoutineSummary => {
                let mut routine = Routine::from_plan(&ROUTINES[0], &self.settings);
                for step in 0..routine.steps.len() - 1 {
                    let kind = routine.steps[step].drill.config.kind;
                    routine.record(staged_stats(kind, 24 + 5 * step as u32, 36), staged_comparison());
                }
                routine.skip();
                start_routine(&mut self.commands, &mut self.next_state, routine);
            },
            Stage::SmartTrial | Stage::SmartRating | Stage::SmartResult => {
                let mut finder = SmartFinder::new(sens, self.settings.finder_focus);
                // The trial and rating screens come up in the middle of a session.
                const EARLIER_TRIALS: usize = 4;
                let mut rated = 0;
                while finder.step == SmartStep::Trial && !(stage == Stage::SmartTrial && rated == EARLIER_TRIALS) {
                    finder.begin_trial();
                    // Scores that peak a little above the starting sens.
                    let off = (finder.current_sens() / (sens * 1.08)).ln();
                    let quality = (-off * off / 0.18).exp();
                    // First flicks that land on target a little above it, too.
                    let landing = finder.current_sens() / (sens * 1.12);
                    for (part, drill) in finder.segment_order().into_iter().enumerate() {
                        let score = 14_000.0 + 9_000.0 * quality + 600.0 * part as f32;
                        let segment = Segment {
                            score,
                            throughput: Some(3.0 + 2.0 * quality),
                            landings: vec![landing; 12],
                            biases: vec![landing - 1.0; 12],
                        };
                        finder.record_segment(drill, segment);
                    }
                    if finder.step != SmartStep::Rate || (stage == Stage::SmartRating && rated == EARLIER_TRIALS) {
                        break;
                    }
                    finder.rate(2 + (quality * 3.0) as u8);
                    rated += 1;
                }
                self.commands.insert_resource(finder);
                self.next_state.set(AppState::Finder);
            },
            Stage::ClassicChoice | Stage::ClassicResult => {
                let mut finder = Finder::new(sens, self.settings.finder_rounds);
                loop {
                    finder.record_trial(21_400.0);
                    finder.record_trial(23_100.0);
                    if finder.step != FinderStep::Choose || (stage == Stage::ClassicChoice && finder.round == 2) {
                        break;
                    }
                    finder.choose(finder.round.is_multiple_of(2));
                }
                self.commands.insert_resource(finder);
                self.next_state.set(AppState::Finder);
            },
            Stage::Replay => match Replay::recent(&self.replays.0, &DrillKind::ALL, 1).pop() {
                Some(replay) => watch(&mut self.commands, &mut self.next_state, replay, AppState::Menu),
                None => warn!("tour: no replay to show"),
            },
            Stage::Menu => {
                self.commands.remove_resource::<Finder>();
                self.commands.remove_resource::<SmartFinder>();
                self.commands.remove_resource::<Routine>();
                self.finder.reset(&mut self.commands);
                self.next_state.set(AppState::Menu);
            },
            Stage::CustomScenario(kind) => {
                let config = DrillConfig::builtin(kind, &self.settings);
                let id = self.custom.upsert_scenario(&format!("Tour {}", kind.name().to_lowercase()), 45.0, config);
                self.editing.edit_scenario(id);
                // As for Play: no rebuild of a page that is already open.
                self.tab.set_if_neq(MenuTab::Custom);
            },
            Stage::HistoryOf(kind) => {
                self.history_view.show(&DrillSpec::builtin(kind, &self.settings).name);
                // As for Play: no rebuild of a page that is already open.
                self.tab.set_if_neq(MenuTab::History);
            },
            Stage::TapResults(kind) => {
                // A run with a leak to name: shots on the move where the player moves, shots
                // fired too soon where not.
                let moves = range::player_moves(kind);
                let movement = MoveStats { moving: 7, speed_total: 31.0, stops: 12, stop_delay_total: 2.9 };
                let stats = TapStats {
                    shots: 20,
                    kills: 11,
                    head_hits: 9,
                    body_hits: 6,
                    first_shots: 12,
                    first_heads: 7,
                    unsettled: if moves { 3 } else { 6 },
                    movement: moves.then_some(movement),
                };
                self.commands.insert_resource(TapRun::with_stats(kind, stats));
                self.commands.insert_resource(LastRun {
                    stats: staged_stats(kind, stats.kills, stats.shots),
                    previous: staged_comparison(),
                    timestamp: 0,
                    drill: DrillSpec::builtin(kind, &self.settings),
                    duration: 30.0,
                });
                self.next_state.set(AppState::Results);
            },
            Stage::ReactionCue => self.reaction_test.cue_now(),
            Stage::GoNoGoCue { friendly } => self.go_no_go.cue_now(friendly),
            Stage::ReactionResults { offset_ms } => {
                let drill = DrillSpec::builtin(DrillKind::ReactionTest, &self.settings);
                // Two days apart, the later one a little faster.
                let tests = [(1_793_000_000, 224.0), (1_793_172_800, 213.0)].map(|(timestamp, around)| {
                    let trials = staged_reactions(around);
                    ReactionEntry {
                        timestamp,
                        drill: drill.name.clone(),
                        trials,
                        false_starts: 1,
                        frame_ms: 5.62,
                        offset_ms,
                    }
                });
                let medians = tests.each_ref().map(|test| test.summary().map_or(0.0, |summary| summary.median));
                for test in &tests {
                    self.reaction_log.record(test);
                }
                let [earlier, score] = medians.map(reaction::score);
                let stats =
                    RunStats { expired: 1, rating: Some(score), ..staged_stats(DrillKind::ReactionTest, 29, 30) };
                self.commands.insert_resource(LastRun {
                    stats,
                    previous: Comparison { best: Some(earlier), recent_avg: Some(earlier), recent_count: 1 },
                    timestamp: tests[1].timestamp,
                    drill,
                    duration: self.settings.run_secs,
                });
                self.next_state.set(AppState::Results);
            },
            Stage::Finder(scene) => {
                self.finder.set(scene, &self.settings, &mut self.commands, &mut self.next_state, &mut self.tab);
            },
            Stage::CalmResult => {
                let drill = DrillSpec::builtin(DrillKind::CalmAim, &self.settings);
                let [earlier, shown] = staged_calm_results();
                // Timestamps no run has: the staged pair is found by them and nothing else is.
                for (timestamp, result) in [(1, earlier), (2, shown)] {
                    self.calm_log.stage(CalmEntry { timestamp, drill: drill.name.clone(), result });
                }
                let stats = RunStats {
                    drill: drill.name.clone(),
                    duration: 90.0,
                    rating: Some(shown.overall()),
                    ..staged_stats(DrillKind::CalmAim, 18, 19)
                };
                let before = Some(earlier.overall());
                let previous = Comparison { best: before, recent_avg: before, recent_count: 1 };
                self.commands.insert_resource(LastRun { stats, previous, timestamp: 2, drill, duration: 90.0 });
                self.next_state.set(AppState::Results);
            },
            Stage::Benchmarks => {
                self.stage_benchmarks();
                self.tab.set_if_neq(MenuTab::Benchmark);
            },
            Stage::BenchmarkBreak | Stage::BenchmarkResult => {
                self.stage_benchmarks();
                let mut routine = benchmark::routine();
                let played = if stage == Stage::BenchmarkBreak { 2 } else { SCENARIOS.len() };
                for scenario in &SCENARIOS[..played] {
                    let stats = RunStats { drill: scenario.name.to_string(), ..staged_stats(scenario.drill, 31, 36) };
                    routine.record(stats, staged_comparison());
                }
                start_routine(&mut self.commands, &mut self.next_state, routine);
            },
            Stage::Plan { done_today } => {
                self.stage_benchmarks();
                *self.plan = staged_plan(self.benchmarks.entries(), done_today);
                self.tab.set_if_neq(MenuTab::Plan);
            },
        }
    }

    /// Turns the view to the target nearest the crosshair, `offset` target radii off its centre.
    fn aim(&mut self, offset: Vec2) {
        let origin = self.camera.translation;
        let forward = self.camera.forward().as_vec3();
        let nearest = self.targets.iter().min_by(|a, b| {
            forward.angle_between(a.translation - origin).total_cmp(&forward.angle_between(b.translation - origin))
        });
        let Some(target) = nearest else { return };
        let to_target = target.translation - origin;
        let radius = target.scale.x / to_target.length();
        let direction = to_target.normalize();
        // The camera looks along -Z, turned by yaw around Y and then pitch around X.
        let yaw = (-direction.x).atan2(-direction.z) + offset.x * radius;
        let pitch = direction.y.asin() + offset.y * radius;
        self.look.set(yaw, pitch);
    }
}

/// Where the tour's shots land, in target radii from the centre: mostly hits, some misses.
const AIM_OFFSETS: [Vec2; 8] = [
    Vec2::new(0.1, 0.1),
    Vec2::new(-0.35, 0.2),
    Vec2::new(0.5, -0.15),
    Vec2::new(0.2, -0.45),
    Vec2::new(1.25, 0.1),
    Vec2::new(-0.15, 0.3),
    Vec2::new(0.7, 0.35),
    Vec2::new(0.3, 0.0),
];

/// A Reaction test's 30 trials around `around` ms for staged screens: most within 15 ms, a few
/// slow ones and a lapse.
fn staged_reactions(around: f32) -> Vec<Trial> {
    const OFF: [f32; 30] = [
        12.0, -8.0, 3.0, -15.0, 22.0, -4.0, 7.0, -11.0, 0.0, 31.0, -6.0, -13.0, 5.0, 18.0, -2.0, -9.0, 64.0, 1.0,
        -17.0, 9.0, 0.0, 14.0, -1.0, -12.0, 6.0, -7.0, 26.0, -3.0, 10.0, -10.0,
    ];
    OFF.iter().enumerate().map(|(i, off)| if i == 20 { Trial::Lapse } else { Trial::Valid(around + off) }).collect()
}

/// A finished run for staged screens.
fn staged_stats(kind: DrillKind, hits: u32, shots: u32) -> RunStats {
    RunStats {
        kind,
        drill: kind.name().to_string(),
        sens: 0.4,
        cm_per_360: 40.8,
        duration: 30.0,
        hits,
        shots,
        expired: 0,
        ttk_total: hits as f32 * 0.42,
        best_ttk: 0.27,
        on_target: 21.0,
        landings: Vec::new(),
        clicks: Vec::new(),
        rating: None,
        rounds_hit: 0,
    }
}

/// Four weekly benchmarks for staged screens, the latest yesterday: most kinds improving, with
/// reactive tracking slipping last time.
fn staged_benchmarks(now: u64) -> Vec<Benchmark> {
    const SCORES: [f32; 8] = [5200.0, 6100.0, 950.0, 6400.0, 7800.0, 1500.0, 2400.0, 1600.0];
    const GROWTH: [[f32; 8]; 4] = [
        [1.0; 8],
        [1.03, 1.06, 1.02, 1.04, 1.05, 1.08, 1.03, 0.97],
        [1.05, 1.09, 1.07, 1.08, 1.08, 1.10, 1.06, 1.04],
        [1.07, 1.15, 1.12, 1.01, 1.11, 1.16, 1.09, 1.10],
    ];
    const DAY: u64 = 86_400;
    GROWTH
        .iter()
        .enumerate()
        .map(|(week, growth)| {
            let timestamp = now - (3 - week as u64) * 7 * DAY - DAY;
            let results = SCENARIOS
                .iter()
                .enumerate()
                .map(|(i, scenario)| {
                    let score = (SCORES[i] * growth[i]).round();
                    ScenarioResult {
                        run: timestamp - (SCENARIOS.len() - i) as u64 * 80,
                        score,
                        throughput: (scenario.measure == Measure::Throughput).then_some(3.6 * growth[i]),
                        on_target: (scenario.measure == Measure::OnTarget).then_some(score / 10_000.0),
                    }
                })
                .collect();
            Benchmark { timestamp, cm_per_360: 40.8, results }
        })
        .collect()
}

/// A plan for this week built on `log` (with today a training day: the day after the latest
/// benchmark), with a weak spot, a Calm aim result, a drill never played and one not played for
/// a while, and last week's plan before it. Sessions before today are done.
fn staged_plan(log: &[Benchmark], done_today: bool) -> PlanStore {
    let (today, offset) = (training::today(), training::utc_offset_secs());
    let mut last_played: Vec<(DrillKind, i64)> = AimKind::ALL
        .iter()
        .flat_map(|aim| aim.drills())
        .filter(|&&drill| drill != DrillKind::Pokeball)
        .map(|&drill| (drill, if drill == DrillKind::MovingGridshot { today - 12 } else { today - 2 }))
        .collect();
    last_played.sort_by_key(|played| played.1);
    let basis_of = |log: &[Benchmark]| {
        (
            log.last().map(|b| (b.timestamp, training::local_day(b.timestamp, offset))),
            benchmark::least_improved(log).map(|(aim, index, overall)| Weakest { aim, index, overall }),
        )
    };
    let (latest, weakest) = basis_of(log);
    let inputs = PlanInputs {
        today,
        days: 4,
        focus: Focus::Valorant,
        last_played,
        benchmark: latest,
        benchmarks: log.len(),
        weakest,
        weak_spot: Some(WeakSpot { sector: Some(5), band: Some(2) }),
        calm: Some((Part::Stillness, 58.0)),
        last_benchmark_weekday: None,
    };
    let earlier = &log[..log.len().saturating_sub(1)];
    let (latest, weakest) = basis_of(earlier);
    let last_week = PlanInputs {
        today: today - 7,
        benchmark: latest,
        benchmarks: earlier.len(),
        weakest,
        weak_spot: None,
        calm: None,
        ..inputs.clone()
    };
    let mut store = PlanStore { days: 4, focus: Focus::Valorant, current: None, previous: None };
    store.adopt(training::plan::generate(&last_week), today - 7);
    store.adopt(training::plan::generate(&inputs), today);
    // Made on Monday, so the days before today were part of it.
    if let Some(plan) = store.current.as_mut() {
        plan.made = training::monday(today);
    }
    let done: Vec<i64> = store
        .current
        .iter()
        .flat_map(|plan| plan.sessions())
        .filter(|day| day.day < today || (done_today && day.day == today))
        .map(|day| day.day)
        .collect();
    for day in done {
        store.mark_done(day);
    }
    store
}

fn staged_comparison() -> Comparison {
    Comparison { best: Some(6400.0), recent_avg: Some(5200.0), recent_count: 5 }
}

fn run_tour(
    time: Res<Time>,
    real_time: Res<Time<Real>>,
    mut tour: ResMut<Tour>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut stage: Stagehand,
    mut exit: MessageWriter<AppExit>,
) {
    if let Some(key) = tour.held.take() {
        keys.release(key);
    }
    if std::mem::take(&mut tour.clicking) {
        mouse.release(MouseButton::Left);
    }
    if tour.firing > 0.0 {
        tour.firing -= time.delta_secs();
        stage.aim(AIM_OFFSETS[0]);
        if tour.firing <= 0.0 {
            mouse.release(MouseButton::Left);
        }
    }
    if let Some((_, frames)) = &mut tour.measuring {
        frames.push(real_time.delta_secs());
    }
    tour.wait -= time.delta_secs();
    while tour.wait <= 0.0 {
        if let Some((name, mut frames)) = tour.measuring.take()
            && let Some(stats) = FrameStats::new(&mut frames)
        {
            let FrameStats { frames, fps, p50, p99, max } = stats;
            info!("bench {name}: {fps:.0} fps over {frames} frames; frame ms p50 {p50:.3}, p99 {p99:.3}, max {max:.3}");
        }
        let Some(step) = tour.steps.next() else { return };
        match step {
            Step::Tab(page) => *stage.tab = page,
            Step::Drill(kind, secs) => stage.play(kind, secs),
            Step::Stage(screen) => stage.set(screen),
            Step::Press(key) => {
                keys.press(key);
                tour.held = Some(key);
                // One key per frame, so it is seen as a fresh press.
                return;
            },
            Step::Shoot => {
                stage.aim(AIM_OFFSETS[tour.shots % AIM_OFFSETS.len()]);
                mouse.press(MouseButton::Left);
                tour.shots += 1;
                tour.clicking = true;
                return;
            },
            Step::Shot(path) => {
                if let Some(dir) = path.parent()
                    && let Err(err) = std::fs::create_dir_all(dir)
                {
                    error!("tour: couldn't create {}: {err}", dir.display());
                }
                stage.commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
                // Nothing changes until the capture (read back a frame or two later) is saved.
                tour.wait = SHOT_SECS;
                return;
            },
            Step::Wait(secs) => tour.wait += secs,
            Step::Measure(name, secs) => {
                tour.measuring = Some((name, Vec::new()));
                tour.wait += secs;
            },
            Step::Exit => {
                exit.write(AppExit::Success);
                return;
            },
            Step::Fire(secs) => {
                stage.aim(AIM_OFFSETS[0]);
                mouse.press(MouseButton::Left);
                tour.firing = secs;
                tour.wait += secs;
            },
            Step::Hold(key) => {
                keys.press(key);
                // One key per frame, so it is seen as a fresh press.
                return;
            },
            Step::Release(key) => keys.release(key),
            Step::ShootAt(offset) => {
                stage.aim(offset);
                mouse.press(MouseButton::Left);
                tour.clicking = true;
                return;
            },
            Step::Aim => stage.aim(Vec2::ZERO),
            Step::Trigger(true) => mouse.press(MouseButton::Left),
            Step::Trigger(false) => mouse.release(MouseButton::Left),
            Step::Look(yaw, pitch) => stage.look.set(yaw.to_radians(), pitch.to_radians()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tour_dir_is_the_argument_after_the_flag() {
        assert_eq!(tour_dir(args(&["aim_trainer", "--tour", "shots"])), Some(PathBuf::from("shots")));
        assert_eq!(tour_dir(args(&["aim_trainer"])), None);
        assert_eq!(tour_dir(args(&["aim_trainer", "--tour"])), None);
    }

    #[test]
    fn window_size_is_width_x_height() {
        assert_eq!(window_size(args(&["aim_trainer", "--bench", "--size", "1920x1080"])), Some((1920, 1080)));
        assert_eq!(window_size(args(&["aim_trainer", "--size", "1920"])), None);
        assert_eq!(window_size(args(&["aim_trainer", "--size", "0x1080"])), None);
        assert_eq!(window_size(args(&["aim_trainer"])), None);
    }

    #[test]
    fn frame_stats_percentiles() {
        let mut secs: Vec<f32> = (1..=100).map(|ms| ms as f32 / 1000.0).collect();
        let stats = FrameStats::new(&mut secs).unwrap();
        assert_eq!(stats.frames, 100);
        assert!((stats.fps - 100.0 / 5.05).abs() < 1e-3);
        assert!((stats.p50 - 51.0).abs() < 1e-3 && (stats.p99 - 99.0).abs() < 1e-3 && (stats.max - 100.0).abs() < 1e-3);
        assert_eq!(FrameStats::new(&mut []), None);
    }

    fn args(list: &[&str]) -> std::vec::IntoIter<String> {
        list.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter()
    }
}
