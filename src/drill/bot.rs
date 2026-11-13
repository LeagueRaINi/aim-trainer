//! The bot: a humanoid target with a head and a body, at a person's proportions. Its head is an
//! ordinary [`Target`](super::targets::Target) sphere (what the replay recorder, the health bar
//! and every other generic system sees); the body stands under it on the
//! [range](super::range)'s deck: neck, a torso tapering from the shoulders to the waist, hips,
//! arms and two legs. A head hit kills at once, the body takes [`BODY_SHOTS`], like a Valorant
//! rifle against a full-health, fully armoured player (Vandal: 160 to the head, 40 to the body,
//! against 150). Arms and legs count as body here; in Valorant legs take one shot more.
//!
//! The body always faces the eye. Its mesh and its hit test are made from one table of parts
//! ([`BODY`]): each part is drawn as a tapered box, and a shot hits the body where its line
//! crosses the body's upright plane inside the front outline of a part. So what is drawn is what
//! is hit, gaps included (between the legs, between an arm and the waist), up to the parts'
//! thickness of a few centimetres seen at an angle.

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::EntityCommands;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::geometry::ray_hit;
use super::health::Health;
use super::range;
use super::targets::{self, SpawnCtx};
use crate::AppState;
use crate::camera::PlayerCamera;
use crate::replay::player::{Playback, ReplayTarget, advance_playback};
use crate::settings::{Settings, TargetStyle};

/// Height of a standing bot's head centre above the floor it stands on, metres: eye height, so a
/// crosshair kept level is on every head.
pub(crate) const EYE_HEIGHT: f32 = 1.62;
/// Body hits that kill a bot.
pub(crate) const BODY_SHOTS: f32 = 4.0;
/// The widest the body gets (at the hands), metres from its axis.
pub(crate) const BODY_HALF_WIDTH: f32 = 0.25;
/// How far the body's color is taken from the target color towards [`BODY_SHADE`]: a calmer,
/// darker tone of the player's target color, so the head is the brightest thing on the bot.
const BODY_DIM: f32 = 0.65;
const BODY_SHADE: Color = Color::srgb(0.1, 0.11, 0.14);

/// A part of the body: a box between two heights whose width, depth and sideways position change
/// evenly from its top to its bottom. Metres, relative to the centre of the head, as seen from
/// the front: x to the bot's side, y up.
#[derive(Clone, Copy)]
struct Part {
    top: f32,
    bottom: f32,
    /// Sideways position of its middle at the top and at the bottom.
    x: (f32, f32),
    /// Half its width at the top and at the bottom.
    half: (f32, f32),
    /// Its depth (front to back) at the top and at the bottom: for the mesh only.
    depth: (f32, f32),
}

impl Part {
    const fn mirrored(self) -> Part {
        Part { x: (-self.x.0, -self.x.1), ..self }
    }

    /// Its sideways position and half width at `y`.
    fn at(&self, y: f32) -> (f32, f32) {
        let down = (self.top - y) / (self.top - self.bottom);
        (self.x.0 + (self.x.1 - self.x.0) * down, self.half.0 + (self.half.1 - self.half.0) * down)
    }

    /// Whether the point of the body's front plane is inside its outline.
    fn covers(&self, x: f32, y: f32) -> bool {
        let (middle, half) = self.at(y);
        (self.bottom..=self.top).contains(&y) && (x - middle).abs() <= half
    }

    /// A corner: at the top or bottom, on the side `right` and the face `front` say (each 1 or -1).
    fn corner(&self, top: bool, right: f32, front: f32) -> Vec3 {
        let (y, x, half, depth) = if top {
            (self.top, self.x.0, self.half.0, self.depth.0)
        } else {
            (self.bottom, self.x.1, self.half.1, self.depth.1)
        };
        Vec3::new(x + right * half, y, front * depth * 0.5)
    }
}

/// From inside a head of the usual sizes down to the shoulders.
const NECK: Part = Part { top: -0.1, bottom: -0.24, x: (0.0, 0.0), half: (0.05, 0.06), depth: (0.1, 0.11) };
/// Shoulders to waist: with the arms 47 cm across at the top.
const TORSO: Part = Part { top: -0.22, bottom: -0.62, x: (0.0, 0.0), half: (0.165, 0.12), depth: (0.2, 0.16) };
const HIPS: Part = Part { top: -0.62, bottom: -0.8, x: (0.0, 0.0), half: (0.12, 0.16), depth: (0.16, 0.18) };
/// The right arm, hanging a little away from the body down to the hand at mid-thigh.
const ARM: Part = Part { top: -0.23, bottom: -0.92, x: (0.185, 0.212), half: (0.05, 0.038), depth: (0.11, 0.08) };
/// The right leg, down to the floor.
const LEG: Part = Part { top: -0.78, bottom: -EYE_HEIGHT, x: (0.083, 0.09), half: (0.078, 0.055), depth: (0.17, 0.11) };
const BODY: [Part; 7] = [NECK, TORSO, HIPS, ARM, ARM.mirrored(), LEG, LEG.mirrored()];

/// What a shot hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Zone {
    Head,
    Body,
}

impl Zone {
    /// What a hit takes off a bot's [`Health`] of [`BODY_SHOTS`]: all of it for the head.
    pub(crate) fn damage(self) -> f32 {
        match self {
            Zone::Head => BODY_SHOTS,
            Zone::Body => 1.0,
        }
    }
}

/// The level direction from a bot's head to the eye: the way its body faces.
fn facing(head: Vec3, eye: Vec3) -> Vec3 {
    Vec3::new(eye.x - head.x, 0.0, eye.z - head.z).normalize_or(Vec3::Z)
}

/// Where the ray from the eye at `origin` along `dir` (a unit vector) hits the bot whose head, of
/// `head_radius`, is centred at `head`, and how far along the ray. A ray through the head is a
/// head hit even if it also crosses the neck; the body is hit where the ray crosses the upright
/// plane the body stands in (facing the eye) inside a part's outline.
pub(crate) fn hit(origin: Vec3, dir: Vec3, head: Vec3, head_radius: f32) -> Option<(f32, Zone)> {
    if let Some(distance) = ray_hit(origin, dir, head, head_radius) {
        return Some((distance, Zone::Head));
    }
    let facing = facing(head, origin);
    let approach = -dir.dot(facing);
    if approach <= 1e-6 {
        return None;
    }
    let distance = (origin - head).dot(facing) / approach;
    let at = origin + dir * distance - head;
    // The parts mirror each other, so which side counts as x > 0 makes no difference.
    let (x, y) = (at.dot(Vec3::Y.cross(facing)), at.y);
    BODY.iter().any(|part| part.covers(x, y)).then_some((distance, Zone::Body))
}

/// A bot's head: the [`Target`](super::targets::Target) entity. Its body is a child, see [`Body`].
#[derive(Component)]
pub(crate) struct Bot;

/// The body mesh of a [`Bot`], a child of the head that [`place_bodies`] keeps upright, facing
/// the eye and at its true size whatever the head's rotation and radius.
#[derive(Component)]
struct Body;

#[derive(Resource)]
struct BotAssets {
    /// The whole body in one mesh, in metres, with the head's centre as its origin and its front
    /// towards +Z.
    body_mesh: Handle<Mesh>,
    body: Handle<StandardMaterial>,
}

/// Spawns a bot whose head is centred at `head`: a target of the drill's radius (the target size
/// setting scales the head, the thing to hit; the body keeps its true size) with a bot's health.
/// For the bot to stand on the deck, `head.y` must be 0.
pub(crate) fn spawn_bot<'a>(ctx: &'a mut SpawnCtx, head: Vec3) -> EntityCommands<'a> {
    let mut bot = ctx.spawn_target(head, None);
    bot.insert((Bot, Health::new(BODY_SHOTS)));
    bot
}

pub(super) fn register(app: &mut App) {
    app.add_systems(Startup, create_bot_assets)
        .add_systems(Update, sync_body_look.run_if(resource_changed::<Settings>))
        .add_systems(Update, dress_replay_bots.after(advance_playback).run_if(in_state(AppState::Replay)))
        .add_systems(
            PostUpdate,
            // After the heads have their final place and rotation for the frame.
            (add_bodies, place_bodies).chain().after(targets::face_eye).before(TransformSystems::Propagate),
        );
}

/// The body's color: the target color, dimmed.
fn body_color(settings: &Settings) -> Color {
    targets::target_color(settings).mix(&BODY_SHADE, BODY_DIM)
}

/// The body's material: shaded like the targets, except that a bullseye's rings only make sense
/// on the head, so its body is flat.
fn body_material(settings: &Settings, images: &mut Assets<Image>) -> StandardMaterial {
    let style = match settings.targets.style {
        TargetStyle::Bullseye => TargetStyle::Flat,
        style => style,
    };
    targets::target_material(body_color(settings), style, images)
}

/// Every part of [`BODY`] as a box with flat faces, in one mesh.
fn body_mesh() -> Mesh {
    // Each face by its corners: (top or bottom, side, front or back).
    const FACES: [[(bool, f32, f32); 4]; 6] = [
        [(true, -1.0, 1.0), (false, -1.0, 1.0), (false, 1.0, 1.0), (true, 1.0, 1.0)],
        [(true, 1.0, -1.0), (false, 1.0, -1.0), (false, -1.0, -1.0), (true, -1.0, -1.0)],
        [(true, 1.0, 1.0), (false, 1.0, 1.0), (false, 1.0, -1.0), (true, 1.0, -1.0)],
        [(true, -1.0, -1.0), (false, -1.0, -1.0), (false, -1.0, 1.0), (true, -1.0, 1.0)],
        [(true, -1.0, -1.0), (true, -1.0, 1.0), (true, 1.0, 1.0), (true, 1.0, -1.0)],
        [(false, -1.0, 1.0), (false, -1.0, -1.0), (false, 1.0, -1.0), (false, 1.0, 1.0)],
    ];
    let (mut positions, mut normals, mut indices) = (Vec::new(), Vec::new(), Vec::new());
    for part in &BODY {
        for face in FACES {
            let corners = face.map(|(top, right, front)| part.corner(top, right, front));
            // The corners go around counter-clockwise seen from outside.
            let normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]).normalize();
            let first = positions.len() as u16;
            positions.extend(corners.map(|corner| corner.to_array()));
            normals.extend([normal.to_array(); 4]);
            indices.extend([0, 1, 2, 0, 2, 3].map(|corner| first + corner));
        }
    }
    let uvs = vec![[0.0f32; 2]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U16(indices))
}

fn create_bot_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    settings: Res<Settings>,
) {
    commands.insert_resource(BotAssets {
        body_mesh: meshes.add(body_mesh()),
        body: materials.add(body_material(&settings, &mut images)),
    });
}

/// Applies a new target color or style to the bodies.
fn sync_body_look(
    settings: Res<Settings>,
    assets: Res<BotAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut applied: Local<Option<([u8; 3], TargetStyle)>>,
) {
    let look = (settings.targets.color, settings.targets.style);
    if applied.replace(look) != Some(look)
        && let Some(mut material) = materials.get_mut(&assets.body)
    {
        *material = body_material(&settings, &mut images);
    }
}

/// A bot drill's replay shows the recorded heads as bots again: a body always stands straight
/// below its head, so the head's position is all a replay needs.
fn dress_replay_bots(mut commands: Commands, playback: Res<Playback>, heads: Query<Entity, Added<ReplayTarget>>) {
    if range::played_on(playback.replay.kind) {
        for head in &heads {
            commands.entity(head).insert(Bot);
        }
    }
}

/// Gives every new bot its body, placed by [`place_bodies`] right after.
fn add_bodies(mut commands: Commands, assets: Res<BotAssets>, bots: Query<Entity, Added<Bot>>) {
    for bot in &bots {
        commands.spawn((
            ChildOf(bot),
            Body,
            Mesh3d(assets.body_mesh.clone()),
            MeshMaterial3d(assets.body.clone()),
            Transform::from_scale(Vec3::ZERO),
        ));
    }
}

/// Keeps each body upright under its head, facing the eye (the player camera) and at its true
/// size: the head is a unit sphere scaled to its radius and turned to face the eye, both of
/// which its child would inherit.
#[allow(clippy::type_complexity)]
fn place_bodies(
    eye: Option<Single<&Transform, (With<PlayerCamera>, Without<Bot>, Without<Body>)>>,
    heads: Query<(&Transform, &Children), (With<Bot>, Without<Body>)>,
    mut bodies: Query<&mut Transform, With<Body>>,
) {
    let eye = eye.map_or(Vec3::ZERO, |eye| eye.translation);
    for (head, children) in &heads {
        let placed = body_transform(head, eye);
        for &child in children {
            if let Ok(mut body) = bodies.get_mut(child)
                && *body != placed
            {
                *body = placed;
            }
        }
    }
}

/// The body's transform as a child of `head`: the inverse of the head's rotation and scale, and
/// a turn about the bot's axis that brings the body's front (+Z) round to the eye. Nothing is
/// shown under a head without a size (a replay's eye anchor, see [`range::EyeAnchor`]).
fn body_transform(head: &Transform, eye: Vec3) -> Transform {
    let radius = head.scale.x;
    if radius <= 1e-4 {
        return Transform::from_scale(Vec3::ZERO);
    }
    let facing = facing(head.translation, eye);
    let turn = Quat::from_rotation_y(facing.x.atan2(facing.z));
    Transform { translation: Vec3::ZERO, rotation: head.rotation.inverse() * turn, scale: Vec3::splat(radius.recip()) }
}

#[cfg(test)]
mod tests {
    use bevy::mesh::VertexAttributeValues;

    use super::*;
    use crate::drill::FacesEye;
    use crate::drill::targets::facing_eye;

    const HEAD: f32 = 0.14;

    /// A shot from the eye at the point `offset` metres from the centre of a head at `head`.
    fn shot_at(eye: Vec3, head: Vec3, offset: Vec3) -> Option<Zone> {
        hit(eye, (head + offset - eye).normalize(), head, HEAD).map(|(_, zone)| zone)
    }

    #[test]
    fn shots_hit_the_head_the_body_or_nothing() {
        for (eye, head) in [
            (Vec3::ZERO, Vec3::new(0.0, 0.0, -10.0)),
            (Vec3::ZERO, Vec3::new(5.0, 0.0, -7.0)),
            // The player has strafed off-centre.
            (Vec3::new(2.5, 0.0, 0.0), Vec3::new(-1.0, 0.0, -13.0)),
        ] {
            // Across the body as the eye sees it: the body faces the eye.
            let side = (head - eye).cross(Vec3::Y).normalize();
            let at = |x: f32, y: f32| shot_at(eye, head, side * x + Vec3::Y * y);
            // The head, out to its edge and no further.
            assert_eq!(at(0.0, 0.0), Some(Zone::Head));
            assert_eq!(at(0.13, 0.0), Some(Zone::Head));
            assert_eq!(at(0.0, 0.13), Some(Zone::Head));
            assert_eq!(at(0.16, 0.0), None, "beside the head");
            assert_eq!(at(0.0, 0.2), None, "over the head");
            for way in [-1.0, 1.0] {
                // Neck, chest, shoulder, arm and hand are body; beside them is not.
                assert_eq!(at(0.03 * way, -0.19), Some(Zone::Body), "the neck");
                assert_eq!(at(0.1 * way, -0.19), None, "beside the neck, over the shoulder");
                assert_eq!(at(0.1 * way, -0.4), Some(Zone::Body), "the chest");
                assert_eq!(at(0.21 * way, -0.3), Some(Zone::Body), "the shoulder");
                assert_eq!(at(0.25 * way, -0.3), None, "beside the shoulder");
                assert_eq!(at(0.21 * way, -0.88), Some(Zone::Body), "the hand");
                assert_eq!(at(0.27 * way, -0.88), None, "beside the hand");
                assert_eq!(at(0.21 * way, -0.97), None, "under the hand");
                // The torso tapers: between the waist and the arm is air.
                assert_eq!(at(0.145 * way, -0.6), None, "between waist and arm");
                assert_eq!(at(0.13 * way, -0.75), Some(Zone::Body), "the hip");
                // Two legs with a gap between them, down to the floor.
                assert_eq!(at(0.08 * way, -1.2), Some(Zone::Body), "the leg");
                assert_eq!(at(0.16 * way, -1.2), None, "beside the leg");
                assert_eq!(at(0.085 * way, -(EYE_HEIGHT - 0.02)), Some(Zone::Body), "the foot");
                assert_eq!(at(0.085 * way, -(EYE_HEIGHT + 0.03)), None, "under the foot");
            }
            assert_eq!(at(0.0, -0.7), Some(Zone::Body), "the hips");
            assert_eq!(at(0.0, -1.4), None, "between the legs");
        }
        // A bot behind the player is never hit.
        assert_eq!(hit(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(0.0, 0.0, 10.0), HEAD), None);
        assert_eq!(hit(Vec3::ZERO, Vec3::new(0.0, -0.05, -1.0).normalize(), Vec3::new(0.0, 0.0, 10.0), HEAD), None);
    }

    /// Of two bots in a line, the nearer one is hit: distances along the ray order them.
    #[test]
    fn hits_report_their_distance() {
        let dir = Vec3::new(0.0, -0.05, -1.0).normalize();
        let near = hit(Vec3::ZERO, dir, Vec3::new(0.0, 0.0, -8.0), HEAD).unwrap();
        let far = hit(Vec3::ZERO, dir, Vec3::new(0.0, 0.0, -12.0), HEAD).unwrap();
        assert_eq!((near.1, far.1), (Zone::Body, Zone::Body));
        assert!((near.0 - 8.0).abs() < 0.1 && (far.0 - 12.0).abs() < 0.1, "{near:?} {far:?}");
    }

    #[test]
    fn one_head_shot_or_four_body_shots_kill() {
        let mut health = Health::new(BODY_SHOTS);
        assert!(health.damage(Zone::Head.damage()), "a head shot kills at once");
        let mut health = Health::new(BODY_SHOTS);
        for shot in 1..=3 {
            assert!(!health.damage(Zone::Body.damage()), "dead after {shot} body shots");
        }
        assert!((health.fraction() - 0.25).abs() < 1e-6);
        assert!(health.damage(Zone::Body.damage()), "alive after four body shots");
        // A head shot finishes a wounded bot too.
        let mut health = Health::new(BODY_SHOTS);
        health.damage(Zone::Body.damage());
        assert!(health.damage(Zone::Head.damage()));
    }

    /// What is drawn is what is hit. Every vertex of the body mesh, where the bot stands turned
    /// to the eye, is on the hit test's outline (a shot a hair inside it hits the body or, at the
    /// top of the neck, the head); the mesh stands on the deck, reaches no wider than
    /// [`BODY_HALF_WIDTH`] and has every face turned outward.
    #[test]
    fn the_body_mesh_is_the_hitbox() {
        let mesh = body_mesh();
        let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("positions")
        };
        let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else {
            panic!("normals")
        };
        assert_eq!(positions.len(), BODY.len() * 24);
        let lowest = positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!((lowest + EYE_HEIGHT).abs() < 1e-6, "feet at {lowest}");
        let widest = positions.iter().map(|p| p[0].abs()).fold(0.0, f32::max);
        assert!((widest - BODY_HALF_WIDTH).abs() < 1e-6, "{widest} m to the side");
        // Faces point away from the middle of their part (six faces of four vertices each).
        for (part, vertices) in positions.chunks(24).enumerate() {
            let middle = vertices.iter().map(|p| Vec3::from(*p)).sum::<Vec3>() / 24.0;
            for (face, corners) in vertices.chunks(4).enumerate() {
                let centre = corners.iter().map(|p| Vec3::from(*p)).sum::<Vec3>() / 4.0;
                let normal = Vec3::from(normals[part * 24 + face * 4]);
                assert!(normal.dot(centre - middle) > 0.0, "part {part}, face {face} faces inward");
            }
        }
        // The bot off to a side of a strafed-out eye, turned to it as `place_bodies` turns it.
        let (eye, at) = (Vec3::new(2.0, 0.0, 0.0), Vec3::new(-3.0, 0.0, -9.0));
        let head = Transform::from_translation(at).with_rotation(facing_eye(at - eye)).with_scale(Vec3::splat(HEAD));
        let body = head.mul_transform(body_transform(&head, eye));
        for (index, vertex) in positions.iter().enumerate() {
            let part = &BODY[index / 24];
            let local = Vec3::from(*vertex);
            // A hair towards the part's own middle, on the body's front plane.
            let (middle, _) = part.at(local.y);
            let inside = Vec3::new(local.x + (middle - local.x).signum() * 0.004, local.y, 0.0)
                + Vec3::Y * if local.y == part.top { -0.004 } else { 0.004 };
            let outside = Vec3::new(local.x + (local.x - middle).signum() * 0.004, inside.y, 0.0);
            let shot = |point: Vec3| hit(eye, (body.transform_point(point) - eye).normalize(), at, HEAD);
            assert!(shot(inside).is_some(), "part {}: a shot at {inside} inside the mesh misses", index / 24);
            // Just outside this part is a miss, unless another part (or the head) is there.
            let others = BODY.iter().any(|other| other.covers(outside.x, outside.y)) || outside.length() <= HEAD;
            assert_eq!(shot(outside).is_some(), others, "part {}: a shot at {outside}", index / 24);
        }
    }

    /// Whatever the head's size and rotation, its child body ends up centred on it, upright, at
    /// its true size and with its front turned to the eye.
    #[test]
    fn bodies_stand_upright_at_their_true_size_facing_the_eye() {
        let eye = Vec3::new(1.5, 0.0, 0.0);
        for (at, radius) in [(Vec3::new(4.0, 0.0, -9.0), 0.14), (Vec3::new(-3.0, 0.0, -6.0), 0.4)] {
            let head =
                Transform::from_translation(at).with_rotation(facing_eye(at - eye)).with_scale(Vec3::splat(radius));
            let body = head.mul_transform(body_transform(&head, eye));
            assert!(body.translation.distance(at) < 1e-5, "{}", body.translation);
            assert!((body.rotation * Vec3::Y - Vec3::Y).length() < 1e-5, "{:?}", body.rotation);
            assert!((body.rotation * Vec3::Z).angle_between(eye - at) < 1e-3, "{:?}", body.rotation);
            assert!((body.scale - Vec3::ONE).length() < 1e-5, "{}", body.scale);
        }
        let anchor = Transform::from_scale(Vec3::ZERO);
        assert_eq!(body_transform(&anchor, eye).scale, Vec3::ZERO);
    }

    /// Heads turn to the eye where the player stands, not to the middle of the room: a strafed-out
    /// player still sees a bullseye's rings as rings.
    #[test]
    fn heads_face_the_eye_wherever_the_player_stands() {
        let mut app = App::new();
        app.add_systems(Update, targets::face_eye);
        let eye = Vec3::new(2.5, 0.0, 0.0);
        app.world_mut().spawn((PlayerCamera, Transform::from_translation(eye)));
        let at = Vec3::new(-1.0, 0.0, -10.0);
        let head = app.world_mut().spawn((FacesEye, Transform::from_translation(at))).id();
        app.update();
        let pole = app.world().get::<Transform>(head).unwrap().rotation * Vec3::Z;
        assert!(pole.angle_between(eye - at) < 1e-3, "the pole points along {pole}");
    }
}
