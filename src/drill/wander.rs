//! Smooth random wandering inside a box of (yaw, pitch) degrees, shared by the drills whose dots
//! drift around (Moving gridshot, Switch tracking).

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::DrillConfig;
use super::geometry::{TARGET_DISTANCE, random_range};

/// How strongly a new heading leans back towards the middle of the box, against a random one.
const HOMING: f32 = 1.5;
/// The least share of its speed a dot turning away from an edge heads away from it at, so it
/// leaves rather than sliding along (which looks like sticking to the floor).
const LEAVE: f32 = 0.6;
/// Time constant of a dot's turn away from an edge, seconds: quicker than its other turns, but
/// still a curve it slows down into. It never changes direction in one frame, which no aim can
/// follow.
const EDGE_EASE: f32 = 0.1;

/// A dot's wandering: it eases its velocity towards a heading that changes now and then
/// (`config.turn_min..turn_max` seconds apart, at `config.strafe_min..strafe_max` m/s), so it
/// curves instead of snapping round, also where it meets an edge of its box.
pub(crate) struct Wander {
    /// Degrees per second.
    velocity: Vec2,
    /// The velocity it is easing towards.
    target: Vec2,
    /// The speed of the latest heading, degrees per second.
    speed: f32,
    /// Seconds until the next heading.
    turn_in: f32,
    /// Time constant of the easing: half the time until the next heading, so each turn is
    /// mostly done before the next starts.
    ease: f32,
    /// Axes turning away from an edge: they ease with [`EDGE_EASE`] until the next heading.
    at_edge: BVec2,
}

impl Wander {
    pub(crate) fn new(config: &DrillConfig) -> Self {
        let (heading, turn_in) = (heading(config, Vec2::ZERO), random_range(config.turn_min, config.turn_max));
        Self {
            velocity: heading,
            target: heading,
            speed: heading.length(),
            turn_in,
            ease: turn_in * 0.5,
            at_edge: BVec2::FALSE,
        }
    }

    /// Moves `at` by `dt` seconds of wandering, kept inside `low..=high`. A dot about to meet an
    /// edge takes a new heading away from it and curves round before it gets there.
    pub(crate) fn step(&mut self, at: &mut Vec2, low: Vec2, high: Vec2, dt: f32, config: &DrillConfig) {
        self.turn_in -= dt;
        if self.turn_in <= 0.0 {
            self.turn(*at, low, high, config);
        }
        // Per axis: +1 to turn away from the low edge, -1 from the high one. Turning round with
        // `EDGE_EASE` takes less than speed × EDGE_EASE of room, so that is when to start.
        let mut away = Vec2::ZERO;
        for axis in 0..2 {
            let speed = self.velocity[axis];
            let room = if speed < 0.0 { at[axis] - low[axis] } else { high[axis] - at[axis] };
            if speed != 0.0 && room <= speed.abs() * EDGE_EASE {
                away[axis] = -speed.signum();
            }
        }
        if away != Vec2::ZERO {
            if (0..2).any(|axis| self.target[axis] * away[axis] < 0.0) {
                self.turn(*at, low, high, config);
            }
            for axis in (0..2).filter(|&axis| away[axis] != 0.0) {
                self.target[axis] = away[axis] * self.target[axis].abs().max(self.speed * LEAVE);
                self.at_edge.set(axis, true);
            }
        }
        let follow = |ease: f32| 1.0 - (-dt / ease.max(1e-3)).exp();
        let eases = Vec2::select(self.at_edge, Vec2::splat(self.ease.min(EDGE_EASE)), Vec2::splat(self.ease));
        self.velocity += (self.target - self.velocity) * Vec2::new(follow(eases.x), follow(eases.y));
        // The clamp only matters in a box too small to turn round in (or after a huge frame).
        *at = (*at + self.velocity * dt).clamp(low, high);
    }

    /// A new heading leaning back towards the middle of the box, held for a new while.
    fn turn(&mut self, at: Vec2, low: Vec2, high: Vec2, config: &DrillConfig) {
        let half = ((high - low) * 0.5).max(Vec2::splat(1e-3));
        self.target = heading(config, ((low + high) * 0.5 - at) / half);
        self.speed = self.target.length();
        self.turn_in = random_range(config.turn_min, config.turn_max);
        self.ease = self.turn_in * 0.5;
        self.at_edge = BVec2::FALSE;
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
    /// along it until its next turn; it now leaves it within a fraction of a second.
    #[test]
    fn dots_leave_the_floor() {
        let config = DrillConfig::defaults(DrillKind::SwitchTracking);
        let (low, high) = (Vec2::new(-30.0, -12.0), Vec2::new(30.0, 12.0));
        for seed in 0..50 {
            fastrand::seed(seed);
            let grazing = Vec2::new(10.0, -0.5);
            let mut wander = Wander {
                velocity: grazing,
                target: grazing,
                speed: grazing.length(),
                turn_in: 99.0,
                ease: 0.3,
                at_edge: BVec2::FALSE,
            };
            let mut at = Vec2::new(0.0, low.y + 0.01);
            for _ in 0..30 {
                wander.step(&mut at, low, high, 1.0 / 60.0, &config);
            }
            assert!(at.y > low.y + 1.0, "seed {seed}: still at {} after 0.5 s", at.y);
        }
    }

    /// Dots curve away from the edges of their box: over a minute they never touch one, and
    /// their velocity never changes by more than easing allows in a frame (a bounce would turn
    /// it round in one).
    #[test]
    fn dots_never_snap_round() {
        // Switch tracking's area and one Moving gridshot cell, with each drill's speeds.
        let boxes = [(DrillKind::SwitchTracking, Vec2::new(30.0, 12.0)), (DrillKind::MovingGridshot, Vec2::splat(2.0))];
        for (kind, high) in boxes {
            let config = DrillConfig::defaults(kind);
            let top_speed = (config.strafe_max / TARGET_DISTANCE).atan().to_degrees() * (1.0 + LEAVE);
            for fps in [60.0f32, 178.0, 500.0] {
                let dt = 1.0 / fps;
                // Between the fastest it can go one way and the other, with the quickest easing.
                let most = 2.0 * top_speed * (1.0 - (-dt / EDGE_EASE).exp());
                for seed in 0..20 {
                    fastrand::seed(seed);
                    let (mut wander, mut at) = (Wander::new(&config), Vec2::ZERO);
                    for frame in 0..(60.0 * fps) as u32 {
                        let before = wander.velocity;
                        wander.step(&mut at, -high, high, dt, &config);
                        let change = (wander.velocity - before).length();
                        assert!(change <= most, "{kind:?} at {fps} fps, seed {seed}, frame {frame}: {change} > {most}");
                        let inside = at.cmpgt(-high).all() && at.cmplt(high).all();
                        assert!(inside, "{kind:?} at {fps} fps, seed {seed}, frame {frame}: touched an edge at {at}");
                    }
                }
            }
        }
    }
}
