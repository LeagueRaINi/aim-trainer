//! Calm aim: a 90 s test of control rather than speed, in four parts played in a fixed order
//! (the [`PLAN`]): following a slow dot, holding an angle until a target appears next to it,
//! flicking without hurry, and coming to rest on a target. Each part opens with a banner
//! ([`INTRO_SECS`], nothing measured) and then measures the hand with
//! [`steadiness`](crate::metrics::steadiness); the scores are [`crate::metrics::calm`]'s. The
//! run's score is the mean of the four part scores, set through [`Run::rate`].
//!
//! Everything about a run in progress is in [`CalmRun`]: the HUD reads the part from it
//! ([`CalmRun::view`]) and the calm log the finished [`CalmResult`]. The length is the plan's,
//! whatever the drill-length setting says: the drill ends the run itself.

use bevy::prelude::*;

use super::microflick::flick_position;
use super::peek::{COVER_MAX, COVER_MIN};
use super::smooth_tracking::Loop;
use crate::AppState;
use crate::camera::{PlayerCamera, ViewCone};
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_respawn, no_spawn_next};
use crate::drill::geometry::{TARGET_DISTANCE, direction, random_range, random_sign, ray_hit};
use crate::drill::health::Health;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{NextSpawn, SpawnCtx, TargetAssets};
use crate::drill::{DrillConfig, DrillKind, DrillSet, Run, facing_eye};
use crate::metrics::calm::{CalmResult, MOVING_SHOT, Part, Smoothness};
use crate::metrics::flick_probe::{angles, screen_offset, wrap};
use crate::metrics::median;
use crate::metrics::steadiness::{self, SpeedFilter, Tracked};
use crate::replay::Recorder;
use crate::settings::Settings;
use crate::sfx::HitSounds;

pub const DEF: DrillDef = DrillDef {
    name: "Calm aim",
    blurb: |_| "A 90 s test of control, not speed: follow, hold still, flick calmly, settle. Four parts.".into(),
    category: Category::Tests,
    // Head-size at the target distance, like Microflick's dot.
    base_radius: 0.2,
    decay: DecayRule::None,
    score: ScoreModel::Calm,
    ttk_from: TtkFrom::Spawn,
    click_kills: false,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    endless: true,
    params: &[],
    tune_defaults: tune,
    spawn_initial,
    respawn: no_respawn,
    spawn_next: no_spawn_next,
};

/// The parts in order, with the seconds each lasts (its banner included).
pub const PLAN: [(Part, f32); 4] =
    [(Part::Smoothness, 21.0), (Part::Stillness, 25.0), (Part::Flick, 23.0), (Part::Settle, 21.0)];
/// Seconds a part's banner is up before the part starts measuring: long enough to read it.
pub const INTRO_SECS: f32 = 3.0;

/// The slow dot of Smoothness: 1.2 m/s at the target distance is 6.8°/s, a walking enemy at
/// 25 m, round a loop reaching 3.5 m (19°) to each side.
const SWEEP_SPEED: f32 = 1.2;
const SWEEP_HALF_WIDTH: f32 = 3.5;

/// The mark of Stillness, (yaw, pitch) degrees: at eye level, 1.5° into the open from the
/// wall's corner as seen from the eye (its far edge, at yaw `atan(0.6 / 6.25)` = 5.5°).
const MARK: Vec2 = Vec2::new(4.0, 0.0);
/// The crosshair rests on the mark within this many degrees of its centre: inside its ring...
const MARK_REACH: f32 = 0.6;
/// How thick the ring is drawn, as a share of its inner radius: 0.3°, a few pixels on screen.
const RING_WIDTH: f32 = 0.5;
/// ...for this long, seconds, before the wait starts.
const REST_SECS: f32 = 0.3;
/// Seconds of waiting before the target appears: too long to count out.
const WAIT_SECS: (f32, f32) = (1.5, 3.5);
/// Where the target appears: this many degrees from the mark (clear of its ring, close enough
/// for one small move)...
const APPEAR_DISTANCE: (f32, f32) = (2.5, 4.5);
/// ...within this many degrees of straight right on screen, which is away from the wall.
const APPEAR_SPREAD: f32 = 70.0;
/// Seconds a target of Stillness or Composed flick stays up: generous, nobody is being rushed.
const CLICK_LIMIT: f32 = 3.0;
/// Seconds between a cleared target and what comes next.
const GAP_SECS: f32 = 0.5;
/// Seconds the crosshair has to stay on a Settle target for it to count.
const HOLD_SECS: f32 = 0.5;
/// Seconds a Settle target stays up.
const SETTLE_LIMIT: f32 = 4.0;

pub(super) fn register(app: &mut App) {
    app.init_resource::<CalmRun>()
        .add_systems(Startup, create_mark_assets)
        .add_systems(OnEnter(AppState::Playing), |mut calm: ResMut<CalmRun>| *calm = CalmRun::default())
        .add_systems(Update, play.in_set(DrillSet::Score));
}

/// Flick targets across the middle of the screen, 6° away or more.
fn tune(config: &mut DrillConfig) {
    config.yaw_range = 25.0;
    config.pitch_range = 10.0;
    config.min_flick = 6.0;
}

/// The part running `elapsed` seconds into the run, as its index in [`PLAN`], and the seconds
/// since it began; `None` once the test is over.
fn schedule(elapsed: f32) -> Option<(usize, f32)> {
    let mut start = 0.0;
    for (index, (_, secs)) in PLAN.iter().enumerate() {
        if elapsed < start + secs {
            return Some((index, elapsed - start));
        }
        start += secs;
    }
    None
}

/// The target of the part being played.
#[derive(Component)]
struct CalmTarget;

/// The slow dot's way round its loop.
#[derive(Component)]
struct Sweeper(Loop);

/// Scenery of Stillness: the wall and the mark.
#[derive(Component)]
struct CalmProp;

/// The spot to rest the crosshair on: a ring.
#[derive(Component)]
struct Mark;

/// The mark's ring and its two looks.
#[derive(Resource, Default)]
struct MarkAssets {
    /// A flat ring facing +Z with an inner radius of 1.
    ring: Handle<Mesh>,
    idle: Handle<StandardMaterial>,
    /// While the crosshair rests in it.
    rested: Handle<StandardMaterial>,
}

fn create_mark_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let unlit = |color: Color| StandardMaterial { base_color: color, unlit: true, ..default() };
    commands.insert_resource(MarkAssets {
        ring: meshes.add(Annulus::new(1.0, 1.0 + RING_WIDTH)),
        // Light enough to find against the wall behind it; green once rested in, like a full
        // health bar.
        idle: materials.add(unlit(Color::srgb(0.80, 0.82, 0.87))),
        rested: materials.add(unlit(Color::srgb(0.36, 0.86, 0.52))),
    });
}

/// The target that is up to be clicked or stayed on.
#[derive(Clone, Copy)]
struct Shown {
    entity: Entity,
    /// Its centre in the room, and its radius, metres.
    centre: Vec3,
    size: f32,
    /// Its centre as (yaw, pitch) and its radius, degrees.
    at: Vec2,
    radius: f32,
    /// Run time it appeared.
    since: f32,
}

/// Where a part is.
#[derive(Clone, Copy, Default)]
enum Stage {
    /// Its banner is up, or (Smoothness) it has only the dot to follow.
    #[default]
    Opening,
    /// Stillness: getting onto the mark; the seconds the crosshair has rested on it.
    Approach(f32),
    /// Stillness: waiting on the mark until this run time.
    Wait(f32),
    /// A target is up.
    Target(Shown),
    /// Nothing is up until this run time.
    Gap(f32),
}

/// The Calm aim run in progress (or the last one finished).
#[derive(Resource, Default)]
pub struct CalmRun {
    /// Index in [`PLAN`] of the part being played.
    part: Option<usize>,
    stage: Stage,
    /// Crosshair speed, fed every live frame.
    speed: SpeedFilter,
    /// Smoothness: every frame of following.
    followed: Vec<Tracked>,
    /// The aim over the current wait, flick or approach to a Settle target.
    trail: Vec<(f32, Vec2)>,
    /// Stillness: the waits' drifts added up.
    drifts: f32,
    /// Settle: seconds on the current target without a break, and every target's settle time.
    held: f32,
    settles: Vec<f32>,
    measured: CalmResult,
    /// The result, once the run is over.
    result: Option<CalmResult>,
}

/// What the HUD shows of the part being played.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartView {
    /// 1 to 4.
    pub number: usize,
    pub part: Part,
    /// Seconds until the part starts, while its banner is up.
    pub starts_in: Option<f32>,
    /// Seconds left of the part.
    pub left: f32,
}

impl CalmRun {
    /// The finished run's result; `None` while it is being played.
    pub fn result(&self) -> Option<&CalmResult> {
        self.result.as_ref()
    }

    /// The part `elapsed` seconds into a run; `None` once the test is over.
    pub fn view(elapsed: f32) -> Option<PartView> {
        let (index, into) = schedule(elapsed)?;
        let (part, secs) = PLAN[index];
        Some(PartView {
            number: index + 1,
            part,
            starts_in: (into < INTRO_SECS).then_some(INTRO_SECS - into),
            left: secs - into,
        })
    }

    /// Takes stock of the part that just ended.
    fn close(&mut self, part: Part) {
        match part {
            Part::Smoothness => {
                if let Some(follow) = steadiness::follow(&self.followed) {
                    self.measured.smoothness = Smoothness {
                        jitter: follow.jitter,
                        with_target: follow.with_target,
                        error: follow.error,
                        on_target: follow.on_target,
                    };
                }
                self.followed = Vec::new();
            },
            Part::Stillness => {
                // A wait cut short by the end of the part was still a wait.
                if matches!(self.stage, Stage::Wait(_)) {
                    self.end_wait();
                }
                let stillness = &mut self.measured.stillness;
                stillness.drift = if stillness.waits > 0 { self.drifts / stillness.waits as f32 } else { 0.0 };
            },
            Part::Flick => {},
            Part::Settle => self.measured.settling.secs = median(&mut self.settles).unwrap_or(0.0),
        }
        self.stage = Stage::Opening;
        self.trail.clear();
    }

    /// Adds the wait in the trail to the Stillness measures, if it is long enough to judge.
    fn end_wait(&mut self) {
        if let Some(hold) = steadiness::hold(&self.trail) {
            let stillness = &mut self.measured.stillness;
            stillness.waits += 1;
            stillness.wait_secs += hold.secs;
            stillness.travel += hold.travel;
            self.drifts += hold.drift;
        }
        self.trail.clear();
    }
}

/// The slow dot, at rest at the bottom of its loop: there to get onto during the countdown.
fn spawn_initial(ctx: &mut SpawnCtx) {
    let sweep = Loop::new(SWEEP_SPEED, SWEEP_HALF_WIDTH, random_sign());
    ctx.spawn_target(sweep.translation(), None).insert((CalmTarget, Sweeper(sweep)));
}

/// Somewhere on screen, a flick away from the crosshair.
fn anywhere(ctx: &SpawnCtx) -> Vec3 {
    flick_position(ctx.aim, &ctx.view, ctx.margin(), ctx.config)
}

/// Spawns a target at `position`, born now.
fn show(ctx: &mut SpawnCtx, position: Vec3) -> Shown {
    let size = ctx.radius();
    let entity = ctx.spawn_target(position, None).insert(CalmTarget).id();
    let distance = position.length();
    Shown {
        entity,
        centre: position,
        size,
        at: angles(position / distance),
        radius: (size / distance).atan().to_degrees(),
        since: ctx.now,
    }
}

/// The wall of Peeks and, just clear of its corner, the mark to rest on: a ring whose hole is
/// where the crosshair counts as resting.
fn spawn_scenery(commands: &mut Commands, assets: &TargetAssets, mark: &MarkAssets) {
    let at = mark_position();
    let reach = TARGET_DISTANCE * MARK_REACH.to_radians().tan();
    commands.spawn((
        CalmProp,
        DespawnOnExit(AppState::Playing),
        Mesh3d(assets.cover_mesh.clone()),
        MeshMaterial3d(assets.cover_material.clone()),
        Transform::from_translation((COVER_MIN + COVER_MAX) * 0.5),
    ));
    commands.spawn((
        CalmProp,
        Mark,
        DespawnOnExit(AppState::Playing),
        Mesh3d(mark.ring.clone()),
        MeshMaterial3d(mark.idle.clone()),
        Transform::from_translation(at).with_rotation(facing_eye(at)).with_scale(Vec3::splat(reach)),
    ));
}

fn mark_position() -> Vec3 {
    direction(MARK.x.to_radians(), MARK.y.to_radians()) * TARGET_DISTANCE
}

/// Where a Stillness target appears: a few degrees from the mark, on its open side.
fn beside_mark() -> Vec3 {
    let offset =
        screen_offset(random_range(-APPEAR_SPREAD, APPEAR_SPREAD), random_range(APPEAR_DISTANCE.0, APPEAR_DISTANCE.1));
    let at = MARK + offset;
    direction(at.x.to_radians(), at.y.to_radians()) * TARGET_DISTANCE
}

/// Gives a target or the mark the `wanted` look, touching it only if that is a change.
fn set_material(material: &mut Mut<MeshMaterial3d<StandardMaterial>>, wanted: &Handle<StandardMaterial>) {
    if material.0 != *wanted {
        material.0 = wanted.clone();
    }
}

/// Plays the test: moves from part to part by the run's clock, runs the current part (its
/// targets, clicks and measurements), and ends the run with its score after the last.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn play(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut run: ResMut<Run>,
    mut calm: ResMut<CalmRun>,
    assets: Res<TargetAssets>,
    mark: Res<MarkAssets>,
    sounds: Res<HitSounds>,
    settings: Res<Settings>,
    mut recorder: ResMut<Recorder>,
    mut next: ResMut<NextSpawn>,
    view: Res<ViewCone>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<CalmTarget>)>,
    mut targets: Query<
        (Entity, &mut Transform, &mut MeshMaterial3d<StandardMaterial>, Option<&mut Sweeper>, Option<&mut Health>),
        With<CalmTarget>,
    >,
    mut props: Query<(Entity, &mut MeshMaterial3d<StandardMaterial>, Has<Mark>), (With<CalmProp>, Without<CalmTarget>)>,
) {
    if run.kind() != DrillKind::CalmAim || !run.live() || calm.result.is_some() {
        return;
    }
    let (now, dt) = (run.elapsed(), time.delta_secs());
    let (origin, forward) = (camera.translation, camera.forward().as_vec3());
    let aim = angles(forward);
    let speed = calm.speed.update(aim, dt);
    let clicked = mouse.just_pressed(MouseButton::Left);
    if clicked {
        run.shot();
    }
    let calm = &mut *calm;
    let mut occupied = Vec::new();
    // What the spawn hooks get, for the targets spawned here.
    macro_rules! ctx {
        () => {
            SpawnCtx {
                commands: &mut commands,
                assets: &assets,
                config: &run.drill.config,
                now,
                aim: forward,
                view: *view,
                live: true,
                occupied: &mut occupied,
                next: &mut next,
            }
        };
    }

    let current = schedule(now);
    if calm.part != current.map(|(index, _)| index) {
        if let Some(ended) = calm.part {
            calm.close(PLAN[ended].0);
            for (entity, ..) in &targets {
                commands.entity(entity).despawn();
            }
            for (entity, ..) in &props {
                commands.entity(entity).despawn();
            }
        }
        if calm.part.is_none() {
            // Room for the whole run's samples up front, rather than growing while it is played.
            calm.followed.reserve(1 << 14);
            calm.trail.reserve(1 << 12);
        }
        calm.part = current.map(|(index, _)| index);
        match current.map(|(index, _)| PLAN[index].0) {
            None => {
                run.rate(calm.measured.overall());
                run.end();
                calm.result = Some(calm.measured);
            },
            // The dot is already there from the countdown, unless the run was started live.
            Some(Part::Smoothness) if targets.is_empty() => {
                let sweep = Loop::new(SWEEP_SPEED, SWEEP_HALF_WIDTH, random_sign());
                let shown = show(&mut ctx!(), sweep.translation());
                commands.entity(shown.entity).insert(Sweeper(sweep));
            },
            Some(Part::Smoothness) => {},
            Some(Part::Stillness) => spawn_scenery(&mut commands, &assets, &mark),
            Some(Part::Flick | Part::Settle) => {},
        }
        // What was just spawned or despawned is there (or gone) from the next frame on.
        return mark_shot(&mut recorder, clicked, false);
    }
    let Some((index, into)) = current else { return };
    let part = PLAN[index].0;
    let live = into >= INTRO_SECS;
    let on = |centre: Vec3, size: f32| ray_hit(origin, forward, centre, size).is_some();
    // Whether this frame's click hit, and whether a target was cleared (clicked or stayed on).
    let (mut hit, mut cleared) = (false, false);

    match (part, calm.stage) {
        (Part::Smoothness, _) => {
            for (_, mut transform, mut material, sweeper, _) in &mut targets {
                let Some(mut sweeper) = sweeper else { continue };
                if live {
                    sweeper.0.step(dt);
                    transform.translation = sweeper.0.translation();
                }
                let (centre, size) = (transform.translation, transform.scale.x);
                set_material(&mut material, if on(centre, size) { &assets.hot } else { &assets.idle });
                if live {
                    let radius = (size / centre.length()).atan().to_degrees();
                    calm.followed.push(Tracked { t: now, aim, target: sweeper.0.position(), radius });
                }
            }
        },
        (_, Stage::Opening) if !live => {},

        (Part::Stillness, Stage::Opening) => calm.stage = Stage::Approach(0.0),
        (Part::Stillness, Stage::Approach(rested)) => {
            calm.measured.stillness.early_shots += u32::from(clicked);
            let resting = wrap(aim - MARK).length() <= MARK_REACH;
            for (_, mut material, is_mark) in &mut props {
                if is_mark {
                    set_material(&mut material, if resting { &mark.rested } else { &mark.idle });
                }
            }
            let rested = if resting { rested + dt } else { 0.0 };
            calm.stage = if rested >= REST_SECS {
                calm.trail.clear();
                Stage::Wait(now + random_range(WAIT_SECS.0, WAIT_SECS.1))
            } else {
                Stage::Approach(rested)
            };
        },
        (Part::Stillness, Stage::Wait(until)) => {
            calm.measured.stillness.early_shots += u32::from(clicked);
            calm.trail.push((now, aim));
            if now >= until {
                calm.end_wait();
                // The move to the target starts from here.
                calm.trail.push((now, aim));
                calm.stage = Stage::Target(show(&mut ctx!(), beside_mark()));
            }
        },
        (Part::Stillness, Stage::Target(shown)) => {
            calm.trail.push((now, aim));
            let stillness = &mut calm.measured.stillness;
            hit = clicked && on(shown.centre, shown.size);
            if hit {
                run.kill(now - shown.since);
                stillness.targets += 1;
                // A move too slow to count as one (crept there) was still one.
                stillness.moves += steadiness::moves(&calm.trail).max(1);
            } else if now - shown.since >= CLICK_LIMIT {
                run.expire();
                stillness.expired += 1;
            }
            if hit || now - shown.since >= CLICK_LIMIT {
                commands.entity(shown.entity).despawn();
                calm.stage = Stage::Gap(now + GAP_SECS);
            }
            cleared = hit;
        },
        (Part::Stillness, Stage::Gap(until)) => {
            calm.measured.stillness.early_shots += u32::from(clicked);
            if now >= until {
                calm.stage = Stage::Approach(0.0);
            }
        },

        // The first target of the part comes as its banner goes.
        (Part::Flick | Part::Settle, Stage::Opening) => calm.stage = Stage::Gap(now),
        (Part::Flick | Part::Settle, Stage::Gap(until)) => {
            if part == Part::Flick && clicked {
                calm.measured.flicks.shots += 1;
                calm.measured.flicks.moving_shots += u32::from(speed > MOVING_SHOT);
            }
            if now >= until {
                calm.trail.clear();
                calm.trail.push((now, aim));
                calm.held = 0.0;
                let mut ctx = ctx!();
                let position = anywhere(&ctx);
                let shown = show(&mut ctx, position);
                if part == Part::Settle {
                    commands.entity(shown.entity).insert(Health::new(HOLD_SECS));
                }
                calm.stage = Stage::Target(shown);
            }
        },
        (Part::Flick, Stage::Target(shown)) => {
            calm.trail.push((now, aim));
            let flicks = &mut calm.measured.flicks;
            if clicked {
                flicks.shots += 1;
                flicks.moving_shots += u32::from(speed > MOVING_SHOT);
            }
            hit = clicked && on(shown.centre, shown.size);
            if hit {
                run.kill(now - shown.since);
                flicks.kills += 1;
                if let Some(flick) = steadiness::flick(&calm.trail, shown.at, shown.radius) {
                    flicks.overshot += u32::from(flick.overshot);
                    flicks.corrections += flick.moves.saturating_sub(1);
                }
            } else if now - shown.since >= CLICK_LIMIT {
                run.expire();
                flicks.expired += 1;
            }
            if hit || now - shown.since >= CLICK_LIMIT {
                commands.entity(shown.entity).despawn();
                calm.stage = Stage::Gap(now + GAP_SECS);
            }
            cleared = hit;
        },
        (Part::Settle, Stage::Target(shown)) => {
            calm.trail.push((now, aim));
            let staying = on(shown.centre, shown.size);
            let broke_off = calm.held > 0.0 && !staying;
            calm.held = if staying { calm.held + dt } else { 0.0 };
            // The bar over the target shows what is left of the stay; it refills when broken.
            if let Ok((_, _, mut material, _, Some(mut health))) = targets.get_mut(shown.entity) {
                set_material(&mut material, if staying { &assets.hot } else { &assets.idle });
                if staying {
                    health.damage(dt);
                } else if broke_off {
                    *health = Health::new(HOLD_SECS);
                }
            }
            let settling = &mut calm.measured.settling;
            cleared = calm.held >= HOLD_SECS;
            if cleared {
                settling.targets += 1;
                if let Some(settle) = steadiness::settle(&calm.trail, shown.at, shown.radius) {
                    settling.exits += settle.exits;
                    calm.settles.push(settle.secs);
                }
            } else if now - shown.since >= SETTLE_LIMIT {
                run.expire();
                settling.expired += 1;
            }
            if cleared || now - shown.since >= SETTLE_LIMIT {
                commands.entity(shown.entity).despawn();
                calm.stage = Stage::Gap(now + GAP_SECS);
            }
        },
        // Stages a part never gets into.
        (Part::Flick | Part::Settle, Stage::Approach(_) | Stage::Wait(_)) => {},
    }
    if cleared {
        sounds.play(&mut commands, &settings.audio);
    }
    mark_shot(&mut recorder, clicked, hit);
}

/// Marks this frame's shot, if there was one, in the replay.
fn mark_shot(recorder: &mut Recorder, clicked: bool, hit: bool) {
    if clicked {
        recorder.mark_shot(hit);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::PlayState;
    use crate::calm_log::{CalmLog, record_calm};
    use crate::drill::run::{RunFinished, tick_run};
    use crate::drill::{DrillSpec, RunPurpose, RunStats};
    use crate::history::{History, RunRecorded, record_finished};
    use crate::test_util::TempDir;

    #[test]
    fn the_plan_is_ninety_seconds_of_four_parts_each_behind_its_banner() {
        assert_eq!(PLAN.map(|(part, _)| part), Part::ALL);
        assert_eq!(PLAN.iter().map(|(_, secs)| secs).sum::<f32>(), 90.0);
        assert_eq!(schedule(0.0), Some((0, 0.0)));
        assert_eq!(schedule(20.99).map(|(index, _)| index), Some(0));
        assert_eq!(schedule(21.0), Some((1, 0.0)));
        assert_eq!(schedule(89.99).map(|(index, _)| index), Some(3));
        assert_eq!(schedule(90.0), None);
        // The banner counts down to the part, then the part to its end.
        let opening = CalmRun::view(47.0).unwrap();
        assert_eq!((opening.number, opening.part, opening.starts_in), (3, Part::Flick, Some(2.0)));
        let running = CalmRun::view(50.0).unwrap();
        assert_eq!((running.starts_in, running.left), (None, 19.0));
        assert_eq!(CalmRun::view(90.0), None);
    }

    /// Simulated time per frame: 200 fps.
    const FRAME_MS: u64 = 5;

    /// Who plays the test.
    #[derive(Resource, Clone, Copy, PartialEq)]
    enum Player {
        /// Looks straight at whatever is up (the target, else the mark) and, where targets are
        /// to be clicked, clicks a quarter of a second after getting there.
        Composed,
        /// Never touches the mouse.
        Absent,
    }

    /// Every run that finished, as the history recorded it.
    #[derive(Resource, Default)]
    struct Finished(Vec<(RunStats, u64)>);

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn scripted(
        player: Res<Player>,
        time: Res<Time>,
        run: Res<Run>,
        mut mouse: ResMut<ButtonInput<MouseButton>>,
        mut camera: Single<&mut Transform, (With<PlayerCamera>, Without<CalmTarget>, Without<Mark>)>,
        targets: Query<&Transform, With<CalmTarget>>,
        marks: Query<&Transform, (With<Mark>, Without<CalmTarget>)>,
        mut on_target_for: Local<f32>,
    ) {
        // No input plugin here to do this between frames.
        mouse.release(MouseButton::Left);
        mouse.clear();
        if *player == Player::Absent {
            return;
        }
        let target = targets.iter().next();
        *on_target_for = if target.is_some() { *on_target_for + time.delta_secs() } else { 0.0 };
        if let Some(goal) = target.or(marks.iter().next()) {
            camera.look_at(goal.translation, Vec3::Y);
        }
        let clicking =
            schedule(run.elapsed()).is_some_and(|(index, _)| matches!(PLAN[index].0, Part::Stillness | Part::Flick));
        if clicking && *on_target_for >= 0.25 {
            mouse.press(MouseButton::Left);
            *on_target_for = 0.0;
        }
    }

    /// A Calm aim run, live, with everything that reacts to its end: the history, the calm log
    /// and a results screen to go to. No rendering, no real clock.
    fn test_app(dir: &TempDir, player: Player) -> App {
        let settings = Settings::default();
        let drill = DrillSpec::builtin(DrillKind::CalmAim, &settings);
        // A drill length the test must ignore.
        let run = Run::new(drill, 0.4, 30.0, RunPurpose::Practice, "Calm aim".into()).started();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(AppState::Playing)
            .add_sub_state::<PlayState>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(FRAME_MS)))
            .insert_resource(settings)
            .insert_resource(player)
            .insert_resource(run)
            .insert_resource(History::load(&dir.join("results.csv")))
            .insert_resource(CalmLog::load(&dir.join("calm.csv")))
            .insert_resource(ViewCone::default())
            .insert_resource(NextSpawn::default())
            .insert_resource(HitSounds::default())
            .insert_resource(Recorder::default())
            .insert_resource(TargetAssets {
                mesh: default(),
                idle: default(),
                hot: default(),
                wait: default(),
                cover_mesh: default(),
                cover_material: default(),
            })
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Finished>()
            .init_resource::<CalmRun>()
            .init_resource::<MarkAssets>()
            .add_message::<RunFinished>()
            .add_message::<RunRecorded>()
            .configure_sets(
                Update,
                (DrillSet::Score, DrillSet::Finish, DrillSet::Log, DrillSet::Outcome)
                    .chain()
                    .run_if(in_state(PlayState::Running)),
            )
            .add_systems(
                Update,
                (
                    (scripted, play).chain().in_set(DrillSet::Score),
                    tick_run.in_set(DrillSet::Finish),
                    record_finished.in_set(DrillSet::Log),
                    (
                        record_calm,
                        |mut recorded: MessageReader<RunRecorded>,
                         mut finished: ResMut<Finished>,
                         mut next: ResMut<NextState<AppState>>| {
                            for recorded in recorded.read() {
                                finished.0.push((recorded.run.stats.clone(), recorded.timestamp));
                                next.set(AppState::Results);
                            }
                        },
                    )
                        .in_set(DrillSet::Outcome),
                ),
            );
        app.world_mut().spawn((PlayerCamera, Transform::IDENTITY));
        app
    }

    /// Plays the test to its end and for two seconds more; returns the one finished run and
    /// its log entry.
    fn play_through(dir: &TempDir, player: Player) -> (RunStats, CalmResult) {
        fastrand::seed(11);
        let mut app = test_app(dir, player);
        let frames = (92_000 / FRAME_MS) as usize;
        for frame in 0..frames {
            app.update();
            let elapsed = app.world().resource::<Run>().elapsed();
            let over = !app.world().resource::<Finished>().0.is_empty();
            assert!(!over || elapsed >= 90.0, "frame {frame}: over {elapsed} s in");
        }
        assert_eq!(*app.world().resource::<State<AppState>>().get(), AppState::Results);
        let finished = &app.world().resource::<Finished>().0;
        assert_eq!(finished.len(), 1, "finished {} times", finished.len());
        let (stats, timestamp) = finished[0].clone();
        // Its length is the plan's, whatever the drill length was set to.
        assert!((stats.duration - 90.0).abs() < 0.02, "{} s", stats.duration);
        // The history row and the log row are of the same run and say the same score.
        let history = std::fs::read_to_string(dir.join("results.csv")).unwrap();
        assert_eq!(history.lines().count(), 2, "header + one row");
        let score = format!(",{},Calm aim", stats.score());
        assert!(history.lines().nth(1).unwrap().ends_with(&score), "{history}");
        let log = CalmLog::load(&dir.join("calm.csv"));
        let entry = log.entry(timestamp).expect("logged under the history timestamp");
        assert_eq!(std::fs::read_to_string(dir.join("calm.csv")).unwrap().lines().count(), 2);
        assert_eq!((entry.drill.as_str(), entry.result.overall()), ("Calm aim", stats.score()));
        assert_eq!(stats.rating, Some(stats.score()));
        // Nothing of the test is left in the room.
        let world = app.world_mut();
        assert_eq!(world.query::<&CalmTarget>().iter(world).count(), 0);
        assert_eq!(world.query::<&CalmProp>().iter(world).count(), 0);
        (stats, entry.result)
    }

    #[test]
    fn a_composed_player_goes_through_all_four_parts_and_scores_high() {
        let dir = TempDir::new("calm_composed");
        let (stats, result) = play_through(&dir, Player::Composed);
        // Every part was played and measured.
        assert!(result.smoothness.on_target > 0.99 && result.smoothness.jitter < 0.01, "{:?}", result.smoothness);
        let still = result.stillness;
        assert!(still.waits >= 4 && still.targets >= 4 && still.wait_secs > 8.0, "{still:?}");
        assert_eq!((still.early_shots, still.expired, still.moves), (0, 0, still.targets), "{still:?}");
        assert!(still.drift < 1e-3 && still.travel < 1e-3, "{still:?}");
        let flicks = result.flicks;
        assert!(flicks.kills >= 15, "{flicks:?}");
        assert_eq!((flicks.shots, flicks.moving_shots, flicks.overshot, flicks.expired), (flicks.kills, 0, 0, 0));
        let settling = result.settling;
        assert!(settling.targets >= 10 && settling.exits == 0 && settling.expired == 0, "{settling:?}");
        for part in Part::ALL {
            assert!(result.score(part) >= 85.0, "{part:?}: {}", result.score(part));
        }
        assert!(stats.score() >= 90.0, "{}", stats.score());
        // Only clicked targets are hits: staying on one fires nothing. Every shot hit, but for
        // one the script may fire in the frame a part ends, when its target is already going.
        assert_eq!(stats.hits, still.targets + flicks.kills);
        assert!((stats.hits..=stats.hits + 2).contains(&stats.shots), "{} shots", stats.shots);
    }

    #[test]
    fn an_absent_player_finishes_once_too_and_scores_nothing() {
        let dir = TempDir::new("calm_absent");
        let (stats, result) = play_through(&dir, Player::Absent);
        assert_eq!(stats.score(), 0.0, "{result:?}");
        assert_eq!((stats.hits, stats.shots), (0, 0));
        assert!(result.flicks.expired >= 5 && result.settling.expired >= 3, "{result:?}");
    }
}
