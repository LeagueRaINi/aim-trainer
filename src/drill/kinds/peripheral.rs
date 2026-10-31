//! Peripheral: brief dots far off-center that have to be spotted before the flick.

use crate::drill::{
    DrillConfig,
    def::{Category, DecayRule, DrillDef, Environment, TtkFrom},
    params::ParamId,
    stats::ScoreModel,
};

pub const DEF: DrillDef = DrillDef {
    name: "Peripheral",
    blurb: "Brief dots far off-center. Spot + flick.",
    category: Category::Eyes,
    base_radius: 0.2,
    decay: DecayRule::Own,
    score: ScoreModel::Flick,
    ttk_from: TtkFrom::Spawn,
    click_kills: true,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: true,
    params: &[ParamId::YawRange, ParamId::PitchRange, ParamId::MinFlick],
    tune_defaults: tune,
};

/// Far off-center targets that expire quickly.
fn tune(config: &mut DrillConfig) {
    (config.yaw_range, config.min_flick, config.lifetime) = (45.0, 18.0, 1.1);
}
