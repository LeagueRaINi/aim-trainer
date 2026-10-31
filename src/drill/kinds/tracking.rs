//! Tracking: keep the crosshair on a strafing dot.

use crate::drill::{
    def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_tuning},
    stats::ScoreModel,
};

pub const DEF: DrillDef = DrillDef {
    name: "Tracking",
    blurb: "Keep the crosshair on a strafing dot.",
    category: Category::Aim,
    base_radius: 0.45,
    decay: DecayRule::None,
    score: ScoreModel::Tracking,
    ttk_from: TtkFrom::Spawn,
    click_kills: false,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: true,
    flick_analysis: false,
    tune_defaults: no_tuning,
};
