//! Valorant-style crosshair: center dot, four inner lines and outlines, sized in physical pixels.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::{
    AppState,
    settings::{CrosshairSettings, Settings},
};

pub struct CrosshairPlugin;

impl Plugin for CrosshairPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Playing), crosshair_overlay(AppState::Playing))
            .add_systems(OnEnter(AppState::Replay), crosshair_overlay(AppState::Replay))
            .add_systems(Update, rebuild_previews);
    }
}

/// Put this on a UI node to show a live preview of the crosshair centered inside it.
#[derive(Component, Clone, Copy, Default)]
pub struct CrosshairPreview;

/// Logical-to-physical pixel factor for UI nodes: window DPI scale times the UI scale.
fn pixel_scale(window: &Window, ui_scale: &UiScale) -> f32 {
    window.scale_factor() * ui_scale.0
}

#[allow(clippy::type_complexity)]
/// A system that shows the in-game crosshair for as long as `state` is active.
fn crosshair_overlay(
    state: AppState,
) -> impl FnMut(Commands, Res<Settings>, Single<&Window, With<PrimaryWindow>>, Res<UiScale>) {
    move |mut commands, settings, window, ui_scale| {
        let root = commands
            .spawn((
                DespawnOnExit(state),
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
            ))
            .id();
        spawn_crosshair(&mut commands, root, &settings.crosshair, pixel_scale(&window, &ui_scale));
    }
}

fn rebuild_previews(
    mut commands: Commands,
    settings: Res<Settings>,
    ui_scale: Res<UiScale>,
    window: Single<&Window, With<PrimaryWindow>>,
    previews: Query<(Entity, Ref<CrosshairPreview>)>,
) {
    for (entity, preview) in &previews {
        if settings.is_changed() || ui_scale.is_changed() || preview.is_added() {
            commands.entity(entity).despawn_related::<Children>();
            let scale = pixel_scale(&window, &ui_scale);
            spawn_crosshair(&mut commands, entity, &settings.crosshair, scale);
        }
    }
}

/// Spawns the crosshair centered in `parent`. Sizes are converted from physical to logical pixels.
fn spawn_crosshair(commands: &mut Commands, parent: Entity, cfg: &CrosshairSettings, scale: f32) {
    let [r, g, b] = cfg.color;
    let color = Color::srgb_u8(r, g, b);
    let outline = cfg.outlines.then(|| Outline {
        width: px(cfg.outline_thickness / scale),
        offset: px(0),
        color: Color::BLACK.with_alpha(cfg.outline_opacity),
    });

    // (center offset from crosshair center, size), in physical pixels.
    let mut parts: Vec<(Vec2, Vec2)> = Vec::with_capacity(5);
    if cfg.center_dot {
        parts.push((Vec2::ZERO, Vec2::splat(cfg.dot_thickness)));
    }
    if cfg.inner_lines {
        let reach = cfg.line_offset + cfg.line_length * 0.5;
        let vertical = Vec2::new(cfg.line_thickness, cfg.line_length);
        let horizontal = Vec2::new(cfg.line_length, cfg.line_thickness);
        parts.extend([
            (Vec2::new(0.0, -reach), vertical),
            (Vec2::new(0.0, reach), vertical),
            (Vec2::new(-reach, 0.0), horizontal),
            (Vec2::new(reach, 0.0), horizontal),
        ]);
    }

    // A zero-sized anchor at the parent's center; parts are positioned around it.
    let anchor = commands
        .spawn((
            ChildOf(parent),
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                ..default()
            },
        ))
        .id();
    for (center, size) in parts {
        let top_left = (center - size * 0.5) / scale;
        let mut part = commands.spawn((
            ChildOf(anchor),
            Node {
                position_type: PositionType::Absolute,
                left: px(top_left.x),
                top: px(top_left.y),
                width: px(size.x / scale),
                height: px(size.y / scale),
                ..default()
            },
            BackgroundColor(color),
        ));
        if let Some(outline) = outline {
            part.insert(outline);
        }
    }
}
