//! Pokeball: a few large static targets spread over a wide wall. They are hard to miss, so the
//! score is how fast the crosshair gets from one to the next. Spawns like
//! [`wall_flicks`](super::wall_flicks), with its own wall and target size.

use std::borrow::Cow;

use super::wall_flicks::{count, respawn, spawn_initial};
use crate::drill::DrillConfig;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_spawn_next};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;

pub const DEF: DrillDef = DrillDef {
    name: "Pokeball",
    blurb,
    category: Category::Aim,
    // 3.4° across its radius: three times Wall flicks' dot.
    base_radius: 0.6,
    decay: DecayRule::Global,
    score: ScoreModel::Flick,
    ttk_from: TtkFrom::PreviousKill,
    click_kills: true,
    environment: Environment::Open,
    first_flick_probe: false,
    // Targets this large are hit with almost any sensitivity and anywhere on them, so neither the
    // finder nor the weak-spot analysis (first-click precision) learns anything from these runs.
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    endless: false,
    params: &[ParamId::GridActive, ParamId::YawRange, ParamId::PitchRange, ParamId::MinFlick],
    tune_defaults: tune,
    spawn_initial,
    respawn,
    spawn_next: no_spawn_next,
};

/// Three big targets on a wall 72° wide and 24° tall, each at least 15° from the one just shot:
/// long swipes that end on something easy to hit. At Valorant's 103° the wall spans the middle
/// 58% of the screen's width, so all of it is in view from its middle; from one end the other is
/// out of sight, which is why a new target only appears where the player can see it.
fn tune(config: &mut DrillConfig) {
    config.grid_active = 3;
    config.yaw_range = 36.0;
    config.pitch_range = 12.0;
    config.min_flick = 15.0;
}

fn blurb(config: &DrillConfig) -> Cow<'static, str> {
    format!("{}, far apart. How fast you get from one to the next.", count(config, "big target", "big targets")).into()
}
