//! The training room: a lit box with subtly tiled surfaces, so turning has a visual reference.
//! Its look follows the chosen [`SceneTheme`]; open themes drop the walls and ceiling for a
//! wide floor under a sky.

use std::f32::consts::FRAC_PI_2;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::cluster::GlobalClusterSettings;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::settings::{SceneTheme, Settings};

pub struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::BLACK))
            .insert_resource(GlobalAmbientLight { color: Color::WHITE, brightness: 1500.0, ..default() })
            .add_systems(Startup, spawn_lights)
            .add_systems(Update, build_room.run_if(resource_changed::<Settings>));
    }
}

// Room bounds relative to the eye at the origin.
const FLOOR_Y: f32 = -2.5;
const CEILING_Y: f32 = 7.5;
const BACK_Z: f32 = -16.0;
const FRONT_Z: f32 = 8.0;
const SIDE_X: f32 = 20.0;
/// World size of one texture tile.
const TILE: f32 = 2.0;
/// Side of the floor of the open themes, reaching close to the horizon.
const OPEN_FLOOR: f32 = 400.0;
/// Render layer of objects only the settings preview camera shows (its targets); the room's
/// lights reach it too.
pub const PREVIEW_LAYER: usize = 1;

/// A surface of the current room, replaced when the theme changes.
#[derive(Component)]
struct RoomPart;

/// Tile colors (fill, line) and backdrop of a theme.
struct ThemeLook {
    /// `None`: no walls or ceiling.
    walls: Option<([u8; 3], [u8; 3])>,
    floor: ([u8; 3], [u8; 3]),
    /// Background where nothing is drawn, and the color distant floor fades into on open themes.
    sky: Color,
    /// A sky dome shading from the first color overhead to the second at the horizon.
    dome: Option<([u8; 3], [u8; 3])>,
}

fn look(theme: SceneTheme) -> ThemeLook {
    match theme {
        SceneTheme::Slate => ThemeLook {
            walls: Some(([44, 47, 56], [62, 66, 78])),
            floor: ([30, 32, 38], [42, 45, 53]),
            sky: Color::BLACK,
            dome: None,
        },
        SceneTheme::Midnight => ThemeLook {
            walls: Some(([12, 13, 17], [22, 24, 30])),
            floor: ([9, 10, 13], [18, 20, 25]),
            sky: Color::BLACK,
            dome: None,
        },
        SceneTheme::Light => ThemeLook {
            walls: Some(([150, 154, 162], [128, 132, 140])),
            floor: ([112, 116, 124], [94, 98, 106]),
            sky: Color::srgb(0.75, 0.77, 0.8),
            dome: None,
        },
        SceneTheme::Void => {
            ThemeLook { walls: None, floor: ([14, 16, 22], [40, 46, 62]), sky: Color::srgb_u8(4, 5, 8), dome: None }
        },
        SceneTheme::Sky => ThemeLook {
            walls: None,
            floor: ([128, 126, 118], [142, 140, 131]),
            sky: Color::srgb_u8(214, 228, 240),
            dome: Some(([40, 96, 186], [214, 228, 240])),
        },
    }
}

/// Floor distance (m) over which open themes fade into the sky: well past every target, so
/// targets keep their color, and done before the floor ends.
const HORIZON_FADE: (f32, f32) = (40.0, 180.0);
/// Radius of the sky dome: outside the open floor's corners.
const DOME_RADIUS: f32 = 380.0;

/// Key light from behind-above the player, plus a dim fill from the opposite side so no surface
/// (ceiling, side walls) drops to black.
///
/// Directional and ambient light only: nothing to cluster, so the cameras opt out of light
/// clustering (`ClusterConfig::None`), which saves a compute pass and its upload every frame.
/// Bevy's GPU clustering can't handle views without clusters, so clustering stays on the CPU,
/// where it then does no work.
fn spawn_lights(mut commands: Commands, mut clustering: ResMut<GlobalClusterSettings>) {
    clustering.gpu_clustering = None;
    let layers = RenderLayers::layer(0).with(PREVIEW_LAYER);
    commands.spawn((
        DirectionalLight { illuminance: 2000.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(4.0, 10.0, 6.0).looking_at(Vec3::new(0.0, 0.0, -10.0), Vec3::Y),
        layers.clone(),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 600.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(-6.0, -3.0, -10.0).looking_at(Vec3::ZERO, Vec3::Y),
        layers,
    ));
}

/// (Re)builds the room when the theme changes (and at startup).
#[allow(clippy::too_many_arguments)]
fn build_room(
    mut commands: Commands,
    settings: Res<Settings>,
    mut built: Local<Option<SceneTheme>>,
    parts: Query<Entity, With<RoomPart>>,
    cameras: Query<Entity, With<Camera3d>>,
    mut clear: ResMut<ClearColor>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let theme = settings.video.scene;
    if *built == Some(theme) {
        return;
    }
    *built = Some(theme);
    for part in &parts {
        commands.entity(part).despawn();
    }
    let look = look(theme);
    clear.0 = look.sky;
    // Open themes fade the floor into the sky towards the horizon, so it has no hard edge.
    for camera in &cameras {
        if look.walls.is_none() {
            let falloff = FogFalloff::Linear { start: HORIZON_FADE.0, end: HORIZON_FADE.1 };
            commands.entity(camera).insert(DistanceFog { color: look.sky, falloff, ..default() });
        } else {
            commands.entity(camera).remove::<DistanceFog>();
        }
    }
    if let Some((zenith, horizon)) = look.dome {
        commands.spawn((
            RoomPart,
            Mesh3d(meshes.add(Sphere::new(DOME_RADIUS).mesh().uv(48, 24))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(images.add(sky_texture(zenith, horizon))),
                unlit: true,
                fog_enabled: false,
                // Seen from inside.
                cull_mode: None,
                ..default()
            })),
            // The UV sphere's texture rows start at its +Z pole; turn that pole overhead.
            Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
            RenderLayers::layer(0).with(PREVIEW_LAYER),
        ));
    }

    let floor_tex = images.add(tile_texture(look.floor.0, look.floor.1));
    let width = 2.0 * SIDE_X;
    let height = CEILING_Y - FLOOR_Y;
    let depth = FRONT_Z - BACK_Z;
    let mid_y = (FLOOR_Y + CEILING_Y) * 0.5;
    let mid_z = (FRONT_Z + BACK_Z) * 0.5;

    // (normal, extents along the plane's local X/Z, center, texture). Plane3d rotates +Y onto the
    // normal, so on the side walls local X ends up vertical.
    let mut surfaces = Vec::with_capacity(6);
    match look.walls {
        Some((fill, line)) => {
            let wall_tex = images.add(tile_texture(fill, line));
            surfaces.extend([
                (Vec3::Y, Vec2::new(width, depth), Vec3::new(0.0, FLOOR_Y, mid_z), floor_tex),
                (Vec3::NEG_Y, Vec2::new(width, depth), Vec3::new(0.0, CEILING_Y, mid_z), wall_tex.clone()),
                (Vec3::Z, Vec2::new(width, height), Vec3::new(0.0, mid_y, BACK_Z), wall_tex.clone()),
                (Vec3::NEG_Z, Vec2::new(width, height), Vec3::new(0.0, mid_y, FRONT_Z), wall_tex.clone()),
                (Vec3::X, Vec2::new(height, depth), Vec3::new(-SIDE_X, mid_y, mid_z), wall_tex.clone()),
                (Vec3::NEG_X, Vec2::new(height, depth), Vec3::new(SIDE_X, mid_y, mid_z), wall_tex),
            ]);
        },
        None => surfaces.push((Vec3::Y, Vec2::splat(OPEN_FLOOR), Vec3::new(0.0, FLOOR_Y, 0.0), floor_tex)),
    }
    for (normal, size, center, texture) in surfaces {
        commands.spawn((
            RoomPart,
            Mesh3d(meshes.add(Plane3d::new(normal, size * 0.5))),
            MeshMaterial3d(materials.add(tiled(texture, size))),
            Transform::from_translation(center),
        ));
    }
}

/// A matte surface of `size` metres covered in `texture`'s tiles.
fn tiled(texture: Handle<Image>, size: Vec2) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(texture),
        uv_transform: Affine2::from_scale(size / TILE),
        perceptual_roughness: 0.9,
        reflectance: 0.1,
        ..default()
    }
}

/// The material of `theme`'s floor for a surface of `size` metres: for scenery that lays a floor
/// of its own into the room (the bot drills' deck) and should look like the room's.
pub(crate) fn floor_material(theme: SceneTheme, size: Vec2, images: &mut Assets<Image>) -> StandardMaterial {
    let (fill, line) = look(theme).floor;
    tiled(images.add(tile_texture(fill, line)), size)
}

/// The sky dome's colors by texture row (pole to pole): `zenith` overhead, easing into
/// `horizon` towards the horizon (the middle row) and below.
fn sky_texture(zenith: [u8; 3], horizon: [u8; 3]) -> Image {
    const ROWS: u32 = 256;
    let mut pixels = Vec::with_capacity((ROWS * 4) as usize);
    for row in 0..ROWS {
        // 0 overhead, 1 at the horizon and below.
        let down = ((row as f32 + 0.5) / ROWS as f32 * 2.0).min(1.0);
        let t = down.powf(2.2);
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        pixels.extend_from_slice(&[
            mix(zenith[0], horizon[0]),
            mix(zenith[1], horizon[1]),
            mix(zenith[2], horizon[2]),
            255,
        ]);
    }
    let mut image = Image::new(
        Extent3d { width: 1, height: ROWS, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

/// A square tile with a soft border, mip-mapped so the lines don't shimmer at a distance.
fn tile_texture(fill: [u8; 3], line: [u8; 3]) -> Image {
    const SIZE: u32 = 256;
    const LINE: u32 = 4;
    let mut base = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let edge = x.min(SIZE - 1 - x).min(y).min(SIZE - 1 - y);
            let rgb = if edge < LINE / 2 { line } else { fill };
            base.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }

    let mut image = Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        base,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );

    // Append the mip chain after level 0.
    let data = image.data.as_mut().expect("Image::new stores the pixel data");
    let (mut start, mut size, mut mip_count) = (0, SIZE, 1);
    while size > 1 {
        let level = downsample(&data[start..], size);
        start = data.len();
        data.extend_from_slice(&level);
        size /= 2;
        mip_count += 1;
    }
    image.texture_descriptor.mip_level_count = mip_count;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    image
}

/// 2x2 box filter of an RGBA8 square image.
fn downsample(pixels: &[u8], size: u32) -> Vec<u8> {
    let half = size / 2;
    let mut out = Vec::with_capacity((half * half * 4) as usize);
    for y in 0..half {
        for x in 0..half {
            for channel in 0..4 {
                let at = |dx: u32, dy: u32| pixels[(((2 * y + dy) * size + 2 * x + dx) * 4 + channel) as usize] as u32;
                out.push(((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8);
            }
        }
    }
    out
}
