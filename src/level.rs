//! The training room: a lit box with subtly tiled surfaces, so turning has a visual reference.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    math::Affine2,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

pub struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::BLACK))
            .insert_resource(GlobalAmbientLight {
                color: Color::WHITE,
                brightness: 1500.0,
                ..default()
            })
            .add_systems(Startup, spawn_room);
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

fn spawn_room(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let wall_tex = images.add(tile_texture([44, 47, 56], [62, 66, 78]));
    let floor_tex = images.add(tile_texture([30, 32, 38], [42, 45, 53]));

    let width = 2.0 * SIDE_X;
    let height = CEILING_Y - FLOOR_Y;
    let depth = FRONT_Z - BACK_Z;
    let mid_y = (FLOOR_Y + CEILING_Y) * 0.5;
    let mid_z = (FRONT_Z + BACK_Z) * 0.5;

    // (normal, extents along the plane's local X/Z, center, texture). Plane3d rotates +Y onto the
    // normal, so on the side walls local X ends up vertical.
    let surfaces = [
        (Vec3::Y, Vec2::new(width, depth), Vec3::new(0.0, FLOOR_Y, mid_z), &floor_tex),
        (Vec3::NEG_Y, Vec2::new(width, depth), Vec3::new(0.0, CEILING_Y, mid_z), &wall_tex),
        (Vec3::Z, Vec2::new(width, height), Vec3::new(0.0, mid_y, BACK_Z), &wall_tex),
        (Vec3::NEG_Z, Vec2::new(width, height), Vec3::new(0.0, mid_y, FRONT_Z), &wall_tex),
        (Vec3::X, Vec2::new(height, depth), Vec3::new(-SIDE_X, mid_y, mid_z), &wall_tex),
        (Vec3::NEG_X, Vec2::new(height, depth), Vec3::new(SIDE_X, mid_y, mid_z), &wall_tex),
    ];
    for (normal, size, center, texture) in surfaces {
        commands.spawn((
            Mesh3d(meshes.add(Plane3d::new(normal, size * 0.5))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(texture.clone()),
                uv_transform: Affine2::from_scale(size / TILE),
                perceptual_roughness: 0.9,
                reflectance: 0.1,
                ..default()
            })),
            Transform::from_translation(center),
        ));
    }

    // Key light from behind-above the player, plus a dim fill from the opposite side so no
    // surface (ceiling, side walls) drops to black.
    commands.spawn((
        DirectionalLight { illuminance: 2000.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(4.0, 10.0, 6.0).looking_at(Vec3::new(0.0, 0.0, -10.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 600.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(-6.0, -3.0, -10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
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
