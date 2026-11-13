//! Wall flicks: several small static dots at free random spots on a wall; a kill puts a new one
//! elsewhere. Static clicking: flick, stop, click. Pokeball reuses its spawning.
//!
//! The wall is fixed in the room (like Gridshot's grid): `yaw_range` to each side of straight
//! ahead and `pitch_range` up and down, see [`Wall`]. Spots are free, but snapped to a lattice of
//! [`TICK`] degrees so that a target's [`Slot`] is its spot: the generic shooting and decay hand
//! the respawn hook the slots of the targets on the wall and of the one just cleared, which is
//! all the hook needs to keep targets apart.

use std::borrow::Cow;

use bevy::prelude::*;

use crate::drill::DrillConfig;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_spawn_next};
use crate::drill::geometry::{CEILING_PITCH, FLOOR_PITCH, TARGET_DISTANCE, direction};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{Slot, SpawnCtx};

pub const DEF: DrillDef = DrillDef {
    name: "Wall flicks",
    blurb,
    category: Category::Aim,
    // Microflick's dot: 1.15° across its radius.
    base_radius: 0.2,
    decay: DecayRule::Global,
    score: ScoreModel::Flick,
    ttk_from: TtkFrom::PreviousKill,
    click_kills: true,
    environment: Environment::Open,
    first_flick_probe: false,
    // Small dots 6° to 45° apart, like Gridshot's: sensitivities score differently on them, and
    // every kill is a flick from the previous one that the weak-spot analysis can read.
    classic_finder: true,
    flick_analysis: true,
    adaptive: false,
    endless: false,
    params: &[ParamId::GridActive, ParamId::YawRange, ParamId::PitchRange, ParamId::MinFlick],
    tune_defaults: tune,
    spawn_initial,
    respawn,
    spawn_next: no_spawn_next,
};

/// Four small dots on a moderate wall, as in KovaaK's 1w4ts: 40° wide and 24° tall, which at
/// Valorant's 103° is the middle third of the screen's width and height, so every dot is in
/// clear view and the flicks are 6° to about 45° long.
fn tune(config: &mut DrillConfig) {
    config.grid_active = 4;
    config.yaw_range = 20.0;
    config.pitch_range = 12.0;
    config.min_flick = 6.0;
}

fn blurb(config: &DrillConfig) -> Cow<'static, str> {
    format!("{} at random spots on a wall. Flick, stop, click.", count(config, "dot", "dots")).into()
}

/// `1 dot` / `4 dots`: the targets this config has up at once.
pub(super) fn count(config: &DrillConfig, one: &str, many: &str) -> String {
    match config.grid_active {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

/// Degrees between neighbouring spots of the lattice target centres sit on: far below a
/// target's size, so spots are as good as free.
const TICK: f32 = 0.1;
/// The lattice covers every yaw the editor allows (`ParamId::YawRange`'s hard limit), in ticks
/// to each side, and every pitch between floor and ceiling.
const YAW_TICKS: i32 = 800;
/// [`FLOOR_PITCH`] in ticks: the lattice's bottom row.
const FLOOR_TICK: i32 = -130;
/// Lattice columns: the slot of a spot is its row times this plus its column.
const COLUMNS: i32 = 2 * YAW_TICKS + 1;

/// Distance kept between the centres of two targets, in target radii: a gap of one radius
/// between their edges, so small targets may sit close and big ones stay clearly separate.
const SPACING: f32 = 3.0;
/// Random spots tried for a new target before the wall counts as crowded.
const TRIES: usize = 32;
/// About how many spots are compared on a crowded wall.
const SCAN_SPOTS: f32 = 4096.0;

/// The wall: the lattice spots (yaw, pitch in ticks, inclusive) target centres may sit on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Wall {
    low: IVec2,
    high: IVec2,
}

impl Wall {
    /// `yaw_range` to each side and `pitch_range` up and down from eye height, between floor and
    /// ceiling: a wall too tall for that is raised until its bottom edge sits at the floor (as
    /// tall grids are), and cut off at the ceiling.
    pub(super) fn of(config: &DrillConfig) -> Self {
        // Whole ticks inside the range; the slack keeps a range that is a whole number of ticks
        // from losing its last one to rounding.
        let inside = |degrees: f32| (degrees / TICK + 1e-3).floor() as i32;
        let yaw = inside(config.yaw_range).min(YAW_TICKS);
        let bottom = (-config.pitch_range).max(FLOOR_PITCH);
        let top = (bottom + 2.0 * config.pitch_range).min(CEILING_PITCH);
        let low = -inside(-bottom);
        Self { low: IVec2::new(-yaw, low), high: IVec2::new(yaw, inside(top).max(low)) }
    }

    fn random_spot(&self) -> IVec2 {
        IVec2::new(fastrand::i32(self.low.x..=self.high.x), fastrand::i32(self.low.y..=self.high.y))
    }

    /// The (yaw, pitch) in degrees of the wall's middle.
    #[cfg(test)]
    fn middle(&self) -> Vec2 {
        (self.low + self.high).as_vec2() * 0.5 * TICK
    }
}

fn slot_of(spot: IVec2) -> usize {
    ((spot.y - FLOOR_TICK) * COLUMNS + spot.x + YAW_TICKS) as usize
}

fn spot_of(slot: usize) -> IVec2 {
    let slot = slot as i32;
    IVec2::new(slot % COLUMNS - YAW_TICKS, slot / COLUMNS + FLOOR_TICK)
}

/// The (yaw, pitch) in degrees of a spot.
fn spot_angles(spot: IVec2) -> Vec2 {
    spot.as_vec2() * TICK
}

/// The view direction of a spot.
fn spot_direction(spot: IVec2) -> Vec3 {
    let at = spot_angles(spot);
    direction(at.x.to_radians(), at.y.to_radians())
}

/// A target's angular radius in degrees, seen from the eye.
fn angular_radius(config: &DrillConfig) -> f32 {
    (config.kind.def().base_radius * config.size / TARGET_DISTANCE).atan().to_degrees()
}

/// The target just cleared: where it was, and how far (degrees) the new one must be from it.
struct Cleared {
    at: Vec3,
    flick: f32,
}

/// A spot on `wall` for a new target of `radius` degrees.
///
/// It is at least [`SPACING`] radii from every target in `taken`, at least `cleared.flick` from
/// the target just cleared, and `visible`. Random spots are tried first. On a wall too crowded
/// for that (none of [`TRIES`] fits) the spots of a coarser lattice are compared instead and the
/// one that comes closest wins: first the largest share of the wanted distances it can keep (a
/// smaller gap, shrunk no more than it must be), then being visible, then the most room. So it
/// always returns, and targets only ever touch when the wall has no room left.
fn pick_spot(
    wall: &Wall,
    radius: f32,
    taken: &[Vec3],
    cleared: Option<&Cleared>,
    visible: impl Fn(Vec3) -> bool,
) -> IVec2 {
    let apart = (SPACING * radius).to_radians();
    // The share of the wanted distances a spot keeps: 1 or more when it keeps them all.
    let room = |at: Vec3| {
        let nearest = taken.iter().map(|other| at.angle_between(*other)).fold(f32::INFINITY, f32::min);
        let flick = cleared.map_or(f32::INFINITY, |c| at.angle_between(c.at) / c.flick.to_radians().max(apart));
        (nearest / apart).min(flick)
    };
    for _ in 0..TRIES {
        let spot = wall.random_spot();
        let at = spot_direction(spot);
        if room(at) >= 1.0 && visible(at) {
            return spot;
        }
    }
    let size = (wall.high - wall.low + 1).as_vec2();
    let stride = (size.x * size.y / SCAN_SPOTS).sqrt().ceil().max(1.0) as usize;
    let mut best = (wall.low, (f32::NEG_INFINITY, false, f32::NEG_INFINITY));
    for yaw in (wall.low.x..=wall.high.x).step_by(stride) {
        for pitch in (wall.low.y..=wall.high.y).step_by(stride) {
            let spot = IVec2::new(yaw, pitch);
            let at = spot_direction(spot);
            let room = room(at);
            let rank = (room.min(1.0), visible(at), room);
            if rank > best.1 {
                best = (spot, rank);
            }
        }
    }
    best.0
}

/// The spots of the first `grid_active` targets, none placed with regard to the view: the wall
/// is where it is, and the run starts looking at it.
fn initial_spots(config: &DrillConfig) -> Vec<IVec2> {
    let (wall, radius) = (Wall::of(config), angular_radius(config));
    let mut spots = Vec::new();
    let mut taken = Vec::new();
    for _ in 0..config.grid_active {
        let spot = pick_spot(&wall, radius, &taken, None, |_| true);
        taken.push(spot_direction(spot));
        spots.push(spot);
    }
    spots
}

/// The spot of the target replacing the one in `cleared_slot`, marked occupied: apart from the
/// targets in `occupied`, a real flick (`min_flick`, and never less than the spacing) from the
/// cleared one, and where `visible` says the player can see it.
fn next_spot(
    config: &DrillConfig,
    occupied: &mut Vec<usize>,
    cleared_slot: Option<usize>,
    visible: impl Fn(Vec3) -> bool,
) -> IVec2 {
    // Only the cleared target: on a wall without room, others may share its spot.
    if let Some(index) = occupied.iter().position(|slot| Some(*slot) == cleared_slot) {
        occupied.swap_remove(index);
    }
    let taken: Vec<Vec3> = occupied.iter().map(|&slot| spot_direction(spot_of(slot))).collect();
    let cleared = cleared_slot.map(|slot| Cleared { at: spot_direction(spot_of(slot)), flick: config.min_flick });
    let spot = pick_spot(&Wall::of(config), angular_radius(config), &taken, cleared.as_ref(), visible);
    occupied.push(slot_of(spot));
    spot
}

fn spawn_at(ctx: &mut SpawnCtx, spot: IVec2) {
    let lifetime = ctx.lifetime();
    ctx.spawn_target(spot_direction(spot) * TARGET_DISTANCE, lifetime).insert(Slot(slot_of(spot)));
}

/// `grid_active` targets spread over the wall.
pub(super) fn spawn_initial(ctx: &mut SpawnCtx) {
    for spot in initial_spots(ctx.config) {
        spawn_at(ctx, spot);
    }
}

/// A new target elsewhere on the wall, on screen from where the player aims now (on a wall wider
/// than the view, the far end is out of sight from the near one).
pub(super) fn respawn(ctx: &mut SpawnCtx, cleared_slot: Option<usize>) {
    let (aim, view, margin) = (ctx.aim, ctx.view, ctx.margin());
    let spot = next_spot(ctx.config, ctx.occupied, cleared_slot, |at| view.contains(aim, at, margin));
    spawn_at(ctx, spot);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::ViewCone;
    use crate::drill::DrillKind;
    use crate::metrics::flick_probe::angles;

    /// Plays `kills` kills on a wall of `config`: each clears a random target, aiming at it, and
    /// spawns its replacement. Checks every target ever placed with `check(new, others, cleared)`
    /// (directions; `cleared` is `None` for the initial targets).
    fn play(config: &DrillConfig, kills: usize, check: impl Fn(Vec3, &[Vec3], Option<Vec3>)) {
        let view = ViewCone::default();
        let margin = (angular_radius(config) + 1.5).to_radians();
        let directions = |slots: &[usize]| slots.iter().map(|&slot| spot_direction(spot_of(slot))).collect::<Vec<_>>();
        let mut slots = Vec::new();
        for spot in initial_spots(config) {
            check(spot_direction(spot), &directions(&slots), None);
            slots.push(slot_of(spot));
        }
        assert_eq!(slots.len(), config.grid_active as usize);
        for _ in 0..kills {
            let cleared = slots[fastrand::usize(..slots.len())];
            let aim = spot_direction(spot_of(cleared));
            let spot = next_spot(config, &mut slots, Some(cleared), |at| view.contains(aim, at, margin));
            assert_eq!(slots.len(), config.grid_active as usize);
            assert_eq!(slots.last(), Some(&slot_of(spot)));
            check(spot_direction(spot), &directions(&slots[..slots.len() - 1]), Some(aim));
        }
    }

    /// Whether `at` is on the wall of `config`, between floor and ceiling.
    fn on_wall(at: Vec3, config: &DrillConfig) -> bool {
        let at = angles(at);
        let bottom = (-config.pitch_range).max(FLOOR_PITCH);
        let top = (bottom + 2.0 * config.pitch_range).min(CEILING_PITCH);
        at.x.abs() <= config.yaw_range + 1e-3 && (bottom - 1e-3..=top + 1e-3).contains(&at.y)
    }

    fn degrees_between(a: Vec3, b: Vec3) -> f32 {
        a.angle_between(b).to_degrees()
    }

    #[test]
    fn slots_are_spots() {
        assert_eq!(YAW_TICKS as f32 * TICK, ParamId::YawRange.spec().hard.1);
        assert!((FLOOR_TICK as f32 * TICK - FLOOR_PITCH).abs() < 1e-4);
        let corners = [IVec2::new(-YAW_TICKS, FLOOR_TICK), IVec2::new(YAW_TICKS, (CEILING_PITCH / TICK) as i32)];
        for spot in [IVec2::ZERO, IVec2::new(-7, 12), IVec2::new(200, -120), corners[0], corners[1]] {
            assert_eq!(spot_of(slot_of(spot)), spot);
        }
        assert!((angles(spot_direction(IVec2::new(-123, 45))) - Vec2::new(-12.3, 4.5)).length() < 1e-3);
    }

    /// On the default walls of both drills, over thousands of kills: every target is on the wall,
    /// a full gap from every other, and a real flick from the one it replaces; and the new one
    /// is on screen from the old one.
    #[test]
    fn default_walls_keep_targets_apart_and_flicks_real() {
        let view = ViewCone::default();
        for kind in [DrillKind::WallFlicks, DrillKind::Pokeball] {
            let config = DrillConfig::defaults(kind);
            let radius = angular_radius(&config);
            let margin = (radius + 1.5).to_radians();
            for seed in 0..4 {
                fastrand::seed(seed);
                play(&config, 3000, |new, others, cleared| {
                    assert!(on_wall(new, &config), "{kind:?}: {} off the wall", angles(new));
                    for other in others {
                        let apart = degrees_between(new, *other);
                        assert!(apart >= SPACING * radius - 1e-3, "{kind:?}: targets {apart}° apart");
                    }
                    if let Some(cleared) = cleared {
                        let flick = degrees_between(new, cleared);
                        assert!(flick >= config.min_flick - 1e-3, "{kind:?}: a {flick}° flick");
                        assert!(view.contains(cleared, new, margin), "{kind:?}: {} off screen", angles(new));
                    }
                });
            }
        }
    }

    /// A wall with no room for the full gap (eight double-size dots on 28° x 18°): the gap
    /// shrinks, but targets never overlap, never leave the wall, and a new one is never on the
    /// one it replaces.
    #[test]
    fn crowded_wall_shrinks_the_gap_but_never_overlaps() {
        let mut config = DrillConfig {
            size: 2.0,
            grid_active: 8,
            yaw_range: 14.0,
            pitch_range: 9.0,
            min_flick: 12.0,
            ..DrillConfig::defaults(DrillKind::WallFlicks)
        };
        config.sanitize();
        assert_eq!(config.grid_active, 8);
        let radius = angular_radius(&config);
        for seed in 0..4 {
            fastrand::seed(seed);
            let tightest = std::cell::Cell::new(f32::INFINITY);
            play(&config, 3000, |new, others, cleared| {
                assert!(on_wall(new, &config), "{} off the wall", angles(new));
                for other in others.iter().chain(&cleared) {
                    let apart = degrees_between(new, *other);
                    assert!(apart >= 2.0 * radius, "seed {seed}: targets {apart}° apart overlap");
                    tightest.set(tightest.get().min(apart));
                }
            });
            // The test is of a wall that really is crowded.
            assert!(tightest.get() < SPACING * radius, "seed {seed}: never short of room");
        }
    }

    /// Walls with no free spot at all still get their targets, at once.
    #[test]
    fn a_wall_without_room_still_fills() {
        for (yaw_range, pitch_range) in [(0.0, 0.0), (3.0, 0.0), (0.0, 40.0)] {
            let mut config = DrillConfig {
                size: 5.0,
                grid_active: 8,
                yaw_range,
                pitch_range,
                ..DrillConfig::defaults(DrillKind::Pokeball)
            };
            config.sanitize();
            play(&config, 200, |new, _, _| assert!(on_wall(new, &config), "{} off the wall", angles(new)));
        }
    }

    /// Every pitch range the editor allows stays between floor and ceiling; walls that fit are
    /// centred on eye height and taller ones stand on the floor.
    #[test]
    fn walls_stay_between_floor_and_ceiling() {
        let limit = ParamId::PitchRange.spec().hard.1;
        for pitch_range in [0.0, 0.04, 5.0, 12.0, 13.0, 13.5, 20.0, 30.45, limit] {
            let config = DrillConfig { pitch_range, ..DrillConfig::defaults(DrillKind::WallFlicks) };
            let wall = Wall::of(&config);
            let (bottom, top) = (wall.low.y as f32 * TICK, wall.high.y as f32 * TICK);
            assert!(bottom >= FLOOR_PITCH - 1e-3 && top <= CEILING_PITCH + 1e-3 && bottom <= top, "{wall:?}");
            assert!(bottom >= -pitch_range - 1e-3 && top - bottom <= 2.0 * pitch_range + 1e-3, "{wall:?}");
            if pitch_range <= -FLOOR_PITCH {
                assert!(wall.middle().y.abs() < 1e-3, "{pitch_range}°: {wall:?} not centred");
            } else {
                assert!((bottom - FLOOR_PITCH).abs() < 1e-3, "{pitch_range}°: {wall:?} not on the floor");
            }
        }
    }

    /// Looking at the middle of either default wall at Valorant's 103°, every spot of it is on
    /// screen with the whole target and room to spare.
    #[test]
    fn default_walls_are_on_screen_from_their_middle() {
        let view = ViewCone::default();
        for kind in [DrillKind::WallFlicks, DrillKind::Pokeball] {
            let config = DrillConfig::defaults(kind);
            let wall = Wall::of(&config);
            let middle = wall.middle();
            let aim = direction(middle.x.to_radians(), middle.y.to_radians());
            let margin = (angular_radius(&config) + 1.5).to_radians();
            for yaw in [wall.low.x, 0, wall.high.x] {
                for pitch in [wall.low.y, 0, wall.high.y] {
                    let at = spot_direction(IVec2::new(yaw, pitch));
                    assert!(view.contains(aim, at, margin), "{kind:?}: {} off screen", angles(at));
                }
            }
        }
    }
}
