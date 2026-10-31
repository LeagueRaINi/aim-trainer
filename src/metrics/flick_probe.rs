//! First-flick landing probe: how far the first ballistic submovement towards a new target
//! travels, as a fraction of the target's distance. Higher sensitivities overshoot (> 1), lower
//! ones undershoot (< 1); unlike scores this barely depends on luck (NVIDIA, CHI 2022).

use bevy::math::{Vec2, Vec3};

/// Targets closer than this (degrees) need no real flick, so they aren't measured.
pub const MIN_DISTANCE: f32 = 3.0;
/// Time constant of the angular speed low-pass (~7 Hz cutoff).
const SPEED_TAU: f32 = 0.023;
/// Filtered speed (deg/s) above which the first submovement starts.
pub const START_SPEED: f32 = 8.0;
/// Filtered speed (deg/s) below which it ends, once it has lasted `MIN_SUBMOVEMENT` seconds.
pub const END_SPEED: f32 = 4.0;
const MIN_SUBMOVEMENT: f32 = 0.08;
const MAX_RATIO: f32 = 3.0;
/// The first submovement must head within ~60 degrees of the target direction.
const MIN_HEADING_COS: f32 = 0.5;

/// Tracks the aim from the moment a flick target appears until the first submovement ends.
/// Angles are (yaw, pitch) in degrees.
#[derive(Clone, Debug)]
pub struct FlickProbe {
    start: Vec2,
    /// Target offset from `start`.
    offset: Vec2,
    last: Vec2,
    /// Low-passed angular speed, deg/s.
    speed: f32,
    /// Seconds since the first submovement started.
    moving: Option<f32>,
}

impl FlickProbe {
    /// Starts measuring a flick from `aim` to `target`. `None` if the target is too close.
    pub fn new(aim: Vec2, target: Vec2) -> Option<Self> {
        let offset = wrap(target - aim);
        (offset.length() >= MIN_DISTANCE).then_some(Self { start: aim, offset, last: aim, speed: 0.0, moving: None })
    }

    /// Feeds one frame. Returns the landing ratio once the first submovement has ended.
    pub fn update(&mut self, aim: Vec2, dt: f32) -> Option<f32> {
        if dt <= 0.0 {
            return None;
        }
        let delta = wrap(aim - self.last);
        let raw = delta.length() / dt;
        self.last = aim;
        self.speed += (raw - self.speed) * (1.0 - (-dt / SPEED_TAU).exp());
        match &mut self.moving {
            None => {
                // Leftover motion from the previous kill (clicking while still moving) isn't the
                // flick: only motion heading towards the target starts it.
                let towards = delta.normalize_or_zero().dot(self.offset.normalize()) > MIN_HEADING_COS;
                if self.speed > START_SPEED && towards {
                    self.moving = Some(0.0);
                }
                None
            },
            Some(time) => {
                *time += dt;
                (*time >= MIN_SUBMOVEMENT && self.speed < END_SPEED).then(|| self.landing(aim))
            },
        }
    }

    /// Aim travel along the target direction as a fraction of the distance, clamped to [0, 3].
    /// Also used when the target is killed before the submovement ends.
    pub fn landing(&self, aim: Vec2) -> f32 {
        (wrap(aim - self.start).dot(self.offset) / self.offset.length_squared()).clamp(0.0, MAX_RATIO)
    }

    /// Aim travel across the target direction (off-axis error) as a fraction of the distance;
    /// positive is counter-clockwise of the start-to-target line on screen.
    pub fn off_axis(&self, aim: Vec2) -> f32 {
        wrap(aim - self.start).perp_dot(self.offset) / self.offset.length_squared()
    }

    /// Low-passed angular speed of the aim, deg/s.
    pub fn speed(&self) -> f32 {
        self.speed
    }
}

/// (yaw, pitch) in degrees of a view direction, matching the camera's YXZ euler angles.
pub fn angles(dir: Vec3) -> Vec2 {
    Vec2::new((-dir.x).atan2(-dir.z).to_degrees(), dir.y.clamp(-1.0, 1.0).asin().to_degrees())
}

/// Angle differences wrapped into [-180, 180) degrees.
pub fn wrap(delta: Vec2) -> Vec2 {
    delta.map(|d| (d + 180.0).rem_euclid(360.0) - 180.0)
}

/// On-screen direction of a (yaw, pitch) offset in degrees counter-clockwise from right (90 =
/// up), in [0, 360). Yaw grows to the left, as in the camera.
pub fn screen_angle(offset: Vec2) -> f32 {
    offset.y.atan2(-offset.x).to_degrees().rem_euclid(360.0)
}

/// The (yaw, pitch) offset `distance` degrees towards the on-screen direction `angle`.
pub fn screen_offset(angle: f32, distance: f32) -> Vec2 {
    let (sin, cos) = angle.to_radians().sin_cos();
    Vec2::new(-cos, sin) * distance
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 240.0;

    /// Minimum-jerk position profile, 0 to 1 over `t` in [0, 1].
    fn min_jerk(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * t * (10.0 - 15.0 * t + 6.0 * t * t)
    }

    /// Aim path through `(fraction of target offset, seconds)` legs, each a minimum-jerk
    /// movement followed by a hold, sampled every `DT`.
    fn path(offset: Vec2, legs: &[(f32, f32, f32)]) -> Vec<Vec2> {
        let mut aims = Vec::new();
        let mut from = 0.0;
        for &(to, move_secs, hold_secs) in legs {
            let steps = ((move_secs + hold_secs) / DT) as usize;
            for i in 1..=steps {
                let t = i as f32 * DT / move_secs;
                aims.push(offset * (from + (to - from) * min_jerk(t)));
            }
            from = to;
        }
        aims
    }

    /// Runs the probe over a path and returns the first landing ratio it reports.
    fn measure(target: Vec2, aims: &[Vec2]) -> Option<f32> {
        let mut probe = FlickProbe::new(Vec2::ZERO, target).unwrap();
        aims.iter().find_map(|&aim| probe.update(aim, DT))
    }

    #[test]
    fn clean_flick_lands_on_target() {
        let target = Vec2::new(20.0, 5.0);
        let ratio = measure(target, &path(target, &[(0.0, 0.05, 0.0), (1.0, 0.2, 0.3)])).unwrap();
        assert!((ratio - 1.0).abs() < 0.02, "{ratio}");
    }

    #[test]
    fn overshoot_is_measured_before_the_correction() {
        let target = Vec2::new(-25.0, 0.0);
        let ratio = measure(target, &path(target, &[(1.2, 0.2, 0.2), (1.0, 0.15, 0.2)])).unwrap();
        assert!((ratio - 1.2).abs() < 0.02, "{ratio}");
    }

    #[test]
    fn undershoot_is_measured_before_the_correction() {
        let target = Vec2::new(10.0, -8.0);
        let ratio = measure(target, &path(target, &[(0.85, 0.18, 0.15), (1.0, 0.15, 0.2)])).unwrap();
        assert!((ratio - 0.85).abs() < 0.02, "{ratio}");
    }

    #[test]
    fn kill_mid_flick_uses_the_aim_at_kill_time() {
        let target = Vec2::new(30.0, 0.0);
        let aims = path(target, &[(1.0, 0.2, 0.0)]);
        let mut probe = FlickProbe::new(Vec2::ZERO, target).unwrap();
        let half = aims.len() / 2;
        assert!(aims[..half].iter().all(|&aim| probe.update(aim, DT).is_none()));
        assert!(probe.moving.is_some());
        let ratio = probe.landing(aims[half - 1]);
        assert!((ratio - 0.5).abs() < 0.03, "{ratio}");
    }

    #[test]
    fn tiny_flicks_are_skipped() {
        assert!(FlickProbe::new(Vec2::new(10.0, 0.0), Vec2::new(12.0, 1.0)).is_none());
        // Offsets wrap at +-180 degrees of yaw: 175 to -175 is 10 degrees, not 350.
        let probe = FlickProbe::new(Vec2::new(175.0, 0.0), Vec2::new(-175.0, 0.0)).unwrap();
        assert!((probe.landing(Vec2::new(-175.0, 0.0)) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn leftover_motion_away_from_the_target_is_not_the_flick() {
        // Still drifting down-left from the previous kill when the new target appears up-right:
        // the drift settles, then the real flick lands on target.
        let target = Vec2::new(20.0, 6.0);
        let drift = Vec2::new(-1.5, -0.5);
        let mut aims: Vec<Vec2> = (1..=24).map(|i| drift * (i as f32 / 24.0)).collect();
        aims.extend(path(target - drift, &[(0.0, 0.05, 0.0), (1.0, 0.2, 0.3)]).into_iter().map(|p| drift + p));
        let ratio = measure(target, &aims).unwrap();
        assert!((ratio - 1.0).abs() < 0.05, "{ratio}");
    }

    #[test]
    fn angles_match_the_camera_convention() {
        let dir = bevy::math::Quat::from_euler(bevy::math::EulerRot::YXZ, 0.3, -0.2, 0.0) * Vec3::NEG_Z;
        let got = angles(dir);
        assert!((got - Vec2::new(0.3f32.to_degrees(), -0.2f32.to_degrees())).length() < 1e-3);
    }

    #[test]
    fn off_axis_is_the_sideways_part_of_the_travel() {
        // A flick to the left (yaw grows to the left); drifting up is clockwise on screen.
        let probe = FlickProbe::new(Vec2::ZERO, Vec2::new(20.0, 0.0)).unwrap();
        assert!((probe.off_axis(Vec2::new(18.0, 2.0)) + 0.1).abs() < 1e-5);
        assert!((probe.off_axis(Vec2::new(18.0, -3.0)) - 0.15).abs() < 1e-5);
        assert!(probe.off_axis(Vec2::new(20.0, 0.0)).abs() < 1e-5);
    }

    #[test]
    fn screen_directions_follow_the_camera() {
        // Turning right lowers the yaw.
        assert!((screen_angle(Vec2::new(-10.0, 0.0)) - 0.0).abs() < 1e-4);
        assert!((screen_angle(Vec2::new(0.0, 10.0)) - 90.0).abs() < 1e-4);
        assert!((screen_angle(Vec2::new(10.0, -10.0)) - 225.0).abs() < 1e-4);
        for angle in [0.0, 45.0, 135.0, 300.0] {
            assert!((screen_angle(screen_offset(angle, 7.0)) - angle).abs() < 1e-3);
        }
    }
}
