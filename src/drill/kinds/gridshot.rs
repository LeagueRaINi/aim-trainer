//! Gridshot: several dots on a grid; a kill respawns one in another cell.

use bevy::prelude::*;

use crate::drill::{
    DrillConfig,
    def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_spawn_next, no_tuning},
    geometry::{TARGET_DISTANCE, direction},
    params::ParamId,
    stats::ScoreModel,
    targets::{Slot, SpawnCtx},
};

pub const DEF: DrillDef = DrillDef {
    name: "Gridshot",
    blurb: "3 dots on a 3x3 grid. Speed + accuracy.",
    category: Category::Aim,
    base_radius: 0.45,
    decay: DecayRule::Global,
    score: ScoreModel::Flick,
    ttk_from: TtkFrom::PreviousKill,
    click_kills: true,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: true,
    flick_analysis: true,
    adaptive: false,
    params: &[ParamId::GridCols, ParamId::GridRows, ParamId::GridSpacing, ParamId::GridActive],
    tune_defaults: no_tuning,
    spawn_initial,
    respawn,
    spawn_next: no_spawn_next,
};

/// `grid_active` dots in random cells.
fn spawn_initial(ctx: &mut SpawnCtx) {
    let mut slots: Vec<usize> = (0..grid_cells(ctx.config)).collect();
    fastrand::shuffle(&mut slots);
    let lifetime = ctx.lifetime();
    for &slot in &slots[..ctx.config.grid_active as usize] {
        let position = grid_position(slot, ctx.config);
        ctx.spawn_target(position, lifetime).insert(Slot(slot));
    }
}

/// A new dot in another free cell.
fn respawn(ctx: &mut SpawnCtx, cleared_slot: Option<usize>) {
    ctx.occupied.retain(|s| Some(*s) != cleared_slot);
    let Some(slot) = pick_grid_slot(grid_cells(ctx.config), ctx.occupied, cleared_slot) else { return };
    ctx.occupied.push(slot);
    let (position, lifetime) = (grid_position(slot, ctx.config), ctx.lifetime());
    ctx.spawn_target(position, lifetime).insert(Slot(slot));
}

fn grid_cells(config: &DrillConfig) -> usize {
    (config.grid_cols * config.grid_rows) as usize
}

/// Cells are centered on the crosshair, `grid_spacing` degrees apart.
fn grid_position(slot: usize, config: &DrillConfig) -> Vec3 {
    let cols = config.grid_cols as usize;
    let col = (slot % cols) as f32 - (config.grid_cols as f32 - 1.0) * 0.5;
    let row = (slot / cols) as f32 - (config.grid_rows as f32 - 1.0) * 0.5;
    direction((col * config.grid_spacing).to_radians(), (row * config.grid_spacing).to_radians())
        * TARGET_DISTANCE
}

/// A random free grid cell. The cell just cleared is avoided so every kill forces a new flick,
/// unless it is the only free one (a 1x1 grid).
fn pick_grid_slot(cells: usize, occupied: &[usize], cleared_slot: Option<usize>) -> Option<usize> {
    let free: Vec<usize> = (0..cells).filter(|s| !occupied.contains(s) && Some(*s) != cleared_slot).collect();
    if free.is_empty() {
        return cleared_slot.filter(|s| *s < cells && !occupied.contains(s));
    }
    Some(free[fastrand::usize(..free.len())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_cell_grid_respawns_in_place() {
        assert_eq!(pick_grid_slot(1, &[], Some(0)), Some(0));
        assert_eq!(pick_grid_slot(1, &[], None), Some(0));
        // With other cells free, the cleared one is never reused.
        for _ in 0..50 {
            assert_eq!(pick_grid_slot(2, &[], Some(0)), Some(1));
        }
        assert_eq!(pick_grid_slot(2, &[0, 1], None), None);
    }

    #[test]
    fn custom_grid_is_centered_and_spaced() {
        let config = DrillConfig { grid_cols: 5, grid_rows: 1, grid_spacing: 10.0, ..DrillConfig::default() };
        let middle = grid_position(2, &config).normalize();
        assert!(middle.angle_between(Vec3::NEG_Z) < 1e-4);
        let left = grid_position(0, &config).normalize();
        assert!((left.angle_between(Vec3::NEG_Z).to_degrees() - 20.0).abs() < 1e-3);
    }
}
