//! The first-person gun model ([`WeaponModel`]): low-poly guns built from primitives and baked
//! into one vertex-colored mesh each at startup, held in the lower right of the player camera
//! while a drill is played, with a light recoil on each shot. Purely cosmetic: shots are ray
//! tests against targets only, so a gun never blocks one, and guns are hidden outside `Playing`
//! (menus with the settings preview, replays).
//!
//! The guns are children of the camera, closer than anything in the room (which is at least 2 m
//! from the eye), so they need no camera of their own. Like games with a fixed viewmodel FOV, they
//! are laid out for Valorant's 103° at 16:9 and rescaled to the actual view, so they cover the
//! same part of the screen at any FOV or aspect.

use std::f32::consts::{E, FRAC_PI_2};

use bevy::prelude::*;

use crate::AppState;
use crate::camera::PlayerCamera;
use crate::drill::Run;
use crate::settings::{Settings, WeaponModel};

pub struct ViewmodelPlugin;

impl Plugin for ViewmodelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_guns).add_systems(
            Update,
            (
                show_gun.run_if(state_changed::<AppState>.or_else(resource_changed::<Settings>)),
                fit_to_view,
                recoil.run_if(in_state(AppState::Playing)),
            ),
        );
    }
}

/// The camera child holding the guns, scaled to the view.
#[derive(Component)]
struct ViewmodelRig;

/// One gun, posed by [`recoil`]. Its origin is the top of the grip, where the hand holds it, so
/// the recoil pitch pivots there.
#[derive(Component)]
struct Gun {
    model: WeaponModel,
    handling: &'static Handling,
    kick: Spring,
}

/// A gun's rest pose in the reference view and its recoil.
struct Handling {
    /// Low and right, the muzzle turned a little towards the crosshair (yaw) and up (pitch).
    rest: Vec3,
    yaw: f32,
    pitch: f32,
    /// At the kick's peak: back (m), up (m) and muzzle rise (rad).
    kick_back: f32,
    kick_up: f32,
    kick_pitch: f32,
    /// Stiffness of the recoil spring (rad/s): the kick peaks after 1/ω.
    omega: f32,
}

/// Snappy: peaks after 36 ms and has all but settled after 200 ms, before the next Gridshot click.
const PISTOL: Handling = Handling {
    rest: Vec3::new(0.277, -0.229, -0.42),
    yaw: 0.07,
    pitch: 0.03,
    kick_back: 0.022,
    kick_up: 0.006,
    kick_pitch: 0.09,
    omega: 28.0,
};

/// Heavier: a longer shove back with less muzzle rise, peaking after 45 ms.
const RIFLE: Handling = Handling {
    rest: Vec3::new(0.27, -0.265, -0.47),
    yaw: 0.06,
    pitch: 0.025,
    kick_back: 0.03,
    kick_up: 0.007,
    kick_pitch: 0.055,
    omega: 22.0,
};

impl Handling {
    /// The pose `kick` into the recoil, held on the side `sign` says (+1 right, -1 left: the
    /// rest position and the turn towards the crosshair mirror; the guns themselves are
    /// symmetric enough to stay as they are).
    fn pose(&self, kick: f32, sign: f32) -> Transform {
        let rotation = Quat::from_euler(EulerRot::YXZ, self.yaw * sign, self.pitch + self.kick_pitch * kick, 0.0);
        let rest = self.rest * Vec3::new(sign, 1.0, 1.0);
        Transform::from_translation(rest + Vec3::new(0.0, self.kick_up, self.kick_back) * kick).with_rotation(rotation)
    }
}

/// tan(103° / 2): the half horizontal view the layout is made for.
const REF_TAN_H: f32 = 1.257_17;
const REF_ASPECT: f32 = 16.0 / 9.0;
const REF_TAN_V: f32 = REF_TAN_H / REF_ASPECT;
/// Horizontal position (m) around which the guns are held: on wider screens the rig shifts so
/// this stays as far from the right edge as at 16:9.
const ANCHOR_X: f32 = 0.26;

/// Recoil displacement (1 = a full kick) on a critically damped spring, stepped with its exact
/// solution so it moves the same at any frame rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Spring {
    offset: f32,
    velocity: f32,
}

impl Spring {
    /// A shot: the velocity that makes a kick from rest peak at exactly 1.
    fn kick(&mut self, omega: f32) {
        self.velocity += omega * E;
    }

    /// x(t) = (x₀ + (v₀ + ωx₀)t)e^(−ωt).
    fn step(&mut self, dt: f32, omega: f32) {
        let decay = (-omega * dt).exp();
        let k = (self.velocity + omega * self.offset) * dt;
        self.offset = (self.offset + k) * decay;
        self.velocity = (self.velocity - omega * k) * decay;
    }

    fn at_rest(&self) -> bool {
        self.offset.abs() < 1e-4 && self.velocity.abs() < 1e-3
    }
}

/// The three matte finishes every gun shares.
#[derive(Clone, Copy)]
enum Finish {
    /// Slide, upper receiver, sight posts, magazines.
    Dark,
    /// Frame, lower receiver, grips, stock.
    Grey,
    /// A coral side stripe and the front sight tip.
    Accent,
}

impl Finish {
    fn color(self) -> Color {
        match self {
            Finish::Dark => Color::srgb(0.13, 0.14, 0.16),
            Finish::Grey => Color::srgb(0.36, 0.38, 0.41),
            Finish::Accent => Color::srgb(0.85, 0.32, 0.34),
        }
    }
}

/// A mesh, its finish and where it sits on the gun (metres, −Z is the muzzle).
type Part = (Mesh, Finish, Transform);

/// Bakes a gun's parts into one mesh, their finishes as vertex colors, so each gun is a single
/// entity drawn in one call with one material.
fn bake(parts: Vec<Part>) -> Mesh {
    let mut baked: Option<Mesh> = None;
    for (mesh, finish, transform) in parts {
        let color = finish.color().to_linear().to_f32_array();
        let colors = vec![color; mesh.count_vertices()];
        let part = mesh.transformed_by(transform).with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        match &mut baked {
            Some(baked) => baked.merge(&part).expect("gun parts share one vertex layout"),
            None => baked = Some(part),
        }
    }
    baked.expect("a gun has parts")
}

fn block(finish: Finish, size: [f32; 3], center: [f32; 3]) -> Part {
    (Cuboid::new(size[0], size[1], size[2]).into(), finish, Transform::from_translation(center.into()))
}

/// A box hanging below `top` (the center of its top face), tilted about X: a positive `tilt`
/// swings its bottom forward.
fn hung(finish: Finish, size: [f32; 3], top: Vec3, tilt: f32) -> Part {
    let rotation = Quat::from_rotation_x(tilt);
    let center = top + rotation * Vec3::new(0.0, -size[1] * 0.5, 0.0);
    (Cuboid::new(size[0], size[1], size[2]).into(), finish, Transform::from_translation(center).with_rotation(rotation))
}

/// A cylinder along Z.
fn tube(finish: Finish, radius: f32, length: f32, center: [f32; 3]) -> Part {
    let mesh = Cylinder::new(radius, length).mesh().resolution(12).build();
    (mesh, finish, Transform::from_translation(center.into()).with_rotation(Quat::from_rotation_x(FRAC_PI_2)))
}

/// The point `d` down an axis tilted like [`hung`] from `top`.
fn down(top: Vec3, tilt: f32, d: f32) -> Vec3 {
    top + Quat::from_rotation_x(tilt) * Vec3::new(0.0, -d, 0.0)
}

/// A compact, Valorant-like sidearm: dark slide with a coral stripe and front sight, grey frame
/// with a raked grip.
fn pistol() -> Vec<Part> {
    use Finish::*;
    let grip_top = Vec3::new(0.0, -0.002, 0.012);
    let grip_tilt = -0.28;
    vec![
        // Slide, its chamfered top and the stripe along both sides.
        block(Dark, [0.026, 0.032, 0.19], [0.0, 0.028, -0.06]),
        block(Dark, [0.017, 0.005, 0.18], [0.0, 0.0465, -0.06]),
        block(Accent, [0.0275, 0.0035, 0.1], [0.0, 0.021, -0.085]),
        // Frame and the accessory rail under the dust cover.
        block(Grey, [0.023, 0.016, 0.175], [0.0, 0.004, -0.0525]),
        block(Grey, [0.021, 0.008, 0.06], [0.0, -0.008, -0.11]),
        // Barrel bushing and bore.
        tube(Grey, 0.0075, 0.01, [0.0, 0.03, -0.157]),
        tube(Dark, 0.0042, 0.011, [0.0, 0.03, -0.1575]),
        // Sights: a coral front post and a two-post rear notch.
        block(Accent, [0.004, 0.006, 0.006], [0.0, 0.052, -0.145]),
        block(Dark, [0.0065, 0.007, 0.007], [-0.0065, 0.0525, 0.026]),
        block(Dark, [0.0065, 0.007, 0.007], [0.0065, 0.0525, 0.026]),
        // Grip (its slanted top buried in the frame, so no corner shows), its darker panels and
        // the magazine base.
        hung(Grey, [0.022, 0.11, 0.036], down(grip_top, grip_tilt, -0.01), grip_tilt),
        hung(Dark, [0.0275, 0.066, 0.026], down(grip_top, grip_tilt, 0.022), grip_tilt),
        hung(Grey, [0.027, 0.008, 0.04], down(grip_top, grip_tilt, 0.096), grip_tilt),
        // Trigger guard and trigger.
        block(Grey, [0.007, 0.005, 0.059], [0.0, -0.034, -0.0255]),
        block(Grey, [0.007, 0.03, 0.006], [0.0, -0.021, -0.052]),
        hung(Dark, [0.005, 0.018, 0.005], Vec3::new(0.0, -0.005, -0.024), -0.25),
    ]
}

/// A Vandal/Phantom-like assault rifle in the pistol's finishes: dark upper receiver, handguard
/// top and curved magazine, grey lower receiver, grip and stock, a coral stripe along the
/// handguard and a coral front sight tip.
fn rifle() -> Vec<Part> {
    use Finish::*;
    let grip_top = Vec3::new(0.0, -0.012, 0.01);
    let grip_tilt = -0.35;
    let mag_top = Vec3::new(0.0, -0.012, -0.085);
    let (mag_upper, mag_lower) = (0.2, 0.45);
    vec![
        // Upper receiver with its top rail, lower receiver and magazine well.
        block(Dark, [0.03, 0.038, 0.25], [0.0, 0.031, -0.035]),
        block(Dark, [0.016, 0.006, 0.23], [0.0, 0.053, -0.04]),
        block(Grey, [0.028, 0.024, 0.21], [0.0, 0.0, -0.015]),
        block(Grey, [0.03, 0.02, 0.05], [0.0, -0.018, -0.085]),
        // Handguard: dark top, grey bottom, the stripe on the seam.
        block(Dark, [0.03, 0.026, 0.2], [0.0, 0.033, -0.26]),
        block(Grey, [0.028, 0.022, 0.2], [0.0, 0.009, -0.26]),
        block(Accent, [0.0315, 0.0035, 0.15], [0.0, 0.02, -0.27]),
        // Barrel and muzzle brake.
        tube(Grey, 0.0065, 0.08, [0.0, 0.028, -0.4]),
        block(Dark, [0.018, 0.018, 0.035], [0.0, 0.028, -0.4575]),
        // Front sight post with a coral tip, rear sight block.
        block(Dark, [0.006, 0.026, 0.008], [0.0, 0.059, -0.345]),
        block(Accent, [0.0045, 0.006, 0.006], [0.0, 0.075, -0.345]),
        block(Dark, [0.022, 0.016, 0.016], [0.0, 0.064, 0.07]),
        // Magazine, curving forward in two segments.
        hung(Dark, [0.022, 0.06, 0.04], mag_top, mag_upper),
        hung(Dark, [0.022, 0.05, 0.04], down(mag_top, mag_upper, 0.058), mag_lower),
        // Pistol grip (its slanted top buried in the receiver), trigger guard and trigger.
        hung(Grey, [0.024, 0.085, 0.032], down(grip_top, grip_tilt, -0.01), grip_tilt),
        block(Grey, [0.007, 0.005, 0.05], [0.0, -0.034, -0.02]),
        block(Grey, [0.007, 0.024, 0.006], [0.0, -0.024, -0.045]),
        hung(Dark, [0.005, 0.016, 0.005], Vec3::new(0.0, -0.012, -0.022), -0.25),
        // Stock with a dark cheek rest.
        block(Grey, [0.026, 0.05, 0.2], [0.0, 0.012, 0.19]),
        block(Dark, [0.024, 0.01, 0.16], [0.0, 0.041, 0.19]),
    ]
}

/// Bakes every gun into a mesh under the player camera, hidden until [`show_gun`] shows the
/// chosen one.
fn spawn_guns(
    mut commands: Commands,
    camera: Single<Entity, With<PlayerCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // White, so the vertex colors are the finishes.
    let matte = materials.add(StandardMaterial { perceptual_roughness: 0.75, reflectance: 0.25, ..default() });
    let rig = commands.spawn((ViewmodelRig, Transform::IDENTITY, Visibility::Inherited, ChildOf(*camera))).id();
    for (model, handling, parts) in [(WeaponModel::Pistol, &PISTOL, pistol()), (WeaponModel::Rifle, &RIFLE, rifle())] {
        commands.spawn((
            Gun { model, handling, kick: Spring::default() },
            Mesh3d(meshes.add(bake(parts))),
            MeshMaterial3d(matte.clone()),
            // Right-handed until `show_gun` applies the chosen side.
            handling.pose(0.0, 1.0),
            Visibility::Hidden,
            ChildOf(rig),
        ));
    }
}

/// Shows the chosen gun while a drill is played; hiding one also drops its recoil.
fn show_gun(
    settings: Res<Settings>,
    state: Res<State<AppState>>,
    mut guns: Query<(&mut Gun, &mut Transform, &mut Visibility)>,
) {
    let playing = *state.get() == AppState::Playing;
    for (mut gun, mut transform, mut visibility) in &mut guns {
        let shown = playing && gun.model == settings.video.weapon;
        visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
        if !shown {
            gun.kick = Spring::default();
        }
        // Also re-poses the shown gun when the side changes.
        if !shown || gun.kick.at_rest() {
            transform.set_if_neq(gun.handling.pose(0.0, settings.video.weapon_side.sign()));
        }
    }
}

/// Rescales the rig from the reference view to the camera's: scaling x and y by the ratio of
/// the vertical half-view tangents keeps the guns' size relative to the screen height, and the
/// shift keeps their distance from the screen edge on their side on wider screens.
fn fit_to_view(
    settings: Res<Settings>,
    camera: Query<Ref<Projection>, With<PlayerCamera>>,
    mut rig: Single<&mut Transform, With<ViewmodelRig>>,
) {
    let Ok(projection) = camera.single() else { return };
    let Projection::Perspective(perspective) = &*projection else { return };
    if !projection.is_changed() && !settings.is_changed() {
        return;
    }
    let tan_v = (perspective.fov * 0.5).tan();
    let tan_h = tan_v * perspective.aspect_ratio;
    let scale = tan_v / REF_TAN_V;
    let shift = ANCHOR_X * (tan_h / (REF_ASPECT * REF_TAN_V) - scale) * settings.video.weapon_side.sign();
    rig.set_if_neq(Transform::from_xyz(shift, 0.0, 0.0).with_scale(Vec3::new(scale, scale, 1.0)));
}

/// Kicks the shown gun on each click of a live run and lets it spring back.
fn recoil(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    settings: Res<Settings>,
    run: Option<Res<Run>>,
    mut guns: Query<(&mut Gun, &mut Transform)>,
) {
    let shot = mouse.just_pressed(MouseButton::Left) && run.is_some_and(|run| run.live());
    for (mut gun, mut transform) in &mut guns {
        if gun.model != settings.video.weapon {
            continue;
        }
        let omega = gun.handling.omega;
        if shot {
            gun.kick.kick(omega);
        }
        let sign = settings.video.weapon_side.sign();
        if gun.kick.at_rest() {
            if gun.kick != Spring::default() {
                gun.kick = Spring::default();
                transform.set_if_neq(gun.handling.pose(0.0, sign));
            }
            continue;
        }
        gun.kick.step(time.delta_secs(), omega);
        let pose = gun.handling.pose(gun.kick.offset, sign);
        transform.set_if_neq(pose);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(mut spring: Spring, steps: u32, secs: f32, omega: f32) -> Spring {
        for _ in 0..steps {
            spring.step(secs / steps as f32, omega);
        }
        spring
    }

    #[test]
    fn reference_view_is_valorants() {
        assert!((REF_TAN_H - 51.5_f32.to_radians().tan()).abs() < 1e-5);
    }

    /// Every part merges into the baked mesh, each vertex with its finish.
    #[test]
    fn guns_bake_into_one_mesh() {
        for parts in [pistol(), rifle()] {
            let vertices: usize = parts.iter().map(|(mesh, ..)| mesh.count_vertices()).sum();
            let baked = bake(parts);
            assert_eq!(baked.count_vertices(), vertices);
            assert_eq!(baked.attribute(Mesh::ATTRIBUTE_COLOR).map(|colors| colors.len()), Some(vertices));
        }
    }

    /// The exact step moves the same at 30 and 360 fps.
    #[test]
    fn recoil_is_frame_rate_independent() {
        for omega in [PISTOL.omega, RIFLE.omega] {
            let mut kicked = Spring::default();
            kicked.kick(omega);
            for secs in [0.02, 0.05, 0.12] {
                let slow = settle(kicked, 1, secs, omega);
                let fast = settle(kicked, 12, secs, omega);
                assert!((slow.offset - fast.offset).abs() < 1e-4, "{slow:?} vs {fast:?}");
                assert!((slow.velocity - fast.velocity).abs() < 1e-2, "{slow:?} vs {fast:?}");
            }
        }
    }

    /// A kick peaks at 1 after 1/ω, then settles without overshooting.
    #[test]
    fn kick_peaks_once_and_settles() {
        for omega in [PISTOL.omega, RIFLE.omega] {
            let mut spring = Spring::default();
            spring.kick(omega);
            assert!((settle(spring, 10, 1.0 / omega, omega).offset - 1.0).abs() < 1e-4);
            let mut previous = f32::MAX;
            for frame in 1..=60 {
                let now = settle(spring, frame, frame as f32 / 120.0, omega).offset;
                assert!(now >= 0.0);
                if frame as f32 / 120.0 > 1.0 / omega {
                    assert!(now <= previous);
                }
                previous = now;
            }
            assert!(settle(spring, 120, 0.8, omega).at_rest());
        }
    }
}
