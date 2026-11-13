//! The range: the scenery of the bot drills. The room's floor is 2.5 m below the eye, too far
//! down for a person of true proportions to stand on with the head at eye height, so the range
//! lays a deck over it at [`EYE_HEIGHT`] below the eye, in the room's own floor tiles. Drills
//! played on it ([`played_on`]) have [bots](super::bot) for targets and are shot with the
//! [tap rifle](super::rifle).
//!
//! The scenery hangs under one root ([`Scenery`]). Where the player can move
//! ([`Environment::Pillar`]) the root is also an [`EyeAnchor`]: replays keep the eye at the
//! origin and store everything relative to it, so the recorder notes where the scenery is as
//! seen from the eye and the replay moves the scenery there ([`follow_replay_eye`]).

use bevy::prelude::*;

use super::DrillKind;
use super::bot::EYE_HEIGHT;
use super::def::Environment;
use super::kinds::counter_strafe;
use super::targets::TargetAssets;
use crate::replay::player::{Playback, advance_playback};
use crate::settings::{SceneTheme, Settings};
use crate::{AppState, level};

/// Side of the deck, metres: as large as the open themes' floor, which it covers to the horizon.
const DECK_SIZE: f32 = 400.0;

/// The root of a range's scenery.
#[derive(Component)]
pub(crate) struct Scenery;

/// Scenery the eye moves past. The replay recorder stores its position relative to the eye in
/// every frame, as a target of radius 0 (nothing to see, and no change to the replay format).
#[derive(Component)]
pub(crate) struct EyeAnchor;

#[derive(Resource)]
struct RangeAssets {
    deck_mesh: Handle<Mesh>,
    pillar_mesh: Handle<Mesh>,
    /// The deck's material in the theme it was last built for.
    deck: Option<(SceneTheme, Handle<StandardMaterial>)>,
}

/// Whether the drill is played on the range: with bots, and the tap rifle.
pub(crate) fn played_on(kind: DrillKind) -> bool {
    matches!(kind.def().environment, Environment::Range | Environment::Pillar)
}

/// Whether the player strafes in the drill (see [`movement`](super::movement)).
pub(crate) fn player_moves(kind: DrillKind) -> bool {
    kind.def().environment == Environment::Pillar
}

pub(super) fn register(app: &mut App) {
    app.add_systems(Startup, create_range_assets)
        .add_systems(Update, follow_replay_eye.after(advance_playback).run_if(in_state(AppState::Replay)));
}

fn create_range_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(RangeAssets {
        deck_mesh: meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(DECK_SIZE * 0.5))),
        pillar_mesh: meshes.add(Cuboid::from_size(counter_strafe::PILLAR_MAX - counter_strafe::PILLAR_MIN)),
        deck: None,
    });
}

/// Spawns the range for as long as `state` lasts: the deck and, for [`Environment::Pillar`], the
/// Counter-strafe drill's cover on it.
pub(crate) fn spawn(commands: &mut Commands, environment: Environment, state: AppState) {
    // Queued: the deck's look depends on the scene theme, which a spawn hook has no access to.
    commands.queue(move |world: &mut World| {
        let material = deck_material(world);
        let assets = world.resource::<RangeAssets>();
        let (deck_mesh, pillar_mesh) = (assets.deck_mesh.clone(), assets.pillar_mesh.clone());
        let cover = world.resource::<TargetAssets>().cover_material.clone();
        let root = world.spawn((DespawnOnExit(state), Scenery, Transform::IDENTITY, Visibility::Inherited)).id();
        world.spawn((
            ChildOf(root),
            Mesh3d(deck_mesh),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, -EYE_HEIGHT, 0.0),
        ));
        if environment == Environment::Pillar {
            world.entity_mut(root).insert(EyeAnchor);
            world.spawn((
                ChildOf(root),
                Mesh3d(pillar_mesh),
                MeshMaterial3d(cover),
                // Primitive meshes are centered on the origin; move it into place.
                Transform::from_translation((counter_strafe::PILLAR_MIN + counter_strafe::PILLAR_MAX) * 0.5),
            ));
        }
    });
}

/// The deck's material in the current scene theme, rebuilt only when the theme has changed.
fn deck_material(world: &mut World) -> Handle<StandardMaterial> {
    let theme = world.resource::<Settings>().video.scene;
    if let Some((built, material)) = &world.resource::<RangeAssets>().deck
        && *built == theme
    {
        return material.clone();
    }
    let material = level::floor_material(theme, Vec2::splat(DECK_SIZE), &mut world.resource_mut::<Assets<Image>>());
    let material = world.resource_mut::<Assets<StandardMaterial>>().add(material);
    world.resource_mut::<RangeAssets>().deck = Some((theme, material.clone()));
    material
}

/// Where the scenery is as seen from the eye in a replay frame's targets: the recorded
/// [`EyeAnchor`], or the origin for drills in which the eye never moves.
fn anchor(targets: &[(Vec3, f32)]) -> Vec3 {
    targets.iter().find(|target| target.1 == 0.0).map_or(Vec3::ZERO, |target| target.0)
}

/// Replays look from the origin: instead of the eye, the scenery moves, to where the eye saw it.
fn follow_replay_eye(playback: Res<Playback>, mut scenery: Query<&mut Transform, With<Scenery>>) {
    let frames = &playback.replay.frames;
    let Some(frame) = frames.get(frames.partition_point(|frame| frame.t <= playback.time).saturating_sub(1)) else {
        return;
    };
    let at = anchor(&frame.targets);
    for mut root in &mut scenery {
        if root.translation != at {
            root.translation = at;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The anchor is told from the bots' heads by its radius of 0, wherever it is in the list.
    #[test]
    fn the_replay_scenery_follows_the_recorded_anchor() {
        let head = (Vec3::new(-1.0, 0.0, -12.0), 0.14);
        let recorded = Vec3::new(-2.0, 0.0, 0.0);
        assert_eq!(anchor(&[head, (recorded, 0.0)]), recorded);
        assert_eq!(anchor(&[(recorded, 0.0), head]), recorded);
        assert_eq!(anchor(&[head]), Vec3::ZERO);
        assert_eq!(anchor(&[]), Vec3::ZERO);
    }
}
