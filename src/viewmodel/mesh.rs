//! The builder the gun models are made with ([`Sculpt`]): closed, flat-shaded shells from three
//! tools (a side profile given thickness, sections skinned along the gun, a profile turned around
//! an axis), every face in one of a few [`Finish`]es. Everything a gun is made of ends up in one
//! mesh with the finishes as vertex colors, so it is drawn in one call with one material.
//!
//! Models are written in metres forward (towards the muzzle), up and right of the gun's origin
//! (see [`at`]); the mesh has Bevy's axes, -Z forward.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// The point `forward`, `up` and `right` of a gun's origin, in the mesh's axes.
pub(super) const fn at(forward: f32, up: f32, right: f32) -> Vec3 {
    Vec3::new(right, up, -forward)
}

/// What a face is made of. The tones follow the app's palette: blue-black, steel, one cyan
/// accent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Finish {
    /// Blue-black polymer: frames, grips, handguards, stocks.
    Polymer,
    /// Gunmetal: slides, receivers, barrels.
    Metal,
    /// Bright worn steel: small controls and pins.
    Steel,
    /// Near black: rubber, sights, and plates that stand for openings (ports, slots, bores).
    Shadow,
    /// The cyan of the UI: thin stripes and sight dots.
    Accent,
    /// Glove fabric.
    Glove,
    /// The glove's harder parts: the plates over the knuckles and the cuff.
    Pad,
    /// The sleeve on the forearm.
    Sleeve,
}

impl Finish {
    fn color(self) -> LinearRgba {
        let srgb = |hex: u32| Color::srgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8).to_linear();
        match self {
            Finish::Polymer => srgb(0x2D323D),
            Finish::Metal => srgb(0x565E6C),
            Finish::Steel => srgb(0x8A93A3),
            Finish::Shadow => srgb(0x101216),
            Finish::Accent => srgb(0x4CC9F0),
            Finish::Glove => srgb(0x4C515C),
            Finish::Pad => srgb(0x70757F),
            Finish::Sleeve => srgb(0x2A2F39),
        }
    }
}

/// How much of a finish's color a face keeps, by where it faces on the gun. The room's key light
/// comes from behind the player, so it lights the tops and backs of the gun and leaves the side
/// turned to the player to the ambient light: sides a little darker than tops, undersides
/// darker still and backs toned down keep the shapes apart whichever way the player looks, and
/// make every bevel a highlight or a shadow line.
fn shade(normal: Vec3) -> f32 {
    0.9 + 0.2 * normal.y - 0.2 * normal.z.max(0.0)
}

/// A mesh being built.
pub(super) struct Sculpt {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    /// How much of their finish's color the faces being added keep (see `dimmed`).
    light: f32,
}

impl Sculpt {
    pub(super) fn new() -> Self {
        Self { positions: Vec::new(), normals: Vec::new(), colors: Vec::new(), indices: Vec::new(), light: 1.0 }
    }

    /// The mirror image of the mesh: the gun for the other hand. (A negative scale on the entity
    /// would turn its faces inside out for the renderer.)
    pub(super) fn mirrored(&self) -> Self {
        let mirror = Vec3::new(-1.0, 1.0, 1.0);
        Self {
            positions: self.positions.iter().map(|&position| position * mirror).collect(),
            normals: self.normals.iter().map(|&normal| normal * mirror).collect(),
            colors: self.colors.clone(),
            // Mirrored triangles run the other way round.
            indices: (self.indices.as_chunks::<3>().0.iter()).flat_map(|&[a, b, c]| [a, c, b]).collect(),
            light: self.light,
        }
    }

    pub(super) fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
            .with_inserted_indices(Indices::U32(self.indices))
    }

    /// A flat face with its own vertices (so its edges stay sharp): `corners` and the
    /// `triangles` indexing them run counter-clockwise seen from outside, or all `clockwise`.
    fn polygon(
        &mut self,
        finish: Finish,
        corners: &[Vec3],
        clockwise: bool,
        triangles: impl Iterator<Item = [usize; 3]>,
    ) {
        let base = self.positions.len();
        self.positions.extend_from_slice(corners);
        let placed = &self.positions[base..];
        // Newell's method: exact for flat faces, the average plane of a slightly bent one.
        let area: Vec3 = (0..placed.len()).map(|i| placed[i].cross(placed[(i + 1) % placed.len()])).sum();
        let normal = (if clockwise { -area } else { area }).normalize_or_zero();
        let before = self.indices.len();
        for [a, b, c] in triangles {
            // Corners that coincide (a section that closes to an edge or a point) leave slivers.
            if (placed[b] - placed[a]).cross(placed[c] - placed[a]).length_squared() < 1e-18 {
                continue;
            }
            let (b, c) = if clockwise { (c, b) } else { (b, c) };
            self.indices.extend([a, b, c].map(|corner| (base + corner) as u32));
        }
        if self.indices.len() == before {
            self.positions.truncate(base);
            return;
        }
        let color = (finish.color() * (shade(normal) * self.light)).with_alpha(1.0).to_f32_array();
        self.normals.extend(std::iter::repeat_n(normal, corners.len()));
        self.colors.extend(std::iter::repeat_n(color, corners.len()));
    }

    /// Adds what `parts` builds keeping only `light` of its finishes' colors: shading that is
    /// worked out elsewhere (the hands').
    pub(super) fn dimmed(&mut self, light: f32, parts: impl FnOnce(&mut Self)) {
        self.light = light;
        parts(self);
        self.light = 1.0;
    }

    /// A flat convex face, its corners counter-clockwise seen from outside.
    pub(super) fn face(&mut self, finish: Finish, corners: &[Vec3]) {
        self.polygon(finish, corners, false, (1..corners.len() - 1).map(|i| [0, i, i + 1]));
    }

    /// A side profile (`[forward, up]` corners, in either direction) given thickness:
    /// `half_width` to each side of `right`, with the edges around both side faces cut off by
    /// `bevel`. The profile may be concave; `bevel` must be smaller than its thinnest feature.
    pub(super) fn extrude(&mut self, finish: Finish, profile: &[[f32; 2]], right: f32, half_width: f32, bevel: f32) {
        let mut outline: Vec<Vec2> = profile.iter().map(|&corner| corner.into()).collect();
        if signed_area(&outline) < 0.0 {
            outline.reverse();
        }
        let count = outline.len();
        let face_outline = if bevel > 0.0 { inset(&outline, bevel) } else { outline.clone() };
        debug_assert!(
            (0..count).all(|i| {
                let j = (i + 1) % count;
                (face_outline[j] - face_outline[i]).dot(outline[j] - outline[i]) > 0.0
            }),
            "bevel {bevel} is too large for the profile {profile:?}"
        );
        let triangles = triangulate(&face_outline);
        let wall = half_width - bevel;
        for side in [1.0, -1.0] {
            let corners: Vec<Vec3> = face_outline.iter().map(|p| at(p.x, p.y, right + side * half_width)).collect();
            // The profile is counter-clockwise seen from the right.
            self.polygon(finish, &corners, side < 0.0, triangles.iter().copied());
            if bevel > 0.0 {
                for i in 0..count {
                    let j = (i + 1) % count;
                    let (a, b) = (outline[i], outline[j]);
                    let (inner_a, inner_b) = (face_outline[i], face_outline[j]);
                    let quad = [
                        at(a.x, a.y, right + side * wall),
                        at(b.x, b.y, right + side * wall),
                        at(inner_b.x, inner_b.y, right + side * half_width),
                        at(inner_a.x, inner_a.y, right + side * half_width),
                    ];
                    self.face(finish, &if side > 0.0 { quad } else { [quad[1], quad[0], quad[3], quad[2]] });
                }
            }
        }
        for i in 0..count {
            let (a, b) = (outline[i], outline[(i + 1) % count]);
            self.face(
                finish,
                &[
                    at(a.x, a.y, right - wall),
                    at(b.x, b.y, right - wall),
                    at(b.x, b.y, right + wall),
                    at(a.x, a.y, right + wall),
                ],
            );
        }
    }

    /// Sections across the gun skinned into one body: `rings` from the rear one to the front
    /// one, each counter-clockwise as the player sees it (from behind), all with their corners in
    /// the same order. The first and last are closed with flat ends.
    pub(super) fn loft<const N: usize>(&mut self, finish: Finish, rings: &[[Vec3; N]]) {
        let (Some(rear), Some(front)) = (rings.first(), rings.last()) else { return };
        // Seen from behind, right and up are the mesh's X and Y.
        let outline = |ring: &[Vec3; N]| ring.map(|corner| corner.truncate());
        self.polygon(finish, rear, false, triangulate(&outline(rear)).into_iter());
        self.polygon(finish, front, true, triangulate(&outline(front)).into_iter());
        for pair in rings.windows(2) {
            for i in 0..N {
                let j = (i + 1) % N;
                self.face(finish, &[pair[0][j], pair[0][i], pair[1][i], pair[1][j]]);
            }
        }
    }

    /// A turned part: `profile` (`[forward, radius]` pairs, from its rear end to its front end;
    /// stepping back inwards makes a bore) swept around the axis that runs forward through
    /// `axis`, as `sides` flat facets. Both ends are closed.
    pub(super) fn lathe(&mut self, finish: Finish, axis: Vec3, profile: &[[f32; 2]], sides: usize) {
        let (Some(&[rear, _]), Some(&[front, _])) = (profile.first(), profile.last()) else { return };
        let spoke = |i: usize| {
            let (sin, cos) = (i as f32 / sides as f32 * std::f32::consts::TAU).sin_cos();
            Vec3::new(cos, sin, 0.0)
        };
        let point = |[forward, radius]: [f32; 2], i: usize| axis + at(forward, 0.0, 0.0) + spoke(i % sides) * radius;
        let closed = std::iter::once([rear, 0.0]).chain(profile.iter().copied()).chain(std::iter::once([front, 0.0]));
        let closed: Vec<[f32; 2]> = closed.collect();
        for pair in closed.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            if from[0] == to[0] && from[1].min(to[1]) == 0.0 {
                // A flat end: one disc, facing backwards if the profile grows out of the axis.
                let grows = to[1] > from[1];
                let rim = if grows { to } else { from };
                let disc: Vec<Vec3> = (0..sides).map(|i| point(rim, if grows { i } else { sides - i })).collect();
                self.face(finish, &disc);
                continue;
            }
            for i in 0..sides {
                self.face(finish, &[point(from, i), point(to, i), point(to, i + 1), point(from, i + 1)]);
            }
        }
    }
}

/// A section across the gun at `forward`: a rectangle `half_width` to each side of `right`, from
/// `bottom` to `top`, with its upper corners cut by `shoulder` and its lower ones by `chin`.
/// Corners run counter-clockwise seen from behind, starting at the bottom right.
pub(super) fn section(
    forward: f32,
    right: f32,
    half_width: f32,
    [bottom, top]: [f32; 2],
    [chin, shoulder]: [f32; 2],
) -> [Vec3; 8] {
    [
        at(forward, bottom, right + half_width - chin),
        at(forward, bottom + chin, right + half_width),
        at(forward, top - shoulder, right + half_width),
        at(forward, top, right + half_width - shoulder),
        at(forward, top, right - half_width + shoulder),
        at(forward, top - shoulder, right - half_width),
        at(forward, bottom + chin, right - half_width),
        at(forward, bottom, right - half_width + chin),
    ]
}

/// A rectangle as a side profile.
pub(super) fn rect([back, front]: [f32; 2], [bottom, top]: [f32; 2]) -> [[f32; 2]; 4] {
    [[back, bottom], [front, bottom], [front, top], [back, top]]
}

/// `ring` leant over: every corner moved forward by `slope` for each metre it is above `level`.
pub(super) fn raked<const N: usize>(ring: [Vec3; N], slope: f32, level: f32) -> [Vec3; N] {
    ring.map(|corner| corner + at(slope * (corner.y - level), 0.0, 0.0))
}

/// The outline of `path` drawn `thickness` wide with square ends: the profile of a bent bar
/// such as a trigger guard.
pub(super) fn stroke(path: &[[f32; 2]], thickness: f32) -> Vec<[f32; 2]> {
    let points: Vec<Vec2> = path.iter().map(|&point| point.into()).collect();
    let last = points.len() - 1;
    let left_of = |from: Vec2, to: Vec2| (to - from).normalize().perp();
    let offset = |i: usize| {
        let before = left_of(points[i.saturating_sub(1)], points[i.max(1)]);
        let after = left_of(points[i.min(last - 1)], points[(i + 1).min(last)]);
        // A mitre: the corner both offset edges pass through.
        (before + after) / (1.0 + before.dot(after)) * (thickness * 0.5)
    };
    let left = (0..=last).map(|i| points[i] + offset(i));
    let right = (0..=last).rev().map(|i| points[i] - offset(i));
    left.chain(right).map(|point| point.into()).collect()
}

/// Twice the area of a polygon, positive if it runs counter-clockwise.
fn signed_area(outline: &[Vec2]) -> f32 {
    (0..outline.len()).map(|i| outline[i].perp_dot(outline[(i + 1) % outline.len()])).sum()
}

/// A counter-clockwise outline with every edge moved inwards by `distance` (mitred corners).
fn inset(outline: &[Vec2], distance: f32) -> Vec<Vec2> {
    let count = outline.len();
    let inward = |i: usize| (outline[(i + 1) % count] - outline[i]).normalize().perp();
    (0..count)
        .map(|i| {
            let (before, after) = (inward((i + count - 1) % count), inward(i));
            outline[i] + (before + after) / (1.0 + before.dot(after)) * distance
        })
        .collect()
}

/// Splits a counter-clockwise simple polygon into triangles of its own corners (ear clipping).
fn triangulate(outline: &[Vec2]) -> Vec<[usize; 3]> {
    let mut left: Vec<usize> = (0..outline.len()).collect();
    let mut triangles = Vec::with_capacity(outline.len().saturating_sub(2));
    while left.len() > 3 {
        let count = left.len();
        let is_ear = |at: usize| {
            let [a, b, c] = [left[(at + count - 1) % count], left[at], left[(at + 1) % count]];
            let (pa, pb, pc) = (outline[a], outline[b], outline[c]);
            // A corner doubled by a zero bevel: cutting it off changes nothing.
            if pa == pb || pb == pc {
                return true;
            }
            // Convex here, and no other corner inside the triangle it would cut off.
            (pb - pa).perp_dot(pc - pb) > 0.0
                && left.iter().all(|&other| {
                    let p = outline[other];
                    p == pa
                        || p == pb
                        || p == pc
                        || (pb - pa).perp_dot(p - pa) < 0.0
                        || (pc - pb).perp_dot(p - pb) < 0.0
                        || (pa - pc).perp_dot(p - pc) < 0.0
                })
        };
        // A simple polygon always has an ear; rounding can hide it, and then any corner will do.
        let ear = (0..count).find(|&at| is_ear(at)).unwrap_or(0);
        triangles.push([left[(ear + count - 1) % count], left[ear], left[(ear + 1) % count]]);
        left.remove(ear);
    }
    if let [a, b, c] = left[..] {
        triangles.push([a, b, c]);
    }
    triangles
}

#[cfg(test)]
impl Sculpt {
    pub(super) fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    /// The mesh as Wavefront OBJ with vertex colors (`v x y z r g b`, linear).
    pub(super) fn to_obj(&self) -> String {
        let mut obj = String::new();
        for (place, [r, g, b, _]) in self.positions.iter().zip(&self.colors) {
            obj += &format!("v {} {} {} {r} {g} {b}\n", place.x, place.y, place.z);
        }
        for [a, b, c] in self.indices.as_chunks::<3>().0 {
            obj += &format!("f {} {} {}\n", a + 1, b + 1, c + 1);
        }
        obj
    }

    /// The volume the mesh encloses (m³), after checking what the renderer relies on: every
    /// triangle's edge is shared with exactly one neighbour running the other way (so each shell
    /// is closed and wound one way round), every vertex normal agrees with its triangle, and
    /// every shell encloses a positive volume (so its faces look outwards).
    pub(super) fn sound_volume(&self) -> f32 {
        use std::collections::HashMap;

        fn root(shells: &mut [usize], mut corner: usize) -> usize {
            while shells[corner] != corner {
                shells[corner] = shells[shells[corner]];
                corner = shells[corner];
            }
            corner
        }

        // Faces have their own vertices: corners at the same place (to a micrometre) are one.
        let mut places: HashMap<[i32; 3], usize> = HashMap::new();
        let welded: Vec<usize> = (self.positions.iter())
            .map(|&position| {
                let next = places.len();
                *places.entry((position * 1e6).round().as_ivec3().to_array()).or_insert(next)
            })
            .collect();
        let mut shells: Vec<usize> = (0..places.len()).collect();
        let mut edges: HashMap<(usize, usize), i32> = HashMap::new();
        assert_eq!(self.indices.len() % 3, 0);
        for triangle in self.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|corner| corner as usize);
            let facing = (self.positions[b] - self.positions[a]).cross(self.positions[c] - self.positions[a]);
            for corner in [a, b, c] {
                let agreement = facing.normalize().dot(self.normals[corner]);
                assert!(agreement > 0.7, "a normal {agreement} off its triangle at {}", self.positions[corner]);
            }
            let corners = [welded[a], welded[b], welded[c]];
            for (from, to) in [(corners[0], corners[1]), (corners[1], corners[2]), (corners[2], corners[0])] {
                assert_ne!(from, to, "a sliver at {}", self.positions[a]);
                *edges.entry((from.min(to), from.max(to))).or_default() += if from < to { 1 } else { -1 };
                let (from, to) = (root(&mut shells, from), root(&mut shells, to));
                shells[from] = to;
            }
        }
        assert!(edges.values().all(|&balance| balance == 0), "an open or inconsistently wound edge");
        let mut volumes: HashMap<usize, f32> = HashMap::new();
        for triangle in self.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|corner| self.positions[corner as usize]);
            *volumes.entry(root(&mut shells, welded[triangle[0] as usize])).or_default() += a.dot(b.cross(c)) / 6.0;
        }
        assert!(volumes.values().all(|&volume| volume > 0.0), "a shell is inside out: {volumes:?}");
        assert_eq!(self.positions.len(), self.normals.len());
        assert_eq!(self.positions.len(), self.colors.len());
        volumes.values().sum()
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    /// An L with a notch: concave corners on two sides.
    const BRACKET: [[f32; 2]; 8] =
        [[0.0, 0.0], [0.06, 0.0], [0.06, 0.02], [0.04, 0.02], [0.035, 0.03], [0.02, 0.03], [0.02, 0.08], [0.0, 0.08]];

    fn area(outline: &[Vec2]) -> f32 {
        signed_area(outline) * 0.5
    }

    /// Ear clipping covers a concave outline exactly, with triangles turning the outline's way.
    #[test]
    fn triangles_cover_concave_outlines() {
        let bent_bar = stroke(&[[0.0, 0.0], [0.04, -0.005], [0.06, 0.01], [0.065, 0.03]], 0.005);
        for outline in [BRACKET.to_vec(), bent_bar] {
            let mut outline: Vec<Vec2> = outline.into_iter().map(Vec2::from).collect();
            if signed_area(&outline) < 0.0 {
                outline.reverse();
            }
            let triangles = triangulate(&outline);
            assert_eq!(triangles.len(), outline.len() - 2);
            let mut covered = 0.0;
            for [a, b, c] in triangles {
                let triangle = area(&[outline[a], outline[b], outline[c]]);
                assert!(triangle > 0.0, "a triangle turns the wrong way");
                covered += triangle;
            }
            assert!((covered - area(&outline)).abs() < 1e-9, "{covered} of {}", area(&outline));
        }
    }

    /// A bar drawn along a path is as wide as asked on every straight stretch.
    #[test]
    fn strokes_keep_their_thickness() {
        let path = [[0.0, 0.0], [0.04, 0.0], [0.04, 0.03]];
        let outline: Vec<Vec2> = stroke(&path, 0.006).into_iter().map(Vec2::from).collect();
        // Length of the centre line times the thickness (the mitred corner neither adds nor
        // loses area).
        assert!((area(&outline).abs() - 0.07 * 0.006).abs() < 1e-9);
    }

    /// Every tool makes closed shells facing outwards, of the size asked for: as written, with
    /// bevels, placed at an angle, and mirrored.
    #[test]
    fn tools_build_sound_shells() {
        let (half_width, bevel) = (0.012, 0.002);
        let bracket: Vec<Vec2> = BRACKET.iter().map(|&corner| corner.into()).collect();
        let face = inset(&bracket, bevel);
        // A sharp-edged slab, plus what the bevels cut off its two sides (each a frustum
        // between the outline and the smaller face).
        let slab = area(&bracket) * 2.0 * half_width;
        let frustum = bevel / 3.0 * (area(&bracket) + area(&face) + (area(&bracket) * area(&face)).sqrt());
        let bevelled = area(&bracket) * 2.0 * (half_width - bevel) + 2.0 * frustum;

        let mut sharp = Sculpt::new();
        sharp.extrude(Finish::Polymer, &BRACKET, 0.01, half_width, 0.0);
        assert!((sharp.sound_volume() - slab).abs() < slab * 1e-4);

        let mut cut = Sculpt::new();
        let mut clockwise = BRACKET;
        clockwise.reverse();
        cut.extrude(Finish::Polymer, &clockwise, 0.0, half_width, bevel);
        // The frustum formula is exact only for similar outlines: this one is close.
        assert!((cut.sound_volume() - bevelled).abs() < bevelled * 0.01);
        assert!(cut.sound_volume() < slab);

        // An octagonal bar that narrows to a ridge across it: the straight part, then a wedge
        // whose sections shrink from the octagon to the ridge's line.
        let (wide, mid) = (|forward| section(forward, 0.0, 0.01, [0.0, 0.03], [0.003, 0.005]), 0.03);
        let rings = [wide(0.0), wide(mid), section(0.05, 0.0, 0.01, [0.015, 0.015], [0.0, 0.0])];
        let octagon = 0.02 * 0.03 - 0.003 * 0.003 - 0.005 * 0.005;
        let narrowing = octagon * mid + (0.05 - mid) * (octagon / 3.0 + 0.02 * 0.03 / 6.0);
        let mut bar = Sculpt::new();
        bar.loft(Finish::Metal, &rings);
        assert!((bar.sound_volume() - narrowing).abs() < narrowing * 1e-4);
        // Leaning it over is a shear: the same volume.
        let mut leaning = Sculpt::new();
        leaning.loft(Finish::Metal, &rings.map(|ring| raked(ring, 0.3, 0.01)));
        assert!((leaning.sound_volume() - narrowing).abs() < narrowing * 1e-4);

        // A barrel with a cone behind and a bore in front: a prism's volume, less the bore.
        let sides = 10;
        let mut barrel = Sculpt::new();
        let profile = [[0.0, 0.0], [0.01, 0.008], [0.1, 0.008], [0.1, 0.004], [0.08, 0.004]];
        barrel.lathe(Finish::Metal, at(0.0, 0.03, 0.0), &profile, sides);
        let polygon = |radius: f32| sides as f32 * 0.5 * radius * radius * (TAU / sides as f32).sin();
        let turned = polygon(0.008) * (0.01 / 3.0 + 0.09) - polygon(0.004) * 0.02;
        assert!((barrel.sound_volume() - turned).abs() < turned * 1e-3);
    }

    /// Faces are shaded by where they look, the same on both sides of the gun: a top face is
    /// lighter than a side, a side lighter than an underside.
    #[test]
    fn shading_tells_top_from_side_from_underside() {
        let (top, side, under) = (shade(Vec3::Y), shade(Vec3::X), shade(Vec3::NEG_Y));
        assert!(top > side && side > under && under > 0.0);
        assert_eq!(shade(Vec3::X), shade(Vec3::NEG_X));
    }
}
