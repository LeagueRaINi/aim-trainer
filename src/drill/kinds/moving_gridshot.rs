//! Moving gridshot: Gridshot whose dots drift around their cells, so every kill needs a small
//! correction on top of the flick.

use std::borrow::Cow;
use std::f32::consts::TAU;

use bevy::prelude::*;

use super::gridshot::{cell_angles, dots_on_grid, initial_slots, next_slot, position_at};
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_spawn_next};
use crate::drill::geometry::{CEILING_PITCH, FLOOR_PITCH, TARGET_DISTANCE, random_range};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{Slot, SpawnCtx};
use crate::drill::{DrillConfig, DrillSet, Run};

/// How far a dot strays from its cell centre, as a fraction of the cell spacing: neighbours
/// drifting towards each other still keep apart.
const DRIFT_REACH: f32 = 0.25;

pub const DEF: DrillDef = DrillDef {
    name: "Moving gridshot",
    blurb,
    category: Category::Aim,
    // Gridshot's, with its default size (see `tune`).
    base_radius: 0.45,
    decay: DecayRule::Global,
    score: ScoreModel::Flick,
    ttk_from: TtkFrom::PreviousKill,
    click_kills: true,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    endless: false,
    params: &[
        ParamId::GridCols,
        ParamId::GridRows,
        ParamId::GridSpacing,
        ParamId::GridActive,
        ParamId::StrafeMin,
        ParamId::StrafeMax,
        ParamId::TurnMin,
        ParamId::TurnMax,
    ],
    tune_defaults: tune,
    spawn_initial,
    respawn,
    spawn_next: no_spawn_next,
};

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, drift.in_set(DrillSet::Move));
}

/// Gridshot's grid with slow, smooth wandering: 3.4°/s to 9.2°/s, easing into a new heading every
/// 0.6 s to 1.4 s. Shorter direction changes make the dots jerkier (more dynamic).
fn tune(config: &mut DrillConfig) {
    super::gridshot::tune(config);
    config.strafe_min = 0.6;
    config.strafe_max = 1.6;
    config.turn_min = 0.6;
    config.turn_max = 1.4;
}

fn blurb(config: &DrillConfig) -> Cow<'static, str> {
    format!("{}, drifting. Flick, then correct.", dots_on_grid(config)).into()
}

/// A dot wandering around its cell; angles in degrees.
#[derive(Component)]
struct Drift {
    /// The cell centre (yaw, pitch).
    cell: Vec2,
    /// Current offset from the cell centre.
    offset: Vec2,
    /// Degrees per second.
    velocity: Vec2,
    /// The velocity it is easing towards.
    target: Vec2,
    /// Seconds until the next heading.
    turn_in: f32,
    /// Time constant of the easing, seconds: half the time until the next heading, so each turn
    /// is mostly done before the next starts.
    ease: f32,
}

impl Drift {
    fn new(cell: Vec2, config: &DrillConfig) -> Self {
        let (heading, turn_in) = (heading(config, Vec2::ZERO), random_range(config.turn_min, config.turn_max));
        Self { cell, offset: Vec2::ZERO, velocity: heading, target: heading, turn_in, ease: turn_in * 0.5 }
    }
}

/// A random direction at a random strafe speed (m/s at the target distance), in degrees/s,
/// leaning back towards the middle of the patch the further `home` (from -1 to 1 per axis: the
/// way back to the middle, in half-patches) says the dot has strayed.
fn heading(config: &DrillConfig, home: Vec2) -> Vec2 {
    let speed = (random_range(config.strafe_min, config.strafe_max) / TARGET_DISTANCE).atan().to_degrees();
    let wander = Vec2::from_angle(fastrand::f32() * TAU);
    (wander + home * 1.5).normalize_or(wander) * speed
}

fn spawn_initial(ctx: &mut SpawnCtx) {
    for slot in initial_slots(ctx.config) {
        spawn_dot(ctx, slot);
    }
}

fn respawn(ctx: &mut SpawnCtx, cleared_slot: Option<usize>) {
    if let Some(slot) = next_slot(ctx, cleared_slot) {
        spawn_dot(ctx, slot);
    }
}

fn spawn_dot(ctx: &mut SpawnCtx, slot: usize) {
    let cell = cell_angles(slot, ctx.config);
    let (drift, lifetime) = (Drift::new(cell, ctx.config), ctx.lifetime());
    ctx.spawn_target(position_at(cell), lifetime).insert((Slot(slot), drift));
}

/// Moves every dot, easing its velocity towards a heading that changes now and then and leans
/// back to the middle of its patch (the cell, cut by the floor and ceiling), so dots curve
/// instead of snapping round. A dot that still reaches the edge slides along it.
fn drift(time: Res<Time>, run: Res<Run>, mut dots: Query<(&mut Drift, &mut Transform)>) {
    if !run.live() {
        return;
    }
    let config = &run.drill.config;
    let reach = config.grid_spacing * DRIFT_REACH;
    let dt = time.delta_secs();
    for (mut drift, mut transform) in &mut dots {
        let low = Vec2::new(-reach, (-reach).max(FLOOR_PITCH - drift.cell.y));
        let high = Vec2::new(reach, reach.min(CEILING_PITCH - drift.cell.y));
        drift.turn_in -= dt;
        if drift.turn_in <= 0.0 {
            let half = ((high - low) * 0.5).max(Vec2::splat(1e-3));
            let home = ((low + high) * 0.5 - drift.offset) / half;
            drift.target = heading(config, home);
            drift.turn_in = random_range(config.turn_min, config.turn_max);
            drift.ease = drift.turn_in * 0.5;
        }
        let follow = 1.0 - (-dt / drift.ease.max(1e-3)).exp();
        drift.velocity = drift.velocity + (drift.target - drift.velocity) * follow;
        let mut offset = drift.offset + drift.velocity * dt;
        for axis in 0..2 {
            if offset[axis] < low[axis] {
                offset[axis] = low[axis];
                drift.velocity[axis] = drift.velocity[axis].max(0.0);
                drift.target[axis] = drift.target[axis].abs();
            } else if offset[axis] > high[axis] {
                offset[axis] = high[axis];
                drift.velocity[axis] = drift.velocity[axis].min(0.0);
                drift.target[axis] = -drift.target[axis].abs();
            }
        }
        drift.offset = offset;
        transform.translation = position_at(drift.cell + offset);
    }
}
