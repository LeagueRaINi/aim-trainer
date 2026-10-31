//! Shared target geometry: where targets live, view directions, random picks and ray tests.

use bevy::prelude::*;

use crate::camera::ViewCone;

/// Distance from the camera to the plane/sphere targets live on.
pub(crate) const TARGET_DISTANCE: f32 = 10.0;

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
/// qualifies, the last one is pulled towards the aim until it is on screen.
pub(crate) fn away_from(aim: Vec3, min_deg: f32, view: &ViewCone, margin: f32, mut pick: impl FnMut() -> Vec3) -> Vec3 {
    let max_cos = min_deg.to_radians().cos();
    let mut dir = pick();
    for _ in 0..32 {
        if dir.dot(aim) < max_cos && view.contains(aim, dir, margin) {
            return dir * TARGET_DISTANCE;
        }
        dir = pick();
    }
    view.pull_in(aim, dir, margin) * TARGET_DISTANCE
}
