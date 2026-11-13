//! Chart primitives: plain UI entities spawned by systems inside containers they rebuild.

use bevy::feathers::constants::fonts;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemeToken};
use bevy::prelude::*;

use super::theme::*;
use super::widgets::styled;

/// Spawns `scene` as a child of `parent`: how systems fill the containers they rebuild.
pub fn spawn_in(commands: &mut Commands, parent: Entity, scene: impl Scene) -> Entity {
    commands.spawn_scene(scene).insert(ChildOf(parent)).id()
}

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

/// The plot area of a chart: a `width` x `height` box its marks are positioned in and clipped to.
pub fn plot(commands: &mut Commands, parent: Entity, width: f32, height: f32) -> Entity {
    commands
        .spawn((
            ChildOf(parent),
            Node {
                width: px(width),
                height: px(height),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                border_radius: BorderRadius::all(px(RADIUS_S)),
                ..default()
            },
            ThemeBackgroundColor(CHART_BG),
        ))
        .id()
}

/// The value range a score chart shows: the scores with some room above and below.
pub fn score_range(scores: &[f32]) -> (f32, f32) {
    let (min, max) = scores.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)));
    let pad = ((max - min) * 0.15).max(1.0);
    ((min - pad).max(0.0), max + pad)
}

/// One bar per score, oldest left, filling a [`plot`] of this size; the best score is gold.
pub fn score_bars(commands: &mut Commands, plot: Entity, scores: &[f32], width: f32, height: f32) {
    let (lo, hi) = score_range(scores);
    let slot = width / scores.len() as f32;
    let bar_width = (slot * 0.6).clamp(2.0, 28.0);
    let best = scores.iter().copied().fold(f32::MIN, f32::max);
    for (i, &score) in scores.iter().enumerate() {
        let top = height - (score - lo) / (hi - lo) * height;
        let color = if score >= best { GOLD } else { CHART_BAR };
        commands.spawn((
            ChildOf(plot),
            Node {
                border_radius: BorderRadius::top(px(2)),
                ..absolute(i as f32 * slot + (slot - bar_width) * 0.5, top, bar_width, height - top)
            },
            ThemeBackgroundColor(color),
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_range_pads_and_stays_positive() {
        let (lo, hi) = score_range(&[100.0, 200.0]);
        assert!((lo - 85.0).abs() < 1e-3 && (hi - 215.0).abs() < 1e-3);
        // A single score (or equal ones) still spans a range, and never goes below zero.
        assert_eq!(score_range(&[0.5]), (0.0, 1.5));
    }
}
