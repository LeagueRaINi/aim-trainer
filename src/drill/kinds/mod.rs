//! One module per built-in drill. Each defines its [`DrillDef`](super::def::DrillDef) as `DEF`
//! (data plus spawn hooks) and, if it has per-frame behaviour, a `register` adding its systems
//! to the [`DrillSet`](super::DrillSet)s.

use bevy::prelude::*;

pub mod calm_aim;
pub mod go_no_go;
pub mod gridshot;
pub mod head_level;
pub mod microflick;
pub mod moving_gridshot;
pub mod peek;
pub mod peripheral;
pub mod pokeball;
pub mod reaction;
pub mod reaction_test;
pub mod smooth_tracking;
pub mod survival;
pub mod switch_tracking;
pub mod target_switch;
pub mod tracking;
pub mod wall_flicks;

pub mod counter_strafe;
pub mod head_taps;
pub mod strafing_bot;

/// Adds every drill's own systems.
pub(super) fn register(app: &mut App) {
    microflick::register(app);
    tracking::register(app);
    peek::register(app);
    reaction::register(app);
    moving_gridshot::register(app);
    survival::register(app);
    switch_tracking::register(app);
    target_switch::register(app);
    strafing_bot::register(app);
    counter_strafe::register(app);
    reaction_test::register(app);
    go_no_go::register(app);
    smooth_tracking::register(app);
    calm_aim::register(app);
}
