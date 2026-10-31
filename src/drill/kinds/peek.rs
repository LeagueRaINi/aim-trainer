//! Peeks: targets swing out from behind cover, hold briefly and retreat.

use crate::drill::{
    def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_tuning},
    params::ParamId,
    stats::ScoreModel,
};

pub const DEF: DrillDef = DrillDef {
    name: "Peeks",
    blurb: "Hold an angle: enemies swing out of cover.",
    category: Category::Tactical,
    base_radius: 0.2,
    decay: DecayRule::None,
    score: ScoreModel::Flick,
    ttk_from: TtkFrom::Spawn,
    click_kills: true,
    environment: Environment::Cover,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    params: &[ParamId::PeekSpeedMin, ParamId::PeekSpeedMax, ParamId::HoldMin, ParamId::HoldMax],
    tune_defaults: no_tuning,
};
