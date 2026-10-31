//! Head level: head-sized flick targets that always sit at head height.

use crate::drill::{
    DrillConfig,
    def::{Category, DecayRule, DrillDef, Environment, TtkFrom},
    params::ParamId,
    stats::ScoreModel,
};

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
};

/// Wide and flat: the pitch range of 0 keeps targets exactly at head height.
fn tune(config: &mut DrillConfig) {
    (config.yaw_range, config.pitch_range, config.min_flick) = (35.0, 0.0, 6.0);
}
