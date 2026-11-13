//! The rifle: a modern carbine with a gunmetal upper under a full-length sight rail, a blue-black
//! polymer lower, handguard and stock, and a curved magazine. An original design in the
//! proportions of a tactical shooter's assault rifle.
//!
//! Its origin is the top of the pistol grip, which is what the recoil turns about. Lengths are
//! metres; profiles are `[forward, up]`.

use bevy::prelude::*;

use super::mesh::Finish::*;
use super::mesh::{Sculpt, at, raked, rect, section, stroke};

/// Half the receiver's width, and the handguard's.
const RECEIVER: f32 = 0.0175;
const HANDGUARD: f32 = 0.0185;
/// Where the receiver ends and the handguard starts, and where the handguard ends.
const SHOULDER: f32 = 0.175;
const NOSE: f32 = 0.335;
/// Top of the upper receiver and the handguard: the sight rail sits on this line.
const DECK: f32 = 0.056;
/// Top of the rail's teeth, and the height of the sight line above the gun's origin.
const RAIL_TOP: f32 = 0.0625;
const SIGHT_LINE: f32 = 0.0800;
/// Height of the bore above the gun's origin.
const BORE: f32 = 0.034;
/// Where the muzzle flash starts: just ahead of the brake.
pub(super) const MUZZLE: Vec3 = at(0.448, BORE, 0.0);

pub(super) fn rifle() -> Sculpt {
    let mut gun = Sculpt::new();
    receiver(&mut gun);
    furniture(&mut gun);
    handguard_and_barrel(&mut gun);
    sights(&mut gun);
    gun
}

fn receiver(gun: &mut Sculpt) {
    // Upper receiver.
    let upper = |forward| section(forward, 0.0, RECEIVER, [0.010, DECK], [0.002, 0.008]);
    gun.loft(
        Metal,
        &[
            section(-0.107, 0.0, RECEIVER - 0.002, [0.012, DECK - 0.002], [0.002, 0.007]),
            upper(-0.105),
            upper(SHOULDER),
        ],
    );
    // Lower receiver with the magazine well, flared at its mouth.
    let lower = [
        [SHOULDER, 0.0130],
        [-0.1000, 0.0130],
        [-0.1040, 0.0040],
        [-0.0960, -0.0120],
        [-0.0500, -0.0180],
        [0.0640, -0.0180],
        [0.0620, -0.0540],
        [0.1280, -0.0620],
        [0.1310, -0.0160],
        [SHOULDER, -0.0100],
    ];
    gun.extrude(Polymer, &lower, 0.0, RECEIVER - 0.001, 0.002);
    // Charging handle behind the rail.
    gun.extrude(Shadow, &rect([-0.114, -0.100], [0.0440, 0.0530]), 0.0, 0.0135, 0.001);
    // Ejection port with the bolt showing, on the right.
    gun.extrude(Shadow, &rect([0.048, 0.112], [0.0275, 0.0445]), RECEIVER, 0.0005, 0.0);
    gun.extrude(Steel, &rect([0.052, 0.090], [0.0300, 0.0420]), RECEIVER, 0.0008, 0.0);
    // The controls on the left: selector, bolt catch and the two receiver pins.
    let selector = [[-0.020, 0.0010], [0.004, 0.0030], [0.004, 0.0070], [-0.020, 0.0090]];
    gun.extrude(Steel, &selector, -(RECEIVER - 0.001), 0.001, 0.0);
    gun.extrude(Steel, &rect([0.086, 0.096], [0.0140, 0.0300]), -RECEIVER, 0.001, 0.0003);
    for pin in [-0.088, 0.150] {
        gun.extrude(Shadow, &rect([pin, pin + 0.006], [0.0000, 0.0060]), 0.0, RECEIVER - 0.0005, 0.0);
    }
    for side in [-1.0, 1.0] {
        // Grooves at the back of the upper, and the seam that the handguard's stripe continues.
        for groove in 0..4 {
            let back = -0.094 + 0.0075 * groove as f32;
            let groove = [[back, 0.0180], [back + 0.0032, 0.0180], [back + 0.0092, 0.0440], [back + 0.006, 0.0440]];
            gun.extrude(Shadow, &groove, side * RECEIVER, 0.0005, 0.0);
        }
        let seam = [[-0.050, 0.0215], [0.168, 0.0215], [0.170, 0.0240], [-0.048, 0.0240]];
        gun.extrude(Shadow, &seam, side * RECEIVER, 0.0004, 0.0);
    }
}

/// Grip, trigger group, magazine and stock.
fn furniture(gun: &mut Sculpt) {
    let grip = [
        [-0.0040, -0.0100],
        [-0.0500, -0.0100],
        [-0.0570, -0.0300],
        [-0.0740, -0.0700],
        [-0.0870, -0.1010],
        [-0.0830, -0.1090],
        [-0.0460, -0.1130],
        [-0.0400, -0.1050],
        [-0.0220, -0.0500],
        [-0.0080, -0.0240],
    ];
    gun.extrude(Polymer, &grip, 0.0, 0.0135, 0.003);
    let panel = [[-0.0500, -0.0300], [-0.0680, -0.0720], [-0.0780, -0.0980], [-0.0490, -0.1010], [-0.0300, -0.0500]];
    gun.extrude(Shadow, &panel, 0.0, 0.0141, 0.0006);
    let guard = [[-0.0200, -0.0430], [0.0400, -0.0465], [0.0580, -0.0390], [0.0660, -0.0220]];
    gun.extrude(Polymer, &stroke(&guard, 0.0046), 0.0, 0.006, 0.001);
    let trigger = [[0.0300, -0.0160], [0.0260, -0.0280], [0.0295, -0.0380]];
    gun.extrude(Steel, &stroke(&trigger, 0.004), 0.0, 0.003, 0.0008);

    // The magazine, curving forward, with two ribs.
    let magazine = [
        [0.0680, -0.0400],
        [0.0700, -0.0850],
        [0.0800, -0.1220],
        [0.0960, -0.1540],
        [0.1420, -0.1360],
        [0.1300, -0.1000],
        [0.1240, -0.0400],
    ];
    gun.extrude(Metal, &magazine, 0.0, 0.0115, 0.002);
    for rib in [[[0.0760, -0.0800], [0.1210, -0.0740]], [[0.0830, -0.1120], [0.1280, -0.0990]]] {
        gun.extrude(Shadow, &stroke(&rib, 0.004), 0.0, 0.0119, 0.0);
    }
    let floor = [[0.0930, -0.1530], [0.0960, -0.1610], [0.1470, -0.1410], [0.1430, -0.1330]];
    gun.extrude(Polymer, &floor, 0.0, 0.0128, 0.001);

    // Stock: the buffer tube, the body around it and a rubber butt pad.
    gun.lathe(Metal, at(0.0, BORE, 0.0), &[[-0.300, 0.0130], [-0.100, 0.0130]], 10);
    let stock = [
        [-0.1180, 0.0520],
        [-0.3300, 0.0520],
        [-0.3440, 0.0450],
        [-0.3440, -0.0620],
        [-0.3340, -0.0720],
        [-0.3000, -0.0600],
        [-0.2000, 0.0040],
        [-0.1180, 0.0100],
    ];
    gun.extrude(Polymer, &stock, 0.0, 0.0150, 0.003);
    // A coordinate in metres that happens to be close to 1/π.
    #[allow(clippy::approx_constant)]
    let window = [[-0.3180, -0.0440], [-0.3180, 0.0080], [-0.2400, 0.0080]];
    gun.extrude(Shadow, &window, 0.0, 0.0154, 0.0);
    let pad = [[-0.3430, 0.0470], [-0.3520, 0.0430], [-0.3520, -0.0640], [-0.3430, -0.0700]];
    gun.extrude(Shadow, &pad, 0.0, 0.0140, 0.002);
}

fn handguard_and_barrel(gun: &mut Sculpt) {
    // Octagonal handguard, its underside rising and its nose cut back towards the top.
    let guard = |forward, bottom| section(forward, 0.0, HANDGUARD, [bottom, DECK], [0.008, 0.008]);
    let nose = section(NOSE, 0.0, HANDGUARD - 0.003, [0.010, DECK - 0.003], [0.007, 0.007]);
    gun.loft(
        Polymer,
        &[guard(SHOULDER - 0.004, 0.000), raked(guard(NOSE - 0.003, 0.007), -0.5, 0.007), raked(nose, -0.5, 0.007)],
    );
    for side in [-1.0, 1.0] {
        // Vent slots and the stripe under them.
        for slot in 0..2 {
            let back = 0.196 + 0.056 * slot as f32;
            let slot = [[back, 0.0300], [back + 0.038, 0.0300], [back + 0.043, 0.0390], [back + 0.005, 0.0390]];
            gun.extrude(Shadow, &slot, side * HANDGUARD, 0.0005, 0.0);
        }
        let stripe = [[0.184, 0.0180], [0.304, 0.0180], [0.306, 0.0205], [0.186, 0.0205]];
        gun.extrude(Accent, &stripe, side * HANDGUARD, 0.0005, 0.0);
    }
    // Barrel, muzzle brake and bore.
    let bore = at(0.0, BORE, 0.0);
    gun.lathe(Metal, bore, &[[NOSE - 0.030, 0.0080], [0.396, 0.0080]], 10);
    gun.lathe(Shadow, bore, &[[0.384, 0.0090], [0.388, 0.0120], [0.440, 0.0120], [0.444, 0.0098]], 10);
    gun.lathe(Steel, bore, &[[0.444, 0.0066], [0.4445, 0.0066]], 10);
    gun.lathe(Shadow, bore, &[[0.4445, 0.0038], [0.4448, 0.0038]], 10);
}

/// The sight rail along receiver and handguard, and the iron sights on it.
fn sights(gun: &mut Sculpt) {
    const PITCH: f32 = 0.010;
    const TOOTH: f32 = 0.0058;
    let (back, front) = (-0.094, NOSE - 0.030);
    let teeth = ((front - back) / PITCH) as usize;
    let mut rail = vec![[back + teeth as f32 * PITCH, DECK - 0.001], [back, DECK - 0.001]];
    for tooth in 0..teeth {
        let start = back + tooth as f32 * PITCH;
        // Each tooth starts with the gap before the next one already cut behind it.
        rail.extend([
            [start, RAIL_TOP],
            [start + TOOTH, RAIL_TOP],
            [start + TOOTH, DECK + 0.003],
            [start + PITCH, DECK + 0.003],
        ]);
    }
    gun.extrude(Polymer, &rail, 0.0, 0.0090, 0.0);

    // Rear sight: a base with two ears and their dots.
    gun.extrude(Shadow, &rect([-0.084, -0.066], [RAIL_TOP - 0.001, 0.0720]), 0.0, 0.0100, 0.001);
    for side in [-1.0, 1.0] {
        let ear = [[-0.084, 0.0710], [-0.068, 0.0710], [-0.073, SIGHT_LINE + 0.002], [-0.084, SIGHT_LINE + 0.002]];
        gun.extrude(Shadow, &ear, side * 0.0072, 0.0026, 0.0008);
        gun.extrude(Accent, &rect([-0.0844, -0.0830], [0.0775, 0.0800]), side * 0.0072, 0.0011, 0.0);
    }
    // Front sight: a tower on the rail, a post between two guards, the post's tip marked.
    let tower = [[0.268, DECK - 0.001], [0.294, DECK - 0.001], [0.289, 0.0700], [0.275, 0.0700]];
    gun.extrude(Shadow, &tower, 0.0, 0.0096, 0.001);
    for side in [-1.0, 1.0] {
        let guard = [[0.275, 0.0690], [0.289, 0.0690], [0.286, SIGHT_LINE + 0.003], [0.278, SIGHT_LINE + 0.003]];
        gun.extrude(Shadow, &guard, side * 0.0074, 0.0015, 0.0005);
    }
    gun.extrude(Shadow, &rect([0.280, 0.284], [0.0690, SIGHT_LINE]), 0.0, 0.0012, 0.0);
    gun.extrude(Accent, &rect([0.2796, 0.2800], [0.0755, SIGHT_LINE]), 0.0, 0.0012, 0.0);
}
