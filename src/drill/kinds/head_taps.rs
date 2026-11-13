//! Head taps: bots stand still at head height, a few at a time, at different distances. A tap on
//! the head kills, the body takes four shots, and shots fired before the
//! [rifle](crate::drill::rifle) has settled scatter: first-bullet accuracy and tap spacing.
//!
//! The bots stand in lanes fanned out in front of the player (their [`Slot`]s), one bot to a
//! lane, so they never hide each other and a killed bot's replacement appears somewhere else.

use bevy::prelude::*;

use crate::camera::ViewCone;
use crate::drill::DrillConfig;
use crate::drill::bot::spawn_bot;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_spawn_next};
use crate::drill::geometry::{pull_in, random_range};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{Slot, SpawnCtx};

/// Distances (m) bots stand at: a head is 1.3° wide to its edge at the nearest, 0.6° at the
/// farthest.
const DEPTH: (f32, f32) = (6.0, 14.0);
/// Lanes the horizontal range is divided into.
const LANES: usize = 5;
/// Degrees a bot keeps from its lane's edges, so neighbours stand at least twice this apart:
/// clear of each other's arms (2.4° from a bot's middle at the nearest distance).
const LANE_PAD: f32 = 2.5;
/// Degrees a head keeps from the screen's edges, beyond its own size.
const EDGE_ROOM: f32 = 1.5;
/// Random places tried for a new bot before the last one is moved into view.
const TRIES: usize = 32;

pub const DEF: DrillDef = DrillDef {
    name: "Head taps",
    blurb: |_| {
        "Bots at head height: the head takes one shot, the body four. Tap, and let the rifle settle. Approximates Valorant."
            .into()
    },
    category: Category::Tactical,
    base_radius: 0.14,
    decay: DecayRule::None,
    score: ScoreModel::Taps,
    ttk_from: TtkFrom::PreviousKill,
    click_kills: false,
    environment: Environment::Range,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    endless: false,
    params: &[ParamId::GridActive, ParamId::YawRange, ParamId::MinFlick],
    tune_defaults: tune,
    spawn_initial,
    respawn,
    spawn_next: no_spawn_next,
};

/// Two bots at a time within 25° to each side, the next at least 4° from the crosshair.
fn tune(config: &mut DrillConfig) {
    (config.grid_active, config.yaw_range, config.min_flick) = (2, 25.0, 4.0);
}

fn spawn_initial(ctx: &mut SpawnCtx) {
    // A lane always stays free for the next bot.
    for _ in 0..ctx.config.grid_active.clamp(1, LANES as u32 - 1) {
        spawn_in_free_lane(ctx);
    }
}

/// The killed bot's lane still counts as taken, so its replacement stands somewhere else.
fn respawn(ctx: &mut SpawnCtx, _: Option<usize>) {
    spawn_in_free_lane(ctx);
}

fn spawn_in_free_lane(ctx: &mut SpawnCtx) {
    let (lane, head) = place(ctx.aim, &ctx.view, ctx.radius(), ctx.config, ctx.occupied);
    ctx.occupied.push(lane);
    spawn_bot(ctx, head).insert(Slot(lane));
}

/// The head of a bot standing somewhere in `lane` (counted from the left), at a random distance.
fn head_in(lane: usize, config: &DrillConfig) -> Vec3 {
    let width = 2.0 * config.yaw_range / LANES as f32;
    let centre = config.yaw_range - (lane as f32 + 0.5) * width;
    let play = (width * 0.5 - LANE_PAD).max(0.0);
    let (sin, cos) = (centre + random_range(-play, play)).to_radians().sin_cos();
    // Yaw turns left; a height of exactly 0 is eye height.
    Vec3::new(-sin, 0.0, -cos) * random_range(DEPTH.0, DEPTH.1)
}

/// How far (radians) a head's centre must stay from the screen's edges to be seen whole.
fn margin(radius: f32, head: Vec3) -> f32 {
    (radius / head.length()).atan() + EDGE_ROOM.to_radians()
}

/// A lane not in `occupied` and where in it the new bot's head goes: on screen and at least the
/// minimum flick from the crosshair. If no place like that turns up (the player has turned away
/// from the lanes), the last one tried is moved into view, still at eye height.
fn place(aim: Vec3, view: &ViewCone, radius: f32, config: &DrillConfig, occupied: &[usize]) -> (usize, Vec3) {
    let free = |lane: &usize| !occupied.contains(lane);
    let count = (0..LANES).filter(free).count().max(1);
    let max_cos = config.min_flick.to_radians().cos();
    let pick = || {
        let lane = (0..LANES).filter(free).nth(fastrand::usize(..count)).unwrap_or(0);
        (lane, head_in(lane, config))
    };
    let mut tried = pick();
    for _ in 0..TRIES {
        let dir = tried.1.normalize();
        if dir.dot(aim) < max_cos && view.contains(aim, dir, margin(radius, tried.1)) {
            return tried;
        }
        tried = pick();
    }
    let (lane, head) = tried;
    let pulled = pull_in(aim, head.normalize(), view, margin(radius, head));
    (lane, Vec3::new(pulled.x, 0.0, pulled.z).normalize_or(Vec3::NEG_Z) * head.length())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::DrillKind;
    use crate::drill::geometry::direction;

    /// Wherever the player looks (within reason), new bots stand at eye height, at a distance in
    /// range, and their heads are on screen.
    #[test]
    fn bots_appear_on_screen_with_their_heads_at_eye_height() {
        let (config, view, radius) = (DrillConfig::defaults(DrillKind::HeadTaps), ViewCone::default(), DEF.base_radius);
        // Straight ahead, off to a side looking up or down a little, and turned past the lanes.
        for aim in
            [Vec3::NEG_Z, direction(0.3, 0.12), direction(-0.4, -0.15), direction(1.3, 0.1), direction(-2.0, 0.0)]
        {
            for seed in 0..40 {
                fastrand::seed(seed);
                let (lane, head) = place(aim, &view, radius, &config, &[seed as usize % LANES]);
                assert_eq!(head.y, 0.0, "aim {aim}: head at {head}");
                assert!((DEPTH.0 - 1e-3..=DEPTH.1 + 1e-3).contains(&head.length()), "{head}");
                let seen = view.contains(aim, head.normalize(), (radius / head.length()).atan());
                assert!(seen, "aim {aim}: head at {head} off screen");
                assert_ne!(lane, seed as usize % LANES, "placed in a taken lane");
            }
        }
    }

    /// With the player looking at the lanes, a new bot is a flick away from the crosshair.
    #[test]
    fn new_bots_are_a_flick_away() {
        let (config, view) = (DrillConfig::defaults(DrillKind::HeadTaps), ViewCone::default());
        for seed in 0..100 {
            fastrand::seed(seed);
            let aim = direction(random_range(-0.4, 0.4), 0.0);
            let (_, head) = place(aim, &view, DEF.base_radius, &config, &[]);
            let off = head.normalize().angle_between(aim).to_degrees();
            assert!(off >= config.min_flick - 1e-3, "seed {seed}: only {off}° from the crosshair");
        }
    }

    /// Bots in different lanes stand apart, whatever their distances: none hides another.
    #[test]
    fn lanes_keep_bots_apart() {
        let config = DrillConfig::defaults(DrillKind::HeadTaps);
        let yaw = |head: Vec3| (-head.x).atan2(-head.z).to_degrees();
        for seed in 0..50 {
            fastrand::seed(seed);
            let yaws: Vec<f32> = (0..LANES).map(|lane| yaw(head_in(lane, &config))).collect();
            // Lanes are counted from the left: the yaw falls from one to the next.
            for pair in yaws.windows(2) {
                assert!(pair[0] - pair[1] >= 2.0 * LANE_PAD - 1e-3, "seed {seed}: {yaws:?}");
            }
            assert!(yaws[0] <= config.yaw_range && yaws[LANES - 1] >= -config.yaw_range, "{yaws:?}");
        }
    }
}
