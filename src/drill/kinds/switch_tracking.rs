//! Switch tracking: several dots wander around; holding fire on one for long enough breaks it,
//! and a new one appears elsewhere. Tracking with a target switch after every kill.

use std::borrow::Cow;

use bevy::prelude::*;

use super::microflick::flick_position;
use crate::camera::{PlayerCamera, ViewCone};
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_respawn, no_spawn_next};
use crate::drill::geometry::{CEILING_PITCH, FLOOR_PITCH, TARGET_DISTANCE, direction, ray_hit};
use crate::drill::health::Health;
use crate::drill::held_fire::HeldFire;
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{NextSpawn, SpawnCtx, Target, TargetAssets};
use crate::drill::wander::Wander;
use crate::drill::{DrillConfig, DrillKind, DrillSet, Run};
use crate::metrics::flick_probe::angles;
use crate::replay::Recorder;
use crate::settings::Settings;
use crate::sfx::HitSounds;

pub const DEF: DrillDef = DrillDef {
    name: "Switch tracking",
    blurb,
    category: Category::Aim,
    base_radius: 0.35,
    decay: DecayRule::None,
    score: ScoreModel::Switch,
    ttk_from: TtkFrom::Spawn,
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

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, (wander.in_set(DrillSet::Move), track_and_break.in_set(DrillSet::Score)));
}

/// Three dots at once, wandering at 6°/s to 17°/s with smooth turns.
fn tune(config: &mut DrillConfig) {
    config.grid_active = 3;
    config.strafe_min = 1.0;
    config.strafe_max = 3.0;
    config.turn_min = 0.5;
    config.turn_max = 1.2;
}

fn blurb(config: &DrillConfig) -> Cow<'static, str> {
    format!("Hold fire on a moving dot for {:.2} s to break it, then switch to the next.", config.track_time).into()
}

/// A wandering dot; angles in degrees. Its [`Health`] is the seconds of fire it still takes.
#[derive(Component)]
struct Wanderer {
    /// Where it is (yaw, pitch).
    at: Vec2,
    wander: Wander,
}

/// The area dots stay in: the yaw/pitch ranges, between floor and ceiling.
fn area(config: &DrillConfig) -> (Vec2, Vec2) {
    let low = Vec2::new(-config.yaw_range, (-config.pitch_range).max(FLOOR_PITCH));
    (low, Vec2::new(config.yaw_range, config.pitch_range.min(CEILING_PITCH).max(low.y)))
}

fn spawn_initial(ctx: &mut SpawnCtx) {
    for _ in 0..ctx.config.grid_active {
        spawn_dot(ctx);
    }
}

/// A new dot on screen, away from the crosshair, inside the area.
fn spawn_dot(ctx: &mut SpawnCtx) {
    let at = angles(flick_position(ctx.aim, &ctx.view, ctx.margin(), ctx.config));
    let (low, high) = area(ctx.config);
    let wanderer = Wanderer { at: at.clamp(low, high), wander: Wander::new(ctx.config) };
    let health = Health::new(ctx.config.track_time);
    ctx.spawn_target(position(wanderer.at), None).insert((wanderer, health));
}

fn position(at: Vec2) -> Vec3 {
    direction(at.x.to_radians(), at.y.to_radians()) * TARGET_DISTANCE
}

/// Moves every dot around the area.
fn wander(time: Res<Time>, run: Res<Run>, mut dots: Query<(&mut Wanderer, &mut Transform)>) {
    if !run.live() {
        return;
    }
    let config = &run.drill.config;
    let (low, high) = area(config);
    let dt = time.delta_secs();
    for (mut dot, mut transform) in &mut dots {
        let Wanderer { at, wander, .. } = &mut *dot;
        wander.step(at, low, high, dt, config);
        transform.translation = position(*at);
    }
}

/// While the fire button is held, drains the dot under the crosshair (lit up while it is hit;
/// its bar shows what is left) and counts the time on target, and breaks it once its time is
/// used up: a kill timed from its spawn, with the hit sound, and a new dot elsewhere. Every
/// round fired is a shot, and a hit if a dot is under the crosshair.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn track_and_break(
    mut commands: Commands,
    time: Res<Time>,
    gun: Res<HeldFire>,
    mut run: ResMut<Run>,
    mut recorder: ResMut<Recorder>,
    assets: Res<TargetAssets>,
    sounds: Res<HitSounds>,
    settings: Res<Settings>,
    mut next: ResMut<NextSpawn>,
    view: Res<ViewCone>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Wanderer>)>,
    mut dots: Query<(Entity, &mut Health, &Target, &Transform, &mut MeshMaterial3d<StandardMaterial>), With<Wanderer>>,
) {
    // The rounds are counted here whether a dot is hit or not: only in this drill.
    if run.kind() != DrillKind::SwitchTracking {
        return;
    }
    let (origin, aim) = (camera.translation, camera.forward().as_vec3());
    let dt = time.delta_secs();
    // Only the nearest dot under the crosshair is tracked.
    let tracked = dots
        .iter()
        .filter_map(|(entity, _, _, transform, _)| {
            ray_hit(origin, aim, transform.translation, transform.scale.x).map(|t| (t, entity))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, entity)| entity);
    if run.live() {
        for _ in 0..gun.rounds() {
            run.shot();
            if tracked.is_some() {
                run.round_hit();
            }
        }
        if gun.rounds() > 0 {
            recorder.mark_shot(tracked.is_some());
        }
    }
    let mut broken = 0;
    for (entity, mut health, target, _, mut material) in &mut dots {
        let on = Some(entity) == tracked;
        // Before the run starts the dot under the crosshair lights up; in it, the one being hit.
        let wanted = if on && (gun.held() || !run.live()) { &assets.hot } else { &assets.idle };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
        if !on || !run.live() || !gun.held() {
            continue;
        }
        run.track_on_target(dt);
        if health.damage(dt) {
            let ttk = run.elapsed - target.born;
            run.kill(ttk);
            commands.entity(entity).despawn();
            sounds.play(&mut commands, &settings.audio);
            broken += 1;
        }
    }
    if broken == 0 {
        return;
    }
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
    for _ in 0..broken {
        spawn_dot(&mut ctx);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::drill::held_fire::pull;
    use crate::drill::held_fire::tests::hold;
    use crate::drill::{DrillSpec, RunPurpose};

    /// A dot that doesn't move (only `track_and_break` runs), with `left` seconds to go.
    fn dot(left: f32) -> (Wanderer, Health, Target, MeshMaterial3d<StandardMaterial>) {
        let wander = Wander::new(&DrillConfig::defaults(DrillKind::SwitchTracking));
        let still = Wanderer { at: Vec2::ZERO, wander };
        let target = Target { base_radius: 0.35, born: 0.0, lifetime: None, spawn_aim: None };
        (still, Health::new(left), target, MeshMaterial3d::default())
    }

    /// The drill's scoring system alone, with the fire button up.
    fn app(drill: DrillSpec) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)))
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
            .add_systems(Update, (pull, track_and_break).chain());
        app.world_mut().spawn((PlayerCamera, Transform::IDENTITY));
        app
    }

    /// Nothing drains while the fire button is up. Held, only the dot under the crosshair drains;
    /// once its time is used up it breaks (a kill) and a new dot takes its place, so the number
    /// of dots stays the same; and the rounds fired were all hits.
    #[test]
    fn fired_at_dot_breaks_and_is_replaced() {
        let config = DrillConfig { track_time: 0.25, ..DrillConfig::defaults(DrillKind::SwitchTracking) };
        let mut app = app(DrillSpec { name: "Switch tracking".into(), config });
        let ahead =
            app.world_mut().spawn((dot(0.25), Transform::from_xyz(0.0, 0.0, -10.0).with_scale(Vec3::splat(0.35)))).id();
        let aside =
            app.world_mut().spawn((dot(0.25), Transform::from_xyz(5.0, 0.0, -10.0).with_scale(Vec3::splat(0.35)))).id();
        // The first update has no elapsed time. Half a second on the dot without firing does
        // nothing to it.
        for _ in 0..6 {
            app.update();
        }
        assert_eq!(app.world().get::<Health>(ahead).unwrap().fraction(), 1.0, "a dot drained without fire");
        assert_eq!(app.world().resource::<Run>().live_stats(&Settings::default()).on_target, 0.0);
        // Three updates of fire add 0.3 s on the dot ahead.
        hold(&mut app, true);
        for _ in 0..3 {
            app.update();
        }
        let world = app.world_mut();
        assert!(world.get_entity(ahead).is_err(), "tracked dot still there");
        assert_eq!(world.get::<Health>(aside).unwrap().fraction(), 1.0, "untracked dot drained");
        assert_eq!(world.query::<&Wanderer>().iter(world).count(), 2);
        let stats = world.resource::<Run>().live_stats(&Settings::default());
        assert_eq!(stats.hits, 1);
        assert!(stats.on_target > 0.2);
        // Rounds at 0, 0.1 and 0.2 s of fire, all on a dot.
        assert_eq!((stats.shots, stats.rounds_hit, stats.accuracy()), (3, 3, 1.0));
    }

    /// The other held-fire drills count their own rounds: this drill's system leaves a Tracking
    /// run's alone (it once booked every round a second time, as a miss).
    #[test]
    fn rounds_of_other_drills_are_not_counted_here() {
        let config = DrillConfig::defaults(DrillKind::Tracking);
        let mut app = app(DrillSpec { name: "Tracking".into(), config });
        hold(&mut app, true);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(app.world().resource::<Run>().live_stats(&Settings::default()).shots, 0);
    }
}
