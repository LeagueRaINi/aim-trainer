//! Peeks: hold one corner. Enemies swing out from behind a wall on the left, at random depths,
//! speeds and widths, hold briefly and fall back. Owns the wall (the [`Environment::Cover`]
//! scenery), which replays of this drill show too.

use bevy::prelude::*;

use crate::AppState;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_tuning};
use crate::drill::geometry::{random_range, ray_box};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{NextSpawn, SpawnCtx, TargetAssets};
use crate::drill::{DrillSet, Run};

/// The wall on the left whose right edge is the held corner, a few metres in front of every
/// peek. Its back right edge (x = `COVER_MAX.x`, z = `COVER_MIN.z`) is what the eye sees past.
pub(crate) const COVER_MIN: Vec3 = Vec3::new(-14.0, -2.5, -6.25);
pub(crate) const COVER_MAX: Vec3 = Vec3::new(-0.6, 3.0, -5.75);
/// Depths (m) peeks happen at: close and far swings look and move differently on screen.
const DEPTH: (f32, f32) = (7.0, 14.0);
/// How far past the corner (m, at the peek's depth) an enemy swings out: from a shoulder to a
/// wide swing.
const SWING: (f32, f32) = (0.3, 2.2);
/// Pause between peeks (s), long enough to be unpredictable.
const GAP: (f32, f32) = (0.6, 2.2);

pub const DEF: DrillDef = DrillDef {
    name: "Peeks",
    blurb: |_| "Hold one corner: enemies swing out at random depths, speeds and widths.".into(),
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
    endless: false,
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
    speed: f32,
    /// x where the enemy is fully behind the wall; it falls back to here.
    hidden_x: f32,
    /// x where its swing out stops.
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
    ctx.next.0 = Some(random_range(GAP.0, GAP.1));
}

/// The next peek comes after a pause.
fn respawn(ctx: &mut SpawnCtx, _: Option<usize>) {
    ctx.next.0 = Some(ctx.now + random_range(GAP.0, GAP.1));
}

/// x of the corner as seen from the eye, at `depth` metres.
fn corner_x(depth: f32) -> f32 {
    COVER_MAX.x * depth / -COVER_MIN.z
}

/// An enemy at a random depth, around head height, just behind the corner.
fn spawn_next(ctx: &mut SpawnCtx) {
    let depth = random_range(DEPTH.0, DEPTH.1);
    let corner = corner_x(depth);
    let hidden_x = corner - ctx.radius() - 0.05;
    let peek = PeekMove {
        speed: random_range(ctx.config.peek_speed_min, ctx.config.peek_speed_max),
        hidden_x,
        out_x: corner + random_range(SWING.0, SWING.1),
        phase: PeekPhase::Out,
    };
    let position = Vec3::new(hidden_x, random_range(-0.15, 0.15), -depth);
    ctx.spawn_target(position, None).insert(peek);
}

/// Enemies swing out from behind the corner, hold briefly, then fall back. Getting back to cover
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
                *x = (*x + peek.speed * dt).min(peek.out_x);
                if *x >= peek.out_x {
                    peek.phase = PeekPhase::Hold(random_range(hold_min, hold_max));
                }
            },
            PeekPhase::Hold(left) => {
                peek.phase = if left > dt { PeekPhase::Hold(left - dt) } else { PeekPhase::Back };
            },
            PeekPhase::Back => {
                *x -= peek.speed * dt;
                if *x <= peek.hidden_x {
                    run.expire();
                    commands.entity(entity).despawn();
                    next.0 = Some(run.elapsed + random_range(GAP.0, GAP.1));
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wall hides an enemy waiting behind the corner at every depth (all of it, at the
    /// largest size the editor allows), and stops shots at it; past the corner it is in the open.
    #[test]
    fn wall_hides_waiting_enemies_only() {
        let radius = DEF.base_radius * ParamId::Size.spec().soft.1;
        for depth in [DEPTH.0, 10.0, DEPTH.1] {
            let hidden = Vec3::new(corner_x(depth) - radius - 0.05, 0.0, -depth);
            for edge in [hidden + Vec3::X * radius, hidden - Vec3::X * radius, hidden + Vec3::Y * radius] {
                assert!(cover_hit(Vec3::ZERO, edge.normalize()).is_some(), "depth {depth}: {edge} visible");
            }
            let out = Vec3::new(corner_x(depth) + SWING.0 + radius, 0.0, -depth);
            assert!(cover_hit(Vec3::ZERO, out.normalize()).is_none(), "depth {depth}: swung out but blocked");
        }
        // Straight ahead is the open lane being held.
        assert!(cover_hit(Vec3::ZERO, Vec3::NEG_Z).is_none());
    }
}
