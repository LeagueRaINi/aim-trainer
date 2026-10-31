//! Chart primitives: plain UI entities spawned by systems inside containers they rebuild.

use bevy::feathers::constants::fonts;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemeToken};
use bevy::prelude::*;

use super::widgets::styled;

/// Chart labels: small text in the regular Feathers font.
pub fn text(commands: &mut Commands, parent: Entity, value: String, size: f32, color: ThemeToken, node: Node) {
    commands.spawn_scene(bsn! { @styled(value, fonts::REGULAR, size, color) node }).insert(ChildOf(parent));
}

pub fn absolute(left: f32, top: f32, width: f32, height: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(left),
        top: px(top),
        width: px(width),
        height: px(height),
        ..default()
    }
}

/// Connected 2 px line through `points` (in the parent's pixel space), as rotated thin nodes.
pub fn polyline(commands: &mut Commands, parent: Entity, points: &[Vec2], color: ThemeToken) {
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let delta = b - a;
        let length = delta.length();
        let center = (a + b) * 0.5;
        commands.spawn((
            ChildOf(parent),
            absolute(center.x - length * 0.5, center.y - 1.0, length, 2.0),
            UiTransform { rotation: Rot2::radians(delta.y.atan2(delta.x)), ..default() },
            ThemeBackgroundColor(color.clone()),
        ));
    }
}
