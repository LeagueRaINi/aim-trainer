//! One module per built-in drill. Each defines its [`DrillDef`](super::def::DrillDef) as `DEF`
//! (data plus spawn hooks) and, if it has per-frame behaviour, a `register` adding its systems
//! to the [`DrillSet`](super::DrillSet)s.

use bevy::prelude::*;

pub mod gridshot;
pub mod head_level;
pub mod microflick;
pub mod peek;
pub mod peripheral;
pub mod reaction;
pub mod tracking;

/// Adds every drill's own systems.
pub(super) fn register(app: &mut App) {
    microflick::register(app);
    tracking::register(app);
    peek::register(app);
    reaction::register(app);
}
