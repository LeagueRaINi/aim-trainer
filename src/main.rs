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
#[cfg(test)] mod test_util;
mod ui;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::feathers::FeathersPlugins;
use bevy::prelude::*;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;

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
    let (settings, settings_load) = settings::Settings::load();
    let mut app = App::new();
    app.add_plugins((
        // Pipelined rendering overlaps a frame's render with the next simulation step,
        // adding a frame of input latency; an aim trainer wants the opposite trade.
        DefaultPlugins
            .set(WindowPlugin { primary_window: Some(display::primary_window(&settings.video)), ..default() })
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
    .add_plugins(adaptive_log::AdaptiveLogPlugin);
    app.run();
}
