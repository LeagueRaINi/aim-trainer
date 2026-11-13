//! The muzzle flash: a small star of light across the barrel with a tongue along it, drawn
//! additively and unlit for the few frames after a shot. One mesh and material for every gun.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// Reach of the star's long points across the barrel, and of the tongue ahead of the muzzle (m),
/// at full size.
const STAR: f32 = 0.05;
const TONGUE: f32 = 0.085;
/// Points of the star.
const POINTS: usize = 5;

/// Hot white at the core, amber at the rim. Blending is additive, so the alpha is the brightness
/// and the rim fades into nothing.
const CORE: [f32; 4] = [1.0, 0.95, 0.82, 0.9];
const GLOW: [f32; 4] = [1.0, 0.72, 0.36, 0.45];
const RIM: [f32; 4] = [1.0, 0.5, 0.2, 0.0];

/// The flash at full size with the muzzle at its origin, the barrel along -Z.
pub(super) fn flash() -> Mesh {
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    // A fan: bright in the middle, dark along the outline.
    let mut fan = |center: Vec3, outline: &[(Vec3, [f32; 4])]| {
        let base = positions.len() as u32;
        positions.push(center);
        colors.push(CORE);
        for &(point, color) in outline {
            positions.push(point);
            colors.push(color);
        }
        let count = outline.len() as u32;
        indices.extend((0..count).flat_map(|i| [base, base + 1 + i, base + 1 + (i + 1) % count]));
    };
    // The star, just ahead of the muzzle: long points alternate with short ones.
    let star: Vec<(Vec3, [f32; 4])> = (0..POINTS * 2)
        .map(|i| {
            let (sin, cos) = (i as f32 / (POINTS * 2) as f32 * TAU).sin_cos();
            let (reach, color) = if i % 2 == 0 { (STAR, RIM) } else { (STAR * 0.3, GLOW) };
            (Vec3::new(cos * reach, sin * reach, -0.004), color)
        })
        .collect();
    fan(Vec3::new(0.0, 0.0, -0.004), &star);
    // The tongue: two crossed blades along the barrel.
    for across in [Vec3::X, Vec3::Y] {
        let blade = [
            (Vec3::ZERO, GLOW),
            (across * 0.011 - Vec3::Z * TONGUE * 0.3, RIM),
            (-Vec3::Z * TONGUE, RIM),
            (-across * 0.011 - Vec3::Z * TONGUE * 0.3, RIM),
        ];
        fan(-Vec3::Z * TONGUE * 0.25, &blade);
    }
    let normals = vec![Vec3::Z; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
}

/// Light added to the picture, seen from either side.
pub(super) fn material() -> StandardMaterial {
    StandardMaterial { unlit: true, alpha_mode: AlphaMode::Add, cull_mode: None, ..default() }
}
