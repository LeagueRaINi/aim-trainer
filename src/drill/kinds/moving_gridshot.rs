//! Moving gridshot: Gridshot whose dots drift around their cells, so every kill needs a small
//! correction on top of the flick.

use std::borrow::Cow;

use bevy::prelude::*;

use super::gridshot::{cell_angles, dots_on_grid, initial_slots, next_slot, position_at};
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_spawn_next};
use crate::drill::geometry::{CEILING_PITCH, FLOOR_PITCH};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{Slot, SpawnCtx};
use crate::drill::wander::Wander;
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
    wander: Wander,
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
    let drift = Drift { cell, offset: Vec2::ZERO, wander: Wander::new(ctx.config) };
    let lifetime = ctx.lifetime();
    ctx.spawn_target(position_at(cell), lifetime).insert((Slot(slot), drift));
}

/// Moves every dot around its patch: the cell, cut by the floor and ceiling.
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
        let Drift { cell, offset, wander } = &mut *drift;
        wander.step(offset, low, high, dt, config);
        transform.translation = position_at(*cell + *offset);
    }
}
