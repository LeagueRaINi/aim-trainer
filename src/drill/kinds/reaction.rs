//! Reaction: click the instant the dot lights up.

use crate::drill::{
    def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_tuning},
    stats::ScoreModel,
};

pub const DEF: DrillDef = DrillDef {
    name: "Reaction",
    blurb: "Click the instant the dot lights up.",
    category: Category::Eyes,
    base_radius: 0.5,
    decay: DecayRule::None,
    score: ScoreModel::Reaction,
    ttk_from: TtkFrom::Spawn,
    click_kills: false,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    tune_defaults: no_tuning,
};
