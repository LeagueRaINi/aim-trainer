//! Built-in screenshots and benchmark. F12 saves the window to `screenshots/<unix ms>.png` at
//! any time.
//!
//! `aim_trainer --tour <dir>` walks through every menu page and a few drills (their ready card,
//! a moment of play, and a results screen), saves a PNG of each to `<dir>`, then quits: a
//! repeatable way to look at the whole UI, e.g. after a layout change.
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

use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::winit::WinitSettings;

use super::menu::MenuTab;
use crate::AppState;
use crate::drill::{DrillKind, DrillSpec, KeepRunsOnFocusLoss, Run, RunPurpose, begin_run};
use crate::settings::Settings;

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
    Exit,
}

#[derive(Resource)]
struct Tour {
    steps: std::vec::IntoIter<Step>,
    /// Seconds until the next step.
    wait: f32,
    /// A key pressed by the last step, released before the next.
    held: Option<KeyCode>,
    /// The scenario being measured and its frame times so far, seconds.
    measuring: Option<(String, Vec<f32>)>,
}

/// Long enough for a page (and the preview camera) to settle.
const SETTLE: f32 = 0.8;

impl Tour {
    fn new(dir: PathBuf) -> Self {
        let shot = |name: String| Step::Shot(dir.join(format!("{name}.png")));
        let mut steps = vec![Step::Wait(2.0)];
        for tab in MenuTab::ALL {
            steps.extend([Step::Tab(tab), Step::Wait(SETTLE), shot(format!("menu_{}", tab.name().to_lowercase()))]);
        }
        steps.push(Step::Tab(MenuTab::Play));
        let drills = [
            DrillKind::Gridshot,
            DrillKind::MovingGridshot,
            DrillKind::Survival,
            DrillKind::SwitchTracking,
            DrillKind::Peek,
            DrillKind::Tracking,
        ];
        for kind in drills {
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
        }
        // A short run played to the end, for the results screen.
        steps.extend([
            Step::Drill(DrillKind::Gridshot, 5.0),
            Step::Wait(SETTLE),
            Step::Press(KeyCode::Space),
            Step::Wait(8.0),
            shot("results".into()),
            Step::Exit,
        ]);
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
        }
        steps.push(Step::Exit);
        Self::from_steps(steps)
    }

    fn from_steps(steps: Vec<Step>) -> Self {
        Self { steps: steps.into_iter(), wait: 0.0, held: None, measuring: None }
    }
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

#[allow(clippy::too_many_arguments)]
fn run_tour(
    mut commands: Commands,
    time: Res<Time>,
    real_time: Res<Time<Real>>,
    mut tour: ResMut<Tour>,
    mut tab: ResMut<MenuTab>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    settings: Res<Settings>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
) {
    if let Some(key) = tour.held.take() {
        keys.release(key);
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
            Step::Tab(page) => *tab = page,
            Step::Drill(kind, secs) => {
                let drill = DrillSpec::builtin(kind, &settings);
                let run = Run::new(drill, settings.sens, secs, RunPurpose::Practice, kind.name().to_string());
                begin_run(&mut commands, &mut next_state, run);
            },
            Step::Press(key) => {
                keys.press(key);
                tour.held = Some(key);
                // One key per frame, so it is seen as a fresh press.
                return;
            },
            Step::Shot(path) => {
                if let Some(dir) = path.parent()
                    && let Err(err) = std::fs::create_dir_all(dir)
                {
                    error!("tour: couldn't create {}: {err}", dir.display());
                }
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
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
