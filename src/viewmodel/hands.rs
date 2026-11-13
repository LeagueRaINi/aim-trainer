//! The player's gloved hands and forearms on the guns. Unlike the guns they are not modelled in
//! code: they are the CC0 "WRAD ARMS" by wriks (`tools/hands`), posed on each gun in Blender by
//! `tools/hands/pose.py` and saved in the gun's space as `assets/models/<gun>_hands.obj`. Those
//! files are embedded here and their faces added to the gun's mesh in the glove's [`Finish`]es,
//! so a gun and the hands on it are still one mesh, mirrored as one for the left hand.

use bevy::prelude::*;

use super::mesh::{Finish, Sculpt};
use super::{pistol, rifle};

/// The pistol in the firing hand.
pub(super) fn pistol() -> Sculpt {
    held(pistol::pistol(), include_str!("../../assets/models/pistol_hands.obj"))
}

/// The rifle in both hands.
pub(super) fn rifle() -> Sculpt {
    held(rifle::rifle(), include_str!("../../assets/models/rifle_hands.obj"))
}

/// `gun` with the hands in `obj` on it. Of Wavefront OBJ this reads what `pose.py` writes:
/// `v x y z` (metres, the mesh's axes), `usemtl <finish>.<percent>` and `f a b c ..` (flat convex
/// faces, counter-clockwise seen from outside, corners counted from 1). The percent is how much
/// of its finish's color a face keeps: `pose.py` darkens the faces between the fingers and
/// against the gun, which tells the fingers apart.
fn held(mut gun: Sculpt, obj: &str) -> Sculpt {
    let mut places: Vec<Vec3> = Vec::new();
    let mut finish = (Finish::Glove, 1.0);
    let mut corners = Vec::new();
    for line in obj.lines() {
        let mut words = line.split_ascii_whitespace();
        match words.next() {
            Some("v") => {
                let mut axes = words.map(|word| word.parse::<f32>().expect("a coordinate"));
                places.push(Vec3::from_array(std::array::from_fn(|_| axes.next().expect("three coordinates"))));
            },
            Some("usemtl") => {
                let (name, percent) = words.next().and_then(|word| word.split_once('.')).expect("a finish");
                let name = match name {
                    "glove" => Finish::Glove,
                    "pad" => Finish::Pad,
                    "sleeve" => Finish::Sleeve,
                    other => panic!("no finish {other}"),
                };
                finish = (name, percent.parse::<f32>().expect("a percent") / 100.0);
            },
            Some("f") => {
                corners.clear();
                corners.extend(words.map(|word| places[word.parse::<usize>().expect("a corner") - 1]));
                gun.dimmed(finish.1, |gun| gun.face(finish.0, &corners));
            },
            _ => {},
        }
    }
    gun
}
