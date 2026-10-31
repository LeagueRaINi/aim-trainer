//! Peripheral: brief dots far off-center that have to be spotted before the flick.

use bevy::prelude::*;

use crate::camera::ViewCone;
use crate::drill::DrillConfig;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom};
use crate::drill::geometry::{away_from, direction, random_range, random_sign};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::SpawnCtx;

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
    adaptive: false,
    params: &[ParamId::YawRange, ParamId::PitchRange, ParamId::MinFlick],
    tune_defaults: tune,
    spawn_initial,
    respawn,
    spawn_next,
};

/// Far off-center targets that expire quickly.
fn tune(config: &mut DrillConfig) {
    (config.yaw_range, config.min_flick, config.lifetime) = (45.0, 18.0, 1.1);
}

fn spawn_initial(ctx: &mut SpawnCtx) {
    ctx.next.0 = Some(0.4);
}

/// The next dot appears after a short pause.
fn respawn(ctx: &mut SpawnCtx, _: Option<usize>) {
    ctx.next.0 = Some(ctx.now + random_range(0.25, 0.6));
}

fn spawn_next(ctx: &mut SpawnCtx) {
    let (position, lifetime) = (peripheral_position(ctx.aim, &ctx.view, ctx.margin(), ctx.config), ctx.lifetime());
    ctx.spawn_target(position, lifetime);
}

/// At least `min_flick` degrees to the side, so the eyes have to catch it first, and on screen.
fn peripheral_position(aim: Vec3, view: &ViewCone, margin: f32, config: &DrillConfig) -> Vec3 {
    let near = config.min_flick.min(config.yaw_range);
    away_from(aim, near * 0.8, view, margin, || {
        direction(
            (random_sign() * random_range(near, config.yaw_range)).to_radians(),
            random_range(-config.pitch_range, config.pitch_range).to_radians(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::DrillKind;

    #[test]
    fn peripheral_targets_appear_off_center() {
        let config = DrillConfig::defaults(DrillKind::Peripheral);
        for _ in 0..50 {
            let dir = peripheral_position(Vec3::NEG_Z, &ViewCone::default(), 0.04, &config).normalize();
            assert!(ViewCone::default().contains(Vec3::NEG_Z, dir, 0.04));
            assert!(dir.angle_between(Vec3::NEG_Z).to_degrees() >= config.min_flick - 1e-3);
        }
    }
}
