//! The first-person replay player: pause, seek, frame step, slow motion, shot markers and the
//! aim tracer. Its HUD is `ui::replay_hud`.

use bevy::prelude::*;

use super::format::{Replay, Shot};
use crate::AppState;
use crate::camera::Look;
use crate::drill::{TargetAssets, spawn_environment};
use crate::metrics::bow;

const SPEEDS: [f32; 5] = [0.1, 0.25, 0.5, 1.0, 2.0];
/// How long (replay time) a shot marker stays visible.
const MARKER_SECS: f32 = 0.6;
/// Shot markers are drawn this far along the aim ray, i.e. on the target plane.
const MARKER_DISTANCE: f32 = 10.0;
/// How much of the recent aim path (replay time) the tracer shows.
const TRACER_SECS: f32 = 0.6;

#[derive(Resource)]
pub struct Playback {
    pub(crate) replay: Replay,
    /// Replay time shown.
    pub(crate) time: f32,
    speed_index: usize,
    pub(crate) paused: bool,
    /// Screen to go back to when the replay is closed.
    return_to: AppState,
    /// Replay time up to which shot markers have been spawned.
    shown_until: f32,
    /// Whether the aim tracer is drawn (toggled with T).
    tracer: bool,
}

/// Opens the replay player.
pub fn watch(commands: &mut Commands, next_state: &mut NextState<AppState>, replay: Replay, return_to: AppState) {
    commands.insert_resource(Playback {
        replay,
        time: 0.0,
        speed_index: 3,
        paused: false,
        return_to,
        // So a shot at t = 0 is shown too.
        shown_until: -MARKER_SECS,
        tracer: true,
    });
    next_state.set(AppState::Replay);
}

impl Playback {
    /// Playback speed factor.
    pub(crate) fn speed(&self) -> f32 {
        SPEEDS[self.speed_index]
    }

    pub(crate) fn end(&self) -> f32 {
        self.replay.frames.last().map_or(0.0, |f| f.t)
    }

    /// Index of the last frame at or before `time`.
    fn frame_at(&self, time: f32) -> usize {
        self.replay.frames.partition_point(|f| f.t <= time).saturating_sub(1)
    }

    /// View angles at `time`, interpolated between recorded frames.
    fn angles_at(&self, time: f32) -> (f32, f32) {
        let frames = &self.replay.frames;
        let i = self.frame_at(time);
        let a = &frames[i];
        let Some(b) = frames.get(i + 1) else { return (a.yaw, a.pitch) };
        let s = ((time - a.t) / (b.t - a.t).max(f32::EPSILON)).clamp(0.0, 1.0);
        (a.yaw + (b.yaw - a.yaw) * s, a.pitch + (b.pitch - a.pitch) * s)
    }

    pub(crate) fn shots_until(&self, time: f32) -> (usize, usize) {
        let frames = &self.replay.frames[..self.replay.frames.partition_point(|f| f.t <= time)];
        let hits = frames.iter().filter(|f| f.shot == Shot::Hit).count();
        let misses = frames.iter().filter(|f| f.shot == Shot::Miss).count();
        (hits, misses)
    }
}

#[derive(Resource)]
pub(crate) struct MarkerAssets {
    mesh: Handle<Mesh>,
    hit: Handle<StandardMaterial>,
    miss: Handle<StandardMaterial>,
}

#[derive(Component)]
pub(crate) struct ReplayTarget;

#[derive(Component)]
pub(crate) struct ShotMarker {
    /// Replay time of the shot.
    t: f32,
}

pub(super) fn create_marker_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let unlit = |color: Color| StandardMaterial { base_color: color, unlit: true, ..default() };
    commands.insert_resource(MarkerAssets {
        mesh: meshes.add(Sphere::new(0.06)),
        hit: materials.add(unlit(Color::srgb(0.2, 1.0, 0.4))),
        miss: materials.add(unlit(Color::srgb(1.0, 0.25, 0.25))),
    });
}

pub(super) fn clear_look(mut look: ResMut<Look>, mut commands: Commands) {
    look.set(0.0, 0.0);
    commands.remove_resource::<Playback>();
}

pub(super) fn playback_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut playback: ResMut<Playback>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        next_state.set(playback.return_to);
        return;
    }
    let end = playback.end();
    if keys.just_pressed(KeyCode::Space) {
        // Restart when pressing play at the very end.
        if playback.paused && playback.time >= end {
            playback.time = 0.0;
        }
        playback.paused = !playback.paused;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        playback.time = 0.0;
        playback.paused = false;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        playback.time = (playback.time - 5.0).max(0.0);
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        playback.time = (playback.time + 5.0).min(end);
    }
    // Frame stepping while paused, for checking flicks in detail.
    if playback.paused && keys.just_pressed(KeyCode::Comma) {
        playback.time = (playback.time - 1.0 / 60.0).max(0.0);
    }
    if playback.paused && keys.just_pressed(KeyCode::Period) {
        playback.time = (playback.time + 1.0 / 60.0).min(end);
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        playback.speed_index = (playback.speed_index + 1).min(SPEEDS.len() - 1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        playback.speed_index = playback.speed_index.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::KeyT) {
        playback.tracer = !playback.tracer;
    }
}

/// Draws the crosshair's path over the last `TRACER_SECS`, colored by how straight each
/// movement was (see [`bow`]), on the target plane and on top of the targets.
pub(super) fn draw_tracer(playback: Res<Playback>, mut gizmos: Gizmos<TracerGizmos>) {
    if !playback.tracer {
        return;
    }
    let now = playback.time;
    let frames = &playback.replay.frames;
    let first = frames.partition_point(|f| f.t <= now - TRACER_SECS);
    let last = playback.frame_at(now);
    let mut points: Vec<(f32, Vec2)> =
        frames[first.min(last)..=last].iter().map(|f| (f.t, Vec2::new(f.yaw, f.pitch))).collect();
    // End exactly at the crosshair, between recorded frames.
    let (yaw, pitch) = playback.angles_at(now);
    if points.last().is_some_and(|p| p.0 < now) {
        points.push((now, Vec2::new(yaw, pitch)));
    }
    let in_degrees: Vec<(f32, Vec2)> =
        points.iter().map(|&(t, a)| (t, Vec2::new(a.x.to_degrees(), a.y.to_degrees()))).collect();
    let on_plane = |a: Vec2| Quat::from_euler(EulerRot::YXZ, a.x, a.y, 0.0) * Vec3::NEG_Z * MARKER_DISTANCE;
    for (pair, bow) in points.windows(2).zip(bow::segment_bows(&in_degrees)) {
        // Older parts of the trail fade out.
        let fade = 1.0 - ((now - pair[1].0) / TRACER_SECS).clamp(0.0, 1.0);
        let color = bow_color(bow);
        let color = color.with_alpha(color.alpha() * fade);
        gizmos.line(on_plane(pair[0].1), on_plane(pair[1].1), color);
    }
}

/// Tracer lines: thicker than default gizmos and drawn over the targets.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct TracerGizmos;

pub(super) fn configure_tracer(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<TracerGizmos>();
    config.line.width = 3.0;
    config.depth_bias = -1.0;
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn advance_playback(
    time: Res<Time>,
    mut playback: ResMut<Playback>,
    mut look: ResMut<Look>,
    target_assets: Res<TargetAssets>,
    marker_assets: Res<MarkerAssets>,
    mut commands: Commands,
    mut targets: Query<(&mut Transform, &mut Visibility), With<ReplayTarget>>,
    markers: Query<(Entity, &ShotMarker)>,
) {
    if playback.replay.frames.is_empty() {
        return;
    }
    let shown_until = playback.shown_until;
    if !playback.paused {
        let end = playback.end();
        playback.time = (playback.time + time.delta_secs() * SPEEDS[playback.speed_index]).min(end);
        if playback.time >= end {
            playback.paused = true;
        }
    }
    let now = playback.time;
    let (yaw, pitch) = playback.angles_at(now);
    look.set(yaw, pitch);

    // Targets: reuse a pool of entities, one per target in the current frame.
    let frame = &playback.replay.frames[playback.frame_at(now)];
    let mut pool = targets.iter_mut();
    for &(center, radius) in &frame.targets {
        let transform = Transform::from_translation(center).with_scale(Vec3::splat(radius));
        match pool.next() {
            Some((mut existing, mut visibility)) => {
                *existing = transform;
                *visibility = Visibility::Inherited;
            },
            None => {
                commands.spawn((
                    DespawnOnExit(AppState::Replay),
                    ReplayTarget,
                    Mesh3d(target_assets.mesh.clone()),
                    MeshMaterial3d(target_assets.idle.clone()),
                    transform,
                ));
            },
        }
    }
    for (_, mut visibility) in pool {
        *visibility = Visibility::Hidden;
    }

    // Shot markers: show shots from the last MARKER_SECS of replay time. Seeks and frame steps
    // happen in `playback_controls` before this runs, so compare against what was shown last.
    let (from, reset) = marker_range(shown_until, now);
    for (entity, marker) in &markers {
        if reset || marker.t > now || now - marker.t > MARKER_SECS {
            commands.entity(entity).despawn();
        }
    }
    let frames = &playback.replay.frames;
    let start = frames.partition_point(|f| f.t <= from);
    let stop = frames.partition_point(|f| f.t <= now);
    for frame in &frames[start..stop] {
        let material = match frame.shot {
            Shot::None => continue,
            Shot::Hit => marker_assets.hit.clone(),
            Shot::Miss => marker_assets.miss.clone(),
        };
        let aim = Quat::from_euler(EulerRot::YXZ, frame.yaw, frame.pitch, 0.0) * Vec3::NEG_Z;
        commands.spawn((
            DespawnOnExit(AppState::Replay),
            ShotMarker { t: frame.t },
            Mesh3d(marker_assets.mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(aim * MARKER_DISTANCE),
        ));
    }
    playback.shown_until = now;
}

/// Shots in `(from, now]` get new markers. Moving forward a little (playing or frame stepping)
/// adds only the shots since the last update; jumping back or far ahead rebuilds the whole
/// window, so `reset` asks for existing markers to be cleared first.
fn marker_range(shown_until: f32, now: f32) -> (f32, bool) {
    if now < shown_until || now - shown_until > MARKER_SECS { (now - MARKER_SECS, true) } else { (shown_until, false) }
}

/// The drill's scenery (e.g. the peek cover), rebuilt for the replay.
pub(super) fn spawn_replay_scene(mut commands: Commands, playback: Res<Playback>, assets: Res<TargetAssets>) {
    spawn_environment(&mut commands, &assets, playback.replay.kind.def().environment, AppState::Replay);
}

/// Green for straight, through yellow, to red for curved; faint white while resting.
pub fn bow_color(bow: Option<f32>) -> Color {
    match bow {
        None => Color::srgba(1.0, 1.0, 1.0, 0.35),
        Some(bow) => {
            let curved = ((bow - bow::STRAIGHT) / (bow::CURVED - bow::STRAIGHT)).clamp(0.0, 1.0);
            Color::hsl(130.0 * (1.0 - curved), 0.9, 0.55)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::format::tests::sample_replay;

    #[test]
    fn shot_markers_follow_steps_and_seeks() {
        // Start: a shot at t = 0 is inside the first window.
        assert_eq!(marker_range(-MARKER_SECS, 0.0), (-MARKER_SECS, false));
        // A frame step forward adds just the shots since the last update.
        assert_eq!(marker_range(1.0, 1.004), (1.0, false));
        // Seeking far ahead or stepping back rebuilds the whole window.
        assert_eq!(marker_range(1.0, 6.0), (6.0 - MARKER_SECS, true));
        assert_eq!(marker_range(1.0, 0.996), (0.996 - MARKER_SECS, true));
    }

    #[test]
    fn interpolates_angles_between_frames() {
        let playback = Playback {
            replay: sample_replay(),
            time: 0.0,
            speed_index: 3,
            paused: true,
            return_to: AppState::Menu,
            shown_until: -MARKER_SECS,
            tracer: true,
        };
        let (yaw, pitch) = playback.angles_at(0.006);
        assert!((yaw - 0.35).abs() < 1e-5 && (pitch - 0.05).abs() < 1e-5);
        assert_eq!(playback.angles_at(1.0), (0.4, 0.1));
        assert_eq!(playback.shots_until(0.005), (1, 0));
    }
}
