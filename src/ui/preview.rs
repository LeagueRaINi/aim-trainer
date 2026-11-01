//! Live preview of the scene settings on the Video page: a second camera renders the room (in
//! its current theme) with a few targets (in the current color and style) into an image the
//! page shows, so a choice is visible as soon as it is clicked. The camera only renders while
//! the page is open.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::light::cluster::ClusterConfig;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::menu::MenuTab;
use crate::AppState;
use crate::drill::{FacesEye, TargetAssets, facing_eye};
use crate::level::PREVIEW_LAYER;

/// Size of the preview image, pixels (16:9).
const SIZE: UVec2 = UVec2::new(640, 360);
/// Narrower than any in-game view, so the targets are big enough to see their style.
const PREVIEW_VFOV_DEG: f32 = 32.0;
/// Where the preview's targets sit (yaw, pitch in degrees, 10 m out), as on a Gridshot grid.
const TARGETS: [(f32, f32); 3] = [(-8.0, 3.0), (0.0, -3.0), (8.0, 2.0)];
/// Radius of the preview's targets (Gridshot's at size 1), metres.
const TARGET_RADIUS: f32 = 0.3;

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_preview).add_systems(
            Update,
            (
                // Startup has created the target assets by the first update.
                spawn_preview_targets.run_if(run_once),
                show_preview,
                activate_preview_camera,
            ),
        );
    }
}

/// The image the preview camera renders into.
#[derive(Resource)]
struct ScenePreview(Handle<Image>);

#[derive(Component)]
struct PreviewCamera;

/// A UI node that shows the preview image.
#[derive(Component, Clone, Copy, Default)]
pub struct PreviewView;

fn setup_preview(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = images.add(Image::new_target_texture(
        SIZE.x,
        SIZE.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));
    commands.spawn((
        PreviewCamera,
        Camera3d::default(),
        // Before the player camera, and off until the Video page is open.
        Camera { order: -1, is_active: false, ..default() },
        RenderTarget::Image(image.clone().into()),
        Projection::from(PerspectiveProjection { fov: PREVIEW_VFOV_DEG.to_radians(), ..default() }),
        Transform::IDENTITY,
        RenderLayers::layer(0).with(PREVIEW_LAYER),
        // Lit like the player camera (see `level::spawn_lights`).
        ClusterConfig::None,
    ));
    commands.insert_resource(ScenePreview(image));
}

/// The preview's targets, sharing the game's target mesh and material so they follow the
/// settings like real ones.
fn spawn_preview_targets(mut commands: Commands, assets: Res<TargetAssets>) {
    for (yaw, pitch) in TARGETS {
        let position = Quat::from_euler(EulerRot::YXZ, yaw.to_radians(), pitch.to_radians(), 0.0) * Vec3::NEG_Z * 10.0;
        commands.spawn((
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.idle.clone()),
            Transform::from_translation(position)
                .with_rotation(facing_eye(position))
                .with_scale(Vec3::splat(TARGET_RADIUS)),
            FacesEye,
            RenderLayers::layer(PREVIEW_LAYER),
        ));
    }
}

/// Puts the preview image into newly built preview nodes.
fn show_preview(mut commands: Commands, preview: Res<ScenePreview>, views: Query<Entity, Added<PreviewView>>) {
    for view in &views {
        commands.entity(view).insert(ImageNode::new(preview.0.clone()));
    }
}

/// Renders the preview only while it can be seen.
fn activate_preview_camera(
    state: Res<State<AppState>>,
    tab: Res<MenuTab>,
    mut camera: Single<&mut Camera, With<PreviewCamera>>,
) {
    let visible = *state.get() == AppState::Menu && *tab == MenuTab::Video;
    if camera.is_active != visible {
        camera.is_active = visible;
    }
}
