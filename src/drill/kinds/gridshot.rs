//! Gridshot: several dots on a grid; a kill respawns one in another cell.

use std::borrow::Cow;

use bevy::prelude::*;

use crate::drill::DrillConfig;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_spawn_next};
use crate::drill::geometry::{CEILING_PITCH, FLOOR_PITCH, TARGET_DISTANCE, direction};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{Slot, SpawnCtx};

/// Tallest a grid may be (bottom row to top row, degrees): every row between floor and ceiling.
pub(crate) const MAX_GRID_HEIGHT: f32 = CEILING_PITCH - FLOOR_PITCH;

pub const DEF: DrillDef = DrillDef {
    name: "Gridshot",
    blurb,
    category: Category::Aim,
    base_radius: 0.3,
    decay: DecayRule::Global,
    score: ScoreModel::Flick,
    ttk_from: TtkFrom::PreviousKill,
    click_kills: true,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: true,
    flick_analysis: true,
    adaptive: false,
    endless: false,
    params: &[ParamId::GridCols, ParamId::GridRows, ParamId::GridSpacing, ParamId::GridActive],
    tune_defaults: tune,
    spawn_initial,
    respawn,
    spawn_next: no_spawn_next,
};

/// Small dots, 8° to 40° apart: big, neighbouring dots on a 3x3 grid let any sensitivity score
/// about the same, which leaves the finder nothing to compare.
pub(super) fn tune(config: &mut DrillConfig) {
    config.grid_cols = 5;
    config.grid_rows = 4;
    config.grid_spacing = 8.0;
    config.grid_active = 3;
}

fn blurb(config: &DrillConfig) -> Cow<'static, str> {
    format!("{}. Speed + accuracy.", dots_on_grid(config)).into()
}

/// The dots and grid this config actually plays, e.g. `3 dots on a 5x4 grid`.
pub(super) fn dots_on_grid(config: &DrillConfig) -> String {
    let dots = match config.grid_active {
        1 => "1 dot".to_string(),
        n => format!("{n} dots"),
    };
    format!("{dots} on a {}x{} grid", config.grid_cols, config.grid_rows)
}

/// `grid_active` dots in random cells.
fn spawn_initial(ctx: &mut SpawnCtx) {
    let lifetime = ctx.lifetime();
    for slot in initial_slots(ctx.config) {
        let position = grid_position(slot, ctx.config);
        ctx.spawn_target(position, lifetime).insert(Slot(slot));
    }
}

/// A new dot in another free cell.
fn respawn(ctx: &mut SpawnCtx, cleared_slot: Option<usize>) {
    let Some(slot) = next_slot(ctx, cleared_slot) else { return };
    let (position, lifetime) = (grid_position(slot, ctx.config), ctx.lifetime());
    ctx.spawn_target(position, lifetime).insert(Slot(slot));
}

/// `grid_active` distinct random cells for the first dots.
pub(super) fn initial_slots(config: &DrillConfig) -> Vec<usize> {
    let mut slots: Vec<usize> = (0..grid_cells(config)).collect();
    fastrand::shuffle(&mut slots);
    slots.truncate(config.grid_active as usize);
    slots
}

/// The cell for the dot replacing the one in `cleared_slot`, marked occupied.
pub(super) fn next_slot(ctx: &mut SpawnCtx, cleared_slot: Option<usize>) -> Option<usize> {
    ctx.occupied.retain(|s| Some(*s) != cleared_slot);
    let slot = pick_grid_slot(grid_cells(ctx.config), ctx.occupied, cleared_slot)?;
    ctx.occupied.push(slot);
    Some(slot)
}

fn grid_cells(config: &DrillConfig) -> usize {
    (config.grid_cols * config.grid_rows) as usize
}

/// Cells are `grid_spacing` degrees apart, centred on the crosshair, except that a grid too tall
/// for that is raised until its bottom row sits on the floor (a lower row would sink into it).
/// Returns the cell's (yaw, pitch) in degrees.
pub(super) fn cell_angles(slot: usize, config: &DrillConfig) -> Vec2 {
    let cols = config.grid_cols as usize;
    let col = (slot % cols) as f32 - (config.grid_cols as f32 - 1.0) * 0.5;
    let row = (slot / cols) as f32 - (config.grid_rows as f32 - 1.0) * 0.5;
    let half_height = (config.grid_rows as f32 - 1.0) * 0.5 * config.grid_spacing;
    let centre = (FLOOR_PITCH + half_height).max(0.0);
    Vec2::new(col * config.grid_spacing, centre + row * config.grid_spacing)
}

/// The target position for (yaw, pitch) in degrees.
pub(super) fn position_at(angles: Vec2) -> Vec3 {
    direction(angles.x.to_radians(), angles.y.to_radians()) * TARGET_DISTANCE
}

fn grid_position(slot: usize, config: &DrillConfig) -> Vec3 {
    position_at(cell_angles(slot, config))
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
    use crate::metrics::flick_probe::angles;

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

    /// Tall grids used to put their lower rows into the floor (5 rows at 7° already did). Every
    /// cell of every grid the editor allows sits between floor and ceiling, and grids that fit
    /// stay centred on the crosshair.
    #[test]
    fn every_cell_stays_between_floor_and_ceiling() {
        let (min, max) = ParamId::GridSpacing.spec().hard;
        for rows in 1..=9 {
            for spacing in [min, 2.0, 7.0, 8.0, 12.5, 20.0, max] {
                let mut config = DrillConfig { grid_rows: rows, grid_spacing: spacing, ..DrillConfig::default() };
                config.sanitize();
                let pitch = |slot| angles(grid_position(slot, &config).normalize()).y;
                for slot in 0..grid_cells(&config) {
                    let at = pitch(slot);
                    assert!(
                        (FLOOR_PITCH - 1e-3..=CEILING_PITCH + 1e-3).contains(&at),
                        "{rows} rows at {spacing}°: cell {slot} at pitch {at}"
                    );
                }
                let half_height = (rows - 1) as f32 * 0.5 * config.grid_spacing;
                if half_height <= -FLOOR_PITCH {
                    assert!((pitch(0) + half_height).abs() < 1e-3, "{rows} rows at {spacing}° not centred");
                }
            }
        }
    }
}
