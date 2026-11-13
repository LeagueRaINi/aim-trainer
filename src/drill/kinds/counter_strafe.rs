//! Counter-strafe: stop, then shoot. The player stands behind a pillar and strafes with A and D
//! ([`movement`](crate::drill::movement)). A bot waits behind the pillar, out of sight from
//! cover; strafing out to one side brings it into view. Shots fired while still moving scatter,
//! so: out, stop, tap the head. The next bot can only be seen from the other side, so the way
//! there is back through cover and on.
//!
//! Owns the pillar (the [`Environment::Pillar`] scenery, on the [range](crate::drill::range)'s
//! deck), which replays of this drill show too.

use bevy::prelude::*;

use crate::AppState;
use crate::camera::{PlayerCamera, ViewCone};
use crate::drill::bot::{BODY_HALF_WIDTH, Bot, EYE_HEIGHT, spawn_bot};
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_respawn, no_spawn_next, no_tuning};
use crate::drill::geometry::{random_range, ray_box};
use crate::drill::movement::PlayerMove;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{NextSpawn, SpawnCtx, TargetAssets};
use crate::drill::{DrillSet, Run, range};

/// The pillar: 2 m wide, 4 m in front of the start position, standing on the deck and reaching
/// above the eye. Its back corners are what the eye sees past once it has left cover.
pub(crate) const PILLAR_MIN: Vec3 = Vec3::new(-1.0, -EYE_HEIGHT, -4.6);
pub(crate) const PILLAR_MAX: Vec3 = Vec3::new(1.0, 0.9, -4.0);
/// How far (m) the player can strafe to each side of the start position.
const LANE: f32 = 2.6;
/// The player is in cover within this many metres of the start position: the next bot takes its
/// post while the player is here, and can't be seen from anywhere in here.
const COVER: f32 = 0.5;
/// Distances (m) bots stand at.
const DEPTH: (f32, f32) = (9.0, 14.0);
/// How far (m) past the pillar's edge the eye has to be for a bot's whole head to show: from a
/// shoulder peek to a wide one.
const PEEK: (f32, f32) = (0.35, 1.3);

pub const DEF: DrillDef = DrillDef {
    name: "Counter-strafe",
    blurb: |_| {
        "Strafe out of cover, stop, then tap the head: shots on the move scatter. Bots alternate sides. Approximates Valorant."
            .into()
    },
    category: Category::Tactical,
    base_radius: 0.14,
    decay: DecayRule::None,
    score: ScoreModel::Taps,
    ttk_from: TtkFrom::Spawn,
    click_kills: false,
    environment: Environment::Pillar,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    endless: false,
    params: &[],
    tune_defaults: no_tuning,
    spawn_initial: wait_for_the_run,
    respawn: no_respawn,
    spawn_next: no_spawn_next,
};

pub(super) fn register(app: &mut App) {
    app.add_systems(OnEnter(AppState::Playing), set_up)
        .add_systems(Update, post_bot.in_set(DrillSet::Spawn).run_if(resource_exists::<Posts>));
}

/// Which side of the pillar the next bot is seen from: 1 the right, -1 the left. The first is on
/// the right, then they alternate, so the player always knows which way to go.
#[derive(Resource)]
struct Posts {
    next_side: f32,
}

/// Distance along the ray to the pillar, if the ray hits it.
pub(crate) fn pillar_hit(origin: Vec3, dir: Vec3) -> Option<f32> {
    ray_box(origin, dir, PILLAR_MIN, PILLAR_MAX)
}

/// Gives the drill its moving player and its posts; any other drill has neither.
fn set_up(mut commands: Commands, run: Res<Run>) {
    if range::player_moves(run.kind()) {
        commands.insert_resource(PlayerMove::new(LANE));
        commands.insert_resource(Posts { next_side: 1.0 });
    } else {
        commands.remove_resource::<Posts>();
    }
}

/// Nothing appears during the countdown: [`post_bot`] posts the first bot when the run goes live.
fn wait_for_the_run(_: &mut SpawnCtx) {}

/// Where the head (of `radius`) of a bot stands that is seen from `side` of the pillar, `depth`
/// metres out: its whole head shows once the eye is `peek` metres past the pillar's edge, and
/// none of the bot shows from cover.
fn post(side: f32, depth: f32, peek: f32, radius: f32) -> Vec3 {
    // Mirrored to the right side. The sight line from the eye at the reveal point past the
    // pillar's back corner, carried on to the bot's depth; the head's centre is a radius
    // (measured across the line) further out.
    let (corner, back) = (PILLAR_MAX.x, -PILLAR_MIN.z);
    let reveal = corner + peek;
    let across = (corner - reveal).hypot(back) / back;
    let seen_from = reveal + (corner - reveal) * depth / back + radius * across;
    // From the edge of cover the eye sees past the pillar's front corner: the widest part of the
    // bot stays behind that line, even if that asks for a wider peek.
    let front = -PILLAR_MAX.z;
    let shadow = COVER + (corner - COVER) * depth / front;
    let hidden = shadow - radius.max(BODY_HALF_WIDTH) * (corner - COVER).hypot(front) / front - 0.05;
    Vec3::new(side * seen_from.min(hidden), 0.0, -depth)
}

/// Posts the next bot once the previous one is dead and the player is back in cover (which is
/// where the run starts), alternating sides.
#[allow(clippy::too_many_arguments)]
fn post_bot(
    mut commands: Commands,
    run: Res<Run>,
    mut posts: ResMut<Posts>,
    player: Res<PlayerMove>,
    assets: Res<TargetAssets>,
    mut next: ResMut<NextSpawn>,
    view: Res<ViewCone>,
    camera: Single<&Transform, With<PlayerCamera>>,
    bots: Query<(), With<Bot>>,
) {
    if !run.live() || !bots.is_empty() || player.x().abs() > COVER {
        return;
    }
    let side = posts.next_side;
    posts.next_side = -side;
    let mut occupied = Vec::new();
    let mut ctx = SpawnCtx {
        commands: &mut commands,
        assets: &assets,
        config: &run.drill.config,
        now: run.elapsed(),
        aim: camera.forward().as_vec3(),
        view: *view,
        live: true,
        occupied: &mut occupied,
        next: &mut next,
    };
    let head = post(side, random_range(DEPTH.0, DEPTH.1), random_range(PEEK.0, PEEK.1), ctx.radius());
    spawn_bot(&mut ctx, head);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::params::ParamId;

    /// Points on the outline of a bot whose head (of `radius`) is at `head`, as seen from `eye`:
    /// around the head, and the body's full width at the shoulders, waist, hands and feet.
    fn outline(eye: Vec3, head: Vec3, radius: f32) -> (Vec<Vec3>, Vec<Vec3>) {
        let side = (head - eye).cross(Vec3::Y).normalize();
        let around = (0..16).map(|step| {
            let (sin, cos) = (step as f32 / 16.0 * std::f32::consts::TAU).sin_cos();
            head + (side * cos + Vec3::Y * sin) * radius
        });
        let body = [-0.2, -0.6, -0.9, -1.6]
            .into_iter()
            .flat_map(|y| [-1.0, 1.0].map(|way| head + side * way * BODY_HALF_WIDTH + Vec3::Y * y));
        (around.collect(), body.collect())
    }

    /// Whether the pillar is between `eye` and `point`.
    fn covered(eye: Vec3, point: Vec3) -> bool {
        pillar_hit(eye, (point - eye).normalize()).is_some_and(|at| at < eye.distance(point))
    }

    /// From anywhere in cover (the start position included) nothing of a posted bot shows; from
    /// its reveal point on its side the whole head does, and shots at it get past the pillar;
    /// a little short of that point part of the head is still covered. For every side, depth,
    /// peek width and head size the drill uses.
    #[test]
    fn bots_are_hidden_from_cover_and_seen_after_the_peek() {
        let sizes = [1.0, ParamId::Size.spec().soft.0, ParamId::Size.spec().soft.1];
        for (side, depth, peek, size) in [-1.0, 1.0].into_iter().flat_map(|side| {
            [DEPTH.0, 11.0, DEPTH.1].into_iter().flat_map(move |depth| {
                [PEEK.0, 0.8, PEEK.1].into_iter().flat_map(move |peek| sizes.map(|size| (side, depth, peek, size)))
            })
        }) {
            let radius = DEF.base_radius * size;
            let head = post(side, depth, peek, radius);
            assert_eq!(head.y, 0.0);
            let case = format!("side {side}, depth {depth}, peek {peek}, size {size}: head at {head}");
            for from in [0.0, -COVER, COVER] {
                let eye = Vec3::new(from, 0.0, 0.0);
                let (around, body) = outline(eye, head, radius);
                assert!(around.iter().chain(&body).all(|point| covered(eye, *point)), "{case} shows from {from} m");
            }
            // Just past the reveal point, inside the lane.
            let reveal = PILLAR_MAX.x + peek;
            let eye = Vec3::new(side * (reveal + 0.03), 0.0, 0.0);
            assert!(eye.x.abs() < LANE);
            let (around, _) = outline(eye, head, radius);
            assert!(around.iter().all(|point| !covered(eye, *point)), "{case} still covered from {} m", eye.x);
            let short = Vec3::new(side * (reveal - 0.25), 0.0, 0.0);
            let (around, _) = outline(short, head, radius);
            assert!(around.iter().any(|point| covered(short, *point)), "{case} shows early, from {} m", short.x);
        }
    }

    /// Even a head far larger than the editor's sliders allow stays hidden from cover.
    #[test]
    fn oversized_heads_stay_hidden_from_cover() {
        let radius = DEF.base_radius * ParamId::Size.spec().hard.1;
        for (side, depth) in [(-1.0, DEPTH.0), (1.0, DEPTH.0), (1.0, DEPTH.1)] {
            let head = post(side, depth, PEEK.0, radius);
            for from in [0.0, -COVER, COVER] {
                let eye = Vec3::new(from, 0.0, 0.0);
                let (around, body) = outline(eye, head, radius);
                assert!(around.iter().chain(&body).all(|point| covered(eye, *point)), "{head} shows from {from} m");
            }
        }
    }

    /// The pillar stands on the deck and is tall enough to cover a standing bot at any depth.
    #[test]
    fn the_pillar_covers_from_the_deck_to_above_the_head() {
        assert_eq!(PILLAR_MIN.y, -EYE_HEIGHT);
        let top_of_head = Vec3::new(0.0, DEF.base_radius * ParamId::Size.spec().soft.1, -DEPTH.0);
        assert!(covered(Vec3::ZERO, top_of_head));
        // Straight ahead from the start is the pillar; past its edge the view is open.
        assert!(pillar_hit(Vec3::ZERO, Vec3::NEG_Z).is_some());
        assert!(pillar_hit(Vec3::new(PILLAR_MAX.x + 0.1, 0.0, 0.0), Vec3::NEG_Z).is_none());
    }
}
