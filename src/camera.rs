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
            .add_systems(Startup, spawn_camera)
            .add_systems(OnEnter(AppState::Playing), (reset_look, lock_cursor))
            .add_systems(OnExit(AppState::Playing), (reset_look, unlock_cursor))
            .add_systems(
                Update,
                (
                    mouse_look.run_if(in_state(AppState::Playing)),
                    apply_look,
                    apply_fov.run_if(resource_changed::<Settings>),
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

fn lock_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.visible = false;
    cursor.grab_mode = CursorGrabMode::Locked;
}

fn unlock_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.visible = true;
    cursor.grab_mode = CursorGrabMode::None;
}
