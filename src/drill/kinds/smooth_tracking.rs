//! Smooth tracking: hold fire on a dot gliding round a flat loop at a steady speed. Nothing about it
//! is random, so there is nothing to react to: what it trains is matching the target's speed
//! (Tracking trains reacting to its changes of direction). Calm aim's first part follows the
//! same [`Loop`], slower.

use std::borrow::Cow;
use std::f32::consts::PI;

use bevy::prelude::*;

use crate::camera::PlayerCamera;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_respawn, no_spawn_next, no_tuning};
use crate::drill::geometry::{TARGET_DISTANCE, direction, random_sign, ray_hit};
use crate::drill::held_fire::HeldFire;
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{SpawnCtx, TargetAssets};
use crate::drill::{DrillConfig, DrillSet, Run};
use crate::replay::Recorder;

pub const DEF: DrillDef = DrillDef {
    name: "Smooth tracking",
    blurb,
    category: Category::Aim,
    // Smaller than Tracking's dot: its path is known in advance, so only a target that a tenth
    // of a second of lag already slides off leaves room to get better.
    base_radius: 0.25,
    decay: DecayRule::None,
    score: ScoreModel::Tracking,
    ttk_from: TtkFrom::Spawn,
    click_kills: false,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    endless: false,
    params: &[ParamId::SweepSpeed, ParamId::Lane],
    tune_defaults: no_tuning,
    spawn_initial,
    respawn: no_respawn,
    spawn_next: no_spawn_next,
};

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, (glide.in_set(DrillSet::Move), track.in_set(DrillSet::Score)));
}

fn blurb(config: &DrillConfig) -> Cow<'static, str> {
    format!("Hold fire on a dot gliding round a loop at a steady {:.1} m/s. Match its speed.", config.sweep_speed)
        .into()
}

/// How fast the dot gets up to speed from rest, degrees per second squared (about 5 m/s² at the
/// target distance): it sets off like something with weight, in half a second at the default
/// speed, rather than jumping to full speed in a frame.
const ACCEL: f32 = 30.0;
/// The most the loop's turns curve, as their radius in metres at the target distance: wide
/// enough to steer through (at the default 2.5 m/s the dot's direction turns at 2.5 rad/s, where
/// Tracking's strafes reverse in 0.4 s). Narrow loops turn tighter: a quarter of their width.
const TURN_RADIUS: f32 = 1.0;

/// A dot's way round a flat loop, in (yaw, pitch) degrees around straight ahead: two level
/// straights joined by half circles, travelled at a steady angular speed, so the mouse speed it
/// takes is the same all the way along the straights.
pub(crate) struct Loop {
    /// Half the length of each straight, degrees.
    straight: f32,
    /// Radius of the turns, degrees: the loop reaches this far above and below eye level.
    radius: f32,
    /// Degrees travelled along the loop from the middle of its bottom straight: the dot starts
    /// below the crosshair, clear of the countdown and of Calm aim's banner above it.
    travelled: f32,
    /// Degrees per second now, and the speed it is getting up to.
    speed: f32,
    cruise: f32,
    /// +1 if it sets off to the right along the bottom, -1 to the left.
    way: f32,
}

impl Loop {
    /// A loop reaching `half_width` metres (at the target distance) to each side, travelled at
    /// `speed` m/s there, setting off from rest to the right (`way` 1) or left (-1).
    pub(crate) fn new(speed: f32, half_width: f32, way: f32) -> Self {
        let degrees = |metres: f32| (metres / TARGET_DISTANCE).atan().to_degrees();
        let half_width = degrees(half_width);
        let radius = degrees(TURN_RADIUS).min(half_width * 0.5);
        Self { straight: half_width - radius, radius, travelled: 0.0, speed: 0.0, cruise: degrees(speed), way }
    }

    /// Moves on by `dt` seconds.
    pub(crate) fn step(&mut self, dt: f32) {
        self.speed = if (self.cruise - self.speed).abs() <= ACCEL * dt {
            self.cruise
        } else {
            self.speed + ACCEL * dt * (self.cruise - self.speed).signum()
        };
        let lap = 4.0 * self.straight + 2.0 * PI * self.radius;
        self.travelled = (self.travelled + self.speed * dt).rem_euclid(lap);
    }

    /// Where the dot is: (yaw, pitch) in degrees.
    pub(crate) fn position(&self) -> Vec2 {
        let (a, r) = (self.straight, self.radius);
        let turn = PI * r;
        let s = self.travelled;
        // (right, down) on screen, going round from the middle of the bottom, first to the right.
        let (right, down) = if s < a {
            (s, r)
        } else if s < a + turn {
            let (sin, cos) = ((s - a) / r).sin_cos();
            (a + r * sin, r * cos)
        } else if s < 3.0 * a + turn {
            (2.0 * a + turn - s, -r)
        } else if s < 3.0 * a + 2.0 * turn {
            let (sin, cos) = ((s - 3.0 * a - turn) / r).sin_cos();
            (-a - r * sin, -r * cos)
        } else {
            (s - 4.0 * a - 2.0 * turn, r)
        };
        // Yaw grows to the left.
        Vec2::new(-right * self.way, -down)
    }

    /// Where the dot is in the room.
    pub(crate) fn translation(&self) -> Vec3 {
        let at = self.position();
        direction(at.x.to_radians(), at.y.to_radians()) * TARGET_DISTANCE
    }
}

#[derive(Component)]
struct Glider(Loop);

/// One dot at rest in the middle of the loop's bottom straight; it sets off when the run goes
/// live.
fn spawn_initial(ctx: &mut SpawnCtx) {
    let glide = Loop::new(ctx.config.sweep_speed, ctx.config.lane, random_sign());
    ctx.spawn_target(glide.translation(), None).insert(Glider(glide));
}

fn glide(time: Res<Time>, run: Res<Run>, mut gliders: Query<(&mut Glider, &mut Transform)>) {
    if !run.live() {
        return;
    }
    for (mut glider, mut transform) in &mut gliders {
        glider.0.step(time.delta_secs());
        transform.translation = glider.0.translation();
    }
}

/// Books the frame's fire at the target (see [`HeldFire::book`]) and lights it up while it is
/// being hit; before the run starts, while the crosshair is on it.
fn track(
    time: Res<Time>,
    gun: Res<HeldFire>,
    mut run: ResMut<Run>,
    mut recorder: ResMut<Recorder>,
    assets: Res<TargetAssets>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Glider>)>,
    mut targets: Query<(&Transform, &mut MeshMaterial3d<StandardMaterial>), With<Glider>>,
) {
    let origin = camera.translation;
    let aim = camera.forward().as_vec3();
    for (transform, mut material) in &mut targets {
        let on_target = ray_hit(origin, aim, transform.translation, transform.scale.x).is_some();
        let lit = on_target && (gun.held() || !run.live());
        let wanted = if lit { &assets.hot } else { &assets.idle };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
        if run.live() {
            gun.book(&mut run, &mut recorder, on_target, time.delta_secs());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::DrillKind;
    use crate::drill::geometry::{CEILING_PITCH, FLOOR_PITCH};

    /// Two laps of the loop at `fps`: the dot gets up to speed without a jump, then keeps
    /// exactly that speed; its direction never turns faster than the loop's curve allows; it
    /// stays inside the loop's box and between floor and ceiling, and comes back to its start.
    fn glides_steadily(speed: f32, half_width: f32, fps: f32) {
        /// The path's direction is taken over stretches at least this long, degrees: single
        /// frames at 500 fps are too short for `f32` positions to give one.
        const CHORD: f32 = 0.25;
        let dt = 1.0 / fps;
        let mut glide = Loop::new(speed, half_width, 1.0);
        let cruise = glide.cruise;
        let lap = 4.0 * glide.straight + 2.0 * PI * glide.radius;
        let reach = Vec2::new(glide.straight + glide.radius, glide.radius);
        let label = format!("{speed} m/s, {half_width} m, {fps} fps");
        let start = glide.position();
        assert!(start.x.abs() < 1e-4 && (start.y + glide.radius).abs() < 1e-4, "{label}: starts at {start}");
        let mut before = start;
        // Where the current chord began, and the previous chord's direction and length.
        let mut chord_from = start;
        let mut last_chord: Option<(Vec2, f32)> = None;
        let mut travelled = 0.0f64;
        // Getting up to speed takes `cruise / ACCEL` seconds and half that distance.
        let frames = ((2.0 * lap / cruise + cruise / ACCEL * 0.5) * fps).round() as u32;
        for frame in 0..frames {
            let speed_before = glide.speed;
            glide.step(dt);
            assert!(glide.speed - speed_before <= ACCEL * dt * 1.001, "{label}: speed jumped at frame {frame}");
            let at = glide.position();
            assert!(at.abs().cmple(reach * 1.0001).all(), "{label}: left the loop at {at}");
            assert!((FLOOR_PITCH..=CEILING_PITCH).contains(&at.y), "{label}: {at} outside the room");
            let step = at.distance(before);
            // Chords of the turns are a little shorter than the arc; rounding adds a hair.
            assert!(step <= glide.speed * dt * 1.001 + 1e-5, "{label}: moved {step} in a frame");
            travelled += f64::from(glide.speed * dt);
            if speed_before >= cruise {
                assert!(step >= cruise * dt * 0.98 - 1e-5, "{label}: slowed down at frame {frame}");
            }
            before = at;
            let chord = at - chord_from;
            if chord.length() >= CHORD {
                let (heading, length) = (chord.normalize(), chord.length());
                if let Some((last_heading, last_length)) = last_chord {
                    // Between the middles of two chords the path turns by at most its curve
                    // (1 / radius, radians per degree travelled) over that distance.
                    let turned = last_heading.angle_to(heading).abs();
                    let most = (last_length + length) * 0.5 / glide.radius;
                    assert!(turned <= most * 1.05 + 1e-3, "{label}: turned {turned} rad, at most {most}");
                }
                (chord_from, last_chord) = (at, Some((heading, length)));
            }
        }
        assert!(glide.speed == cruise, "{label}: never got up to speed");
        // Whole frames: up to one frame's travel either way, for getting up to speed and the end.
        let off = (travelled as f32 - 2.0 * lap).abs();
        assert!(off < 2.0 * cruise * dt + lap * 1e-3, "{label}: {travelled} of {} travelled", 2.0 * lap);
        // Tens of thousands of tiny `f32` steps (a slow dot at 500 fps) round a little each.
        let end = glide.position();
        assert!(end.distance(start) < 2.0 * cruise * dt + lap * 3e-3, "{label}: ended at {end}");
    }

    #[test]
    fn the_dot_glides_at_a_steady_speed_round_a_closed_loop() {
        let defaults = DrillConfig::defaults(DrillKind::SmoothTracking);
        for fps in [60.0, 178.0, 500.0] {
            glides_steadily(defaults.sweep_speed, defaults.lane, fps);
            // Calm aim's slow loop, and the narrowest and fastest the editor's drag range allows.
            glides_steadily(1.2, 3.5, fps);
            glides_steadily(8.0, 1.0, fps);
            glides_steadily(0.5, 12.0, fps);
        }
    }

    #[test]
    fn the_loop_is_mirrored_for_the_other_way_round() {
        let (mut right, mut left) = (Loop::new(2.5, 5.0, 1.0), Loop::new(2.5, 5.0, -1.0));
        for _ in 0..2000 {
            right.step(0.005);
            left.step(0.005);
            let (a, b) = (right.position(), left.position());
            assert!((a.x + b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4, "{a} vs {b}");
        }
        // Setting off to the right on screen is towards smaller yaw.
        let mut start = Loop::new(2.5, 5.0, 1.0);
        start.step(0.5);
        assert!(start.position().x < 0.0);
    }
}
