// Release builds are a windowed game: no console window next to it. Debug builds keep the
// console for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adaptive_log;
mod analysis;
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
mod replay;
mod routine;
mod settings;
mod sfx;
mod signing;
#[cfg(test)] mod test_util;
mod ui;
mod update;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::feathers::FeathersPlugins;
use bevy::log::{BoxedLayer, LogPlugin, tracing_subscriber};
use bevy::prelude::*;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;

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

fn main() {
    let updater = update::Updater::default();
    // An update staged by an earlier run goes in before anything else; the new exe takes over.
    if updater.install_staged_update() {
        return;
    }
    let (settings, settings_load) = settings::Settings::load();
    let mut app = App::new();
    app.add_plugins((
        // Pipelined rendering overlaps a frame's render with the next simulation step,
        // adding a frame of input latency; an aim trainer wants the opposite trade.
        DefaultPlugins
            .set(WindowPlugin { primary_window: Some(display::primary_window(&settings.video)), ..default() })
            .set(LogPlugin { custom_layer: log_file, ..default() })
            .disable::<PipelinedRenderingPlugin>(),
        FeathersPlugins,
        FrameTimeDiagnosticsPlugin::default(),
    ))
    .insert_resource(ui::theme::ui_theme())
    .init_state::<AppState>()
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
    .add_plugins((adaptive_log::AdaptiveLogPlugin, update::UpdatePlugin(updater.clone())));
    app.run();
    // Everything is saved by now, so a restarted exe reads the latest settings.
    updater.restart_if_requested();
}

/// Also writes the log to `LOG_PATH`, if it can be created.
fn log_file(_: &mut App) -> Option<BoxedLayer> {
    let file = std::fs::File::create(LOG_PATH).ok()?;
    Some(Box::new(tracing_subscriber::fmt::Layer::default().with_writer(file).with_ansi(false)))
}
