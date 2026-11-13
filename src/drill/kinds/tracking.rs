//! Tracking: hold fire on a strafing dot. Only the time it is being hit counts.

use bevy::prelude::*;

use crate::camera::PlayerCamera;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_respawn, no_spawn_next, no_tuning};
use crate::drill::geometry::{TARGET_DISTANCE, random_range, ray_hit};
use crate::drill::held_fire::HeldFire;
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{SpawnCtx, TargetAssets};
use crate::drill::{DrillConfig, DrillSet, Run};
use crate::replay::Recorder;

pub const DEF: DrillDef = DrillDef {
    name: "Tracking",
    blurb: |_| "Hold fire and keep the crosshair on a strafing dot.".into(),
    category: Category::Aim,
    base_radius: 0.45,
    decay: DecayRule::None,
    score: ScoreModel::Tracking,
    ttk_from: TtkFrom::Spawn,
    click_kills: false,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: true,
    flick_analysis: false,
    adaptive: false,
    endless: false,
    params: &[ParamId::StrafeMin, ParamId::StrafeMax, ParamId::TurnMin, ParamId::TurnMax, ParamId::Lane],
    tune_defaults: no_tuning,
    spawn_initial,
    respawn: no_respawn,
    spawn_next: no_spawn_next,
};

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, (move_strafers.in_set(DrillSet::Move), track.in_set(DrillSet::Score)));
}

/// How fast a strafing dot changes its speed, m/s²: it has inertia like a strafing player, so a
/// direction change shows as slowing down first and can be followed (4 m/s one way to 4 m/s the
/// other takes 0.4 s). A dot that flips its velocity in one frame is off the crosshair for a
/// whole reaction time whatever the player does.
const ACCEL: f32 = 20.0;

#[derive(Component)]
struct Strafer {
    /// Metres per second along the lane.
    velocity: f32,
    /// The velocity it is accelerating to.
    wanted: f32,
    /// Seconds until the next direction change.
    turn_in: f32,
}

impl Strafer {
    /// At rest, about to set off at the slowest strafe.
    fn new(config: &DrillConfig) -> Self {
        Self { velocity: 0.0, wanted: config.strafe_min, turn_in: config.turn_max }
    }

    /// Moves `x` (metres along the lane) by `dt` seconds of ADAD-style strafing: now and then a
    /// new random speed the other way, reached at [`ACCEL`], and never past the lane's edges.
    fn step(&mut self, x: &mut f32, dt: f32, config: &DrillConfig) {
        self.turn_in -= dt;
        if self.turn_in <= 0.0 {
            self.turn(-self.wanted.signum(), config);
        }
        let step = ACCEL * dt;
        let mut velocity = self.velocity + (self.wanted - self.velocity).clamp(-step, step);
        // Not past the point where it could still stop at the edge ahead: braking takes
        // velocity² / 2·ACCEL metres after this frame's travel. It turns back there instead.
        let ahead = velocity.signum();
        let room = config.lane - *x * ahead;
        if self.wanted * ahead > 0.0 && room < velocity * velocity / (2.0 * ACCEL) + velocity.abs() * dt {
            self.turn(-ahead, config);
            velocity = self.velocity + (self.wanted - self.velocity).clamp(-step, step);
        }
        self.velocity = velocity;
        // The clamp only matters for a lane narrowed mid-run.
        *x = (*x + velocity * dt).clamp(-config.lane, config.lane);
    }

    /// A new random speed in `direction`, held for a new while.
    fn turn(&mut self, direction: f32, config: &DrillConfig) {
        self.wanted = direction * random_range(config.strafe_min, config.strafe_max);
        self.turn_in = random_range(config.turn_min, config.turn_max);
    }
}

/// One strafing dot straight ahead.
fn spawn_initial(ctx: &mut SpawnCtx) {
    let strafer = Strafer::new(ctx.config);
    ctx.spawn_target(Vec3::new(0.0, 0.0, -TARGET_DISTANCE), None).insert(strafer);
}

fn move_strafers(time: Res<Time>, run: Res<Run>, mut strafers: Query<(&mut Strafer, &mut Transform)>) {
    let config = &run.drill.config;
    let dt = time.delta_secs();
    for (mut strafer, mut transform) in &mut strafers {
        strafer.step(&mut transform.translation.x, dt, config);
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
    camera: Single<&Transform, (With<PlayerCamera>, Without<Strafer>)>,
    mut targets: Query<(&Transform, &mut MeshMaterial3d<StandardMaterial>), With<Strafer>>,
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

    /// A minute of strafing at `fps`: the velocity never changes faster than [`ACCEL`], the dot
    /// stays in its lane, and it goes both ways at up to the configured speed.
    fn strafes_with_inertia(config: &DrillConfig, fps: f32) {
        let dt = 1.0 / fps;
        for seed in 0..20 {
            fastrand::seed(seed);
            let mut strafer = Strafer::new(config);
            let (mut x, mut fastest, mut turns) = (0.0f32, (0.0f32, 0.0f32), 0);
            for frame in 0..(60.0 * fps) as u32 {
                let before = strafer.velocity;
                strafer.step(&mut x, dt, config);
                let change = (strafer.velocity - before).abs();
                assert!(change <= ACCEL * dt * 1.001, "seed {seed}, frame {frame}: velocity jumped by {change} m/s");
                // Strictly inside: at the edge it would have been stopped by the clamp, not by braking.
                assert!(x.abs() < config.lane, "seed {seed}, frame {frame}: at {x} m, ran into the lane's edge");
                assert!(strafer.velocity.abs() <= config.strafe_max * 1.001, "seed {seed}: {} m/s", strafer.velocity);
                turns += u32::from(before * strafer.velocity < 0.0);
                fastest = (fastest.0.min(strafer.velocity), fastest.1.max(strafer.velocity));
            }
            assert!(turns >= 20, "seed {seed}: only {turns} direction changes in a minute");
            assert!(fastest.0 < 0.0 && fastest.1 > 0.0, "seed {seed}: {fastest:?}");
        }
    }

    #[test]
    fn strafing_dots_have_inertia() {
        let defaults = DrillConfig::defaults(DrillKind::Tracking);
        for fps in [30.0, 60.0, 178.0, 500.0] {
            strafes_with_inertia(&defaults, fps);
            // The fastest strafes the editor allows, in a lane too short to ever reach them.
            let fast = DrillConfig { strafe_min: 12.0, strafe_max: 20.0, lane: 1.0, ..defaults.clone() };
            strafes_with_inertia(&fast, fps);
        }
    }

    #[test]
    fn a_direction_change_takes_a_moment() {
        let config = DrillConfig { strafe_min: 4.0, strafe_max: 4.0, ..DrillConfig::defaults(DrillKind::Tracking) };
        let mut strafer = Strafer { velocity: 4.0, wanted: 4.0, turn_in: 0.0 };
        let (mut x, dt) = (0.0, 1.0 / 240.0);
        // The turn starts on the first frame; 4 m/s to -4 m/s at 20 m/s² takes 0.4 s.
        let mut frames = 0;
        while strafer.velocity > -4.0 {
            strafer.step(&mut x, dt, &config);
            strafer.turn_in = 99.0;
            frames += 1;
        }
        let secs = frames as f32 * dt;
        assert!((secs - 0.4).abs() < 0.01, "{secs} s");
        // It slows down, stops 0.4 m further on (v² / 2a) and is back where the turn began.
        assert!(x.abs() < 0.05, "{x} m");
    }
}
