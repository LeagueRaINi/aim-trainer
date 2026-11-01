//! Head level: head-sized flick targets that always sit at head height. Spawns like
//! [`microflick`](super::microflick); its flat pitch range keeps targets at eye height.

use super::microflick::{respawn_flick, spawn_flick};
use crate::drill::DrillConfig;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_spawn_next};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;

pub const DEF: DrillDef = DrillDef {
    name: "Head level",
    blurb: |_| "Head-sized dots, always at head height.".into(),
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
    adaptive: true,
    endless: false,
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

    use crate::camera::ViewCone;
    use crate::drill::geometry::direction;
    use crate::drill::kinds::microflick::flick_position;
    use crate::drill::{DrillConfig, DrillKind};

    #[test]
    fn head_level_targets_stay_at_eye_height() {
        let (config, view, margin) = (DrillConfig::defaults(DrillKind::HeadLevel), ViewCone::default(), 0.04);
        // Straight ahead, and turned far past the targets' yaw range, looking up or down.
        for aim in [Vec3::NEG_Z, direction(1.4, 0.2), direction(-1.2, -0.3)] {
            for _ in 0..50 {
                let target = flick_position(aim, &view, margin, &config);
                assert!(target.y.abs() < 1e-4, "aim {aim}: {target} off head height");
                assert!(view.contains(aim, target.normalize(), margin), "aim {aim}: {target} off screen");
            }
        }
    }
}
