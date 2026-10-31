//! Where clicks land within their targets, and effective throughput (TPe, ISO 9241-411): a
//! flick skill measure in bits/s that stays put when you trade speed for accuracy or back
//! (Yamanaka & MacKenzie, CHI 2026), unlike a score.
//!
//! **Task-axis frame.** A click at a flick target is described relative to the target centre:
//! `x` along the flick (start aim to target centre; positive = past the centre, an overshoot),
//! `y` across it (positive = counter-clockwise on screen), in degrees, or in target radii.
//!
//! **Throughput.** Only each flick's first click counts (hit or miss), and only for flicks of at
//! least [`MIN_DISTANCE`] degrees. Flicks are grouped into conditions: distance band ([`BANDS`])
//! x target size class (half-octave steps of the target's full angular radius). For each
//! condition with at least [`MIN_GROUP`] clicks:
//!
//! - `sigma_x` = sample standard deviation of `x` (degrees), `We = 4.133 * sigma_x`;
//! - `A` = mean nominal flick distance, `IDe = log2(A / We + 1)`;
//! - `MT` = mean time from the flick's start to its first click, `TP = IDe / MT`.
//!
//! `TPe` is the mean of the conditions' `TP`.

use std::collections::BTreeMap;

use bevy::math::Vec2;

use super::flick_probe::{MIN_DISTANCE, wrap};

/// Flick distance bands, degrees.
pub const BANDS: [(f32, f32); 5] = [(MIN_DISTANCE, 8.0), (8.0, 15.0), (15.0, 25.0), (25.0, 40.0), (40.0, f32::INFINITY)];
/// Clicks a condition needs before its throughput counts.
pub const MIN_GROUP: usize = 8;
/// ISO 9241-411 effective width per standard deviation: `sqrt(2 * pi * e)`, so `We` covers 96%
/// of normally distributed endpoints.
pub const WE_PER_SIGMA: f32 = 4.133;
/// A click within this fraction of the radius from the centre is a centre hit.
pub const CENTRE: f32 = 0.5;

/// The distance band of a flick of `distance` degrees.
pub fn band(distance: f32) -> usize {
    BANDS.iter().rposition(|band| distance >= band.0).unwrap_or(0)
}

/// One click at a flick target. Angles are (yaw, pitch) in degrees, as in
/// [`flick_probe`](super::flick_probe).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Click {
    /// Aim when the flick started: when its target appeared, or at the previous kill if that was
    /// later.
    pub start: Vec2,
    /// Target centre at the click.
    pub target: Vec2,
    /// Angular radius of the target at the click, degrees.
    pub radius: f32,
    /// Angular radius of the target at full size (before any decay): its size class.
    pub size: f32,
    /// Aim at the click.
    pub aim: Vec2,
    /// Seconds from the flick's start to the click.
    pub time: f32,
    pub hit: bool,
    /// The flick's first click. Only these measure the flick; later ones are corrections.
    pub first: bool,
}

/// A (yaw, pitch) offset as an on-screen vector (x right, y up): yaw grows to the left.
fn screen(offset: Vec2) -> Vec2 {
    Vec2::new(-offset.x, offset.y)
}

impl Click {
    /// Flick distance: degrees from the start aim to the target centre.
    pub fn amplitude(&self) -> f32 {
        wrap(self.target - self.start).length()
    }

    /// The click's offset from the target centre in the task-axis frame, degrees (module docs).
    /// `None` for flicks under [`MIN_DISTANCE`], which have no clear direction.
    pub fn task_offset(&self) -> Option<Vec2> {
        let flick = screen(wrap(self.target - self.start));
        let distance = flick.length();
        if distance < MIN_DISTANCE {
            return None;
        }
        let axis = flick / distance;
        let error = screen(wrap(self.aim - self.target));
        Some(Vec2::new(error.dot(axis), axis.perp_dot(error)))
    }

    /// [`task_offset`](Self::task_offset) in target radii: inside the target within 1.
    pub fn normalized(&self) -> Option<Vec2> {
        Some(self.task_offset()? / self.radius.max(1e-4))
    }

    /// Throughput condition: distance band and half-octave size class.
    fn condition(&self) -> (usize, i32) {
        (band(self.amplitude()), (self.size.max(1e-3).log2() * 2.0).round() as i32)
    }
}

/// The clicks that measure flicks (first clicks of long enough flicks), with their
/// [`Click::normalized`] offsets.
pub fn measured(clicks: &[Click]) -> impl Iterator<Item = (&Click, Vec2)> {
    clicks.iter().filter(|c| c.first).filter_map(|c| Some((c, c.normalized()?)))
}

fn mean(values: impl Iterator<Item = f32>) -> Option<f32> {
    let (sum, n) = values.fold((0.0, 0usize), |(sum, n), v| (sum + v, n + 1));
    (n > 0).then(|| sum / n as f32)
}

/// Effective throughput in bits/s (module docs); `None` until a condition has [`MIN_GROUP`]
/// clicks.
pub fn throughput(clicks: &[Click]) -> Option<f32> {
    let mut conditions: BTreeMap<(usize, i32), Vec<&Click>> = BTreeMap::new();
    for (click, _) in measured(clicks) {
        conditions.entry(click.condition()).or_default().push(click);
    }
    mean(conditions.values().filter(|group| group.len() >= MIN_GROUP).filter_map(|group| {
        let n = group.len() as f32;
        let xs: Vec<f32> = group.iter().map(|c| c.task_offset().map_or(0.0, |offset| offset.x)).collect();
        let mean_x = xs.iter().sum::<f32>() / n;
        let sigma = (xs.iter().map(|x| (x - mean_x).powi(2)).sum::<f32>() / (n - 1.0)).sqrt();
        let width = WE_PER_SIGMA * sigma;
        let amplitude = group.iter().map(|c| c.amplitude()).sum::<f32>() / n;
        let time = group.iter().map(|c| c.time).sum::<f32>() / n;
        (width > 0.0 && time > 0.0).then(|| (amplitude / width + 1.0).log2() / time)
    }))
}

/// Mean along-axis click offset in target radii: > 0 clicks past the centre, < 0 short of it.
pub fn bias(clicks: &[Click]) -> Option<f32> {
    mean(measured(clicks).map(|(_, offset)| offset.x))
}

/// Mean sideways click offset in target radii.
pub fn off_axis(clicks: &[Click]) -> Option<f32> {
    mean(measured(clicks).map(|(_, offset)| offset.y.abs()))
}

/// Fraction of clicks within [`CENTRE`] radii of the target centre.
pub fn centre_hits(clicks: &[Click]) -> Option<f32> {
    mean(measured(clicks).map(|(_, offset)| (offset.length() <= CENTRE) as u8 as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::flick_probe::screen_offset;

    /// A first click of a flick from the origin `distance` degrees towards `angle` (on screen),
    /// landing at `offset` (task-axis degrees) from the centre of a target of `radius`.
    fn click(angle: f32, distance: f32, offset: Vec2, radius: f32, time: f32) -> Click {
        let target = screen_offset(angle, distance);
        let aim = target + screen_offset(angle, offset.x) + screen_offset(angle + 90.0, offset.y);
        Click { start: Vec2::ZERO, target, radius, size: radius, aim, time, hit: offset.length() <= radius, first: true }
    }

    #[test]
    fn offsets_rotate_into_the_task_axis_for_every_direction() {
        for sector in 0..8 {
            let angle = sector as f32 * 45.0;
            // Started away from the origin, across the yaw wrap-around.
            let mut c = click(angle, 20.0, Vec2::new(0.5, -0.25), 1.0, 0.3);
            let shift = Vec2::new(170.0, 3.0);
            c.start += shift;
            c.target = wrap(c.target + shift);
            c.aim = wrap(c.aim + shift);
            let offset = c.task_offset().unwrap();
            assert!((offset - Vec2::new(0.5, -0.25)).length() < 1e-3, "{angle}: {offset}");
            assert!((c.normalized().unwrap() - Vec2::new(0.5, -0.25)).length() < 1e-3);
            assert!((c.amplitude() - 20.0).abs() < 1e-3);
        }
        // Counter-clockwise of a flick to the right is up (pitch grows upward).
        let right = Click { start: Vec2::ZERO, target: Vec2::new(-10.0, 0.0), aim: Vec2::new(-10.0, 0.4), ..click(0.0, 10.0, Vec2::ZERO, 1.0, 0.2) };
        assert!((right.task_offset().unwrap() - Vec2::new(0.0, 0.4)).length() < 1e-5);
        // Too short to have a direction.
        assert_eq!(click(0.0, MIN_DISTANCE - 0.5, Vec2::ZERO, 1.0, 0.2).task_offset(), None);
    }

    #[test]
    fn bias_off_axis_and_centre_hits() {
        let radius = 0.8;
        let clicks = [
            click(0.0, 20.0, Vec2::new(0.4, 0.0), radius, 0.3),   // past, centre
            click(90.0, 20.0, Vec2::new(0.8, -0.4), radius, 0.3), // past, edge
            click(200.0, 20.0, Vec2::new(-0.3, 0.2), radius, 0.3), // short, centre
            click(300.0, 20.0, Vec2::new(1.2, 0.4), radius, 0.3), // past, miss
        ];
        // (0.5 + 1.0 - 0.375 + 1.5) / 4 radii past the centre: overshooting at the click.
        assert!((bias(&clicks).unwrap() - 0.65625).abs() < 1e-4);
        assert!((off_axis(&clicks).unwrap() - (0.0 + 0.5 + 0.25 + 0.5) / 4.0).abs() < 1e-4);
        assert!((centre_hits(&clicks).unwrap() - 0.5).abs() < 1e-6);
        let short: Vec<Click> = clicks.iter().map(|c| click(0.0, 20.0, -c.task_offset().unwrap(), radius, 0.3)).collect();
        assert!(bias(&short).unwrap() < 0.0);
        // Only first clicks count.
        let later = clicks.map(|c| Click { first: false, ..c });
        assert_eq!(bias(&later), None);
        assert_eq!(throughput(&later), None);
    }

    #[test]
    fn throughput_matches_the_iso_formula() {
        // One condition: 20 degrees, radius 1, x = -2..2 with a known spread, MT 0.4 s.
        let xs = [-2.0, -1.0, -1.0, 0.0, 0.0, 1.0, 1.0, 2.0];
        let clicks: Vec<Click> = xs.iter().map(|&x| click(30.0, 20.0, Vec2::new(x * 0.1, 0.0), 1.0, 0.4)).collect();
        let sigma = (12.0f32 * 0.01 / 7.0).sqrt();
        let width = 4.133 * sigma;
        let expected = (20.0 / width + 1.0).log2() / 0.4;
        assert!((throughput(&clicks).unwrap() - expected).abs() < 1e-3, "{:?} {expected}", throughput(&clicks));

        // A second condition (further band) averages in; a third with 7 clicks doesn't count.
        let far: Vec<Click> = xs.iter().map(|&x| click(120.0, 30.0, Vec2::new(x * 0.2, 0.1), 1.0, 0.5)).collect();
        let few: Vec<Click> = xs[..7].iter().map(|&x| click(0.0, 5.0, Vec2::new(x * 0.3, 0.0), 1.0, 0.2)).collect();
        let far_tp = (30.0 / (2.0 * width) + 1.0).log2() / 0.5;
        let all: Vec<Click> = [clicks, far, few].concat();
        assert!((throughput(&all).unwrap() - (expected + far_tp) / 2.0).abs() < 1e-3);
        assert_eq!(throughput(&all[..7]), None);
    }

    /// Flicks of a player on the Fitts' line `MT = IDe / 5 bits/s` who clicks with spread
    /// `sigma` (degrees) at targets of radius 1 degree: 3 distances x 16 clicks.
    fn fitts_player(sigma: f32, seed: u64) -> Vec<Click> {
        let mut rng = fastrand::Rng::with_seed(seed);
        // Evenly spread along-axis errors, scaled to a sample standard deviation of exactly 1.
        let z: Vec<f32> = (0..16).map(|i| i as f32 - 7.5).collect();
        let sd = (z.iter().map(|z| z * z).sum::<f32>() / 15.0).sqrt();
        let mut clicks = Vec::new();
        for (d, distance) in [6.0, 12.0, 20.0].into_iter().enumerate() {
            let id = (distance / (WE_PER_SIGMA * sigma) + 1.0f32).log2();
            for (i, z) in z.iter().enumerate() {
                let angle = (d * 16 + i) as f32 * 47.0;
                // Movement times scatter +-5% around the line.
                let time = id / 5.0 * (0.95 + 0.1 * rng.f32());
                clicks.push(click(angle, distance, Vec2::new(z / sd * sigma, 0.0), 1.0, time));
            }
        }
        clicks
    }

    /// Flick score (as `ScoreModel::Flick`) of back-to-back clicks: accuracy-weighted kills per
    /// minute.
    fn flick_score(clicks: &[Click]) -> f32 {
        let hits = clicks.iter().filter(|c| c.hit).count() as f32;
        let minutes = clicks.iter().map(|c| c.time).sum::<f32>() / 60.0;
        hits * 100.0 * (hits / clicks.len() as f32) / minutes
    }

    #[test]
    fn throughput_ignores_the_speed_accuracy_trade_off() {
        // Same underlying skill: one player rushes (wide spread, often outside the target), the
        // other is careful (tight spread, slower).
        let reckless = fitts_player(1.2, 1);
        let careful = fitts_player(0.25, 2);
        let (fast, slow) = (throughput(&reckless).unwrap(), throughput(&careful).unwrap());
        assert!((fast / slow - 1.0).abs() < 0.05, "{fast} vs {slow}");
        assert!((fast - 5.0).abs() < 0.3, "{fast}");
        let (fast_score, slow_score) = (flick_score(&reckless), flick_score(&careful));
        assert!((fast_score / slow_score - 1.0).abs() > 0.2, "{fast_score} vs {slow_score}");
    }
}
