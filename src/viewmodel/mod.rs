//! The first-person gun ([`WeaponModel`]): a pistol and a rifle modelled in code ([`mesh`],
//! [`pistol`], [`rifle`]), each with the hands that hold it ([`hands`]) baked into one
//! vertex-colored mesh at startup, held in a lower corner of the player camera while a drill is
//! played. A shot kicks it back on a spring and lights a short muzzle flash, and it trails the
//! view a little when the view turns. Purely cosmetic: shots are ray tests against targets only,
//! so a gun never blocks one, and guns are hidden outside `Playing` (menus with the settings
//! preview, replays).
//!
//! The guns are children of the camera, closer than anything in the room (which is at least 2 m
//! from the eye), so they need no camera of their own. Like games with a fixed viewmodel FOV, they
//! are laid out for Valorant's 103° at 16:9 and rescaled to the actual view, so they cover the
//! same part of the screen at any FOV or aspect.
//!
//! Everything that moves is stepped with `Time` deltas in [`animate`], which only runs in
//! `PlayState::Running`: a paused gun stands still and goes on where it was.

mod flash;
mod hands;
mod mesh;
mod pistol;
mod rifle;

use std::f32::consts::E;

use bevy::prelude::*;

use crate::camera::{Look, PlayerCamera};
use crate::drill::{DrillSet, Run};
use crate::settings::{Settings, WeaponModel, WeaponSide};
use crate::{AppState, PlayState};

pub struct ViewmodelPlugin;

impl Plugin for ViewmodelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_guns).add_systems(
            Update,
            (show_gun.run_if(state_changed::<AppState>.or_else(resource_changed::<Settings>)), fit_to_view),
        );
        add_animation(app);
    }
}

/// After the drill has counted the frame's shots and the view has turned, and never while the
/// run is paused.
fn add_animation(app: &mut App) {
    app.add_systems(Update, animate.after(DrillSet::Score).run_if(in_state(PlayState::Running)));
}

/// The camera child holding the guns, scaled to the view.
#[derive(Component)]
struct ViewmodelRig;

/// One gun, posed by [`animate`].
#[derive(Component)]
struct Gun {
    model: WeaponModel,
    handling: &'static Handling,
    /// Its mesh for the right hand, and the mirror image for the left.
    meshes: [Handle<Mesh>; 2],
    /// Its muzzle flash, a child.
    flash: Entity,
    motion: Motion,
    /// The view angles (yaw, pitch) of the frame before, while the gun is shown.
    view: Option<Vec2>,
}

/// The muzzle flash of a [`Gun`].
#[derive(Component)]
struct MuzzleFlash;

/// A gun's rest pose in the reference view, its recoil and where its flash goes. Written for the
/// right hand; [`Handling::pose`] mirrors it for the left.
struct Handling {
    /// Where the gun's origin is held: low and right.
    rest: Vec3,
    /// The muzzle turned towards the crosshair (yaw) and raised (pitch), the top canted inwards
    /// (roll).
    yaw: f32,
    pitch: f32,
    roll: f32,
    /// At the kick's peak: back (m), up (m) and muzzle rise (rad).
    kick_back: f32,
    kick_up: f32,
    kick_pitch: f32,
    /// Sideways turn and cant (rad) of the strongest twist: each shot twists a different way.
    twist_yaw: f32,
    twist_roll: f32,
    /// Stiffness of the recoil spring (rad/s): the kick peaks after 1/ω.
    omega: f32,
    /// The muzzle, in the gun's mesh.
    muzzle: Vec3,
    /// Size of the flash.
    flash: f32,
}

/// Snappy: peaks after 34 ms and has all but settled after 200 ms, before the next Gridshot click.
/// Both guns kick mostly straight back, which on screen moves them outwards, and only a little
/// up: the muzzle must not jump into the targets (see `guns_keep_to_their_corner`).
const PISTOL: Handling = Handling {
    rest: Vec3::new(0.25, -0.205, -0.35),
    yaw: 0.10,
    pitch: 0.03,
    roll: 0.05,
    kick_back: 0.025,
    kick_up: 0.004,
    kick_pitch: 0.06,
    twist_yaw: 0.012,
    twist_roll: 0.03,
    omega: 29.0,
    muzzle: pistol::MUZZLE,
    flash: 0.9,
};

/// Heavier: a longer shove back with less muzzle rise, peaking after 45 ms. Held low and level,
/// so its long barrel stays under the lowest targets.
const RIFLE: Handling = Handling {
    rest: Vec3::new(0.265, -0.305, -0.42),
    yaw: 0.07,
    pitch: -0.01,
    roll: 0.04,
    kick_back: 0.035,
    kick_up: 0.003,
    kick_pitch: 0.03,
    twist_yaw: 0.008,
    twist_roll: 0.02,
    omega: 22.0,
    muzzle: rifle::MUZZLE,
    flash: 1.0,
};

impl Handling {
    /// The gun's place in the rig: `kick` and `twist` into the recoil, trailing the view by
    /// `lag` (yaw, pitch), held on the side `sign` says (+1 right, -1 left). The left hand's
    /// pose is the right hand's in a mirror (and it holds the mirrored mesh), like the games'
    /// left-handed view.
    fn pose(&self, kick: f32, twist: f32, lag: Vec2, sign: f32) -> Transform {
        let aim = Quat::from_euler(
            EulerRot::YXZ,
            self.yaw + self.twist_yaw * twist,
            self.pitch + self.kick_pitch * kick,
            self.roll + self.twist_roll * twist,
        );
        let hold = self.rest + Vec3::new(0.0, self.kick_up, self.kick_back) * kick;
        // The lag turns the gun about the eye. It is the view's, so it isn't mirrored: undo that
        // here for the mirror below.
        let trail = Quat::from_euler(EulerRot::YXZ, lag.x * sign, lag.y, 0.0);
        let (at, turn) = (trail * hold, trail * aim);
        Transform {
            translation: Vec3::new(at.x * sign, at.y, at.z),
            // A rotation seen in a mirror across X turns the other way about Y and Z.
            rotation: Quat::from_xyzw(turn.x, turn.y * sign, turn.z * sign, turn.w),
            scale: Vec3::ONE,
        }
    }
}

/// tan(103° / 2): the half horizontal view the layout is made for.
const REF_TAN_H: f32 = 1.257_17;
const REF_ASPECT: f32 = 16.0 / 9.0;
const REF_TAN_V: f32 = REF_TAN_H / REF_ASPECT;
/// Horizontal position (m) around which the guns are held: at other aspects the rig shifts so
/// this stays the same share of the way from the crosshair to the screen's side edge.
const ANCHOR_X: f32 = 0.26;

/// The rig's place under a camera with these half-view tangents: scaling x and y by the ratio
/// of the vertical tangents keeps the guns' size relative to the screen height, and the shift
/// keeps them in their corner at any aspect (see [`ANCHOR_X`]).
fn rig_placement(tan_v: f32, aspect: f32, sign: f32) -> Transform {
    let scale = tan_v / REF_TAN_V;
    let shift = ANCHOR_X * (tan_v * aspect / REF_TAN_H - scale) * sign;
    Transform::from_xyz(shift, 0.0, 0.0).with_scale(Vec3::new(scale, scale, 1.0))
}

/// A displacement on a critically damped spring, stepped with its exact solution so it moves
/// the same at any frame rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Spring {
    offset: f32,
    velocity: f32,
}

impl Spring {
    /// A push: the velocity that makes a spring at rest peak at exactly `strength`.
    fn kick(&mut self, omega: f32, strength: f32) {
        self.velocity += omega * E * strength;
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

/// How long the gun takes to follow the view (s): at a steady turn it trails by the angle the
/// view turns in this time.
const SWAY_SECS: f32 = 0.035;
/// The furthest the gun is shown trailing (rad), however fast the view turns: under 1°, so a
/// flick nudges the gun and never throws it across the screen.
const SWAY_LIMIT: f32 = 0.016;
/// How long a muzzle flash is shown (s).
const FLASH_SECS: f32 = 0.05;

/// Everything about a gun that moves.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Motion {
    /// Recoil, 1 = a full kick.
    kick: Spring,
    /// The sideways part of the recoil, -1..=1 at a shot's peak.
    twist: Spring,
    /// How far the gun trails the view (yaw, pitch; rad), before [`SWAY_LIMIT`].
    lag: Vec2,
    /// Seconds since the latest shot.
    since_shot: f32,
    /// Shots fired, for the variation between them.
    shots: u32,
}

impl Default for Motion {
    fn default() -> Self {
        Self { kick: Spring::default(), twist: Spring::default(), lag: Vec2::ZERO, since_shot: f32::MAX, shots: 0 }
    }
}

impl Motion {
    /// A shot: a full kick, twisted a different way and amount every time.
    fn fire(&mut self, omega: f32) {
        self.kick.kick(omega, 1.0);
        self.twist.kick(omega, variation(self.shots) * 2.0 - 1.0);
        self.since_shot = 0.0;
        self.shots += 1;
    }

    /// `dt` passes, in which the view turns by `turn` (yaw, pitch; rad). The lag follows
    /// lag' = −lag/τ − turn rate, solved exactly for a steady turn, so like the springs it
    /// moves the same at any frame rate.
    fn step(&mut self, dt: f32, turn: Vec2, omega: f32) {
        self.kick.step(dt, omega);
        self.twist.step(dt, omega);
        let keep = (-dt / SWAY_SECS).exp();
        // (1 − e^(−dt/τ))τ/dt, which is 1 for a turn in no time.
        let taken = if dt > 0.0 { (1.0 - keep) * SWAY_SECS / dt } else { 1.0 };
        self.lag = self.lag * keep - turn * taken;
        self.since_shot += dt;
    }

    fn at_rest(&self) -> bool {
        self.kick.at_rest() && self.twist.at_rest() && self.lag.abs().max_element() < 1e-5 && !self.flashing()
    }

    /// The lag shown: eased into [`SWAY_LIMIT`].
    fn trail(&self) -> Vec2 {
        (self.lag / SWAY_LIMIT).map(f32::tanh) * SWAY_LIMIT
    }

    fn flashing(&self) -> bool {
        self.since_shot < FLASH_SECS
    }

    /// The flash's size (1 = full) and its turn about the barrel, different for every shot.
    fn flash(&self) -> (f32, f32) {
        let age = (self.since_shot / FLASH_SECS).min(1.0);
        // Full for most of its time, then gone quickly.
        (1.0 - age * age * age, variation(self.shots.wrapping_add(7)) * std::f32::consts::TAU)
    }
}

/// A number in 0..1 for each shot, scattered evenly (the golden-ratio sequence).
fn variation(shot: u32) -> f32 {
    (shot as f32 * 0.618_034).fract()
}

/// Bakes every gun into a mesh under the player camera, hidden until [`show_gun`] shows the
/// chosen one.
fn spawn_guns(
    mut commands: Commands,
    camera: Single<Entity, With<PlayerCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // White, so the vertex colors are the finishes; matte, because a glossier surface mirrors
    // the ambient light, which greys the dark finishes.
    let finish = materials.add(StandardMaterial { perceptual_roughness: 0.7, reflectance: 0.2, ..default() });
    let flash_mesh = meshes.add(flash::flash());
    let flash_material = materials.add(flash::material());
    let rig = commands.spawn((ViewmodelRig, Transform::IDENTITY, Visibility::Inherited, ChildOf(*camera))).id();
    for (model, handling, sculpt) in
        [(WeaponModel::Pistol, &PISTOL, hands::pistol()), (WeaponModel::Rifle, &RIFLE, hands::rifle())]
    {
        let left = meshes.add(sculpt.mirrored().into_mesh());
        let right = meshes.add(sculpt.into_mesh());
        let flash = commands
            .spawn((
                MuzzleFlash,
                Mesh3d(flash_mesh.clone()),
                MeshMaterial3d(flash_material.clone()),
                Transform::from_translation(handling.muzzle),
                Visibility::Hidden,
            ))
            .id();
        let gun = commands
            .spawn((
                Mesh3d(right.clone()),
                Gun { model, handling, meshes: [right, left], flash, motion: Motion::default(), view: None },
                MeshMaterial3d(finish.clone()),
                // Right-handed until `show_gun` applies the chosen side.
                handling.pose(0.0, 0.0, Vec2::ZERO, 1.0),
                Visibility::Hidden,
                ChildOf(rig),
            ))
            .id();
        commands.entity(flash).insert(ChildOf(gun));
    }
}

/// Shows the chosen gun while a drill is played; hiding one also drops its recoil.
fn show_gun(
    settings: Res<Settings>,
    state: Res<State<AppState>>,
    mut guns: Query<(&mut Gun, &mut Mesh3d, &mut Transform, &mut Visibility)>,
    mut flashes: Query<&mut Visibility, (With<MuzzleFlash>, Without<Gun>)>,
) {
    let playing = *state.get() == AppState::Playing;
    let side = settings.video.weapon_side;
    for (mut gun, mut mesh, mut transform, mut visibility) in &mut guns {
        mesh.set_if_neq(Mesh3d(gun.meshes[if side == WeaponSide::Right { 0 } else { 1 }].clone()));
        let shown = playing && gun.model == settings.video.weapon;
        visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
        if !shown {
            gun.motion = Motion::default();
            gun.view = None;
            if let Ok(mut flash) = flashes.get_mut(gun.flash) {
                flash.set_if_neq(Visibility::Hidden);
            }
        }
        // Also re-poses the shown gun when the side changes.
        let motion = gun.motion;
        let pose = gun.handling.pose(motion.kick.offset, motion.twist.offset, motion.trail(), side.sign());
        transform.set_if_neq(pose);
    }
}

/// Fits the rig to the camera's view (see [`rig_placement`]).
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
    rig.set_if_neq(rig_placement(tan_v, perspective.aspect_ratio, settings.video.weapon_side.sign()));
}

/// Moves the shown gun: a kick and a flash for each shot of the run (as the drill counts them: a
/// click that fires, or every round of automatic fire), the spring back, and the lag behind a
/// turning view.
#[allow(clippy::type_complexity)]
fn animate(
    time: Res<Time>,
    settings: Res<Settings>,
    run: Option<Res<Run>>,
    look: Res<Look>,
    mut guns: Query<(&mut Gun, &mut Transform)>,
    mut flashes: Query<(&mut Transform, &mut Visibility), (With<MuzzleFlash>, Without<Gun>)>,
    mut seen: Local<u32>,
) {
    let fired = run.map_or(0, |run| run.fired());
    // A new run starts counting from zero again.
    let shot = fired > *seen;
    *seen = fired;
    let view = Vec2::from(look.angles());
    for (mut gun, mut transform) in &mut guns {
        if gun.model != settings.video.weapon {
            continue;
        }
        // No turn in the first frame a gun is shown: the view was reset for the run.
        let turn = view - gun.view.unwrap_or(view);
        if gun.view != Some(view) {
            gun.view = Some(view);
        }
        if !shot && turn == Vec2::ZERO && gun.motion.at_rest() {
            continue;
        }
        let omega = gun.handling.omega;
        let mut motion = gun.motion;
        if shot {
            motion.fire(omega);
        }
        motion.step(time.delta_secs(), turn, omega);
        if motion.at_rest() {
            // Settled: exactly the rest pose, and nothing left to do until the next shot or turn.
            motion = Motion { shots: motion.shots, ..default() };
        }
        gun.motion = motion;
        let pose = gun.handling.pose(
            motion.kick.offset,
            motion.twist.offset,
            motion.trail(),
            settings.video.weapon_side.sign(),
        );
        transform.set_if_neq(pose);
        if let Ok((mut flash, mut visibility)) = flashes.get_mut(gun.flash) {
            let (size, roll) = motion.flash();
            visibility.set_if_neq(if motion.flashing() { Visibility::Inherited } else { Visibility::Hidden });
            if motion.flashing() {
                flash.rotation = Quat::from_rotation_z(roll);
                flash.scale = Vec3::splat(size * gun.handling.flash);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::drill::{DrillKind, DrillSpec, RunPurpose};

    #[allow(clippy::type_complexity)]
    const MODELS: [(&Handling, fn() -> mesh::Sculpt); 2] = [(&PISTOL, hands::pistol), (&RIFLE, hands::rifle)];
    /// Vertices a gun may have: a few thousand cost nothing to draw, and a model that needs
    /// more has stopped being low-poly.
    const MAX_VERTICES: usize = 8_000;

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

    /// Each gun is closed shells that face outwards (see `Sculpt::sound_volume`), small enough to
    /// be cheap, with its muzzle where the flash is put.
    #[test]
    fn guns_are_sound_and_within_budget() {
        for (handling, build) in MODELS {
            let gun = build();
            // More than 50 cm³ of gun.
            let volume = gun.sound_volume();
            assert!(volume > 5e-5);
            // The left hand's mirror image is as sound.
            assert!((gun.mirrored().sound_volume() - volume).abs() < volume * 1e-4);
            assert!(gun.positions().len() <= MAX_VERTICES, "{} vertices", gun.positions().len());
            let front = gun.positions().iter().map(|position| -position.z).fold(f32::MIN, f32::max);
            assert!((0.0..0.006).contains(&(-handling.muzzle.z - front)), "muzzle at {front}");
        }
    }

    /// Not a test but the tool `tools/hands/pose.py` needs first: writes each gun without its
    /// hands, and how it is held in the reference view, to `target/hands`.
    #[test]
    #[ignore = "a tool: cargo test export_guns -- --ignored"]
    fn export_guns() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/hands");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, handling, gun) in [("pistol", &PISTOL, pistol::pistol()), ("rifle", &RIFLE, rifle::rifle())] {
            std::fs::write(dir.join(format!("{name}.obj")), gun.to_obj()).unwrap();
            let rest = handling.pose(0.0, 0.0, Vec2::ZERO, 1.0).to_matrix().to_cols_array();
            let held = format!("{{\"rest\": {rest:?}, \"tan_v\": {REF_TAN_V}, \"aspect\": {REF_ASPECT}}}");
            std::fs::write(dir.join(format!("{name}.json")), held).unwrap();
        }
    }

    /// Where a point of the rig is on screen: half screen heights right of and above the centre.
    fn on_screen(rig: &Transform, point: Vec3, tan_v: f32) -> Vec2 {
        let seen = rig.transform_point(point);
        seen.truncate() / (-seen.z * tan_v)
    }

    /// What the guns leave clear, in half screen heights from the crosshair. Targets appear
    /// down to 13° under it (0.33 at 103°, plus their radius; Gridshot's lowest row is at 0.3), so
    /// in the middle 60% of the width a resting gun stays under 0.42 (71% of the way down the
    /// screen) and a kicking, swaying one under 0.38 (69%). Towards the screen's side edge it may
    /// reach higher, and at any height it stays this far from the vertical through the crosshair.
    const MIDDLE: f32 = 0.6;
    const CLEAR_ABOVE_AT_REST: f32 = -0.42;
    const CLEAR_ABOVE_MOVING: f32 = -0.38;
    const CLEAR_ABOVE_AT_THE_SIDE: f32 = -0.2;
    const CLEAR_BESIDE: f32 = 0.2;
    /// The camera's near plane (m): a forearm is cut off there, on its way out of the picture.
    const NEAR: f32 = 0.1;

    /// The guns keep to their lower corner: on either side, at narrow and ultrawide aspects, at
    /// any FOV, at rest and thrown about by recoil and sway, nothing of them reaches the part of
    /// the screen where the crosshair and the targets are.
    #[test]
    fn guns_keep_to_their_corner() {
        for (handling, build) in MODELS {
            let gun = build();
            for aspect in [5.0 / 4.0, 4.0 / 3.0, 16.0 / 10.0, REF_ASPECT, 21.0 / 9.0, 32.0 / 9.0] {
                for (tan_v, sign) in [(0.45, 1.0), (REF_TAN_V, 1.0), (REF_TAN_V, -1.0), (1.0, -1.0)] {
                    let rig = rig_placement(tan_v, aspect, sign);
                    // Trailing towards the crosshair and upwards, at the peak of a burst, twisted
                    // either way.
                    let lag = Vec2::new(SWAY_LIMIT * sign, SWAY_LIMIT);
                    let poses = [
                        (handling.pose(0.0, 0.0, Vec2::ZERO, sign), CLEAR_ABOVE_AT_REST),
                        (handling.pose(BURST_KICK, -BURST_TWIST, lag, sign), CLEAR_ABOVE_MOVING),
                        (handling.pose(BURST_KICK, BURST_TWIST, lag, sign), CLEAR_ABOVE_MOVING),
                    ];
                    for (pose, clear_above) in poses {
                        // The left hand holds the mirrored mesh.
                        for vertex in gun.positions().iter().map(|&vertex| vertex * Vec3::new(sign, 1.0, 1.0)) {
                            let held = pose.transform_point(vertex);
                            if rig.transform_point(held).z > -NEAR {
                                continue;
                            }
                            let at = on_screen(&rig, held, tan_v);
                            // A forearm leaves the picture through its bottom edge.
                            if at.y < -1.0 {
                                continue;
                            }
                            let top = if at.x.abs() < MIDDLE * aspect { clear_above } else { CLEAR_ABOVE_AT_THE_SIDE };
                            assert!(at.y < top && at.x * sign > CLEAR_BESIDE, "{at} at {aspect}, side {sign}");
                        }
                    }
                }
            }
        }
    }

    /// A gun covers the same part of the screen at any FOV, and is as tall and as far from its
    /// side's edge at any aspect.
    #[test]
    fn guns_fit_any_view() {
        let sample = [Vec3::new(0.2, -0.2, -0.45), Vec3::new(0.3, -0.26, -0.9)];
        let reference = sample.map(|point| on_screen(&rig_placement(REF_TAN_V, REF_ASPECT, 1.0), point, REF_TAN_V));
        for tan_v in [0.4, 0.9] {
            let rig = rig_placement(tan_v, REF_ASPECT, 1.0);
            for (point, expected) in sample.into_iter().zip(reference) {
                assert!((on_screen(&rig, point, tan_v) - expected).length() < 1e-5);
            }
        }
        for aspect in [4.0 / 3.0, 32.0 / 9.0] {
            let rig = rig_placement(REF_TAN_V, aspect, 1.0);
            let seen = sample.map(|point| on_screen(&rig, point, REF_TAN_V));
            assert!((seen[0].y - reference[0].y).abs() < 1e-5 && (seen[1].y - reference[1].y).abs() < 1e-5);
            // The anchor line keeps its place between the centre and the edge.
            let anchor = Vec3::new(ANCHOR_X, -0.2, -0.5);
            let across = |aspect: f32| on_screen(&rig_placement(REF_TAN_V, aspect, 1.0), anchor, REF_TAN_V).x / aspect;
            assert!((across(aspect) - across(REF_ASPECT)).abs() < 1e-5);
        }
    }

    /// The left hand holds the right hand's gun in a mirror; the lag is the view's, the same
    /// for both.
    #[test]
    fn left_hand_mirrors_the_right() {
        let mirror = Vec3::new(-1.0, 1.0, 1.0);
        for (handling, _) in MODELS {
            let right = handling.pose(0.7, -0.4, Vec2::new(0.01, -0.006), 1.0);
            let left = handling.pose(0.7, -0.4, Vec2::new(-0.01, -0.006), -1.0);
            for point in [handling.muzzle, Vec3::new(0.012, -0.1, 0.03), Vec3::ZERO] {
                // The left hand holds the mirrored mesh.
                let (right, left) = (right.transform_point(point), left.transform_point(point * mirror));
                assert!((left - right * mirror).length() < 1e-6, "{left} is not {right} mirrored");
            }
        }
    }

    /// The exact step moves the same at 30 and 360 fps.
    #[test]
    fn recoil_is_frame_rate_independent() {
        for omega in [PISTOL.omega, RIFLE.omega] {
            let mut kicked = Spring::default();
            kicked.kick(omega, 1.0);
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
            spring.kick(omega, 1.0);
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

    /// The furthest held fire throws the rifle: the kicks of a 12 rounds/s burst pile up to this
    /// many, and its twists (which alternate) to this.
    const BURST_KICK: f32 = 1.7;
    const BURST_TWIST: f32 = 1.05;

    /// Held fire doesn't walk the gun off: kick and twist stay within the burst's bounds.
    #[test]
    fn automatic_fire_stays_bounded() {
        let mut motion = Motion::default();
        let (mut kick, mut twist): (f32, f32) = (0.0, 0.0);
        for frame in 0..1200 {
            if frame % 20 == 0 {
                motion.fire(RIFLE.omega);
            }
            motion.step(1.0 / 240.0, Vec2::ZERO, RIFLE.omega);
            kick = kick.max(motion.kick.offset);
            twist = twist.max(motion.twist.offset.abs());
        }
        assert!(kick > 1.0 && kick <= BURST_KICK, "{kick}");
        assert!(twist > 0.5 && twist <= BURST_TWIST, "{twist}");
    }

    /// A shot during a steady turn, then standing still: recoil, lag and flash are where they
    /// should be after the same time whether it passed in 30 fps or 400 fps frames.
    #[test]
    fn motion_is_frame_rate_independent() {
        for (handling, _) in MODELS {
            let play = |frames: u32| {
                let mut motion = Motion::default();
                motion.fire(handling.omega);
                let (turning, still, turn) = (0.066, 0.033, Vec2::new(0.05, -0.02));
                for _ in 0..frames * 2 {
                    motion.step(turning / (frames * 2) as f32, turn / (frames * 2) as f32, handling.omega);
                }
                let flashing = motion.flashing();
                for _ in 0..frames {
                    motion.step(still / frames as f32, Vec2::ZERO, handling.omega);
                }
                (motion, flashing)
            };
            let ((slow, slow_flashing), (fast, fast_flashing)) = (play(1), play(13));
            assert!((slow.kick.offset - fast.kick.offset).abs() < 1e-4, "{slow:?} vs {fast:?}");
            assert!((slow.twist.offset - fast.twist.offset).abs() < 1e-4, "{slow:?} vs {fast:?}");
            assert!((slow.lag - fast.lag).length() < 1e-5, "{slow:?} vs {fast:?}");
            assert!((slow.since_shot - fast.since_shot).abs() < 1e-5);
            assert!(!slow_flashing && !fast_flashing && slow.shots == fast.shots);
        }
        // The flash shrinks with time alone, and is over when it says so.
        let lit = |since_shot: f32| Motion { since_shot, ..default() };
        assert!(lit(0.0).flashing() && lit(0.0).flash().0 == 1.0);
        assert!(
            lit(FLASH_SECS * 0.5).flash().0 < 1.0 && lit(FLASH_SECS * 0.5).flash().0 > lit(FLASH_SECS * 0.9).flash().0
        );
        assert!(!lit(FLASH_SECS).flashing() && lit(FLASH_SECS).flash().0 == 0.0);
    }

    /// The gun trails a turning view by what the view turns in `SWAY_SECS`, to the side the view
    /// came from on either hand, never by more than `SWAY_LIMIT`, and comes back to rest.
    #[test]
    fn gun_trails_the_view_within_its_limit() {
        let mut motion = Motion::default();
        // A slow turn to the left: 0.2 rad/s.
        for _ in 0..100 {
            motion.step(0.005, Vec2::new(0.001, 0.0), PISTOL.omega);
        }
        assert!((motion.lag.x + 0.2 * SWAY_SECS).abs() < 1e-5, "{}", motion.lag);
        for sign in [1.0, -1.0] {
            let rest = PISTOL.pose(0.0, 0.0, Vec2::ZERO, sign).translation;
            let trailing = PISTOL.pose(0.0, 0.0, motion.trail(), sign).translation;
            assert!(trailing.x > rest.x + 1e-4, "the gun stays behind, to the right");
        }
        // A flick.
        motion.step(0.005, Vec2::new(2.0, -1.0), PISTOL.omega);
        assert!(motion.lag.length() > 1.0 && motion.trail().abs().max_element() <= SWAY_LIMIT);
        for _ in 0..200 {
            motion.step(0.005, Vec2::ZERO, PISTOL.omega);
        }
        assert!(motion.at_rest(), "{motion:?}");
    }

    fn app() -> (App, Entity) {
        let mut settings = Settings::default();
        settings.video.weapon = WeaponModel::Rifle;
        let drill = DrillSpec::builtin(DrillKind::Gridshot, &settings);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(4)))
            .insert_state(AppState::Playing)
            .add_sub_state::<PlayState>()
            .insert_resource(settings)
            .init_resource::<Look>()
            .insert_resource(Run::new(drill, 0.4, 60.0, RunPurpose::Practice, String::new()));
        add_animation(&mut app);
        let flash = app.world_mut().spawn((MuzzleFlash, Transform::IDENTITY, Visibility::Hidden)).id();
        let gun = Gun {
            model: WeaponModel::Rifle,
            handling: &RIFLE,
            meshes: default(),
            flash,
            motion: Motion::default(),
            view: None,
        };
        let gun = app.world_mut().spawn((gun, RIFLE.pose(0.0, 0.0, Vec2::ZERO, 1.0))).id();
        (app, gun)
    }

    /// While the run is paused the gun doesn't move, however long the pause lasts, and on
    /// resuming it goes on exactly as if there had been no pause.
    #[test]
    fn paused_gun_stands_still_and_goes_on_where_it_was() {
        let play = |paused_frames: u32| {
            let (mut app, gun) = app();
            let snapshot = |app: &App| {
                let (gun, flash) = (app.world().entity(gun), app.world().get::<Gun>(gun).unwrap().flash);
                let flash = app.world().entity(flash);
                (
                    gun.get::<Gun>().unwrap().motion,
                    *gun.get::<Transform>().unwrap(),
                    *flash.get::<Transform>().unwrap(),
                    *flash.get::<Visibility>().unwrap(),
                )
            };
            app.update();
            app.world_mut().resource_mut::<Run>().shot();
            for frame in 0..4 {
                app.world_mut().resource_mut::<Look>().set(0.004 * frame as f32, 0.0);
                app.update();
            }
            let mid_kick = snapshot(&app);
            if paused_frames > 0 {
                app.world_mut().resource_mut::<NextState<PlayState>>().set(PlayState::Paused);
                for _ in 0..paused_frames {
                    app.update();
                    assert_eq!(snapshot(&app), mid_kick);
                }
                app.world_mut().resource_mut::<NextState<PlayState>>().set(PlayState::Running);
            }
            for _ in 0..4 {
                app.update();
            }
            (mid_kick, snapshot(&app))
        };
        let (mid_kick, after) = play(0);
        // The shot is being shown: kicked back and trailing, the flash lit.
        assert!(mid_kick.0.kick.offset > 0.3 && mid_kick.0.lag.x < 0.0 && mid_kick.3 == Visibility::Inherited);
        assert_ne!(mid_kick.1, RIFLE.pose(0.0, 0.0, Vec2::ZERO, 1.0));
        assert_ne!(after, mid_kick);
        assert_eq!(play(30), (mid_kick, after));
    }
}
