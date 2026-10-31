//! Camera, mouse look and cursor locking.

use std::f32::consts::FRAC_PI_2;

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions},
};

use crate::{AppState, settings::Settings};

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Look>()
            .init_resource::<ActiveSens>()
            .init_resource::<ViewCone>()
            .add_systems(Startup, spawn_camera)
            .add_systems(OnEnter(AppState::Playing), (reset_look, lock_cursor))
            .add_systems(OnExit(AppState::Playing), (reset_look, unlock_cursor))
            .add_systems(
                Update,
                (
                    mouse_look.run_if(in_state(AppState::Playing)),
                    apply_look,
                    apply_fov.run_if(resource_changed::<Settings>),
                    update_view_cone,
                )
                    .chain(),
            );
    }
}

#[derive(Component)]
pub struct PlayerCamera;

/// View angles in radians. Kept separately from the transform so they never drift.
#[derive(Resource, Default)]
pub struct Look {
    yaw: f32,
    pitch: f32,
}

/// The sensitivity mouse look uses, kept up to date by whatever owns the current activity
/// (the drill run), so the camera doesn't depend on it.
#[derive(Resource, Default)]
pub struct ActiveSens(pub f32);

fn spawn_camera(mut commands: Commands, settings: Res<Settings>) {
    commands.spawn((
        PlayerCamera,
        Camera3d::default(),
        // Menus and HUD follow this camera's (possibly 16:9-locked) viewport.
        IsDefaultUiCamera,
        Projection::from(PerspectiveProjection {
            fov: settings.vfov_rad(),
            ..default()
        }),
        Transform::IDENTITY,
    ));
}

impl Look {
    pub fn angles(&self) -> (f32, f32) {
        (self.yaw, self.pitch)
    }

    pub fn set(&mut self, yaw: f32, pitch: f32) {
        self.yaw = yaw;
        self.pitch = pitch;
    }
}

fn reset_look(mut look: ResMut<Look>) {
    *look = Look::default();
}

pub fn mouse_look(motion: Res<AccumulatedMouseMotion>, sens: Res<ActiveSens>, settings: Res<Settings>, mut look: ResMut<Look>) {
    // Raw counts, unaccelerated: no delta-time scaling, exactly like the games do it.
    let rad_per_count = settings.deg_per_count(sens.0).to_radians();
    const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.01;
    look.yaw -= motion.delta.x * rad_per_count;
    look.pitch = (look.pitch - motion.delta.y * rad_per_count).clamp(-PITCH_LIMIT, PITCH_LIMIT);
}

pub fn apply_look(look: Res<Look>, mut camera: Single<&mut Transform, With<PlayerCamera>>) {
    if look.is_changed() {
        camera.rotation = Quat::from_euler(EulerRot::YXZ, look.yaw, look.pitch, 0.0);
    }
}

fn apply_fov(settings: Res<Settings>, mut projection: Single<&mut Projection, With<PlayerCamera>>) {
    if let Projection::Perspective(perspective) = projection.as_mut() {
        perspective.fov = settings.vfov_rad();
    }
}

/// What the player camera shows: the tangents of its half view angles. Comes from the camera's
/// projection, which Bevy keeps in sync with the rendered viewport, so it is the 16:9 area inside
/// black bars when the aspect is locked, or the whole window with Hor+.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct ViewCone {
    tan_half_h: f32,
    tan_half_v: f32,
}

impl Default for ViewCone {
    /// Valorant's 103° at 16:9, until the camera reports its real view.
    fn default() -> Self {
        Self::new(Settings::default().vfov_rad(), 16.0 / 9.0)
    }
}

impl ViewCone {
    pub fn new(vfov: f32, aspect: f32) -> Self {
        let tan_half_v = (vfov * 0.5).tan();
        Self { tan_half_h: tan_half_v * aspect, tan_half_v }
    }

    /// Whether `dir` is on screen when looking along `aim`, at least `margin` radians inside
    /// every edge.
    pub fn contains(&self, aim: Vec3, dir: Vec3, margin: f32) -> bool {
        let local = view_rotation(aim).inverse() * dir;
        if local.z >= 0.0 {
            return false;
        }
        let inside = |tan_half: f32, offset: f32| (offset / -local.z).abs() <= (tan_half.atan() - margin).max(0.0).tan();
        inside(self.tan_half_h, local.x) && inside(self.tan_half_v, local.y)
    }

    /// `dir` moved towards `aim` (along the great circle between them) until it is on screen
    /// with `margin` to spare.
    pub fn pull_in(&self, aim: Vec3, dir: Vec3, margin: f32) -> Vec3 {
        if self.contains(aim, dir, margin) {
            return dir;
        }
        let (mut inside, mut outside) = (0.0, 1.0);
        for _ in 0..16 {
            let t = (inside + outside) * 0.5;
            if self.contains(aim, aim.slerp(dir, t), margin) {
                inside = t;
            } else {
                outside = t;
            }
        }
        aim.slerp(dir, inside)
    }
}

/// The camera rotation looking along `aim` (yaw and pitch, no roll), as in [`apply_look`].
fn view_rotation(aim: Vec3) -> Quat {
    let aim = aim.normalize();
    Quat::from_euler(EulerRot::YXZ, (-aim.x).atan2(-aim.z), aim.y.clamp(-1.0, 1.0).asin(), 0.0)
}

fn update_view_cone(projection: Single<&Projection, With<PlayerCamera>>, mut cone: ResMut<ViewCone>) {
    if let Projection::Perspective(perspective) = *projection {
        cone.set_if_neq(ViewCone::new(perspective.fov, perspective.aspect_ratio));
    }
}

fn lock_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.visible = false;
    cursor.grab_mode = CursorGrabMode::Locked;
}

fn unlock_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.visible = true;
    cursor.grab_mode = CursorGrabMode::None;
}
