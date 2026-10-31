//! Peeks: targets swing out from behind cover, hold briefly and retreat. Owns the cover box
//! (the [`Environment::Cover`] scenery), which replays of this drill show too.

use bevy::prelude::*;

use crate::AppState;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_tuning};
use crate::drill::geometry::{TARGET_DISTANCE, random_range, random_sign, ray_box};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{NextSpawn, SpawnCtx, TargetAssets};
use crate::drill::{DrillSet, Run};

/// Peek cover: a box between the player and the target plane. Targets start hidden behind it.
pub(crate) const COVER_MIN: Vec3 = Vec3::new(-1.5, -2.5, -8.75);
pub(crate) const COVER_MAX: Vec3 = Vec3::new(1.5, 0.9, -8.25);
/// Peeking targets start (and end) at this |x|, fully hidden behind the cover.
const PEEK_HIDDEN_X: f32 = 1.2;

pub const DEF: DrillDef = DrillDef {
    name: "Peeks",
    blurb: "Hold an angle: enemies swing out of cover.",
    category: Category::Tactical,
    base_radius: 0.2,
    decay: DecayRule::None,
    score: ScoreModel::Flick,
    ttk_from: TtkFrom::Spawn,
    click_kills: true,
    environment: Environment::Cover,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    params: &[ParamId::PeekSpeedMin, ParamId::PeekSpeedMax, ParamId::HoldMin, ParamId::HoldMax],
    tune_defaults: no_tuning,
    spawn_initial,
    respawn,
    spawn_next,
};

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, move_peeks.in_set(DrillSet::Move));
}

#[derive(Clone, Copy, PartialEq)]
enum PeekPhase {
    Out,
    Hold(f32),
    Back,
}

#[derive(Component)]
struct PeekMove {
    side: f32,
    speed: f32,
    out_x: f32,
    phase: PeekPhase,
}

/// The cover box.
pub(crate) fn spawn_cover(commands: &mut Commands, assets: &TargetAssets, state: AppState) {
    commands.spawn((
        DespawnOnExit(state),
        Mesh3d(assets.cover_mesh.clone()),
        MeshMaterial3d(assets.cover_material.clone()),
        // Primitive meshes are centered on the origin; move it into place.
        Transform::from_translation((COVER_MIN + COVER_MAX) * 0.5),
    ));
}

/// Distance along the ray to the cover, if the ray hits it.
pub(crate) fn cover_hit(origin: Vec3, dir: Vec3) -> Option<f32> {
    ray_box(origin, dir, COVER_MIN, COVER_MAX)
}

fn spawn_initial(ctx: &mut SpawnCtx) {
    ctx.next.0 = Some(0.6);
}

/// The next peek comes after a pause.
fn respawn(ctx: &mut SpawnCtx, _: Option<usize>) {
    ctx.next.0 = Some(ctx.now + random_range(0.5, 1.4));
}

/// A peek from a random side, starting hidden behind the cover.
fn spawn_next(ctx: &mut SpawnCtx) {
    let side = random_sign();
    let position = Vec3::new(side * PEEK_HIDDEN_X, 0.0, -TARGET_DISTANCE);
    let peek = PeekMove {
        side,
        speed: random_range(ctx.config.peek_speed_min, ctx.config.peek_speed_max),
        out_x: random_range(2.6, 3.6),
        phase: PeekPhase::Out,
    };
    ctx.spawn_target(position, None).insert(peek);
}

/// Peeks swing out from behind the cover, hold briefly, then retreat. Retreating to safety
/// counts as a miss.
fn move_peeks(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextSpawn>,
    mut peeks: Query<(Entity, &mut PeekMove, &mut Transform)>,
) {
    if !run.live() {
        return;
    }
    let dt = time.delta_secs();
    let (hold_min, hold_max) = (run.drill.config.hold_min, run.drill.config.hold_max);
    for (entity, mut peek, mut transform) in &mut peeks {
        let x = &mut transform.translation.x;
        match peek.phase {
            PeekPhase::Out => {
                *x += peek.side * peek.speed * dt;
                if x.abs() >= peek.out_x {
                    peek.phase = PeekPhase::Hold(random_range(hold_min, hold_max));
                }
            },
            PeekPhase::Hold(left) => {
                peek.phase = if left > dt { PeekPhase::Hold(left - dt) } else { PeekPhase::Back };
            },
            PeekPhase::Back => {
                *x -= peek.side * peek.speed * dt;
                if x.abs() <= PEEK_HIDDEN_X {
                    run.expire();
                    commands.entity(entity).despawn();
                    next.0 = Some(run.elapsed + random_range(0.5, 1.4));
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_blocks_rays_through_it_only() {
        // Straight ahead goes through the cover.
        assert!(cover_hit(Vec3::ZERO, Vec3::NEG_Z).is_some());
        // Aiming at a peek past the edge clears it.
        let past_edge = Vec3::new(2.5, 0.0, -TARGET_DISTANCE).normalize();
        assert!(cover_hit(Vec3::ZERO, past_edge).is_none());
        // A hidden peek position is behind the cover.
        let hidden = Vec3::new(PEEK_HIDDEN_X + 0.16, 0.0, -TARGET_DISTANCE).normalize();
        assert!(cover_hit(Vec3::ZERO, hidden).is_some());
    }
}
