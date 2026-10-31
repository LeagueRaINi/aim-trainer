//! Head level: head-sized flick targets that always sit at head height. Spawns like
//! [`microflick`](super::microflick); its flat pitch range keeps targets at eye height.

use crate::drill::{
    DrillConfig,
    def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_spawn_next},
    params::ParamId,
    stats::ScoreModel,
};

use super::microflick::{respawn_flick, spawn_flick};

pub const DEF: DrillDef = DrillDef {
    name: "Head level",
    blurb: "Head-sized dots, always at head height.",
    category: Category::Tactical,
    base_radius: 0.14,
    decay: DecayRule::Global,
    score: ScoreModel::Flick,
    ttk_from: TtkFrom::Spawn,
    click_kills: true,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: true,
    flick_analysis: true,
    params: &[ParamId::YawRange, ParamId::MinFlick],
    tune_defaults: tune,
    spawn_initial: spawn_flick,
    respawn: respawn_flick,
    spawn_next: no_spawn_next,
};

/// Wide and flat: the pitch range of 0 keeps targets exactly at head height.
fn tune(config: &mut DrillConfig) {
    (config.yaw_range, config.pitch_range, config.min_flick) = (35.0, 0.0, 6.0);
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use crate::drill::{DrillConfig, DrillKind, kinds::microflick::flick_position};

    #[test]
    fn head_level_targets_stay_at_eye_height() {
        let config = DrillConfig::defaults(DrillKind::HeadLevel);
        for _ in 0..50 {
            assert!(flick_position(Vec3::NEG_Z, &config).y.abs() < 1e-4);
        }
    }
}
