//! Behaviour every drill shares: target assets, spawning through the drill's
//! [`DrillDef`](super::def::DrillDef) hooks, click shooting, decay and delayed spawns.
//! Per-drill behaviour lives in [`super::kinds`].

use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;

use super::def::{Environment, TtkFrom};
use super::geometry::{TARGET_DISTANCE, ray_hit};
use super::kinds::peek;
use super::{DrillConfig, Run};
use crate::AppState;
use crate::camera::{PlayerCamera, ViewCone};
use crate::metrics::flick_probe::angles;
use crate::metrics::quest::RADIUS_RANGE;
use crate::metrics::throughput::Click;
use crate::replay::Recorder;
use crate::settings::Settings;
use crate::sfx::HitSounds;

/// Fraction of its size a decaying target has left when it expires.
const DECAY_MIN_SCALE: f32 = 0.35;

/// Shared target mesh and materials, also used by the replay player.
#[derive(Resource)]
pub struct TargetAssets {
    pub mesh: Handle<Mesh>,
    pub idle: Handle<StandardMaterial>,
    /// Tracking: the target under the crosshair.
    pub(crate) hot: Handle<StandardMaterial>,
    /// Reaction: a cue that hasn't lit up yet.
    pub(crate) wait: Handle<StandardMaterial>,
    pub(crate) cover_mesh: Handle<Mesh>,
    pub(crate) cover_material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub(crate) struct Target {
    pub(crate) base_radius: f32,
    /// Run time the target appeared (or lit up, for reaction cues).
    pub(crate) born: f32,
    /// Decaying targets shrink and expire after this many seconds.
    pub(crate) lifetime: Option<f32>,
    /// The player's aim (yaw, pitch degrees) when it appeared; `None` for the initial targets,
    /// which appear before the run is live.
    pub(crate) spawn_aim: Option<Vec2>,
}

/// A target that is present but not something to react to yet (an unlit reaction cue). Replays
/// don't record it.
#[derive(Component)]
pub(crate) struct Dormant;

/// The layout slot a target occupies (Gridshot cells); handed to the drill's `respawn` hook when
/// the target is cleared.
#[derive(Component)]
pub(crate) struct Slot(pub(crate) usize);

/// When the next target of a one-at-a-time drill (peeks, peripheral) appears.
#[derive(Resource, Default)]
pub(crate) struct NextSpawn(pub(crate) Option<f32>);

/// A click killed a target; `aim` is the view direction at the shot.
#[derive(Message)]
pub(crate) struct TargetKilled {
    pub(crate) aim: Vec3,
}

/// A decaying target ran out of time.
#[derive(Message)]
pub(crate) struct TargetExpired;

/// What a drill's spawn hooks get to work with.
pub struct SpawnCtx<'a, 'w, 's> {
    pub(crate) commands: &'a mut Commands<'w, 's>,
    pub(crate) assets: &'a TargetAssets,
    pub(crate) config: &'a DrillConfig,
    /// Run time; new targets are born now.
    pub(crate) now: f32,
    /// The view direction.
    pub(crate) aim: Vec3,
    /// What the player can see; new targets must appear inside it.
    pub(crate) view: ViewCone,
    /// Whether the run is live, so the flick to the new target starts at `aim` (before that, the
    /// player keeps moving through the countdown).
    pub(crate) live: bool,
    /// Slots taken by live targets (see [`Slot`]).
    pub(crate) occupied: &'a mut Vec<usize>,
    pub(crate) next: &'a mut NextSpawn,
}

impl SpawnCtx<'_, '_, '_> {
    /// Target radius for this config.
    pub(crate) fn radius(&self) -> f32 {
        self.config.kind.def().base_radius * self.config.size
    }

    /// How far (radians) a target's centre must stay inside the screen edges so all of it is
    /// visible, plus a little room. Adaptive targets are sized after spawning, so they keep room
    /// for the largest size they can get.
    pub(crate) fn margin(&self) -> f32 {
        const EDGE_ROOM: f32 = 1.5;
        let radius = match self.config.adaptive() {
            Some(_) => RADIUS_RANGE.1.to_radians(),
            None => (self.radius() / TARGET_DISTANCE).atan(),
        };
        radius + EDGE_ROOM.to_radians()
    }

    /// Lifetime of decaying targets, if this config decays; adaptive targets always get their
    /// time limit.
    pub(crate) fn lifetime(&self) -> Option<f32> {
        if self.config.adaptive().is_some() {
            return Some(self.config.time_limit());
        }
        (self.config.supports_decay() && self.config.lifetime > 0.0).then_some(self.config.lifetime)
    }

    /// Spawns a target born now at `position`, despawned when leaving the drill.
    pub(crate) fn spawn_target(&mut self, position: Vec3, lifetime: Option<f32>) -> EntityCommands<'_> {
        let radius = self.radius();
        self.commands.spawn((
            DespawnOnExit(AppState::Playing),
            Target { base_radius: radius, born: self.now, lifetime, spawn_aim: self.live.then(|| angles(self.aim)) },
            Mesh3d(self.assets.mesh.clone()),
            MeshMaterial3d(self.assets.idle.clone()),
            Transform::from_translation(position).with_scale(Vec3::splat(radius)),
        ))
    }
}

fn unlit(color: Color) -> StandardMaterial {
    StandardMaterial { base_color: color, unlit: true, ..default() }
}

fn target_color(settings: &Settings) -> Color {
    let [r, g, b] = settings.targets.color;
    Color::srgb_u8(r, g, b)
}

/// The tracking "on target" color: the target color washed towards white.
fn hot_color(settings: &Settings) -> Color {
    target_color(settings).mix(&Color::WHITE, 0.55)
}

pub(super) fn create_target_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    settings: Res<Settings>,
) {
    commands.insert_resource(TargetAssets {
        mesh: meshes.add(Sphere::new(1.0)),
        idle: materials.add(unlit(target_color(&settings))),
        hot: materials.add(unlit(hot_color(&settings))),
        wait: materials.add(unlit(Color::srgb(0.35, 0.37, 0.42))),
        cover_mesh: meshes.add(Cuboid::from_size(peek::COVER_MAX - peek::COVER_MIN)),
        cover_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.24, 0.26, 0.31),
            perceptual_roughness: 0.9,
            ..default()
        }),
    });
}

pub(super) fn sync_target_colors(
    settings: Res<Settings>,
    assets: Res<TargetAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (handle, color) in [(&assets.idle, target_color(&settings)), (&assets.hot, hot_color(&settings))] {
        if let Some(mut material) = materials.get_mut(handle)
            && material.base_color != color
        {
            material.base_color = color;
        }
    }
}

/// Spawns a drill's static scenery; used by drills and by their replays.
pub fn spawn_environment(commands: &mut Commands, assets: &TargetAssets, environment: Environment, state: AppState) {
    match environment {
        Environment::Open => {},
        Environment::Cover => peek::spawn_cover(commands, assets, state),
    }
}

/// How far along the ray a shot is stopped by the scenery (`f32::MAX` if it isn't).
fn blocked_at(environment: Environment, origin: Vec3, aim: Vec3) -> f32 {
    match environment {
        Environment::Open => f32::MAX,
        Environment::Cover => peek::cover_hit(origin, aim).unwrap_or(f32::MAX),
    }
}

/// Scene setup when entering a drill, before the player starts it.
pub(super) fn spawn_static(mut commands: Commands, assets: Res<TargetAssets>, run: Res<Run>) {
    commands.insert_resource(NextSpawn::default());
    spawn_environment(&mut commands, &assets, run.kind().def().environment, AppState::Playing);
}

/// Initial targets, spawned when the countdown starts, around `aim`: the player can already look
/// around while the ready card is up.
pub(super) fn spawn_initial(
    commands: &mut Commands,
    assets: &TargetAssets,
    next: &mut NextSpawn,
    run: &Run,
    aim: Vec3,
    view: &ViewCone,
) {
    let mut occupied = Vec::new();
    let mut ctx = SpawnCtx {
        commands,
        assets,
        config: &run.drill.config,
        now: 0.0,
        aim,
        view: *view,
        live: false,
        occupied: &mut occupied,
        next,
    };
    (run.kind().def().spawn_initial)(&mut ctx);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn shoot(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    mut run: ResMut<Run>,
    assets: Res<TargetAssets>,
    sounds: Res<HitSounds>,
    settings: Res<Settings>,
    mut recorder: ResMut<Recorder>,
    mut next: ResMut<NextSpawn>,
    mut killed: MessageWriter<TargetKilled>,
    camera: Single<&Transform, With<PlayerCamera>>,
    view: Res<ViewCone>,
    targets: Query<(Entity, &Target, &Transform, Option<&Slot>)>,
) {
    let def = run.kind().def();
    if !def.click_kills || !run.live() || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let origin = camera.translation;
    let aim = camera.forward().as_vec3();
    let blocked_at = blocked_at(def.environment, origin, aim);
    let hit = targets
        .iter()
        .filter_map(|hit| ray_hit(origin, aim, hit.2.translation, hit.2.scale.x).map(|t| (t, hit)))
        .filter(|(t, _)| *t < blocked_at)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, hit)| hit);
    // A miss was aimed at the target nearest the crosshair.
    let aimed = hit.or_else(|| {
        let off = |position: Vec3| aim.angle_between(position - origin);
        targets.iter().min_by(|a, b| off(a.2.translation).total_cmp(&off(b.2.translation)))
    });
    if let Some((_, target, transform, _)) = aimed
        && let Some(click) = flick_click(&run, target, transform, origin, aim, hit.is_some())
    {
        run.click(click);
    }
    run.shot();
    let Some((entity, target, _, slot)) = hit else {
        recorder.mark_shot(false);
        return;
    };

    let now = run.elapsed;
    // Gridshot dots wait while you shoot the others, so time them from the previous kill.
    let ttk = if def.ttk_from == TtkFrom::PreviousKill { now - run.last_kill } else { now - target.born };
    run.kill(ttk);
    run.last_kill_aim = Some(angles(aim));
    killed.write(TargetKilled { aim });
    recorder.mark_shot(true);
    commands.entity(entity).despawn();
    sounds.play(&mut commands, &settings.audio);

    let mut occupied: Vec<usize> = targets.iter().filter_map(|t| t.3.map(|s| s.0)).collect();
    let mut ctx = SpawnCtx {
        commands: &mut commands,
        assets: &assets,
        config: &run.drill.config,
        now,
        aim,
        view: *view,
        live: true,
        occupied: &mut occupied,
        next: &mut next,
    };
    (def.respawn)(&mut ctx, slot.map(|s| s.0));
}

/// A click aimed at `target` as one of its flick's clicks. The flick started when the target
/// appeared, or at the previous kill if that was later (Gridshot dots wait while you shoot the
/// others), as in [`analysis`](crate::analysis). `None` for flicks that started before the run
/// went live (the initial targets), where the starting aim isn't known.
fn flick_click(run: &Run, target: &Target, transform: &Transform, origin: Vec3, aim: Vec3, hit: bool) -> Option<Click> {
    let from_kill = run.last_kill_aim.is_some() && run.last_kill >= target.born;
    let (started, start) =
        if from_kill { (run.last_kill, run.last_kill_aim?) } else { (target.born, target.spawn_aim?) };
    // A shot in the frame the target appeared in came before it (shots are handled first).
    let first = if from_kill { !run.shot_since_kill } else { run.last_shot <= target.born };
    let to = transform.translation - origin;
    let distance = to.length().max(1e-3);
    let angular = |radius: f32| (radius / distance).atan().to_degrees();
    Some(Click {
        start,
        target: angles(to / distance),
        radius: angular(transform.scale.x),
        size: angular(target.base_radius),
        aim: angles(aim),
        time: run.elapsed - started,
        hit,
        first,
    })
}

/// Shrinks decaying targets and expires them (counted as a miss) when their time runs out.
/// Adaptive targets keep the size they were given (it is the measured quantity) and only expire.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn decay(
    mut commands: Commands,
    mut run: ResMut<Run>,
    assets: Res<TargetAssets>,
    mut next: ResMut<NextSpawn>,
    mut expired_messages: MessageWriter<TargetExpired>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Target>)>,
    view: Res<ViewCone>,
    mut targets: Query<(Entity, &Target, &mut Transform, Option<&Slot>)>,
) {
    if !run.live() {
        return;
    }
    let now = run.elapsed;
    let aim = camera.forward().as_vec3();
    let shrinks = run.drill.config.adaptive().is_none();
    let mut occupied: Vec<usize> = targets.iter().filter_map(|t| t.3.map(|s| s.0)).collect();
    let mut expired = Vec::new();
    for (entity, target, mut transform, slot) in &mut targets {
        let Some(lifetime) = target.lifetime else { continue };
        let age = (now - target.born) / lifetime;
        if age >= 1.0 {
            expired.push((entity, slot.map(|s| s.0)));
        } else if shrinks {
            transform.scale = Vec3::splat(target.base_radius * (1.0 - (1.0 - DECAY_MIN_SCALE) * age));
        }
    }
    let respawn = run.kind().def().respawn;
    for (entity, slot) in expired {
        run.expire();
        expired_messages.write(TargetExpired);
        commands.entity(entity).despawn();
        let mut ctx = SpawnCtx {
            commands: &mut commands,
            assets: &assets,
            config: &run.drill.config,
            now,
            aim,
            view: *view,
            live: true,
            occupied: &mut occupied,
            next: &mut next,
        };
        respawn(&mut ctx, slot);
    }
}

/// Spawns the next target of one-at-a-time drills once its delay has passed.
pub(super) fn spawn_pending(
    mut commands: Commands,
    run: Res<Run>,
    assets: Res<TargetAssets>,
    mut next: ResMut<NextSpawn>,
    camera: Single<&Transform, With<PlayerCamera>>,
    view: Res<ViewCone>,
) {
    let now = run.elapsed;
    if !run.live() || next.0.is_none_or(|at| now < at) {
        return;
    }
    next.0 = None;
    let mut occupied = Vec::new();
    let mut ctx = SpawnCtx {
        commands: &mut commands,
        assets: &assets,
        config: &run.drill.config,
        now,
        aim: camera.forward().as_vec3(),
        view: *view,
        live: true,
        occupied: &mut occupied,
        next: &mut next,
    };
    (run.kind().def().spawn_next)(&mut ctx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::{DrillKind, DrillSpec, RunPurpose};

    fn target(born: f32, spawn_aim: Option<Vec2>) -> Target {
        Target { base_radius: 0.2, born, lifetime: None, spawn_aim }
    }

    #[test]
    fn flick_clicks_start_at_the_later_of_spawn_and_previous_kill() {
        let settings = Settings::default();
        let drill = DrillSpec::builtin(DrillKind::Gridshot, &settings);
        let mut run = Run::new(drill, 0.4, 30.0, RunPurpose::Practice, String::new());
        // Straight ahead, 10 m out, shrunk to 0.1 m by decay.
        let at = Transform::from_xyz(0.0, 0.0, -10.0).with_scale(Vec3::splat(0.1));
        let aim = Vec3::new(-0.05, 0.0, -10.0).normalize();
        let click = |run: &Run, target: &Target| flick_click(run, target, &at, Vec3::ZERO, aim, true);

        // An initial target before any kill: where the flick started isn't known.
        let initial = target(0.0, None);
        assert_eq!(click(&run, &initial), None);

        // After a kill at 1 s aiming 5 degrees left, the waiting target's flick starts there.
        run.elapsed = 1.0;
        run.shot();
        run.kill(0.3);
        run.last_kill_aim = Some(Vec2::new(5.0, 0.0));
        run.elapsed = 1.4;
        let first = click(&run, &initial).unwrap();
        assert_eq!((first.start, first.target), (Vec2::new(5.0, 0.0), Vec2::ZERO));
        assert!((first.time - 0.4).abs() < 1e-6 && first.first && first.hit);
        assert!((first.aim.x - 0.05f32.atan2(10.0).to_degrees()).abs() < 1e-3);
        assert!((first.radius - 0.01f32.atan().to_degrees()).abs() < 1e-4);
        assert!((first.size - 0.02f32.atan().to_degrees()).abs() < 1e-4);
        // A miss first: the next click is a correction.
        run.shot();
        assert!(!click(&run, &initial).unwrap().first);

        // A target that appeared after the kill starts from the aim at its appearance.
        let later = target(1.2, Some(Vec2::new(-3.0, 2.0)));
        let second = click(&run, &later).unwrap();
        assert_eq!(second.start, Vec2::new(-3.0, 2.0));
        assert!((second.time - 0.2).abs() < 1e-6);
        // The miss at 1.4 s came after it appeared.
        assert!(!second.first);
        // One appearing in the frame of that miss (after the shots) is still unclicked.
        assert!(click(&run, &target(1.4, Some(Vec2::ZERO))).unwrap().first);
    }
}
