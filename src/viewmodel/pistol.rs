//! The pistol: a compact striker-fired sidearm with a stepped gunmetal slide over a blue-black
//! polymer frame. An original design in the chunky proportions of a tactical shooter's sidearm.
//!
//! Its origin is where the web of the hand sits under the slide's tail, which is what the recoil
//! turns about. Lengths are metres; profiles are `[forward, up]`.

use bevy::prelude::*;

use super::mesh::Finish::*;
use super::mesh::{Sculpt, at, rect, section, stroke};

/// Top of the frame's rails: the slide sits on this line.
const RAIL: f32 = 0.011;
/// Half the slide's width at its thick rear part, and at the stepped front part.
const SLIDE: f32 = 0.0145;
const SLIDE_FRONT: f32 = 0.0125;
/// Half the frame's width.
const FRAME: f32 = 0.0136;
/// Height of the bore above the gun's origin.
const BORE: f32 = 0.028;
/// Where the muzzle flash starts: just ahead of the crown.
pub(super) const MUZZLE: Vec3 = at(0.156, BORE, 0.0);

pub(super) fn pistol() -> Sculpt {
    let mut gun = Sculpt::new();
    slide(&mut gun);
    frame(&mut gun);
    gun
}

fn slide(gun: &mut Sculpt) {
    // The body: broad shoulders, a thick rear half with the grasping grooves and a slimmer,
    // lower front half.
    let rear = |forward| section(forward, 0.0, SLIDE, [RAIL, 0.042], [0.0015, 0.0065]);
    let front = |forward| section(forward, 0.0, SLIDE_FRONT, [RAIL, 0.0395], [0.0015, 0.0055]);
    gun.loft(
        Metal,
        &[
            section(-0.0535, 0.0, SLIDE - 0.0015, [RAIL + 0.0015, 0.0405], [0.0015, 0.006]),
            rear(-0.052),
            rear(0.066),
            front(0.071),
            front(0.146),
            section(0.148, 0.0, SLIDE_FRONT - 0.0015, [RAIL + 0.0015, 0.038], [0.0015, 0.005]),
        ],
    );
    // Barrel crown and bore.
    let bore = at(0.0, BORE, 0.0);
    gun.lathe(Steel, bore, &[[0.140, 0.0064], [0.1525, 0.0064], [0.1535, 0.0054]], 10);
    gun.lathe(Shadow, bore, &[[0.1535, 0.0036], [0.1538, 0.0036]], 10);
    // Striker plate on the back, with a status bar.
    gun.extrude(Shadow, &rect([-0.0540, -0.0530], [0.0155, 0.0365]), 0.0, 0.0085, 0.0);
    gun.extrude(Accent, &rect([-0.0544, -0.0530], [0.0180, 0.0200]), 0.0, 0.0045, 0.0);
    // Ejection port on top, towards the right.
    gun.extrude(Shadow, &rect([0.012, 0.058], [0.0410, 0.0424]), 0.0015, 0.006, 0.0);
    gun.extrude(Steel, &rect([0.016, 0.054], [0.0410, 0.0427]), 0.0015, 0.0035, 0.0);
    for side in [-1.0, 1.0] {
        // Grasping grooves, raked like the grip.
        for groove in 0..6 {
            let back = -0.046 + 0.0052 * groove as f32;
            let groove = [[back, 0.0145], [back + 0.0024, 0.0145], [back + 0.0064, 0.0335], [back + 0.004, 0.0335]];
            gun.extrude(Shadow, &groove, side * SLIDE, 0.0005, 0.0);
        }
        // The stripe along the slim front half.
        let stripe = [[0.079, 0.0150], [0.139, 0.0150], [0.141, 0.0178], [0.081, 0.0178]];
        gun.extrude(Accent, &stripe, side * SLIDE_FRONT, 0.0005, 0.0);
        // Rear sight ears with their dots.
        gun.extrude(Shadow, &rect([-0.048, -0.037], [0.0440, 0.0500]), side * 0.0054, 0.0025, 0.0006);
        gun.extrude(Accent, &rect([-0.0484, -0.0470], [0.0462, 0.0484]), side * 0.0054, 0.0011, 0.0);
    }
    // Rear sight base, front sight post and its dot.
    gun.extrude(Shadow, &rect([-0.048, -0.036], [0.0410, 0.0445]), 0.0, 0.0082, 0.0006);
    gun.extrude(Shadow, &[[0.132, 0.0390], [0.143, 0.0390], [0.140, 0.0462], [0.132, 0.0462]], 0.0, 0.0019, 0.0005);
    gun.extrude(Accent, &rect([0.1316, 0.1330], [0.0424, 0.0448]), 0.0, 0.0011, 0.0);
}

fn frame(gun: &mut Sculpt) {
    // Frame and grip in one outline: dust cover with its rail block, the undercut behind the
    // trigger guard, front strap, magazine floor, the raked backstrap and the beavertail.
    let outline = [
        [0.138, RAIL + 0.0015],
        [-0.050, RAIL + 0.0015],
        [-0.063, 0.0095],
        [-0.0665, 0.0020],
        [-0.0570, -0.0050],
        [-0.0555, -0.0200],
        [-0.0640, -0.0600],
        [-0.0800, -0.1040],
        [-0.0780, -0.1120],
        [-0.0340, -0.1160],
        [-0.0300, -0.1100],
        [-0.0145, -0.0500],
        [-0.0060, -0.0180],
        [0.0000, -0.0100],
        [0.0440, -0.0080],
        [0.0500, -0.0160],
        [0.1280, -0.0160],
        [0.1380, -0.0080],
    ];
    gun.extrude(Polymer, &outline, 0.0, FRAME, 0.0022);
    // Grip panels, a little proud of the frame.
    let panel = [
        [-0.0500, -0.0230],
        [-0.0585, -0.0600],
        [-0.0720, -0.0980],
        [-0.0390, -0.1020],
        [-0.0245, -0.0560],
        [-0.0150, -0.0270],
    ];
    gun.extrude(Shadow, &panel, 0.0, FRAME + 0.0007, 0.0007);
    // Magazine floor plate.
    let floor = [[-0.0835, -0.1090], [-0.0815, -0.1200], [-0.0300, -0.1245], [-0.0255, -0.1140]];
    gun.extrude(Metal, &floor, 0.0, FRAME + 0.001, 0.0015);
    // Trigger guard and trigger.
    let guard = [[-0.0170, -0.0420], [0.0260, -0.0465], [0.0430, -0.0400], [0.0500, -0.0260], [0.0520, -0.0120]];
    gun.extrude(Polymer, &stroke(&guard, 0.0046), 0.0, 0.0066, 0.001);
    let trigger = [[0.0200, -0.0060], [0.0160, -0.0200], [0.0190, -0.0320]];
    gun.extrude(Steel, &stroke(&trigger, 0.004), 0.0, 0.003, 0.0008);
    // Accessory rail slots under the dust cover.
    for slot in 0..3 {
        let back = 0.066 + 0.018 * slot as f32;
        gun.extrude(Shadow, &rect([back, back + 0.005], [-0.0155, -0.0075]), 0.0, FRAME + 0.0004, 0.0);
    }
    // The controls on the left: slide stop and magazine release.
    let stop = [[0.004, 0.0030], [0.034, 0.0030], [0.037, 0.0075], [0.004, 0.0095]];
    gun.extrude(Steel, &stop, -FRAME, 0.0009, 0.0);
    gun.extrude(Steel, &rect([-0.0045, 0.0035], [-0.0165, -0.0085]), -FRAME, 0.0012, 0.0004);
}
