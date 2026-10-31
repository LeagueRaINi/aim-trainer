//! Aim tracer for replays: the crosshair's recent path, colored by how straight each movement
//! was. A clean flick travels in a straight line from where you were aiming to the target; a
//! curved path means the arm and wrist (or the horizontal and vertical parts of the motion)
//! weren't coordinated.
//!
//! The path is split into movements (runs of samples moving faster than `MOVE_SPEED`). Each
//! movement is judged by its bow: the largest distance of its path from the straight line between
//! its start and end, relative to that line's length.

use bevy::prelude::*;

/// Angular speed (deg/s) above which the aim counts as moving.
const MOVE_SPEED: f32 = 20.0;
/// Movements shorter than this (degrees) are micro-adjustments and aren't judged.
const MIN_MOVE: f32 = 1.0;
/// Bow at or below which a movement counts as straight (green).
const STRAIGHT: f32 = 0.05;
/// Bow at or above which a movement counts as clearly curved (red).
const CURVED: f32 = 0.25;

/// For each segment between consecutive aim samples `(seconds, (yaw, pitch) in degrees)`, the
/// bow of the movement it belongs to, or `None` while resting or micro-adjusting.
pub fn segment_bows(points: &[(f32, Vec2)]) -> Vec<Option<f32>> {
    let segments = points.len().saturating_sub(1);
    let moving: Vec<bool> = points
        .windows(2)
        .map(|w| (w[1].1 - w[0].1).length() / (w[1].0 - w[0].0).max(1e-6) > MOVE_SPEED)
        .collect();
    let mut bows = vec![None; segments];
    let mut i = 0;
    while i < segments {
        if !moving[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < segments && moving[i] {
            i += 1;
        }
        // Segments start..i span points start..=i.
        let (from, to) = (points[start].1, points[i].1);
        let length = from.distance(to);
        if length < MIN_MOVE {
            continue;
        }
        let direction = (to - from) / length;
        let bow = points[start..=i].iter().map(|p| (p.1 - from).perp_dot(direction).abs()).fold(0.0, f32::max) / length;
        bows[start..i].fill(Some(bow));
    }
    bows
}

/// Green for straight, through yellow, to red for curved; faint white while resting.
pub fn bow_color(bow: Option<f32>) -> Color {
    match bow {
        None => Color::srgba(1.0, 1.0, 1.0, 0.35),
        Some(bow) => {
            let curved = ((bow - STRAIGHT) / (CURVED - STRAIGHT)).clamp(0.0, 1.0);
            Color::hsl(130.0 * (1.0 - curved), 0.9, 0.55)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 240.0;

    /// A 0.2 s movement from the origin to `to`, bulging sideways by `bulge` (degrees) halfway,
    /// with rests before and after.
    fn flick(to: Vec2, bulge: f32) -> Vec<(f32, Vec2)> {
        let side = to.perp().normalize();
        let mut points: Vec<(f32, Vec2)> = (0..24).map(|i| (i as f32 * DT, Vec2::ZERO)).collect();
        let t0 = points.len() as f32 * DT;
        for i in 1..=48 {
            let s = i as f32 / 48.0;
            let eased = s * s * (3.0 - 2.0 * s);
            points.push((t0 + i as f32 * DT, to * eased + side * bulge * (std::f32::consts::PI * eased).sin()));
        }
        let t1 = points.last().unwrap().0;
        points.extend((1..24).map(|i| (t1 + i as f32 * DT, to)));
        points
    }

    fn judged(bows: &[Option<f32>]) -> Vec<f32> {
        let mut values: Vec<f32> = bows.iter().flatten().copied().collect();
        values.dedup();
        values
    }

    #[test]
    fn a_straight_flick_is_straight_and_rests_are_unjudged() {
        let bows = segment_bows(&flick(Vec2::new(20.0, 6.0), 0.0));
        assert!(bows.first().unwrap().is_none() && bows.last().unwrap().is_none());
        let judged = judged(&bows);
        assert_eq!(judged.len(), 1);
        assert!(judged[0] < 0.01, "{judged:?}");
        assert_eq!(bow_color(Some(judged[0])), bow_color(Some(0.0)));
    }

    #[test]
    fn a_curved_flick_measures_its_bow() {
        // 3 degrees of bulge on a 20 degree flick: 15% off straight.
        let judged = judged(&segment_bows(&flick(Vec2::new(20.0, 0.0), 3.0)));
        assert_eq!(judged.len(), 1);
        assert!((judged[0] - 0.15).abs() < 0.01, "{judged:?}");
    }

    #[test]
    fn micro_adjustments_are_not_judged() {
        let bows = segment_bows(&flick(Vec2::new(0.6, 0.2), 0.3));
        assert!(bows.iter().all(Option::is_none));
    }
}
