// Release builds are a windowed game: no console window next to it. Debug builds keep the
// console for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adaptive_log;
mod analysis;
mod calm_log;
mod camera;
mod crosshair;
mod custom;
mod display;
mod drill;
mod finder;
mod history;
mod icon;
mod level;
mod metrics;
mod persist;
mod presence;
mod reaction_log;
mod replay;
mod routine;
mod settings;
mod sfx;
mod signing;
#[cfg(test)] mod test_util;
mod training;
mod ui;
mod update;
mod viewmodel;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::ecs::schedule::SingleThreadedExecutor;
use bevy::feathers::FeathersPlugins;
use bevy::log::{BoxedLayer, LogPlugin, tracing_subscriber};
use bevy::prelude::*;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::{ExtractSchedule, RenderApp};

/// The log, next to the data files and replaced at every start: release builds have no console.
pub const LOG_PATH: &str = "aim_trainer.log";

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Menu,
    Playing,
    Results,
    Finder,
    /// Watching a recorded run.
    Replay,
    /// Break screen between warmup routine steps.
    Routine,
}

/// Whether the drill being played is running. Exists only in [`AppState::Playing`]; re-entering
/// `Playing` (a restart, a finder trial's next part) keeps its value.
#[derive(SubStates, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[source(AppState = AppState::Playing)]
pub enum PlayState {
    #[default]
    Running,
    /// The pause menu is up (Escape, or the window lost focus): the drill stands still.
    Paused,
    /// Counting down back into a live run; the drill still stands still.
    Resuming,
}

fn main() {
    let updater = update::Updater::default();
    // An update staged by an earlier run goes in before anything else; the new exe takes over.
    if updater.install_staged_update() {
        return;
    }
    let (settings, settings_load) = settings::Settings::load();
    let mut window = display::primary_window(&settings.video);
    ui::capture::script_window(&mut window);
    let mut app = App::new();
    app.add_plugins((
        // Pipelined rendering overlaps a frame's render with the next simulation step,
        // adding a frame of input latency; an aim trainer wants the opposite trade.
        DefaultPlugins
            .set(WindowPlugin { primary_window: Some(window), ..default() })
            .set(LogPlugin { custom_layer: log_file, ..default() })
            .disable::<PipelinedRenderingPlugin>(),
        FeathersPlugins,
        FrameTimeDiagnosticsPlugin::default(),
    ))
    .insert_resource(ui::theme::ui_theme())
    .init_state::<AppState>()
    .add_sub_state::<PlayState>()
    .add_plugins((
        persist::PersistPlugin { value: settings, report: settings_load },
        history::HistoryPlugin,
        custom::CustomPlugin,
        level::LevelPlugin,
        camera::CameraPlugin,
        display::DisplayPlugin,
        icon::IconPlugin,
        crosshair::CrosshairPlugin,
        sfx::SfxPlugin,
        drill::DrillPlugin,
        replay::ReplayPlugin,
        finder::FinderPlugin,
        routine::RoutinePlugin,
        presence::PresencePlugin,
        ui::UiPlugin,
    ))
    // Outside the tuple above, which is at Bevy's 15-plugin limit.
    .add_plugins((
        adaptive_log::AdaptiveLogPlugin,
        reaction_log::ReactionLogPlugin,
        update::UpdatePlugin(updater.clone()),
        viewmodel::ViewmodelPlugin,
        SingleThreadedSchedules,
        calm_log::CalmLogPlugin,
        training::TrainingPlugin,
    ));
    app.run();
    // Everything is saved by now, so a restarted exe reads the latest settings.
    updater.restart_if_requested();
}

/// Also writes the log to `LOG_PATH`, if it can be created.
fn log_file(_: &mut App) -> Option<BoxedLayer> {
    let file = std::fs::File::create(LOG_PATH).ok()?;
    Some(Box::new(tracing_subscriber::fmt::Layer::default().with_writer(file).with_ansi(false)))
}

/// Runs every schedule's systems one after another on the main thread. The game's frame is
/// close to 900 tiny systems, which the multi-threaded executor spends more time handing out to
/// worker threads than running: single-threaded takes about 40% off the frame time (measured
/// with `--bench`, see `ui/capture.rs`). Systems that split their own work (`par_iter`) still
/// use the task pools.
struct SingleThreadedSchedules;

impl Plugin for SingleThreadedSchedules {
    fn build(&self, _: &mut App) {}

    /// After every plugin's `finish`, when all schedules (including the render app's) exist.
    fn cleanup(&self, app: &mut App) {
        fn single_threaded(world: &mut World) {
            for (_, schedule) in world.resource_mut::<Schedules>().iter_mut() {
                schedule.set_executor(SingleThreadedExecutor::new());
            }
        }
        single_threaded(app.world_mut());
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            single_threaded(render_app.world_mut());
            // A new executor applies deferred commands at the end of a schedule; Bevy turns that
            // off for extraction (they're applied in `Render` instead), so keep it off.
            if let Some(extract) = render_app.world_mut().resource_mut::<Schedules>().get_mut(ExtractSchedule) {
                extract.set_apply_final_deferred(false);
            }
        }
    }
}
