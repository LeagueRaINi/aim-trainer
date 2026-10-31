//! Tracking: keep the crosshair on a strafing dot.

use bevy::prelude::*;

use crate::camera::PlayerCamera;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_respawn, no_spawn_next, no_tuning};
use crate::drill::geometry::{TARGET_DISTANCE, random_range, ray_hit};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{SpawnCtx, TargetAssets};
use crate::drill::{DrillSet, Run};

pub const DEF: DrillDef = DrillDef {
    name: "Tracking",
    blurb: "Keep the crosshair on a strafing dot.",
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
    params: &[ParamId::StrafeMin, ParamId::StrafeMax, ParamId::TurnMin, ParamId::TurnMax, ParamId::Lane],
    tune_defaults: no_tuning,
    spawn_initial,
    respawn: no_respawn,
    spawn_next: no_spawn_next,
};

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, (move_strafers.in_set(DrillSet::Move), track.in_set(DrillSet::Score)));
}

#[derive(Component)]
struct Strafer {
    velocity: f32,
    turn_in: f32,
}

/// One strafing dot straight ahead.
fn spawn_initial(ctx: &mut SpawnCtx) {
    let strafer = Strafer { velocity: ctx.config.strafe_min, turn_in: ctx.config.turn_max };
    ctx.spawn_target(Vec3::new(0.0, 0.0, -TARGET_DISTANCE), None).insert(strafer);
}

/// ADAD-style strafing: random speed, random direction changes, bounded to the lane.
fn move_strafers(time: Res<Time>, run: Res<Run>, mut strafers: Query<(&mut Strafer, &mut Transform)>) {
    let config = &run.drill.config;
    let dt = time.delta_secs();
    for (mut strafer, mut transform) in &mut strafers {
        transform.translation.x += strafer.velocity * dt;
        strafer.turn_in -= dt;
        let out_of_bounds = transform.translation.x.abs() > config.lane;
        if strafer.turn_in <= 0.0 || out_of_bounds {
            let direction = if out_of_bounds { -transform.translation.x.signum() } else { -strafer.velocity.signum() };
            strafer.velocity = direction * random_range(config.strafe_min, config.strafe_max);
            strafer.turn_in = random_range(config.turn_min, config.turn_max);
        }
    }
}

/// Counts time on target and lights the target up while the crosshair is on it.
fn track(
    time: Res<Time>,
    mut run: ResMut<Run>,
    assets: Res<TargetAssets>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Strafer>)>,
    mut targets: Query<(&Transform, &mut MeshMaterial3d<StandardMaterial>), With<Strafer>>,
) {
    let origin = camera.translation;
    let aim = camera.forward().as_vec3();
    for (transform, mut material) in &mut targets {
        let on_target = ray_hit(origin, aim, transform.translation, transform.scale.x).is_some();
        let wanted = if on_target { &assets.hot } else { &assets.idle };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
        if on_target && run.live() {
            run.track_on_target(time.delta_secs());
        }
    }
}
