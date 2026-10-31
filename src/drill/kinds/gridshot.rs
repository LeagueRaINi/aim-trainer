//! Gridshot: several dots on a grid; a kill respawns one in another cell.

use crate::drill::{
    def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_tuning},
    params::ParamId,
    stats::ScoreModel,
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
    params: &[ParamId::GridCols, ParamId::GridRows, ParamId::GridSpacing, ParamId::GridActive],
    tune_defaults: no_tuning,
};
