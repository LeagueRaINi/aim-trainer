//! Hand steadiness, measured from aim samples in degrees of view: how much the crosshair wobbles
//! while it holds still ([`hold`]) or follows a slow target ([`follow`]), and whether a flick
//! arrives in one move or overshoots and needs corrections ([`flick`], [`settle`], [`moves`]).
//! The Calm aim test scores these; they are kept free of any drill so other code (a sensitivity
//! finder asking "does this hand's noise still fit inside a head at this sensitivity?") can
//! measure the same things from its own samples or from a replay.
//!
//! **Units.** Angles are (yaw, pitch) in degrees, as in [`flick_probe`](super::flick_probe);
//! times are seconds. Distances are taken in the (yaw, pitch) plane, which is the angle of view
//! to within 1% for pitches under 8° and 4% under 16°. A result in degrees belongs to the
//! sensitivity it was measured at: [`mouse_mm`] turns it into millimetres of mouse travel, the
//! hand's own unit, which carries over to any other sensitivity.
//!
//! **Jitter** is the wobble around the slow path: every sample's distance from the centred
//! moving average over [`SLOW_SECS`], as a time-weighted RMS. The moving average follows
//! anything slower than about 1 Hz (reaching, drifting, a steady lag behind a target) and
//! wobble at 4 Hz and above is left over in full, so tremor and the stop-and-go stepping of a
//! tense hand count and deliberate motion doesn't. A mouse turns the view in whole counts, so
//! while the hand moves the floor is about 0.3 counts (`count / sqrt(12)`: 0.008° at Valorant
//! 0.38); a hand at rest sends no counts and measures 0.

use bevy::math::Vec2;

use super::flick_probe::{END_SPEED, START_SPEED, wrap};

/// The moving-average window that separates slow, deliberate motion from jitter, seconds.
pub const SLOW_SECS: f32 = 0.25;
/// The shortest stretch [`hold`] and [`follow`] measure: with less than two windows of samples
/// the slow path is mostly edge.
pub const MIN_SECS: f32 = 2.0 * SLOW_SECS;
/// [`follow`] takes its jitter from the time the crosshair was within this many target radii of
/// the target's centre: farther away it isn't following the target, whatever it does.
pub const WITH_TARGET: f32 = 3.0;
/// Time constant of [`SpeedFilter`], the same ~7 Hz low-pass the first-flick probe uses.
const SPEED_TAU: f32 = 0.023;
const MM_PER_INCH: f32 = 25.4;

/// One frame of following a target: where the aim and the target's centre were, and the target's
/// angular radius, all in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tracked {
    pub t: f32,
    pub aim: Vec2,
    pub target: Vec2,
    pub radius: f32,
}

/// A crosshair that was meant to stay where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hold {
    /// Seconds measured.
    pub secs: f32,
    /// RMS wobble around the slow path, degrees (see the module docs).
    pub jitter: f32,
    /// The farthest the slow path got from where it started, degrees.
    pub drift: f32,
    /// Everything the crosshair travelled, degrees: 0 for a hand that never moved.
    pub travel: f32,
}

/// Measures a crosshair holding still over `points` (seconds, aim). `None` for less than
/// [`MIN_SECS`] of samples.
pub fn hold(points: &[(f32, Vec2)]) -> Option<Hold> {
    let secs = span(points.iter().map(|p| p.0))?;
    let origin = points[0].1;
    // Relative to the first sample, so a hold near the ±180° seam of the yaw has no jump in it.
    let path: Vec<(f32, Vec2)> = points.iter().map(|&(t, aim)| (t, wrap(aim - origin))).collect();
    let (jitter, slow) = detrended(&path, |_| true);
    let start = slow.iter().flatten().next().copied()?;
    let drift = slow.iter().flatten().map(|at| at.distance(start)).fold(0.0, f32::max);
    let travel = path.windows(2).map(|w| w[1].1.distance(w[0].1)).sum();
    Some(Hold { secs, jitter, drift, travel })
}

/// A crosshair following a moving target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Follow {
    /// Seconds measured.
    pub secs: f32,
    /// RMS wobble of the crosshair relative to the target's motion, degrees: the jitter of the
    /// offset from the target's centre, so a steady lead or lag doesn't count. Taken over the
    /// time [`with_target`](Self::with_target); 0 if there was none.
    pub jitter: f32,
    /// Share of the time the crosshair was within [`WITH_TARGET`] radii of the target's centre,
    /// 0 to 1: how much of the stretch the jitter speaks for.
    pub with_target: f32,
    /// Mean distance from the target's centre, degrees.
    pub error: f32,
    /// Share of the time the crosshair was within the target's radius, 0 to 1.
    pub on_target: f32,
}

/// Measures a crosshair following a target over `frames`. `None` for less than [`MIN_SECS`] of
/// samples.
pub fn follow(frames: &[Tracked]) -> Option<Follow> {
    let secs = span(frames.iter().map(|f| f.t))?;
    let offsets: Vec<(f32, Vec2)> = frames.iter().map(|f| (f.t, wrap(f.aim - f.target))).collect();
    let near = |i: usize| offsets[i].1.length() <= WITH_TARGET * frames[i].radius;
    let (jitter, _) = detrended(&offsets, near);
    let (mut error, mut on_target, mut with_target) = (0.0, 0.0, 0.0);
    for (i, frame) in frames.iter().enumerate() {
        let weight = weight(&offsets, i);
        let distance = offsets[i].1.length();
        error += distance * weight;
        on_target += f32::from(distance <= frame.radius) * weight;
        with_target += f32::from(near(i)) * weight;
    }
    Some(Follow { secs, jitter, with_target: with_target / secs, error: error / secs, on_target: on_target / secs })
}

/// The seconds `times` cover, if that is enough to measure.
fn span(mut times: impl DoubleEndedIterator<Item = f32>) -> Option<f32> {
    let secs = times.next_back()? - times.next()?;
    (secs >= MIN_SECS).then_some(secs)
}

/// The time sample `i` stands for: half the gap to each neighbour, so unevenly spaced samples
/// weigh what they cover and the weights add up to the whole span.
fn weight(points: &[(f32, Vec2)], i: usize) -> f32 {
    let before = if i > 0 { points[i].0 - points[i - 1].0 } else { 0.0 };
    let after = if i + 1 < points.len() { points[i + 1].0 - points[i].0 } else { 0.0 };
    (before + after) * 0.5
}

/// The jitter of `points` (see the module docs) over the samples that are `counted`, and their
/// slow path: per sample, the mean of the straight-line path through the samples over the
/// [`SLOW_SECS`] around it, `None` within half a window of either end, where that window isn't
/// covered. Needs [`MIN_SECS`] of samples.
fn detrended(points: &[(f32, Vec2)], counted: impl Fn(usize) -> bool) -> (f32, Vec<Option<Vec2>>) {
    let half = SLOW_SECS * 0.5;
    let (first, last) = (points[0].0, points[points.len() - 1].0);
    // The integral of the path up to each sample; between samples it is a parabola.
    let mut integral = Vec::with_capacity(points.len());
    let mut sum = Vec2::ZERO;
    integral.push(sum);
    for w in points.windows(2) {
        sum += (w[0].1 + w[1].1) * 0.5 * (w[1].0 - w[0].0);
        integral.push(sum);
    }
    // The integral up to time `t`, which lies in the segment starting at sample `i`.
    let integral_at = |t: f32, i: usize| {
        let (a, b) = (points[i], points[(i + 1).min(points.len() - 1)]);
        let gap = b.0 - a.0;
        if gap <= 0.0 {
            return integral[i];
        }
        let into = t - a.0;
        integral[i] + a.1 * into + (b.1 - a.1) * (into * into / (2.0 * gap))
    };
    let (mut from, mut to) = (0, 0);
    let (mut squares, mut weights) = (0.0, 0.0);
    let slow = (0..points.len())
        .map(|i| {
            let (t, at) = points[i];
            if t - half < first || t + half > last {
                return None;
            }
            while from + 1 < points.len() && points[from + 1].0 <= t - half {
                from += 1;
            }
            while to + 1 < points.len() && points[to + 1].0 <= t + half {
                to += 1;
            }
            let mean = (integral_at(t + half, to) - integral_at(t - half, from)) / SLOW_SECS;
            if counted(i) {
                let weight = weight(points, i);
                squares += at.distance_squared(mean) * weight;
                weights += weight;
            }
            Some(mean)
        })
        .collect();
    (if weights > 0.0 { (squares / weights).sqrt() } else { 0.0 }, slow)
}

/// The aim's angular speed in degrees per second, low-passed at about 7 Hz so single mouse
/// counts and frame-time noise don't read as motion. Feed it every frame; its value at a click
/// tells a shot taken on the move from one taken after stopping.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpeedFilter {
    last: Option<Vec2>,
    speed: f32,
}

impl SpeedFilter {
    /// Feeds the aim `dt` seconds after the previous one; returns the filtered speed.
    pub fn update(&mut self, aim: Vec2, dt: f32) -> f32 {
        if let Some(last) = self.last
            && dt > 0.0
        {
            let raw = wrap(aim - last).length() / dt;
            self.speed += (raw - self.speed) * (1.0 - (-dt / SPEED_TAU).exp());
        }
        self.last = Some(aim);
        self.speed
    }
}

/// The filtered speed at each of `points`.
fn speeds(points: &[(f32, Vec2)]) -> impl Iterator<Item = f32> + '_ {
    let mut filter = SpeedFilter::default();
    let mut before = points.first().map_or(0.0, |p| p.0);
    points.iter().map(move |&(t, aim)| {
        let dt = t - before;
        before = t;
        filter.update(aim, dt)
    })
}

/// The separate moves the crosshair made over `points`: stretches where its filtered speed rose
/// above [`START_SPEED`] until it fell below [`END_SPEED`] again, the same limits the
/// first-flick probe and the weak-spot analysis use for submovements. One clean flick is 1;
/// every correction after it adds one. Adjustments slower than [`START_SPEED`] (8°/s: under
/// half a degree in a tenth of a second) don't count.
pub fn moves(points: &[(f32, Vec2)]) -> u32 {
    let (mut moving, mut count) = (false, 0);
    for speed in speeds(points) {
        if !moving && speed > START_SPEED {
            moving = true;
            count += 1;
        } else if moving && speed < END_SPEED {
            moving = false;
        }
    }
    count
}

/// How a flick from the first of its points to a target went.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flick {
    /// The farthest the crosshair got past the target's centre along the flick, degrees; 0 if
    /// it never passed it.
    pub overshoot: f32,
    /// Whether that was past the target's far edge: the crosshair went through the target and
    /// out the other side (or past it beside it), and had to come back.
    pub overshot: bool,
    /// See [`moves`]: 1 for a flick that arrived in one move.
    pub moves: u32,
}

/// Measures the flick over `points`, from where the aim was when the target (centre `target`,
/// angular radius `radius`) appeared to the click or the end of its time. `None` without a
/// movement to judge: fewer than two points, or an aim that started on the target's centre.
pub fn flick(points: &[(f32, Vec2)], target: Vec2, radius: f32) -> Option<Flick> {
    let start = points.first()?.1;
    let offset = wrap(target - start);
    let distance = offset.length();
    if points.len() < 2 || distance <= f32::EPSILON {
        return None;
    }
    let along = offset / distance;
    let farthest = points.iter().map(|p| wrap(p.1 - start).dot(along)).fold(0.0, f32::max);
    let overshoot = (farthest - distance).max(0.0);
    Some(Flick { overshoot, overshot: overshoot > radius, moves: moves(points) })
}

/// How a crosshair came to rest on a target it flicked to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settle {
    /// Seconds from first touching the target to being steady on it: the first moment of its
    /// last unbroken stay at which its filtered speed was under [`END_SPEED`]. A crosshair that
    /// brakes inside the target settles in the time the braking takes; one that shoots through
    /// and comes back takes that round trip longer.
    pub secs: f32,
    /// Times the crosshair left the target again after first touching it.
    pub exits: u32,
}

/// Measures the arrival over `points` at a still target (centre `target`, angular radius
/// `radius`), up to the moment the stay on it counted. `None` if the crosshair never touched it.
pub fn settle(points: &[(f32, Vec2)], target: Vec2, radius: f32) -> Option<Settle> {
    let (mut arrived, mut steady, mut inside, mut exits) = (None, None, false, 0);
    for (&(t, aim), speed) in points.iter().zip(speeds(points)) {
        let on = wrap(aim - target).length() <= radius;
        if on {
            arrived.get_or_insert(t);
            if speed < END_SPEED {
                steady.get_or_insert(t);
            }
        } else if inside {
            exits += 1;
            steady = None;
        }
        inside = on;
    }
    let arrived = arrived?;
    let end = points.last()?.0;
    Some(Settle { secs: steady.unwrap_or(end) - arrived, exits })
}

/// Millimetres the mouse travels to turn the view by `degrees`, at `deg_per_count` degrees per
/// mouse count ([`Settings::deg_per_count`](crate::settings::Settings::deg_per_count)) and `dpi`
/// counts per inch. Jitter in millimetres is the hand's, whatever the sensitivity.
pub fn mouse_mm(degrees: f32, deg_per_count: f32, dpi: f32) -> f32 {
    degrees / deg_per_count / dpi * MM_PER_INCH
}

#[cfg(test)]
pub(crate) mod tests {
    use std::f32::consts::TAU;

    use super::*;

    pub(crate) const RATES: [f32; 3] = [60.0, 178.0, 500.0];

    /// Minimum-jerk position profile, 0 to 1 over `t` in [0, 1].
    pub(crate) fn min_jerk(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * t * (10.0 - 15.0 * t + 6.0 * t * t)
    }

    /// `secs` of samples at `fps` of the aim `at(t)`.
    pub(crate) fn sampled(fps: f32, secs: f32, at: impl Fn(f32) -> Vec2) -> Vec<(f32, Vec2)> {
        (0..=(secs * fps) as usize)
            .map(|i| {
                let t = i as f32 / fps;
                (t, at(t))
            })
            .collect()
    }

    /// A flick from the origin through `(fraction of the way to target, move seconds, rest
    /// seconds)` legs, each a minimum-jerk move.
    pub(crate) fn legs(fps: f32, target: Vec2, legs: &[(f32, f32, f32)]) -> Vec<(f32, Vec2)> {
        let total: f32 = legs.iter().map(|leg| leg.1 + leg.2).sum();
        sampled(fps, total, |t| {
            let (mut from, mut since) = (0.0, t);
            for &(to, moving, rest) in legs {
                if since <= moving + rest {
                    return target * (from + (to - from) * min_jerk(since / moving));
                }
                (from, since) = (to, since - moving - rest);
            }
            target * from
        })
    }

    /// A target sweeping at 7°/s, as slow following is tested with.
    fn sweep(t: f32) -> Vec2 {
        Vec2::new(-10.0 + 7.0 * t, 1.0)
    }

    fn following(fps: f32, aim: impl Fn(f32) -> Vec2) -> Follow {
        let frames: Vec<Tracked> = sampled(fps, 4.0, aim)
            .into_iter()
            .map(|(t, aim)| Tracked { t, aim, target: sweep(t), radius: 1.0 })
            .collect();
        follow(&frames).unwrap()
    }

    /// A wobble of `amplitude` degrees at `hz` across the motion.
    fn wobble(t: f32, amplitude: f32, hz: f32) -> Vec2 {
        Vec2::new(0.0, amplitude * (TAU * hz * t).sin())
    }

    #[test]
    fn a_smooth_follower_has_no_jitter_whatever_its_lag() {
        for fps in RATES {
            let exact = following(fps, sweep);
            assert!(exact.jitter < 1e-3 && exact.error < 1e-3 && exact.on_target > 0.999, "{fps} fps: {exact:?}");
            // Half a degree behind, all the time: off the centre, but not jittery.
            let behind = following(fps, |t| sweep(t) - Vec2::new(0.5, 0.0));
            assert!(behind.jitter < 1e-3, "{fps} fps: {behind:?}");
            assert!((behind.error - 0.5).abs() < 1e-3 && behind.on_target > 0.999, "{fps} fps: {behind:?}");
            // 1.5° behind is off the 1° target, but still with it.
            let off = following(fps, |t| sweep(t) - Vec2::new(1.5, 0.0));
            assert!(off.on_target < 1e-3 && off.with_target > 0.999, "{fps} fps: {off:?}");
        }
    }

    #[test]
    fn jitter_is_the_fast_wobble_in_degrees_at_any_frame_rate() {
        for fps in RATES {
            // 0.2° of 8 Hz tremor has an RMS of 0.2 / sqrt(2); the slow path averages it out.
            let shaky = following(fps, |t| sweep(t) + wobble(t, 0.2, 8.0));
            assert!((shaky.jitter - 0.2 / 2f32.sqrt()).abs() < 0.012, "{fps} fps: {shaky:?}");
            // Twice the wobble, twice the jitter.
            let worse = following(fps, |t| sweep(t) + wobble(t, 0.4, 8.0));
            assert!((worse.jitter / shaky.jitter - 2.0).abs() < 0.02, "{fps} fps: {worse:?}");
            // A slow weave around the target (0.5 Hz) is steering, not jitter.
            let weaving = following(fps, |t| sweep(t) + wobble(t, 0.2, 0.5));
            assert!(weaving.jitter < 0.015, "{fps} fps: {weaving:?}");
            assert!(weaving.error > 0.1, "{fps} fps: {weaving:?}");
        }
    }

    #[test]
    fn a_crosshair_left_behind_is_not_following() {
        // Never moves: the target passes through it once in 4 s. Its stillness isn't steadiness.
        let parked = following(178.0, |_| sweep(2.0));
        assert!(parked.with_target < 0.25 && parked.on_target < 0.1, "{parked:?}");
        // Shakes (0.35° RMS) for 2 s while the target comes to it, then follows smoothly for 2 s.
        // Only the last 0.4 s of the shaking was with the target: 0.15° RMS over that time, where
        // all of it would be 0.25°.
        let late = following(178.0, |t| if t < 2.0 { sweep(2.0) + wobble(t, 0.5, 8.0) } else { sweep(t) });
        assert!(late.with_target > 0.5 && late.with_target < 0.7, "{late:?}");
        assert!(late.jitter > 0.1 && late.jitter < 0.2, "{late:?}");
    }

    #[test]
    fn stop_and_go_following_is_jittery() {
        for fps in RATES {
            // Stands still for 0.15 s, then jumps to where the target is by now.
            let stepping = following(fps, |t| sweep((t / 0.15).floor() * 0.15));
            assert!(stepping.jitter > 0.15, "{fps} fps: {stepping:?}");
        }
    }

    #[test]
    fn a_still_hand_measures_nothing_and_a_drifting_one_its_drift() {
        for fps in RATES {
            let still = hold(&sampled(fps, 2.0, |_| Vec2::new(12.0, -3.0))).unwrap();
            assert_eq!((still.jitter, still.drift, still.travel), (0.0, 0.0, 0.0));
            // Sliding away at 1°/s for 2 s: 2° travelled; the slow path covers all but a window.
            let sliding = hold(&sampled(fps, 2.0, |t| Vec2::new(12.0 + t, -3.0))).unwrap();
            assert!((sliding.travel - 2.0).abs() < 0.02, "{fps} fps: {sliding:?}");
            assert!((sliding.drift - (2.0 - SLOW_SECS)).abs() < 0.03, "{fps} fps: {sliding:?}");
            assert!(sliding.jitter < 2e-3, "{fps} fps: {sliding:?}");
            // Tremor on the spot: jitter and travel, hardly any drift.
            let shaking = hold(&sampled(fps, 2.0, |t| wobble(t, 0.1, 8.0))).unwrap();
            assert!((shaking.jitter - 0.1 / 2f32.sqrt()).abs() < 0.01, "{fps} fps: {shaking:?}");
            assert!(shaking.drift < 0.03 && shaking.travel > 4.0, "{fps} fps: {shaking:?}");
        }
        // Too short to tell slow from fast.
        assert_eq!(hold(&sampled(240.0, 0.3, |_| Vec2::ZERO)), None);
        // Across the yaw seam there is no jump.
        let seam = hold(&sampled(240.0, 2.0, |t| wrap(Vec2::new(179.9 + 0.1 * t, 0.0)))).unwrap();
        assert!((seam.travel - 0.2).abs() < 1e-2, "{seam:?}");
    }

    #[test]
    fn an_overshooting_flick_is_detected_and_a_clean_one_is_not() {
        let (target, radius) = (Vec2::new(-12.0, 5.0), 1.0);
        for fps in RATES {
            let clean = flick(&legs(fps, target, &[(1.0, 0.2, 0.3)]), target, radius).unwrap();
            assert!(clean.overshoot < 0.01 && !clean.overshot, "{fps} fps: {clean:?}");
            assert_eq!(clean.moves, 1, "{fps} fps");
            // 20% too far (2.6° of 13°), then back: one correction.
            let over = flick(&legs(fps, target, &[(1.2, 0.2, 0.15), (1.0, 0.12, 0.3)]), target, radius).unwrap();
            assert!((over.overshoot - 2.6).abs() < 0.05 && over.overshot, "{fps} fps: {over:?}");
            assert_eq!(over.moves, 2, "{fps} fps");
            // Past the centre but still inside the target isn't an overshoot.
            let deep = flick(&legs(fps, target, &[(1.05, 0.2, 0.3)]), target, radius).unwrap();
            assert!((deep.overshoot - 0.65).abs() < 0.03 && !deep.overshot, "{fps} fps: {deep:?}");
            // Short, then two more moves to get there.
            let short =
                flick(&legs(fps, target, &[(0.8, 0.2, 0.15), (0.92, 0.1, 0.15), (1.0, 0.1, 0.2)]), target, radius);
            let short = short.unwrap();
            assert!(short.overshoot == 0.0 && !short.overshot, "{fps} fps: {short:?}");
            assert_eq!(short.moves, 3, "{fps} fps");
        }
        assert_eq!(flick(&[(0.0, target), (0.1, target)], target, radius), None);
    }

    #[test]
    fn a_shot_on_the_move_shows_in_the_filtered_speed() {
        let target = Vec2::new(15.0, 0.0);
        for fps in RATES {
            // Clicked in the middle of the move, at its fastest; and 0.2 s after stopping.
            let moving = legs(fps, target, &[(1.0, 0.2, 0.0)]);
            let mid = speeds(&moving[..moving.len() / 2]).last().unwrap();
            assert!(mid > 80.0, "{fps} fps: {mid}");
            let rested = speeds(&legs(fps, target, &[(1.0, 0.2, 0.2)])).last().unwrap();
            assert!(rested < 1.0, "{fps} fps: {rested}");
        }
    }

    #[test]
    fn settling_counts_the_way_back_after_shooting_through() {
        let (target, radius) = (Vec2::new(10.0, 0.0), 1.0);
        for fps in RATES {
            // Brakes inside the target: steady once the braking is over, no exits.
            let clean = settle(&legs(fps, target, &[(1.0, 0.25, 0.5)]), target, radius).unwrap();
            assert!(clean.exits == 0 && clean.secs < 0.15, "{fps} fps: {clean:?}");
            // 2° through the far edge, a pause, then back.
            let through = legs(fps, target, &[(1.3, 0.25, 0.1), (1.0, 0.15, 0.5)]);
            let wild = settle(&through, target, radius).unwrap();
            assert_eq!(wild.exits, 1, "{fps} fps");
            assert!(wild.secs > clean.secs + 0.15 && wild.secs < 0.5, "{fps} fps: {wild:?}");
        }
        // Never reached.
        assert_eq!(settle(&legs(240.0, target, &[(0.5, 0.2, 0.3)]), target, radius), None);
    }

    #[test]
    fn mouse_travel_matches_cm_per_360() {
        let settings = crate::settings::Settings { dpi: 800.0, ..Default::default() };
        let per_count = settings.deg_per_count(0.4);
        let full_turn = mouse_mm(360.0, per_count, settings.dpi);
        assert!((full_turn - settings.cm_per_360(0.4) * 10.0).abs() < 1e-2, "{full_turn} mm");
        // 0.1° of jitter is 0.11 mm of hand at this sensitivity; at half of it the same 0.11 mm
        // of hand is 0.05°.
        let hand = mouse_mm(0.1, per_count, settings.dpi);
        assert!((hand - 0.1134).abs() < 1e-3, "{hand} mm");
        assert!((mouse_mm(0.05, settings.deg_per_count(0.2), settings.dpi) - hand).abs() < 1e-5);
    }
}
