mod analysis;
mod arena;
mod crosshair;
mod custom;
mod display;
mod finder;
mod history;
mod icon;
mod level;
mod presence;
mod replay;
mod routine;
mod scenario;
mod settings;
mod sfx;
mod smart_finder;
mod tracer;
mod ui;

use bevy::{
    diagnostic::FrameTimeDiagnosticsPlugin, feathers::FeathersPlugins, prelude::*,
    render::pipelined_rendering::PipelinedRenderingPlugin,
};

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
    let settings = settings::Settings::load();
    let mut app = App::new();
    app.add_plugins((
            // Pipelined rendering overlaps a frame's render with the next simulation step,
            // adding a frame of input latency; an aim trainer wants the opposite trade.
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(display::primary_window(&settings.video)),
                    ..default()
                })
                .disable::<PipelinedRenderingPlugin>(),
            FeathersPlugins,
            FrameTimeDiagnosticsPlugin::default(),
        ))
        .insert_resource(ui::theme::ui_theme())
        .init_state::<AppState>()
        .add_plugins((
            settings::SettingsPlugin(settings),
            history::HistoryPlugin,
            custom::CustomPlugin,
            level::LevelPlugin,
            arena::ArenaPlugin,
            display::DisplayPlugin,
            icon::IconPlugin,
            crosshair::CrosshairPlugin,
            sfx::SfxPlugin,
            scenario::ScenarioPlugin,
            replay::ReplayPlugin,
            presence::PresencePlugin,
            ui::UiPlugin,
        ));
    app.run();
}
