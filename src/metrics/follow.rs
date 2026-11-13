//! How a crosshair follows a single moving target through its changes of direction, from a
//! finished run's samples (what a replay holds: time, view angles, the target's centre and
//! radius). Shown on the results of tracking runs.
//!
//! **Change of direction.** Targets have inertia: a strafing dot slows down, stops and sets off
//! the other way over a few tenths of a second, and a dot on a loop turns through a curve. So a
//! change of direction is not a frame but a moment picked out of that: the instant the target's
//! velocity along its main axis of motion passes through zero, i.e. the point where it stops
//! going one way and starts going the other. The main axis is the line its path is longest along
//! (the principal axis of its positions: the lane of a strafing dot, the long side of a loop).
//! Reversals count only between stretches where the target moves at more than [`MOVING`] of its
//! usual speed each way, so a dot dithering around a standstill has none. The slowing down
//! before the reversal is visible and can be read: a player who reads it reverses with the
//! target, and one who waits for the reversal itself is a reaction time behind.
//!
//! **Measures**, all along that axis unless noted:
//! - reaction lag: from the target's reversal to the crosshair's own (its velocity passing
//!   through zero into the new direction, with the same [`MOVING`] limit on each side). Negative
//!   if the crosshair turned first. Measured for turns where the crosshair was going the old
//!   way and reversed within [`REACT_WITHIN`] and before the next turn; the median is reported.
//! - recovery: for turns that threw the crosshair off the target (off it, in any direction, at
//!   some point between the reversal and the next one, or within [`RECOVER_WITHIN`]), the time
//!   from the reversal until it is back on and stays on for [`STAY`]. Turns it stayed on through
//!   are counted as kept, turns it wasn't back from before the next one as lost.
//! - lead: the mean offset of the crosshair from the target's centre in the direction the
//!   target is moving, in degrees, while the target is moving: positive ahead, negative behind.
//! - jitter: [`steadiness::follow`]'s, the crosshair's fast wobble relative to the target's
//!   motion over the whole run, catching up after turns included.
//!
//! Velocities are differences over a centred [`VELOCITY_SECS`] window, the same for target and
//! crosshair, so the smoothing shifts both reversals alike and single mouse counts don't read
//! as reversals. Reversal times are interpolated between samples; at 60 fps and above a known
//! delay is recovered to within a frame or two.

use bevy::math::Vec2;

use super::median;
use super::steadiness::{self, Tracked};

/// The window velocities are taken over, seconds.
const VELOCITY_SECS: f32 = 0.08;
/// The share of the target's usual (median) speed that counts as moving, for the target and for
/// the crosshair.
const MOVING: f32 = 0.25;
/// A crosshair that turned this long before the target wasn't reacting to this turn, seconds.
const REACT_EARLY: f32 = 0.25;
/// The longest reaction measured, seconds.
const REACT_WITHIN: f32 = 1.0;
/// How long after a reversal the crosshair is still looked at for coming off and getting back,
/// seconds (less if the next reversal comes first).
const RECOVER_WITHIN: f32 = 1.5;
/// Back on the target means staying on it this long, seconds: grazing it on the way past isn't.
const STAY: f32 = 0.15;
/// A target slower than this (median, degrees per second) isn't a moving target.
const MIN_PACE: f32 = 0.2;

/// How a run's crosshair followed its target, see the module docs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Following {
    /// Changes of direction the target made.
    pub turns: u32,
    /// Median reaction lag, seconds; `None` if no turn had a measurable one.
    pub reaction: Option<f32>,
    /// Turns the reaction lag was measured at.
    pub reacted: u32,
    /// Median recovery of the turns that threw the crosshair off, seconds; `None` without any.
    pub recovery: Option<f32>,
    /// Turns the crosshair stayed on the target through.
    pub kept: u32,
    /// Turns it was thrown off by and not back from before the next.
    pub lost: u32,
    /// Mean offset along the target's motion, degrees: positive ahead of it, negative behind.
    pub lead: f32,
    /// Fast wobble relative to the target's motion, degrees RMS.
    pub jitter: f32,
    /// The target's mean angular radius, degrees: what the offsets compare with.
    pub radius: f32,
    /// The target's usual (median) speed, degrees per second.
    pub pace: f32,
}

/// Measures how the crosshair followed the target over `frames` (in time order). `None` for less
/// than [`steadiness::MIN_SECS`] of samples or a target that hardly moves.
pub fn following(frames: &[Tracked]) -> Option<Following> {
    let jitter = steadiness::follow(frames)?.jitter;
    let n = frames.len() as f32;
    let centre = frames.iter().map(|f| f.target).sum::<Vec2>() / n;
    let axis = main_axis(frames, centre);
    let times: Vec<f32> = frames.iter().map(|f| f.t).collect();
    let target: Vec<f32> = frames.iter().map(|f| (f.target - centre).dot(axis)).collect();
    // The crosshair relative to the target, so a view turned past ±180° can't jump.
    let offset: Vec<f32> = frames.iter().map(|f| super::flick_probe::wrap(f.aim - f.target).dot(axis)).collect();
    let aim: Vec<f32> = target.iter().zip(&offset).map(|(target, offset)| target + offset).collect();
    let target_speed = velocities(&times, &target);
    let aim_speed = velocities(&times, &aim);
    let pace = median(&mut target_speed.iter().map(|v| v.abs()).collect::<Vec<_>>())?;
    if pace < MIN_PACE {
        return None;
    }
    let moving = MOVING * pace;
    let turns = reversals(&times, &target_speed, moving, 0, times.len());
    let on: Vec<bool> =
        frames.iter().map(|f| super::flick_probe::wrap(f.aim - f.target).length() <= f.radius).collect();

    let (mut reactions, mut recoveries) = (Vec::new(), Vec::new());
    let (mut kept, mut lost) = (0, 0);
    for (k, &(at, towards)) in turns.iter().enumerate() {
        let next = turns.get(k + 1).map_or(f32::INFINITY, |turn| turn.0);
        let from = times.partition_point(|&t| t < at - REACT_EARLY);
        let to = times.partition_point(|&t| t <= next.min(at + REACT_WITHIN));
        let reversed = reversals(&times, &aim_speed, moving, from, to).into_iter().find(|turn| turn.1 == towards);
        reactions.extend(reversed.map(|turn| turn.0 - at));
        match recovery(&times, &on, at, next.min(at + RECOVER_WITHIN)) {
            Recovery::Kept => kept += 1,
            Recovery::Back(secs) => recoveries.push(secs),
            Recovery::Lost => lost += 1,
        }
    }

    let (mut lead, mut weights) = (0.0, 0.0);
    for (i, (offset, speed)) in offset.iter().zip(&target_speed).enumerate() {
        if speed.abs() >= moving {
            let weight = (times[(i + 1).min(times.len() - 1)] - times[i.saturating_sub(1)]) * 0.5;
            lead += offset * speed.signum() * weight;
            weights += weight;
        }
    }
    Some(Following {
        turns: turns.len() as u32,
        reacted: reactions.len() as u32,
        reaction: median(&mut reactions),
        recovery: median(&mut recoveries),
        kept,
        lost,
        lead: if weights > 0.0 { lead / weights } else { 0.0 },
        jitter,
        radius: frames.iter().map(|f| f.radius).sum::<f32>() / n,
        pace,
    })
}

/// The unit vector along the line the target's positions spread out most along.
fn main_axis(frames: &[Tracked], centre: Vec2) -> Vec2 {
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for frame in frames {
        let at = frame.target - centre;
        (xx, xy, yy) = (xx + at.x * at.x, xy + at.x * at.y, yy + at.y * at.y);
    }
    Vec2::from_angle(0.5 * (2.0 * xy).atan2(xx - yy))
}

/// The velocity at each sample: the change of the straight-line path through the samples over
/// the [`VELOCITY_SECS`] around it (less at the ends, where the window is cut off).
fn velocities(times: &[f32], values: &[f32]) -> Vec<f32> {
    let half = VELOCITY_SECS * 0.5;
    let (first, last) = (times[0], times[times.len() - 1]);
    // The value at time `t`, which lies in the segment starting at sample `i`.
    let at = |t: f32, i: usize| {
        let j = (i + 1).min(times.len() - 1);
        let gap = times[j] - times[i];
        if gap <= 0.0 { values[i] } else { values[i] + (values[j] - values[i]) * (t - times[i]) / gap }
    };
    let (mut from, mut to) = (0, 0);
    times
        .iter()
        .map(|&t| {
            let (start, end) = ((t - half).max(first), (t + half).min(last));
            while from + 1 < times.len() && times[from + 1] <= start {
                from += 1;
            }
            while to + 1 < times.len() && times[to + 1] <= end {
                to += 1;
            }
            if end > start { (at(end, to) - at(start, from)) / (end - start) } else { 0.0 }
        })
        .collect()
}

/// The reversals of `speed` among samples `from..to`: (time, new direction as ±1) wherever it
/// goes from beyond `moving` one way to beyond `moving` the other, timed where it passes
/// through zero on the way.
fn reversals(times: &[f32], speed: &[f32], moving: f32, from: usize, to: usize) -> Vec<(f32, f32)> {
    let mut found = Vec::new();
    let mut heading = 0.0;
    for i in from..to {
        if speed[i].abs() < moving {
            continue;
        }
        let sign = speed[i].signum();
        if heading != 0.0 && sign != heading {
            // Back to the first sample already going the new way; zero lies just before it.
            let mut j = i;
            while j > from && speed[j - 1] * sign > 0.0 {
                j -= 1;
            }
            let at = if j == 0 {
                times[0]
            } else {
                let (a, b) = (speed[j - 1], speed[j]);
                times[j - 1] + (times[j] - times[j - 1]) * (a / (a - b)).clamp(0.0, 1.0)
            };
            found.push((at, sign));
        }
        heading = sign;
    }
    found
}

enum Recovery {
    /// On the target all the way.
    Kept,
    /// Off it, and back to stay this many seconds after the reversal.
    Back(f32),
    Lost,
}

/// What a reversal at `at` did to the crosshair up to `until`, given whether it was on the
/// target at each sample.
fn recovery(times: &[f32], on: &[bool], at: f32, until: f32) -> Recovery {
    let from = times.partition_point(|&t| t < at);
    let to = times.partition_point(|&t| t <= until);
    let Some(off) = (from..to).find(|&i| !on[i]) else { return Recovery::Kept };
    let mut i = off;
    while i < to {
        if !on[i] {
            i += 1;
            continue;
        }
        // On from here: for long enough?
        let mut end = i;
        while end + 1 < times.len() && on[end + 1] && times[end] - times[i] < STAY {
            end += 1;
        }
        if times[end] - times[i] >= STAY {
            return Recovery::Back((times[i] - at).max(0.0));
        }
        i = end + 1;
    }
    Recovery::Lost
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::metrics::steadiness::tests::RATES;

    const RADIUS: f32 = 2.5;

    /// A strafing dot with inertia, like Tracking's: its position (degrees) every millisecond
    /// for `secs`, reversing every 0.7 to 1.5 s between 9 and 22°/s at 115°/s².
    pub(crate) fn strafing(secs: f32) -> Vec<f32> {
        const ACCEL: f32 = 115.0;
        let mut seed = 7u32;
        let mut random = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let (mut x, mut velocity, mut wanted, mut turn_in) = (0.0f32, 0.0f32, 12.0f32, 1.0f32);
        (0..(secs * 1000.0) as usize)
            .map(|_| {
                turn_in -= 0.001;
                if turn_in <= 0.0 {
                    wanted = -wanted.signum() * (9.0 + 13.0 * random());
                    turn_in = 0.7 + 0.8 * random();
                }
                velocity += (wanted - velocity).clamp(-ACCEL * 0.001, ACCEL * 0.001);
                x += velocity * 0.001;
                x
            })
            .collect()
    }

    /// The millisecond path at time `t`, held at its ends.
    fn at(path: &[f32], t: f32) -> f32 {
        let at = (t * 1000.0).clamp(0.0, (path.len() - 1) as f32);
        let i = (at as usize).min(path.len() - 2);
        path[i] + (path[i + 1] - path[i]) * (at - i as f32)
    }

    /// Frames at `fps` of a target on `path` (along `axis`) and the aim `aim(t)` along it.
    pub(crate) fn frames(fps: f32, path: &[f32], axis: Vec2, aim: impl Fn(f32) -> f32) -> Vec<Tracked> {
        let secs = path.len() as f32 / 1000.0;
        (0..(secs * fps) as usize)
            .map(|i| {
                let t = i as f32 / fps;
                Tracked { t, aim: axis * aim(t), target: axis * at(path, t), radius: RADIUS }
            })
            .collect()
    }

    #[test]
    fn recovers_a_known_delay_at_any_frame_rate() {
        let path = strafing(30.0);
        for fps in RATES {
            for delay in [0.0, 0.12, 0.25] {
                let result = following(&frames(fps, &path, Vec2::X, |t| at(&path, t - delay))).unwrap();
                assert!(result.turns >= 20, "{fps} fps: {} turns", result.turns);
                let lag = result.reaction.unwrap();
                assert!((lag - delay).abs() <= 2.0 / fps, "{fps} fps, delay {delay}: measured {lag}");
                // Nearly every turn is measurable (the very last may have no time left).
                assert!(result.reacted + 2 >= result.turns, "{fps} fps: {} of {}", result.reacted, result.turns);
            }
        }
    }

    #[test]
    fn a_delayed_follower_trails_and_is_thrown_off_by_turns() {
        let path = strafing(30.0);
        let exact = following(&frames(178.0, &path, Vec2::X, |t| at(&path, t))).unwrap();
        assert_eq!((exact.kept, exact.lost, exact.recovery), (exact.turns, 0, None));
        assert!(exact.lead.abs() < 1e-3 && exact.jitter < 1e-3, "{exact:?}");

        // 0.25 s behind a dot doing 9 to 22°/s is 2 to 5° behind: off a 2.5° target on every
        // fast sweep, back on it around each turn, where the dot comes back to the crosshair.
        let late = following(&frames(178.0, &path, Vec2::X, |t| at(&path, t - 0.25))).unwrap();
        assert!(late.lead < -2.0, "{late:?}");
        assert!(late.kept < late.turns / 2, "{late:?}");
        let recovery = late.recovery.unwrap();
        assert!(recovery > 0.0 && recovery < 0.6, "{late:?}");
        // Ahead of the dot by a steady 0.4°.
        let ahead = following(&frames(178.0, &path, Vec2::X, |t| {
            let heading = (at(&path, t + 0.001) - at(&path, t)).signum();
            at(&path, t) + 0.4 * heading
        }))
        .unwrap();
        assert!((ahead.lead - 0.4).abs() < 0.05, "{ahead:?}");
    }

    #[test]
    fn recovery_is_the_time_back_onto_the_target() {
        let path = strafing(30.0);
        let result = {
            // Freezes for 0.3 s at every turn, then is on the dot again: recovered about 0.3 s
            // after each reversal that carried the dot more than its radius away meanwhile.
            let frames = frames(240.0, &path, Vec2::X, |t| at(&path, t));
            let turns = reversals(
                &frames.iter().map(|f| f.t).collect::<Vec<_>>(),
                &velocities(
                    &frames.iter().map(|f| f.t).collect::<Vec<_>>(),
                    &frames.iter().map(|f| f.target.x).collect::<Vec<_>>(),
                ),
                2.0,
                0,
                frames.len(),
            );
            let frozen: Vec<Tracked> = frames
                .iter()
                .map(|f| match turns.iter().find(|turn| f.t >= turn.0 && f.t < turn.0 + 0.3) {
                    Some(turn) => Tracked { aim: Vec2::X * at(&path, turn.0), ..*f },
                    None => *f,
                })
                .collect();
            following(&frozen).unwrap()
        };
        let recovery = result.recovery.unwrap();
        assert!((recovery - 0.3).abs() < 0.02, "{result:?}");
        assert_eq!(result.lost, 0, "{result:?}");
        // Slow reversals don't carry the dot off the frozen crosshair within 0.3 s.
        assert!(result.kept > 0 && result.kept < result.turns, "{result:?}");
    }

    #[test]
    fn follows_a_lane_in_any_direction_and_a_loop() {
        let path = strafing(20.0);
        // A lane tilted 30° up: the main axis is found, the delay still measured.
        let tilted = following(&frames(178.0, &path, Vec2::from_angle(0.52), |t| at(&path, t - 0.2))).unwrap();
        assert!((tilted.reaction.unwrap() - 0.2).abs() < 0.012, "{tilted:?}");
        // Round a flat loop (30° x 8°) every 6 s: two reversals along its long side per lap.
        let lap = |t: f32| {
            let angle = t * std::f32::consts::TAU / 6.0;
            Vec2::new(15.0 * angle.cos(), 4.0 * angle.sin())
        };
        let looping: Vec<Tracked> = (0..(24.0 * 178.0) as usize)
            .map(|i| {
                let t = i as f32 / 178.0;
                Tracked { t, aim: lap(t - 0.15), target: lap(t), radius: RADIUS }
            })
            .collect();
        let result = following(&looping).unwrap();
        assert!((7..=8).contains(&result.turns), "{result:?}");
        assert!((result.reaction.unwrap() - 0.15).abs() < 0.012, "{result:?}");
    }

    #[test]
    fn a_resting_target_has_nothing_to_follow() {
        let still: Vec<Tracked> = (0..600)
            .map(|i| Tracked { t: i as f32 / 200.0, aim: Vec2::new(0.3, 0.0), target: Vec2::ZERO, radius: RADIUS })
            .collect();
        assert_eq!(following(&still), None);
        assert_eq!(following(&still[..10]), None);
    }
}
