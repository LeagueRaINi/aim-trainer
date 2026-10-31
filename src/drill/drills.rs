//! Per-drill target behaviour: spawning, shooting, decay, strafing, peeks and reaction cues.
//! Everything tunable comes from the run's [`DrillConfig`].

use bevy::prelude::*;

use super::{
    DrillConfig, DrillKind, Run,
    def::{Environment, TtkFrom},
    flick_probe::{FlickProbe, angles, screen_offset},
};
use crate::{AppState, camera::PlayerCamera, replay::Recorder, settings::Settings, sfx::HitSounds};

/// Distance from the camera to the plane/sphere targets live on.
const TARGET_DISTANCE: f32 = 10.0;
/// Fraction of its size a decaying target has left when it expires.
const DECAY_MIN_SCALE: f32 = 0.35;
/// How long a lit reaction cue waits for a click before it counts as missed.
const REACTION_WINDOW: f32 = 1.0;
/// Lowest pitch (degrees) a constrained flick target may sit at: lower ones (at
/// `TARGET_DISTANCE`) would sink into the floor 2.5 m below the eye.
const FLOOR_PITCH: f32 = -13.0;
/// Attempts at finding a constrained flick target before falling back to a reset target.
const CONSTRAINED_TRIES: usize = 32;

/// Peek cover: a box between the player and the target plane. Targets start hidden behind it.
const COVER_MIN: Vec3 = Vec3::new(-1.5, -2.5, -8.75);
const COVER_MAX: Vec3 = Vec3::new(1.5, 0.9, -8.25);
/// Peeking targets start (and end) at this |x|, fully hidden behind the cover.
const PEEK_HIDDEN_X: f32 = 1.2;

/// Shared target mesh and materials, also used by the replay player.
#[derive(Resource)]
pub struct TargetAssets {
    pub mesh: Handle<Mesh>,
    pub idle: Handle<StandardMaterial>,
    hot: Handle<StandardMaterial>,
    wait: Handle<StandardMaterial>,
    cover_mesh: Handle<Mesh>,
    cover_material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub struct Target {
    base_radius: f32,
    /// Run time the target appeared (or lit up, for reaction cues).
    born: f32,
    /// Decaying targets shrink and expire after this many seconds.
    lifetime: Option<f32>,
}

#[derive(Component)]
pub(super) struct GridSlot(usize);

#[derive(Component)]
pub(super) struct Strafer {
    velocity: f32,
    turn_in: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum PeekPhase {
    Out,
    Hold(f32),
    Back,
}

#[derive(Component)]
pub(super) struct PeekMove {
    side: f32,
    speed: f32,
    out_x: f32,
    phase: PeekPhase,
}

#[derive(Component)]
pub struct ReactionCue {
    lit_at: f32,
    lit: bool,
}

impl ReactionCue {
    pub fn is_lit(&self) -> bool {
        self.lit
    }
}

/// When the next target of a one-at-a-time drill (peeks, peripheral) appears.
#[derive(Resource, Default)]
pub(super) struct NextSpawn(Option<f32>);

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
        cover_mesh: meshes.add(Cuboid::from_size(COVER_MAX - COVER_MIN)),
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

/// The peek drill's cover box, also shown in replays of that drill.
pub fn spawn_cover(commands: &mut Commands, assets: &TargetAssets, state: AppState) {
    commands.spawn((
        DespawnOnExit(state),
        Mesh3d(assets.cover_mesh.clone()),
        MeshMaterial3d(assets.cover_material.clone()),
        // Primitive meshes are centered on the origin; move it into place.
        Transform::from_translation((COVER_MIN + COVER_MAX) * 0.5),
    ));
}

/// Scene setup when entering a drill, before the player starts it.
pub(super) fn spawn_static(mut commands: Commands, assets: Res<TargetAssets>, run: Res<Run>) {
    commands.insert_resource(NextSpawn::default());
    if run.kind().def().environment == Environment::Cover {
        spawn_cover(&mut commands, &assets, AppState::Playing);
    }
}

fn direction(yaw: f32, pitch: f32) -> Vec3 {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0) * Vec3::NEG_Z
}

fn random_range(min: f32, max: f32) -> f32 {
    min + fastrand::f32() * (max - min)
}

fn random_sign() -> f32 {
    if fastrand::bool() { 1.0 } else { -1.0 }
}

/// Distance along the ray to the sphere, if the ray hits it.
fn ray_hit(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let to_center = center - origin;
    let t = to_center.dot(dir);
    let miss_sq = to_center.length_squared() - t * t;
    (t > 0.0 && miss_sq <= radius * radius).then_some(t)
}

/// Distance along the ray to an axis-aligned box, if the ray hits it (slab method).
fn ray_box(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = dir.recip();
    let (t1, t2) = ((min - origin) * inv, (max - origin) * inv);
    let near = t1.min(t2).max_element();
    let far = t1.max(t2).min_element();
    (far >= near.max(0.0)).then_some(near.max(0.0))
}

fn base_radius(config: &DrillConfig) -> f32 {
    config.kind.def().base_radius * config.size
}

fn lifetime(config: &DrillConfig) -> Option<f32> {
    (config.supports_decay() && config.lifetime > 0.0).then_some(config.lifetime)
}

fn grid_cells(config: &DrillConfig) -> usize {
    (config.grid_cols * config.grid_rows) as usize
}

/// Cells are centered on the crosshair, `grid_spacing` degrees apart.
fn grid_position(slot: usize, config: &DrillConfig) -> Vec3 {
    let cols = config.grid_cols as usize;
    let col = (slot % cols) as f32 - (config.grid_cols as f32 - 1.0) * 0.5;
    let row = (slot / cols) as f32 - (config.grid_rows as f32 - 1.0) * 0.5;
    direction((col * config.grid_spacing).to_radians(), (row * config.grid_spacing).to_radians())
        * TARGET_DISTANCE
}

/// Draws random directions until one is at least `min_deg` away from `aim`.
fn away_from(aim: Vec3, min_deg: f32, mut pick: impl FnMut() -> Vec3) -> Vec3 {
    let max_cos = min_deg.to_radians().cos();
    let mut dir = pick();
    for _ in 0..16 {
        if dir.dot(aim) < max_cos {
            break;
        }
        dir = pick();
    }
    dir * TARGET_DISTANCE
}

/// Microflick / Head level: anywhere within the yaw/pitch ranges, away from the crosshair.
/// Head level has a pitch range of 0, so its targets always sit exactly at head height.
fn flick_position(aim: Vec3, config: &DrillConfig) -> Vec3 {
    if config.flick_constrained() {
        return constrained_flick_position(aim, config);
    }
    away_from(aim, config.min_flick, || {
        direction(
            random_range(-config.yaw_range, config.yaw_range).to_radians(),
            random_range(-config.pitch_range, config.pitch_range).to_radians(),
        )
    })
}

/// The (yaw, pitch) area constrained flick targets stay in: the yaw/pitch ranges, above the floor.
fn flick_area(config: &DrillConfig) -> (Vec2, Vec2) {
    let min = Vec2::new(-config.yaw_range, (-config.pitch_range).max(FLOOR_PITCH));
    (min, Vec2::new(config.yaw_range, config.pitch_range.max(min.y)))
}

/// Microflick aimed at a weak spot: `min_flick..max_flick` degrees from the aim, towards
/// `flick_direction +- flick_spread` on screen, inside [`flick_area`]. Repeated flicks the same
/// way walk the aim to the edge of the area; once no such target fits, the next one is a reset
/// target on the opposite side of the area, which leaves room for the trained flicks again.
fn constrained_flick_position(aim: Vec3, config: &DrillConfig) -> Vec3 {
    let from = angles(aim);
    let (near, far) = config.flick_distance_range();
    let (min, max) = flick_area(config);
    let to_position = |p: Vec2| direction(p.x.to_radians(), p.y.to_radians()) * TARGET_DISTANCE;
    for _ in 0..CONSTRAINED_TRIES {
        let angle = match config.flick_direction {
            Some(center) => center + random_range(-config.flick_spread, config.flick_spread),
            None => random_range(0.0, 360.0),
        };
        let candidate = from + screen_offset(angle, random_range(near, far));
        if candidate.cmpge(min).all() && candidate.cmple(max).all() {
            return to_position(candidate);
        }
    }
    let middle = (min + max) * 0.5;
    let reset = match config.flick_direction {
        // As far back as the area allows, for the longest run of trained flicks.
        Some(center) => middle - screen_offset(center, far),
        None => middle,
    };
    to_position(reset.clamp(min, max))
}

/// Peripheral: at least `min_flick` degrees to the side, so the eyes have to catch it first.
fn peripheral_position(aim: Vec3, config: &DrillConfig) -> Vec3 {
    let near = config.min_flick.min(config.yaw_range);
    away_from(aim, near * 0.8, || {
        direction(
            (random_sign() * random_range(near, config.yaw_range)).to_radians(),
            random_range(-config.pitch_range, config.pitch_range).to_radians(),
        )
    })
}

fn spawn_target(
    commands: &mut Commands,
    assets: &TargetAssets,
    position: Vec3,
    radius: f32,
    born: f32,
    lifetime: Option<f32>,
) -> Entity {
    commands
        .spawn((
            DespawnOnExit(AppState::Playing),
            Target { base_radius: radius, born, lifetime },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.idle.clone()),
            Transform::from_translation(position).with_scale(Vec3::splat(radius)),
        ))
        .id()
}

/// Initial targets, spawned when the countdown starts.
pub(super) fn spawn_initial(commands: &mut Commands, assets: &TargetAssets, run: &Run) {
    let config = &run.drill.config;
    let radius = base_radius(config);
    let lifetime = lifetime(config);
    match config.kind {
        DrillKind::Gridshot => {
            let mut slots: Vec<usize> = (0..grid_cells(config)).collect();
            fastrand::shuffle(&mut slots);
            for &slot in &slots[..config.grid_active as usize] {
                let id = spawn_target(commands, assets, grid_position(slot, config), radius, 0.0, lifetime);
                commands.entity(id).insert(GridSlot(slot));
            }
        }
        DrillKind::Microflick | DrillKind::HeadLevel => {
            spawn_target(commands, assets, flick_position(Vec3::NEG_Z, config), radius, 0.0, lifetime);
        }
        DrillKind::Tracking => {
            let id = spawn_target(commands, assets, Vec3::new(0.0, 0.0, -TARGET_DISTANCE), radius, 0.0, None);
            commands.entity(id).insert(Strafer { velocity: config.strafe_min, turn_in: config.turn_max });
        }
        DrillKind::Reaction => {
            let id = spawn_target(commands, assets, Vec3::new(0.0, 0.0, -TARGET_DISTANCE), radius, 0.0, None);
            let lit_at = random_range(config.delay_min, config.delay_max);
            commands.entity(id).insert((ReactionCue { lit_at, lit: false }, MeshMaterial3d(assets.wait.clone())));
        }
        DrillKind::Peek => commands.insert_resource(NextSpawn(Some(0.6))),
        DrillKind::Peripheral => commands.insert_resource(NextSpawn(Some(0.4))),
    }
}

/// Replaces a target that was killed or expired, keeping the drill going.
#[allow(clippy::too_many_arguments)]
fn replace(
    commands: &mut Commands,
    assets: &TargetAssets,
    config: &DrillConfig,
    now: f32,
    aim: Vec3,
    cleared_slot: Option<usize>,
    occupied: &mut Vec<usize>,
    next: &mut NextSpawn,
) {
    let radius = base_radius(config);
    let lifetime = lifetime(config);
    match config.kind {
        DrillKind::Gridshot => {
            occupied.retain(|s| Some(*s) != cleared_slot);
            let Some(slot) = pick_grid_slot(grid_cells(config), occupied, cleared_slot) else { return };
            occupied.push(slot);
            let id = spawn_target(commands, assets, grid_position(slot, config), radius, now, lifetime);
            commands.entity(id).insert(GridSlot(slot));
        }
        DrillKind::Microflick | DrillKind::HeadLevel => {
            spawn_target(commands, assets, flick_position(aim, config), radius, now, lifetime);
        }
        DrillKind::Peek => next.0 = Some(now + random_range(0.5, 1.4)),
        DrillKind::Peripheral => next.0 = Some(now + random_range(0.25, 0.6)),
        DrillKind::Tracking | DrillKind::Reaction => {}
    }
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
    camera: Single<&Transform, With<PlayerCamera>>,
    targets: Query<(Entity, &Target, &Transform, Option<&GridSlot>)>,
) {
    let def = run.kind().def();
    if !def.click_kills || !run.live() || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    run.shot();

    let origin = camera.translation;
    let aim = camera.forward().as_vec3();
    let blocked_at = if def.environment == Environment::Cover {
        ray_box(origin, aim, COVER_MIN, COVER_MAX).unwrap_or(f32::MAX)
    } else {
        f32::MAX
    };
    let Some((entity, target, _, slot)) = targets
        .iter()
        .filter_map(|hit| ray_hit(origin, aim, hit.2.translation, hit.2.scale.x).map(|t| (t, hit)))
        .filter(|(t, _)| *t < blocked_at)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, hit)| hit)
    else {
        recorder.mark_shot(false);
        return;
    };

    let now = run.elapsed;
    // Gridshot dots wait while you shoot the others, so time them from the previous kill.
    let ttk = if def.ttk_from == TtkFrom::PreviousKill { now - run.last_kill } else { now - target.born };
    run.kill(ttk);
    if let Some(probe) = run.probe.take() {
        run.land(probe.landing(angles(aim)));
    }
    recorder.mark_shot(true);
    commands.entity(entity).despawn();
    sounds.play(&mut commands, &settings.audio);

    let mut occupied: Vec<usize> = targets.iter().filter_map(|t| t.3.map(|s| s.0)).collect();
    let config = run.drill.config.clone();
    replace(&mut commands, &assets, &config, now, aim, slot.map(|s| s.0), &mut occupied, &mut next);
}

/// Shrinks decaying targets and expires them (counted as a miss) when their time runs out.
#[allow(clippy::type_complexity)]
pub(super) fn decay(
    mut commands: Commands,
    mut run: ResMut<Run>,
    assets: Res<TargetAssets>,
    mut next: ResMut<NextSpawn>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Target>)>,
    mut targets: Query<(Entity, &Target, &mut Transform, Option<&GridSlot>)>,
) {
    if !run.live() {
        return;
    }
    let now = run.elapsed;
    let aim = camera.forward().as_vec3();
    let mut occupied: Vec<usize> = targets.iter().filter_map(|t| t.3.map(|s| s.0)).collect();
    let mut expired = Vec::new();
    for (entity, target, mut transform, slot) in &mut targets {
        let Some(lifetime) = target.lifetime else { continue };
        let age = (now - target.born) / lifetime;
        if age >= 1.0 {
            expired.push((entity, slot.map(|s| s.0)));
        } else {
            transform.scale = Vec3::splat(target.base_radius * (1.0 - (1.0 - DECAY_MIN_SCALE) * age));
        }
    }
    let config = run.drill.config.clone();
    for (entity, slot) in expired {
        run.expire();
        run.probe = None;
        commands.entity(entity).despawn();
        replace(&mut commands, &assets, &config, now, aim, slot, &mut occupied, &mut next);
    }
}

/// Spawns the next target of one-at-a-time drills once its delay has passed.
pub(super) fn spawn_pending(
    mut commands: Commands,
    run: Res<Run>,
    assets: Res<TargetAssets>,
    mut next: ResMut<NextSpawn>,
    camera: Single<&Transform, With<PlayerCamera>>,
) {
    let now = run.elapsed;
    if !run.live() || next.0.is_none_or(|at| now < at) {
        return;
    }
    next.0 = None;
    let config = &run.drill.config;
    let radius = base_radius(config);
    match config.kind {
        DrillKind::Peek => {
            let side = random_sign();
            let position = Vec3::new(side * PEEK_HIDDEN_X, 0.0, -TARGET_DISTANCE);
            let id = spawn_target(&mut commands, &assets, position, radius, now, None);
            commands.entity(id).insert(PeekMove {
                side,
                speed: random_range(config.peek_speed_min, config.peek_speed_max),
                out_x: random_range(2.6, 3.6),
                phase: PeekPhase::Out,
            });
        }
        DrillKind::Peripheral => {
            let position = peripheral_position(camera.forward().as_vec3(), config);
            spawn_target(&mut commands, &assets, position, radius, now, lifetime(config));
        }
        _ => {}
    }
}

/// Peeks swing out from behind the cover, hold briefly, then retreat. Retreating to safety
/// counts as a miss.
pub(super) fn move_peeks(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextSpawn>,
    mut peeks: Query<(Entity, &mut PeekMove, &mut Transform)>,
) {
    if !run.live() {
        return;
    }
    let dt = time.delta_secs();
    let (hold_min, hold_max) = (run.drill.config.hold_min, run.drill.config.hold_max);
    for (entity, mut peek, mut transform) in &mut peeks {
        let x = &mut transform.translation.x;
        match peek.phase {
            PeekPhase::Out => {
                *x += peek.side * peek.speed * dt;
                if x.abs() >= peek.out_x {
                    peek.phase = PeekPhase::Hold(random_range(hold_min, hold_max));
                }
            }
            PeekPhase::Hold(left) => {
                peek.phase = if left > dt { PeekPhase::Hold(left - dt) } else { PeekPhase::Back };
            }
            PeekPhase::Back => {
                *x -= peek.side * peek.speed * dt;
                if x.abs() <= PEEK_HIDDEN_X {
                    run.expire();
                    commands.entity(entity).despawn();
                    next.0 = Some(run.elapsed + random_range(0.5, 1.4));
                }
            }
        }
    }
}

/// ADAD-style strafing: random speed, random direction changes, bounded to the lane.
pub(super) fn move_strafers(time: Res<Time>, run: Res<Run>, mut strafers: Query<(&mut Strafer, &mut Transform)>) {
    let config = &run.drill.config;
    let dt = time.delta_secs();
    for (mut strafer, mut transform) in &mut strafers {
        transform.translation.x += strafer.velocity * dt;
        strafer.turn_in -= dt;
        let out_of_bounds = transform.translation.x.abs() > config.lane;
        if strafer.turn_in <= 0.0 || out_of_bounds {
            let direction = if out_of_bounds {
                -transform.translation.x.signum()
            } else {
                -strafer.velocity.signum()
            };
            strafer.velocity = direction * random_range(config.strafe_min, config.strafe_max);
            strafer.turn_in = random_range(config.turn_min, config.turn_max);
        }
    }
}

pub(super) fn track(
    time: Res<Time>,
    mut run: ResMut<Run>,
    assets: Res<TargetAssets>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Strafer>)>,
    mut targets: Query<(&Transform, &mut MeshMaterial3d<StandardMaterial>), With<Strafer>>,
) {
    let origin = camera.translation;
    let aim = camera.forward().as_vec3();
    for (transform, mut material) in &mut targets {
        let on_target = ray_hit(origin, aim, transform.translation, transform.scale.x).is_some();
        let wanted = if on_target { &assets.hot } else { &assets.idle };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
        if on_target && run.live() {
            run.track_on_target(time.delta_secs());
        }
    }
}

/// Microflick: measures where the first flick towards each new target lands. Runs after
/// `shoot`/`decay`, which finish (kill) or drop (expire) the previous target's probe.
pub(super) fn probe_flicks(
    time: Res<Time>,
    mut run: ResMut<Run>,
    camera: Single<&Transform, With<PlayerCamera>>,
    spawned: Query<&Transform, Added<Target>>,
) {
    if !run.kind().def().first_flick_probe {
        return;
    }
    let aim = angles(camera.forward().as_vec3());
    if let Some(target) = spawned.iter().last() {
        run.probe = FlickProbe::new(aim, angles((target.translation - camera.translation).normalize_or_zero()));
    } else if run.live()
        && let Some(landing) = run.probe.as_mut().and_then(|probe| probe.update(aim, time.delta_secs()))
    {
        run.probe = None;
        run.land(landing);
    }
}

/// Reaction drill: the cue lights up after a random delay; click as fast as possible.
/// Clicking before it lights up, or not at all within the window, counts as a miss.
#[allow(clippy::too_many_arguments)]
pub(super) fn reaction(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    mut run: ResMut<Run>,
    assets: Res<TargetAssets>,
    sounds: Res<HitSounds>,
    settings: Res<Settings>,
    mut recorder: ResMut<Recorder>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<ReactionCue>)>,
    mut cues: Query<(&mut ReactionCue, &mut Target, &Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    if !run.live() {
        return;
    }
    let now = run.elapsed;
    let (delay_min, delay_max) = (run.drill.config.delay_min, run.drill.config.delay_max);
    let clicked = mouse.just_pressed(MouseButton::Left);
    for (mut cue, mut target, transform, mut material) in &mut cues {
        let just_lit = !cue.lit && now >= cue.lit_at;
        if just_lit {
            cue.lit = true;
            target.born = now;
            material.0 = assets.idle.clone();
        }
        let done = if clicked {
            run.shot();
            let aim = camera.forward().as_vec3();
            let aimed = ray_hit(camera.translation, aim, transform.translation, transform.scale.x).is_some();
            let hit = reaction_hit(cue.lit, just_lit, aimed);
            recorder.mark_shot(hit);
            if hit {
                run.kill(now - target.born);
                sounds.play(&mut commands, &settings.audio);
            }
            true
        } else if cue.lit && now - target.born > REACTION_WINDOW {
            run.expire();
            true
        } else {
            false
        };
        if done {
            cue.lit = false;
            cue.lit_at = now + random_range(delay_min, delay_max);
            material.0 = assets.wait.clone();
        }
    }
}

/// A reaction click hits only if the cue was already visible: a click in the frame it lights up
/// was made before it could be seen, so it counts as early (a miss), never a 0 ms reaction.
fn reaction_hit(lit: bool, just_lit: bool, aimed: bool) -> bool {
    lit && !just_lit && aimed
}

/// A random free grid cell. The cell just cleared is avoided so every kill forces a new flick,
/// unless it is the only free one (a 1x1 grid).
fn pick_grid_slot(cells: usize, occupied: &[usize], cleared_slot: Option<usize>) -> Option<usize> {
    let free: Vec<usize> = (0..cells).filter(|s| !occupied.contains(s) && Some(*s) != cleared_slot).collect();
    if free.is_empty() {
        return cleared_slot.filter(|s| *s < cells && !occupied.contains(s));
    }
    Some(free[fastrand::usize(..free.len())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::flick_probe::{screen_angle, wrap};

    #[test]
    fn single_cell_grid_respawns_in_place() {
        assert_eq!(pick_grid_slot(1, &[], Some(0)), Some(0));
        assert_eq!(pick_grid_slot(1, &[], None), Some(0));
        // With other cells free, the cleared one is never reused.
        for _ in 0..50 {
            assert_eq!(pick_grid_slot(2, &[], Some(0)), Some(1));
        }
        assert_eq!(pick_grid_slot(2, &[0, 1], None), None);
    }

    #[test]
    fn click_in_the_frame_the_cue_lights_is_early() {
        assert!(!reaction_hit(true, true, true));
        assert!(reaction_hit(true, false, true));
        assert!(!reaction_hit(false, false, true));
        assert!(!reaction_hit(true, false, false));
    }

    #[test]
    fn cover_blocks_rays_through_it_only() {
        // Straight ahead goes through the cover.
        assert!(ray_box(Vec3::ZERO, Vec3::NEG_Z, COVER_MIN, COVER_MAX).is_some());
        // Aiming at a peek past the edge clears it.
        let past_edge = Vec3::new(2.5, 0.0, -TARGET_DISTANCE).normalize();
        assert!(ray_box(Vec3::ZERO, past_edge, COVER_MIN, COVER_MAX).is_none());
        // A hidden peek position is behind the cover.
        let hidden = Vec3::new(PEEK_HIDDEN_X + 0.16, 0.0, -TARGET_DISTANCE).normalize();
        assert!(ray_box(Vec3::ZERO, hidden, COVER_MIN, COVER_MAX).is_some());
    }

    #[test]
    fn head_level_targets_stay_at_eye_height() {
        let config = DrillConfig::defaults(DrillKind::HeadLevel);
        for _ in 0..50 {
            assert!(flick_position(Vec3::NEG_Z, &config).y.abs() < 1e-4);
        }
    }

    /// A Microflick config trained on down-left flicks of 15-25 degrees.
    fn down_left() -> DrillConfig {
        let mut config = DrillConfig {
            kind: DrillKind::Microflick,
            yaw_range: 40.0,
            pitch_range: 25.0,
            min_flick: 15.0,
            max_flick: 25.0,
            flick_direction: Some(225.0),
            flick_spread: 22.5,
            ..DrillConfig::defaults(DrillKind::Microflick)
        };
        config.sanitize();
        config
    }

    /// Whether the flick from `from` to `to` (both view directions) follows the constraints.
    fn follows(config: &DrillConfig, from: Vec3, to: Vec3) -> bool {
        let offset = wrap(angles(to.normalize()) - angles(from));
        let distance = offset.length();
        let angle = screen_angle(offset);
        let center = config.flick_direction.unwrap();
        let off_center = (angle - center + 180.0).rem_euclid(360.0) - 180.0;
        let (near, far) = config.flick_distance_range();
        off_center.abs() <= config.flick_spread + 1e-2 && distance >= near - 1e-2 && distance <= far + 1e-2
    }

    #[test]
    fn constrained_flicks_spawn_only_in_the_trained_sector_and_distance() {
        let config = down_left();
        for aim in [Vec3::NEG_Z, direction(-0.2, 0.15)] {
            for _ in 0..2000 {
                assert!(follows(&config, aim, flick_position(aim, &config)), "aim {aim}");
            }
        }
    }

    #[test]
    fn repeated_constrained_flicks_reset_and_stay_in_the_area() {
        let config = down_left();
        let (min, max) = flick_area(&config);
        let mut aim = Vec3::NEG_Z;
        let mut trained = 0;
        const SPAWNS: usize = 2000;
        for _ in 0..SPAWNS {
            let target = flick_position(aim, &config);
            let at = angles(target.normalize());
            assert!(at.cmpge(min - 1e-3).all() && at.cmple(max + 1e-3).all(), "{at} outside the area");
            trained += follows(&config, aim, target) as usize;
            aim = target.normalize();
        }
        // Mostly trained flicks, with a reset flick every few targets.
        assert!(trained as f32 / SPAWNS as f32 > 0.6, "{trained} of {SPAWNS}");
    }

    #[test]
    fn default_microflick_is_unconstrained() {
        let config = DrillConfig::defaults(DrillKind::Microflick);
        assert!(!config.flick_constrained());
        // Targets still land on both sides, as before.
        let sides: Vec<bool> = (0..200).map(|_| flick_position(Vec3::NEG_Z, &config).x > 0.0).collect();
        assert!(sides.contains(&true) && sides.contains(&false));
    }

    #[test]
    fn peripheral_targets_appear_off_center() {
        let config = DrillConfig::defaults(DrillKind::Peripheral);
        for _ in 0..50 {
            let dir = peripheral_position(Vec3::NEG_Z, &config).normalize();
            assert!(dir.angle_between(Vec3::NEG_Z).to_degrees() >= config.min_flick - 1e-3);
        }
    }

    #[test]
    fn custom_grid_is_centered_and_spaced() {
        let config = DrillConfig { grid_cols: 5, grid_rows: 1, grid_spacing: 10.0, ..DrillConfig::default() };
        let middle = grid_position(2, &config).normalize();
        assert!(middle.angle_between(Vec3::NEG_Z) < 1e-4);
        let left = grid_position(0, &config).normalize();
        assert!((left.angle_between(Vec3::NEG_Z).to_degrees() - 20.0).abs() < 1e-3);
    }
}
