//! Microflick: one small dot at a time, anywhere in the flick area (or, with flick constraints, in a trained direction and distance).

use crate::drill::{
    def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_tuning},
    params::ParamId,
    stats::ScoreModel,
};

pub const DEF: DrillDef = DrillDef {
    name: "Microflick",
    blurb: "One small dot at a time. Flick precision.",
    category: Category::Aim,
    base_radius: 0.2,
    decay: DecayRule::Global,
    score: ScoreModel::Flick,
    ttk_from: TtkFrom::Spawn,
    click_kills: true,
    environment: Environment::Open,
    first_flick_probe: true,
    classic_finder: true,
    flick_analysis: true,
    params: &[ParamId::YawRange, ParamId::PitchRange, ParamId::MinFlick],
    tune_defaults: no_tuning,
};
