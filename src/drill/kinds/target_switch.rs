//! Target switching: several bots wander around, each with a health bar. Holding the fire button
//! shoots at a fixed rate; the bot under the crosshair loses health with every round and dies
//! when it has none left, and a new one appears elsewhere. Stay on a moving target until it is
//! dead, then get to the next one.

use std::borrow::Cow;

use bevy::prelude::*;

use super::microflick::flick_position;
use crate::camera::{PlayerCamera, ViewCone};
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_respawn, no_spawn_next};
use crate::drill::geometry::{CEILING_PITCH, FLOOR_PITCH, TARGET_DISTANCE, direction, ray_hit};
use crate::drill::health::Health;
use crate::drill::held_fire::{HeldFire, ROUNDS_PER_SEC};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{NextSpawn, SpawnCtx, TargetAssets};
use crate::drill::wander::Wander;
use crate::drill::{DrillConfig, DrillKind, DrillSet, Run};
use crate::metrics::flick_probe::angles;
use crate::replay::Recorder;
use crate::settings::Settings;
use crate::sfx::HitSounds;

pub const DEF: DrillDef = DrillDef {
    name: "Target switching",
    blurb,
    category: Category::Aim,
    base_radius: 0.4,
    decay: DecayRule::None,
    score: ScoreModel::HeldFire,
    // From the previous kill: the switch to a bot is part of killing it.
    ttk_from: TtkFrom::PreviousKill,
    click_kills: false,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    endless: false,
    params: &[
        ParamId::TrackTime,
        ParamId::GridActive,
        ParamId::StrafeMin,
        ParamId::StrafeMax,
        ParamId::TurnMin,
        ParamId::TurnMax,
        ParamId::YawRange,
        ParamId::PitchRange,
    ],
    tune_defaults: tune,
    spawn_initial,
    respawn: no_respawn,
    spawn_next: no_spawn_next,
};

/// Spots compared for a new bot: the one farthest from the other bots is taken.
const SPAWN_CANDIDATES: usize = 6;

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, (wander.in_set(DrillSet::Move), fire.in_set(DrillSet::Score)));
}

/// Four bots that each take 0.8 s of fire (eight rounds), drifting at 6°/s to 14°/s with smooth
/// turns: slow enough to stay on through a kill, so the work is in the switches. A kill that
/// takes a body-shot spray's time (Valorant: four or five rifle rounds) would be over before
/// the tracking starts to matter.
fn tune(config: &mut DrillConfig) {
    config.track_time = 0.8;
    config.grid_active = 4;
    config.strafe_min = 1.0;
    config.strafe_max = 2.5;
    config.turn_min = 0.6;
    config.turn_max = 1.5;
}

fn blurb(config: &DrillConfig) -> Cow<'static, str> {
    let rounds = match rounds_to_kill(config) {
        1 => "1 round".to_string(),
        n => format!("{n} rounds"),
    };
    format!("Hold fire on a moving bot until it dies ({rounds} on target), then switch to the next.").into()
}

/// A bot's health in rounds: `track_time` seconds of fire on it at [`ROUNDS_PER_SEC`], to the
/// nearest whole round and never less than one.
fn rounds_to_kill(config: &DrillConfig) -> u32 {
    (config.track_time * ROUNDS_PER_SEC).round().max(1.0) as u32
}

/// A wandering bot; angles in degrees. Its [`Health`] is the rounds it still takes.
#[derive(Component)]
struct Bot {
    /// Where it is (yaw, pitch).
    at: Vec2,
    wander: Wander,
}

/// The area bots stay in: the yaw/pitch ranges, between floor and ceiling.
fn area(config: &DrillConfig) -> (Vec2, Vec2) {
    let low = Vec2::new(-config.yaw_range, (-config.pitch_range).max(FLOOR_PITCH));
    (low, Vec2::new(config.yaw_range, config.pitch_range.min(CEILING_PITCH).max(low.y)))
}

fn position(at: Vec2) -> Vec3 {
    direction(at.x.to_radians(), at.y.to_radians()) * TARGET_DISTANCE
}

/// Where a new bot appears: on screen and away from the crosshair (as Microflick's targets),
/// inside the area, and of a few such spots the one farthest from the `others`, so it doesn't
/// come up on top of a bot that is already there.
fn spawn_spot(aim: Vec3, view: &ViewCone, margin: f32, config: &DrillConfig, others: &[Vec2]) -> Vec2 {
    let (low, high) = area(config);
    let clearance = |at: Vec2| others.iter().map(|other| at.distance_squared(*other)).fold(f32::INFINITY, f32::min);
    let mut best = (Vec2::ZERO, f32::NEG_INFINITY);
    for _ in 0..SPAWN_CANDIDATES {
        let at = angles(flick_position(aim, view, margin, config)).clamp(low, high);
        let clearance = clearance(at);
        if clearance > best.1 {
            best = (at, clearance);
        }
    }
    best.0
}

fn spawn_bot(ctx: &mut SpawnCtx, others: &[Vec2]) -> Vec2 {
    let at = spawn_spot(ctx.aim, &ctx.view, ctx.margin(), ctx.config, others);
    let bot = Bot { at, wander: Wander::new(ctx.config) };
    let health = Health::new(rounds_to_kill(ctx.config) as f32);
    ctx.spawn_target(position(at), None).insert((bot, health));
    at
}

fn spawn_initial(ctx: &mut SpawnCtx) {
    let mut bots = Vec::new();
    for _ in 0..ctx.config.grid_active {
        let at = spawn_bot(ctx, &bots);
        bots.push(at);
    }
}

/// Moves every bot around the area.
fn wander(time: Res<Time>, run: Res<Run>, mut bots: Query<(&mut Bot, &mut Transform)>) {
    if !run.live() {
        return;
    }
    let config = &run.drill.config;
    let (low, high) = area(config);
    let dt = time.delta_secs();
    for (mut bot, mut transform) in &mut bots {
        let Bot { at, wander } = &mut *bot;
        wander.step(at, low, high, dt, config);
        transform.translation = position(*at);
    }
}

/// Lights up the bot under the crosshair (the nearest one along the ray) and, while the fire
/// button is held, shoots at it: every round counts as a shot, and as a hit if a bot is under
/// the crosshair, which then loses one round of health. A bot with none left is a kill, timed
/// from the previous kill, with the hit sound and a new bot elsewhere. Rounds fired at nothing
/// (also those still due in the frame of a kill) are misses.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn fire(
    mut commands: Commands,
    gun: Res<HeldFire>,
    mut run: ResMut<Run>,
    assets: Res<TargetAssets>,
    sounds: Res<HitSounds>,
    settings: Res<Settings>,
    mut recorder: ResMut<Recorder>,
    mut next: ResMut<NextSpawn>,
    view: Res<ViewCone>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Bot>)>,
    mut bots: Query<(Entity, &mut Health, &Bot, &Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    if run.kind() != DrillKind::TargetSwitch {
        return;
    }
    let (origin, aim) = (camera.translation, camera.forward().as_vec3());
    let mut aimed = bots
        .iter()
        .filter_map(|(entity, _, _, transform, _)| {
            ray_hit(origin, aim, transform.translation, transform.scale.x).map(|t| (t, entity))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, entity)| entity);
    for (entity, _, _, _, mut material) in &mut bots {
        let wanted = if Some(entity) == aimed { &assets.hot } else { &assets.idle };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }
    if !run.live() {
        return;
    }
    let rounds = gun.rounds();
    if rounds == 0 {
        return;
    }
    let (mut landed, mut killed) = (false, None);
    for _ in 0..rounds {
        run.shot();
        let Some(entity) = aimed else { continue };
        let Ok((_, mut health, ..)) = bots.get_mut(entity) else { continue };
        run.round_hit();
        landed = true;
        if health.damage(1.0) {
            let ttk = run.elapsed - run.last_kill;
            run.kill(ttk);
            commands.entity(entity).despawn();
            sounds.play(&mut commands, &settings.audio);
            killed = aimed.take();
        }
    }
    recorder.mark_shot(landed);
    let Some(killed) = killed else { return };
    let others: Vec<Vec2> = bots.iter().filter(|bot| bot.0 != killed).map(|bot| bot.2.at).collect();
    let mut occupied = Vec::new();
    let mut ctx = SpawnCtx {
        commands: &mut commands,
        assets: &assets,
        config: &run.drill.config,
        now: run.elapsed,
        aim,
        view: *view,
        live: true,
        occupied: &mut occupied,
        next: &mut next,
    };
    spawn_bot(&mut ctx, &others);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::drill::held_fire::pull;
    use crate::drill::held_fire::tests::{FRAME_RATES, hold, rounds_due};
    use crate::drill::targets::Target;
    use crate::drill::{DrillSpec, RunPurpose};

    #[test]
    fn health_is_whole_rounds_of_fire() {
        let config = |track_time| DrillConfig { track_time, ..DrillConfig::defaults(DrillKind::TargetSwitch) };
        assert_eq!(rounds_to_kill(&DrillConfig::defaults(DrillKind::TargetSwitch)), 8);
        assert_eq!(rounds_to_kill(&config(0.3)), 3);
        assert_eq!(rounds_to_kill(&config(0.05)), 1);
        assert_eq!(rounds_to_kill(&config(10.0)), 100);
    }

    /// A bot that doesn't move (only `fire` runs), with full health.
    fn bot(config: &DrillConfig, at: Vec3) -> impl Bundle {
        (
            Bot { at: angles(at.normalize()), wander: Wander::new(config) },
            Health::new(rounds_to_kill(config) as f32),
            Target { base_radius: 0.4, born: 0.0, lifetime: None, spawn_aim: None },
            MeshMaterial3d::<StandardMaterial>::default(),
            Transform::from_translation(at).with_scale(Vec3::splat(0.4)),
        )
    }

    /// The drill at `fps` with one bot under the crosshair and one aside; the button is not held.
    fn app(config: &DrillConfig, fps: f32) -> (App, Entity, Entity) {
        let drill = DrillSpec { name: "Target switching".into(), config: config.clone() };
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(1.0 / fps)))
            .insert_resource(Settings::default())
            .insert_resource(ViewCone::default())
            .insert_resource(NextSpawn::default())
            .insert_resource(HitSounds::default())
            .insert_resource(Recorder::default())
            .insert_resource(HeldFire::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(TargetAssets {
                mesh: default(),
                idle: default(),
                hot: default(),
                wait: default(),
                cover_mesh: default(),
                cover_material: default(),
            })
            .insert_resource(Run::new(drill, 0.4, 60.0, RunPurpose::Practice, String::new()).started())
            // The run's clock, as `tick_run` keeps it after the drill's systems.
            .add_systems(
                Update,
                (pull, fire, |time: Res<Time>, mut run: ResMut<Run>| run.elapsed += time.delta_secs()).chain(),
            );
        app.world_mut().spawn((PlayerCamera, Transform::IDENTITY));
        let ahead = app.world_mut().spawn(bot(config, Vec3::new(0.0, 0.0, -10.0))).id();
        let aside = app.world_mut().spawn(bot(config, Vec3::new(5.0, 0.0, -10.0))).id();
        // The first update has no elapsed time.
        app.update();
        (app, ahead, aside)
    }

    /// Holding fire on a bot kills it after its rounds, at the same time whatever the frame
    /// rate; only that bot is hurt; the rounds fired after it died are misses; and a new bot
    /// takes its place away from the crosshair.
    #[test]
    fn held_fire_kills_the_aimed_bot_at_any_frame_rate() {
        let config = DrillConfig::defaults(DrillKind::TargetSwitch);
        for fps in FRAME_RATES {
            let (mut app, ahead, aside) = app(&config, fps);
            hold(&mut app, true);
            // Eight rounds, the first at once: the last goes off 0.7 s after the press.
            let mut killed_after = None;
            let frames = (1.5 * fps).round() as u32;
            for frame in 0..frames {
                app.update();
                if killed_after.is_none() && app.world().get_entity(ahead).is_err() {
                    killed_after = Some(frame as f32 / fps);
                }
            }
            let killed_after = killed_after.unwrap_or_else(|| panic!("{fps} fps: the bot survived 1.5 s of fire"));
            assert!((killed_after - 0.7).abs() <= 1.0 / fps + 1e-3, "{fps} fps: killed after {killed_after} s");

            let world = app.world_mut();
            assert_eq!(world.get::<Health>(aside).unwrap().fraction(), 1.0, "{fps} fps: the other bot was hurt");
            let stats = world.resource::<Run>().live_stats(&Settings::default());
            assert_eq!((stats.hits, stats.rounds_hit), (1, 8), "{fps} fps");
            let due = rounds_due(frames, fps);
            assert!(stats.shots.abs_diff(due) <= 1, "{fps} fps: {} rounds fired in 1.5 s, {due} due", stats.shots);
            assert!((stats.ttk_total - 0.7).abs() <= 1.0 / fps + 1e-3, "{fps} fps: kill time {}", stats.ttk_total);
            assert!((stats.accuracy() - 8.0 / stats.shots as f32).abs() < 1e-6);

            // Two bots again; the new one has full health and is a flick away from the crosshair.
            let mut bots = world.query::<(Entity, &Bot, &Health)>();
            let new: Vec<_> = bots.iter(world).filter(|bot| bot.0 != aside).collect();
            assert_eq!(new.len(), 1, "{fps} fps");
            assert_eq!(new[0].2.fraction(), 1.0);
            assert!(new[0].1.at.length() >= config.min_flick - 1e-3, "{fps} fps: new bot at {}", new[0].1.at);
        }
    }

    /// Fire in bursts: health only goes down by the rounds that were fired on the bot, and it
    /// stays where it was while the button is up.
    #[test]
    fn bursts_add_up_and_released_fire_does_nothing() {
        let config = DrillConfig::defaults(DrillKind::TargetSwitch);
        let (mut app, ahead, _) = app(&config, 100.0);
        let left = |app: &App| app.world().get::<Health>(ahead).map(Health::fraction);
        // 0.25 s held: rounds at 0.0, 0.1 and 0.2 s.
        hold(&mut app, true);
        for _ in 0..25 {
            app.update();
        }
        assert_eq!(left(&app), Some(5.0 / 8.0));
        hold(&mut app, false);
        for _ in 0..100 {
            app.update();
        }
        assert_eq!(left(&app), Some(5.0 / 8.0));
        let stats = app.world().resource::<Run>().live_stats(&Settings::default());
        assert_eq!((stats.hits, stats.shots, stats.rounds_hit), (0, 3, 3));
    }

    /// Fire at nothing counts as shots, hits nothing and lowers the accuracy.
    #[test]
    fn rounds_off_target_are_misses() {
        let config = DrillConfig::defaults(DrillKind::TargetSwitch);
        let (mut app, ahead, aside) = app(&config, 100.0);
        // Look well away from both bots.
        let mut cameras = app.world_mut().query_filtered::<&mut Transform, With<PlayerCamera>>();
        *cameras.single_mut(app.world_mut()).unwrap() = Transform::from_rotation(Quat::from_rotation_y(1.0));
        hold(&mut app, true);
        for _ in 0..50 {
            app.update();
        }
        let world = app.world();
        assert_eq!(world.get::<Health>(ahead).unwrap().fraction(), 1.0);
        assert_eq!(world.get::<Health>(aside).unwrap().fraction(), 1.0);
        let stats = world.resource::<Run>().live_stats(&Settings::default());
        assert_eq!((stats.hits, stats.shots, stats.rounds_hit), (0, 5, 0));
        assert_eq!((stats.accuracy(), stats.score()), (0.0, 0.0));
    }

    /// New bots come up inside the area, away from the crosshair and not on top of another bot.
    #[test]
    fn new_bots_appear_apart_from_the_others() {
        let config = DrillConfig::defaults(DrillKind::TargetSwitch);
        let (view, margin) = (ViewCone::default(), 0.06);
        let (low, high) = area(&config);
        fastrand::seed(7);
        let mut on_top = 0;
        for _ in 0..2000 {
            let others = [
                Vec2::new(fastrand::f32() * 60.0 - 30.0, fastrand::f32() * 24.0 - 12.0),
                Vec2::new(fastrand::f32() * 60.0 - 30.0, fastrand::f32() * 24.0 - 12.0),
                Vec2::new(fastrand::f32() * 60.0 - 30.0, fastrand::f32() * 24.0 - 12.0),
            ];
            let at = spawn_spot(Vec3::NEG_Z, &view, margin, &config, &others);
            assert!(at.cmpge(low).all() && at.cmple(high).all(), "{at} outside the area");
            assert!(at.length() >= config.min_flick - 1e-2, "{at} too near the crosshair");
            // Two bot diameters (9°) apart or more, almost always.
            on_top += u32::from(others.iter().any(|other| other.distance(at) < 4.6));
        }
        assert!(on_top <= 2, "{on_top} of 2000 new bots on top of another");
    }
}
