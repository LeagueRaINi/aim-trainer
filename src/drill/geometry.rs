//! Shared target geometry: where targets live, view directions, random picks and ray tests.

use bevy::prelude::*;

use crate::camera::ViewCone;
use crate::metrics::flick_probe::{angles, wrap};

/// Distance from the camera to the plane/sphere targets live on.
pub(crate) const TARGET_DISTANCE: f32 = 10.0;
/// Lowest pitch (degrees) a target may sit at: lower ones (at [`TARGET_DISTANCE`]) would sink
/// into the floor 2.5 m below the eye.
pub(crate) const FLOOR_PITCH: f32 = -13.0;
/// Highest pitch (degrees) a target may sit at: higher ones would reach the ceiling 7.5 m above
/// the eye (at 48.6°), with room for large targets.
pub(crate) const CEILING_PITCH: f32 = 45.0;

/// The view direction for `yaw`/`pitch` in radians.
pub(crate) fn direction(yaw: f32, pitch: f32) -> Vec3 {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0) * Vec3::NEG_Z
}

pub(crate) fn random_range(min: f32, max: f32) -> f32 {
    min + fastrand::f32() * (max - min)
}

pub(crate) fn random_sign() -> f32 {
    if fastrand::bool() { 1.0 } else { -1.0 }
}

/// A random pitch in radians within `+-range` degrees, between floor and ceiling.
pub(crate) fn random_pitch(range: f32) -> f32 {
    random_range((-range).max(FLOOR_PITCH), range.min(CEILING_PITCH)).to_radians()
}

/// The largest `t` in `0..=1` that `fits`, for a `fits` that holds from 0 up to some point and
/// not beyond (bisection). 0, unchecked, when nothing larger fits.
pub(crate) fn bisect(fits: impl Fn(f32) -> bool) -> f32 {
    if fits(1.0) {
        return 1.0;
    }
    let (mut inside, mut outside) = (0.0, 1.0);
    for _ in 0..16 {
        let t = (inside + outside) * 0.5;
        if fits(t) {
            inside = t;
        } else {
            outside = t;
        }
    }
    inside
}

/// Extra margin (radians, 0.006°) for on-screen checks inside a [`bisect`]: its result sits right
/// at the edge, where rounding (scaling to [`TARGET_DISTANCE`] and back) could tip it outside.
pub(crate) const EDGE_SLACK: f32 = 1e-4;

/// Distance along the ray to the sphere, if the ray hits it.
pub(crate) fn ray_hit(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let to_center = center - origin;
    let t = to_center.dot(dir);
    let miss_sq = to_center.length_squared() - t * t;
    (t > 0.0 && miss_sq <= radius * radius).then_some(t)
}

/// Distance along the ray to an axis-aligned box, if the ray hits it (slab method).
pub(crate) fn ray_box(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = dir.recip();
    let (t1, t2) = ((min - origin) * inv, (max - origin) * inv);
    let near = t1.min(t2).max_element();
    let far = t1.max(t2).min_element();
    (far >= near.max(0.0)).then_some(near.max(0.0))
}

/// Draws random directions until one is at least `min_deg` away from `aim` and on screen (with
/// `margin` radians to the edges); returns a target position at [`TARGET_DISTANCE`]. If no pick
/// qualifies, the last one is [pulled in](pull_in).
pub(crate) fn away_from(aim: Vec3, min_deg: f32, view: &ViewCone, margin: f32, mut pick: impl FnMut() -> Vec3) -> Vec3 {
    let max_cos = min_deg.to_radians().cos();
    let mut dir = pick();
    for _ in 0..32 {
        if dir.dot(aim) < max_cos && view.contains(aim, dir, margin) {
            return dir * TARGET_DISTANCE;
        }
        dir = pick();
    }
    pull_in(aim, dir, view, margin) * TARGET_DISTANCE
}

/// `dir` moved towards `aim` until it is on screen with `margin` to spare. Works in yaw and pitch
/// so the target keeps its pitch where the view allows (Head level stays at head height): the
/// pitch, kept between floor and ceiling, only moves towards the aim's as far as needed to be
/// visible at the aim's yaw; then the yaw moves towards the aim's. When the aim is so steep that
/// no pitch in the room is visible, the target stays at the nearest one.
pub(crate) fn pull_in(aim: Vec3, dir: Vec3, view: &ViewCone, margin: f32) -> Vec3 {
    let room = |pitch: f32| pitch.clamp(FLOOR_PITCH, CEILING_PITCH);
    let (from, to) = (angles(aim), angles(dir));
    let yaw_offset = wrap(to - from).x;
    let at = |yaw_part: f32, pitch: f32| direction((from.x + yaw_part * yaw_offset).to_radians(), pitch.to_radians());
    let (start, end) = (room(from.y), room(to.y));
    // From `end` back towards `start`, so a pitch that fits is kept exactly.
    let pitch_at = |t: f32| end + (start - end) * (1.0 - t);
    let margin = margin + EDGE_SLACK;
    let pitch = pitch_at(bisect(|t| view.contains(aim, at(0.0, pitch_at(t)), margin)));
    at(bisect(|t| view.contains(aim, at(t, pitch), margin)), pitch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulled_in_targets_keep_their_pitch_and_stay_above_the_floor() {
        let (view, margin) = (ViewCone::default(), 0.04);
        // At head height, far to the side of an aim 17° up: only the yaw moves.
        let aim = direction(1.3, 0.3);
        let pulled = pull_in(aim, direction(-0.5, 0.0), &view, margin);
        assert!(view.contains(aim, pulled, margin), "{pulled} off screen");
        assert!(pulled.y.abs() < 1e-5, "{pulled} left head height");
        // Below the floor, looking 40° down: up to the floor, still in view.
        let aim = direction(0.2, -0.7);
        let pulled = pull_in(aim, direction(-0.9, -0.6), &view, margin);
        assert!(view.contains(aim, pulled, margin), "{pulled} off screen");
        assert!((angles(pulled).y - FLOOR_PITCH).abs() < 1e-3, "{pulled} not at the floor");
    }
}
