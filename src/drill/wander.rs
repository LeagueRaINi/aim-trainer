//! Smooth random wandering inside a box of (yaw, pitch) degrees, shared by the drills whose dots
//! drift around (Moving gridshot, Switch tracking).

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::DrillConfig;
use super::geometry::{TARGET_DISTANCE, random_range};

/// How strongly a new heading leans back towards the middle of the box, against a random one.
const HOMING: f32 = 1.5;
/// The least share of its speed a dot that hit an edge gets away from it: it bounces off rather
/// than sliding along (which looks like sticking to the floor).
const BOUNCE: f32 = 0.6;

/// A dot's wandering: it eases its velocity towards a heading that changes now and then
/// (`config.turn_min..turn_max` seconds apart, at `config.strafe_min..strafe_max` m/s), so it
/// curves instead of snapping round.
pub(crate) struct Wander {
    /// Degrees per second.
    velocity: Vec2,
    /// The velocity it is easing towards.
    target: Vec2,
    /// Seconds until the next heading.
    turn_in: f32,
    /// Time constant of the easing: half the time until the next heading, so each turn is
    /// mostly done before the next starts.
    ease: f32,
}

impl Wander {
    pub(crate) fn new(config: &DrillConfig) -> Self {
        let (heading, turn_in) = (heading(config, Vec2::ZERO), random_range(config.turn_min, config.turn_max));
        Self { velocity: heading, target: heading, turn_in, ease: turn_in * 0.5 }
    }

    /// Moves `at` by `dt` seconds of wandering, kept inside `low..=high`. Hitting an edge
    /// reflects the velocity and starts a new heading away from it.
    pub(crate) fn step(&mut self, at: &mut Vec2, low: Vec2, high: Vec2, dt: f32, config: &DrillConfig) {
        self.turn_in -= dt;
        if self.turn_in <= 0.0 {
            self.turn(*at, low, high, config);
        }
        let follow = 1.0 - (-dt / self.ease.max(1e-3)).exp();
        self.velocity += (self.target - self.velocity) * follow;
        let mut next = *at + self.velocity * dt;
        // Per axis: +1 when it hit the low edge, -1 the high one.
        let mut away = Vec2::ZERO;
        for axis in 0..2 {
            if next[axis] < low[axis] {
                away[axis] = 1.0;
            } else if next[axis] > high[axis] {
                away[axis] = -1.0;
            }
        }
        if away != Vec2::ZERO {
            next = next.clamp(low, high);
            self.turn(next, low, high, config);
            let least = self.target.length() * BOUNCE;
            for axis in (0..2).filter(|&axis| away[axis] != 0.0) {
                self.velocity[axis] = away[axis] * self.velocity[axis].abs().max(least * 0.5);
                self.target[axis] = away[axis] * self.target[axis].abs().max(least);
            }
        }
        *at = next;
    }

    /// A new heading leaning back towards the middle of the box, held for a new while.
    fn turn(&mut self, at: Vec2, low: Vec2, high: Vec2, config: &DrillConfig) {
        let half = ((high - low) * 0.5).max(Vec2::splat(1e-3));
        self.target = heading(config, ((low + high) * 0.5 - at) / half);
        self.turn_in = random_range(config.turn_min, config.turn_max);
        self.ease = self.turn_in * 0.5;
    }
}

/// A random direction at a random strafe speed (m/s at the target distance) in degrees/s,
/// leaning towards `home` (from -1 to 1 per axis: the way back to the middle, in half-boxes).
fn heading(config: &DrillConfig, home: Vec2) -> Vec2 {
    let speed = (random_range(config.strafe_min, config.strafe_max) / TARGET_DISTANCE).atan().to_degrees();
    let wander = Vec2::from_angle(fastrand::f32() * TAU);
    (wander + home * HOMING).normalize_or(wander) * speed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::DrillKind;

    /// A dot gliding along the floor edge (heading mostly sideways, slightly down) used to slide
    /// along it until its next turn; it now bounces clear within a fraction of a second.
    #[test]
    fn dots_bounce_off_the_floor() {
        let config = DrillConfig::defaults(DrillKind::SwitchTracking);
        let (low, high) = (Vec2::new(-30.0, -12.0), Vec2::new(30.0, 12.0));
        for seed in 0..50 {
            fastrand::seed(seed);
            let grazing = Vec2::new(10.0, -0.5);
            let mut wander = Wander { velocity: grazing, target: grazing, turn_in: 99.0, ease: 0.3 };
            let mut at = Vec2::new(0.0, low.y + 0.01);
            for _ in 0..30 {
                wander.step(&mut at, low, high, 1.0 / 60.0, &config);
            }
            assert!(at.y > low.y + 1.0, "seed {seed}: still at {} after 0.5 s", at.y);
        }
    }
}
