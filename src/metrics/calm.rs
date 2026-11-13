//! The Calm aim test's result: what each of its four parts measured, and the 0-100 score each
//! gets for it. Calm aim is control, not speed: nothing here is faster-is-better.
//!
//! **Scores.** Every measure has a [`Band`]: full points at or better than a "calm" value, none
//! at or beyond a "tense" value, a straight line between. A part's score is the weighted mean of
//! its measures' points ([`CalmResult::measures`] lists them with their weights), rounded to a
//! whole number; the test's score is the plain mean of the four part scores. So a score can
//! always be read back: which measure lost how many points is in the list. A part without
//! enough to judge ([`CalmResult::judged`]) scores 0.
//!
//! **Where the bands come from.** The calm ends are what a practised hand does when it takes its
//! time, the tense ends what the same hand does under speed pressure or what makes the measure
//! pointless (off the head altogether). They were set from the owner's replays of October 2026
//! (800 dpi, Valorant 0.31 to 0.40), read with the same code as [`steadiness`](super::steadiness):
//! - deliberate single shots (Microflick, Head level, Reaction, Peripheral: 223 shots) were
//!   fired at a crosshair speed under 10°/s 97% of the time (90th percentile 2.5 to 9.3°/s per
//!   run); Gridshot shots (2000) at over 10°/s 66 to 98% of the time (median 16 to 45°/s);
//! - in those flick runs 12 to 30% of flicks went past the target's far edge, a flick took 1.2
//!   to 1.3 moves and 1.03 to 1.12 shots, and the crosshair came to rest 56 to 91 ms (median)
//!   after touching the target, 0.25 to 0.29 s at the 90th percentile, leaving it again 0.06 to
//!   0.31 times per target;
//! - waiting for a Reaction cue (14 waits of 1 to 2.3 s) the crosshair drifted 0.03° (median)
//!   to 0.23° (90th percentile) and travelled 0.04 to 0.25°/s, with a jitter under 0.015°;
//! - following a 20°/s strafing dot between its turns, the jitter was 0.2 to 0.9° (median
//!   0.35°). There is no slow following in those replays; at the test's 7°/s a stop-and-go hand
//!   that steps every 0.15 s makes 0.3°, a glide with a 30% speed ripple 0.05°.
//!
//! These formulas become history scores: change a band or a weight and old results no longer
//! compare. The measures themselves are logged, so a result can be rescored, but shown scores
//! must stay as they were.

/// The four parts of the test, in the order they are played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Smoothness,
    Stillness,
    Flick,
    Settle,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::Smoothness, Part::Stillness, Part::Flick, Part::Settle];

    pub fn title(self) -> &'static str {
        match self {
            Part::Smoothness => "Smoothness",
            Part::Stillness => "Stillness",
            Part::Flick => "Composed flick",
            Part::Settle => "Settle",
        }
    }

    /// What to do in the part, as its banner and the HUD say it.
    pub fn task(self) -> &'static str {
        match self {
            Part::Smoothness => "Follow the dot. Glide with it: no steps, no jerks.",
            Part::Stillness => "Rest the crosshair in the ring and wait. Click the target once it appears.",
            Part::Flick => "One target at a time. Take your time: stop on it, then one shot.",
            Part::Settle => "No clicking. Flick onto the target and stay on it until it breaks.",
        }
    }

    /// What the part is about, for the results.
    pub fn measures(self) -> &'static str {
        match self {
            Part::Smoothness => "gliding with a slow target",
            Part::Stillness => "holding an angle without fidgeting",
            Part::Flick => "flicking without overshoot or panic shots",
            Part::Settle => "coming to rest on a target",
        }
    }
}

/// The fewest clicked or settled targets a part is judged from: rates over one or two targets
/// say nothing.
pub const MIN_TARGETS: u32 = 3;
/// The least waiting (seconds, over all waits) Stillness is judged from.
pub const MIN_WAIT_SECS: f32 = 2.0;
/// A shot fired while the crosshair's filtered speed was above this (degrees per second) was
/// fired on the move, see the module docs.
pub const MOVING_SHOT: f32 = 10.0;

/// How a measure turns into points: 100 at `calm` or better, 0 at `tense` or worse, linear in
/// between. `calm` is the smaller of the two for measures where less is better.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub calm: f32,
    pub tense: f32,
}

impl Band {
    /// 0 to 100; a measure that isn't a number (nothing measured) gets 0.
    pub fn points(self, value: f32) -> f32 {
        let share = (self.tense - value) / (self.tense - self.calm);
        if share.is_nan() { 0.0 } else { share.clamp(0.0, 1.0) * 100.0 }
    }
}

/// What a measure's number is, for showing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Degrees,
    DegreesPerSecond,
    /// A share from 0 to 1, shown as a percentage.
    Share,
    /// Seconds, shown as milliseconds.
    Seconds,
    /// A plain number per target, wait or kill.
    Count,
}

/// One scored measure of a part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measure {
    pub name: &'static str,
    pub value: f32,
    pub unit: Unit,
    pub band: Band,
    /// Its share of the part's score; a part's weights add up to 1.
    pub weight: f32,
    /// The share of its points that count, 0 to 1: Smoothness' jitter only speaks for the time
    /// the crosshair was with the target. 1 for every other measure.
    pub counted: f32,
    /// What it is, in a sentence.
    pub meaning: &'static str,
    /// What to practise when this is what costs the most.
    pub advice: &'static str,
}

impl Measure {
    /// 0 to 100.
    pub fn points(&self) -> f32 {
        self.band.points(self.value) * self.counted
    }

    /// Points of the part's score (out of 100) this measure cost.
    pub fn lost(&self) -> f32 {
        self.weight * (100.0 - self.points())
    }
}

/// Following the slow dot ([`steadiness::follow`](super::steadiness::follow)).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Smoothness {
    /// Fast wobble relative to the dot's motion, degrees RMS.
    pub jitter: f32,
    /// Share of the time the crosshair was near enough the dot for its jitter to mean anything.
    pub with_target: f32,
    /// Mean distance from the dot's centre, degrees.
    pub error: f32,
    /// Share of the time on the dot.
    pub on_target: f32,
}

/// Waiting on the mark, then the small move to the target that appears.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stillness {
    /// Waits that ran their time.
    pub waits: u32,
    /// Seconds waited over all of them.
    pub wait_secs: f32,
    /// Mean over the waits of how far the crosshair's slow path got from where it rested, degrees.
    pub drift: f32,
    /// Degrees the crosshair travelled while waiting, over all waits.
    pub travel: f32,
    /// Shots fired with no target up.
    pub early_shots: u32,
    /// Targets clicked.
    pub targets: u32,
    /// Moves it took to get onto those targets, over all of them (1 each at best).
    pub moves: u32,
    /// Targets that timed out.
    pub expired: u32,
}

/// One target at a time, clicked.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flicks {
    pub kills: u32,
    /// Targets that timed out.
    pub expired: u32,
    /// Every shot of the part.
    pub shots: u32,
    /// Shots fired at a crosshair speed above [`MOVING_SHOT`].
    pub moving_shots: u32,
    /// Killed targets the crosshair first went past the far edge of.
    pub overshot: u32,
    /// Moves after the first on the way to the killed targets, over all of them.
    pub corrections: u32,
}

/// Flick to a target and stay on it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settling {
    /// Targets stayed on for long enough.
    pub targets: u32,
    /// Targets that timed out.
    pub expired: u32,
    /// Median seconds from touching a target to being steady on it.
    pub secs: f32,
    /// Times the crosshair left a target again after touching it, over all of them.
    pub exits: u32,
}

/// What one Calm aim run measured.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CalmResult {
    pub smoothness: Smoothness,
    pub stillness: Stillness,
    pub flicks: Flicks,
    pub settling: Settling,
}

/// `total / count`, or NaN (no points) without a count.
fn per(total: f32, count: f32) -> f32 {
    if count > 0.0 { total / count } else { f32::NAN }
}

impl CalmResult {
    /// The scored measures of `part`, heaviest first.
    pub fn measures(&self, part: Part) -> Vec<Measure> {
        let measure = |name, value, unit, (calm, tense), weight, meaning, advice| Measure {
            name,
            value,
            unit,
            band: Band { calm, tense },
            weight,
            counted: 1.0,
            meaning,
            advice,
        };
        match part {
            Part::Smoothness => {
                let s = &self.smoothness;
                let jitter = measure(
                    "Jitter",
                    s.jitter,
                    Unit::Degrees,
                    // A glide with a slight speed ripple, to a hand that stops and starts.
                    (0.04, 0.30),
                    0.5,
                    "how much the crosshair wobbled around the dot's own motion (stepping and shaking, not a steady lag)",
                    "Your crosshair moved in steps instead of gliding. Loosen your grip, let the arm carry the motion, and play Smooth tracking at a speed you can follow without correcting.",
                );
                vec![
                    Measure { counted: s.with_target, ..jitter },
                    measure(
                        "Off centre",
                        s.error,
                        Unit::Degrees,
                        // A quarter of the head-size dot's 1.15° radius, to off the dot on average.
                        (0.30, 1.15),
                        0.25,
                        "the crosshair's mean distance from the dot's centre",
                        "You were following beside the dot rather than on it. Play Smooth tracking slowly enough to stay on its centre, then raise the speed.",
                    ),
                    measure(
                        "On target",
                        s.on_target,
                        Unit::Share,
                        (0.90, 0.30),
                        0.25,
                        "the share of the time the crosshair was on the dot",
                        "You lost the dot for much of the part. Play Smooth tracking slowly enough to stay on it, then raise the speed.",
                    ),
                ]
            },
            Part::Stillness => {
                let s = &self.stillness;
                vec![
                    measure(
                        "Drift",
                        s.drift,
                        Unit::Degrees,
                        // The owner's 75th percentile at rest, to a head's radius off the angle.
                        (0.15, 1.15),
                        0.3,
                        "how far the crosshair wandered from where it rested, on average per wait",
                        "Your crosshair wandered off the angle while you waited. Rest the weight of your hand on the pad and stop steering once you are on the spot.",
                    ),
                    measure(
                        "Fidgeting",
                        per(s.travel, s.wait_secs),
                        Unit::DegreesPerSecond,
                        // The owner's 90th percentile at rest is 0.25°/s; 2°/s is a hand that
                        // never stops adjusting.
                        (0.20, 2.0),
                        0.3,
                        "how much the crosshair moved per second of waiting, in any direction",
                        "You kept adjusting while there was nothing to adjust to. Once you are on the spot, take the tension out of your fingers and leave the mouse alone until something appears.",
                    ),
                    measure(
                        "Moves per target",
                        per(s.moves as f32, s.targets as f32),
                        Unit::Count,
                        (1.1, 2.0),
                        0.25,
                        "the separate moves it took to get onto a target that appeared a few degrees away (1 is one clean move)",
                        "The small move onto the target took several attempts. Play Microflick with a small range and make each flick one move that ends on the target.",
                    ),
                    measure(
                        "Early shots",
                        per(s.early_shots as f32, s.waits as f32),
                        Unit::Count,
                        // None, to one on every other wait.
                        (0.0, 0.5),
                        0.15,
                        "shots fired with no target up, per wait",
                        "You fired before there was anything to shoot. Keep your finger resting on the button, not pressing into it, until you see the target.",
                    ),
                ]
            },
            Part::Flick => {
                let f = &self.flicks;
                let kills = f.kills as f32;
                vec![
                    measure(
                        "Overshoot",
                        per(f.overshot as f32, kills),
                        Unit::Share,
                        // The owner's best under speed pressure is 12%; half of all flicks is a
                        // hand that throws the mouse and catches it.
                        (0.10, 0.50),
                        0.3,
                        "the share of flicks that went past the target's far edge before coming back",
                        "You flew past the target and had to come back. Slow the last third of the flick: play Microflick and aim to arrive, not to get there first.",
                    ),
                    measure(
                        "Corrections",
                        per(f.corrections as f32, kills),
                        Unit::Count,
                        // 0.2 to 0.3 per flick at the owner's speed; one on every flick is tense.
                        (0.15, 1.0),
                        0.3,
                        "extra moves per flick after the first one",
                        "Your flicks ended beside the target and needed a second move. Play Microflick slowly enough that the first move lands, then speed up.",
                    ),
                    measure(
                        "Shots on the move",
                        per(f.moving_shots as f32, f.shots as f32),
                        Unit::Share,
                        // 3% of the owner's deliberate shots, 66 to 98% of the Gridshot ones.
                        (0.05, 0.60),
                        0.2,
                        "the share of shots fired while the crosshair was still moving faster than 10°/s",
                        "You clicked while the crosshair was still moving. Stop on the target, then shoot: Head level at an easy pace is the drill for it.",
                    ),
                    measure(
                        "Shots per kill",
                        per(f.shots as f32, kills),
                        Unit::Count,
                        (1.05, 1.5),
                        0.2,
                        "shots fired per target killed (1 is one shot each)",
                        "You needed extra shots. Make each click a decision: only when the crosshair is on the target and at rest.",
                    ),
                ]
            },
            Part::Settle => {
                let s = &self.settling;
                vec![
                    measure(
                        "Settle time",
                        s.secs,
                        Unit::Seconds,
                        // The owner's median is 56 to 91 ms; 0.4 s is shooting through, coming
                        // back and steadying.
                        (0.08, 0.40),
                        0.6,
                        "the median time from touching a target to being steady on it",
                        "Your crosshair took long to come to rest after arriving. Brake before the target, not on it: play Switch tracking, where staying on is what counts.",
                    ),
                    measure(
                        "Left the target",
                        per(s.exits as f32, s.targets as f32),
                        Unit::Count,
                        // 0.06 to 0.31 per target for the owner; once per target is tense.
                        (0.10, 1.0),
                        0.4,
                        "times per target the crosshair left it again after touching it",
                        "You touched the target and slid off it again. Arrive slower and let the hand go soft the moment you are on: play Switch tracking.",
                    ),
                ]
            },
        }
    }

    /// Whether `part` has enough behind it to be scored.
    pub fn judged(&self, part: Part) -> bool {
        match part {
            Part::Smoothness => self.smoothness.with_target > 0.0,
            Part::Stillness => self.stillness.wait_secs >= MIN_WAIT_SECS,
            Part::Flick => self.flicks.kills >= MIN_TARGETS,
            Part::Settle => self.settling.targets >= MIN_TARGETS,
        }
    }

    /// The part's score: a whole number from 0 to 100, see the module docs.
    pub fn score(&self, part: Part) -> f32 {
        if !self.judged(part) {
            return 0.0;
        }
        self.measures(part).iter().map(|measure| measure.weight * measure.points()).sum::<f32>().round()
    }

    /// The test's score: the plain mean of the four part scores, so it moves in quarter points.
    pub fn overall(&self) -> f32 {
        Part::ALL.iter().map(|&part| self.score(part)).sum::<f32>() / Part::ALL.len() as f32
    }

    /// The part with the lowest score (the earliest of equals).
    pub fn weakest(&self) -> Part {
        Part::ALL.into_iter().min_by(|&a, &b| self.score(a).total_cmp(&self.score(b))).unwrap_or(Part::Smoothness)
    }

    /// The measure that cost `part` the most points; `None` if none cost any.
    pub fn costliest(&self, part: Part) -> Option<Measure> {
        self.measures(part)
            .into_iter()
            .filter(|measure| measure.lost() > 0.0)
            .max_by(|a, b| a.lost().total_cmp(&b.lost()))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A run every measure of which is at its calm end.
    pub(crate) fn calm() -> CalmResult {
        CalmResult {
            smoothness: Smoothness { jitter: 0.03, with_target: 1.0, error: 0.2, on_target: 0.95 },
            stillness: Stillness {
                waits: 5,
                wait_secs: 12.0,
                drift: 0.05,
                travel: 1.0,
                early_shots: 0,
                targets: 5,
                moves: 5,
                expired: 0,
            },
            flicks: Flicks { kills: 12, expired: 0, shots: 12, moving_shots: 0, overshot: 1, corrections: 1 },
            settling: Settling { targets: 10, expired: 0, secs: 0.06, exits: 1 },
        }
    }

    /// A run every measure of which is at its tense end.
    pub(crate) fn tense() -> CalmResult {
        CalmResult {
            smoothness: Smoothness { jitter: 0.4, with_target: 1.0, error: 1.5, on_target: 0.25 },
            stillness: Stillness {
                waits: 5,
                wait_secs: 12.0,
                drift: 1.5,
                travel: 30.0,
                early_shots: 3,
                targets: 5,
                moves: 11,
                expired: 0,
            },
            flicks: Flicks { kills: 10, expired: 2, shots: 16, moving_shots: 12, overshot: 6, corrections: 11 },
            settling: Settling { targets: 8, expired: 1, secs: 0.45, exits: 9 },
        }
    }

    #[test]
    fn calm_scores_100_tense_0_and_the_overall_is_the_mean() {
        let (calm, tense) = (calm(), tense());
        for part in Part::ALL {
            assert_eq!(calm.score(part), 100.0, "{part:?}");
            assert_eq!(tense.score(part), 0.0, "{part:?}");
            let weights: f32 = calm.measures(part).iter().map(|measure| measure.weight).sum();
            assert!((weights - 1.0).abs() < 1e-6, "{part:?}: weights add up to {weights}");
        }
        assert_eq!((calm.overall(), tense.overall()), (100.0, 0.0));
        // One part tense: three quarters.
        let mixed = CalmResult { settling: tense.settling, ..calm };
        assert_eq!(mixed.overall(), 75.0);
        assert_eq!(mixed.weakest(), Part::Settle);
        assert_eq!(mixed.costliest(Part::Settle).unwrap().name, "Settle time");
        assert_eq!(calm.costliest(Part::Settle), None);
    }

    /// Every part's score only ever falls as any one of its measures gets worse, and does fall
    /// across the measure's band.
    #[test]
    fn part_scores_are_monotonic_in_every_measure() {
        // (part, apply the measure at `x` from 0 (calm end) to 1 (tense end) to a calm run)
        type Worsen = fn(&mut CalmResult, f32);
        let cases: [(Part, Worsen); 12] = [
            (Part::Smoothness, |r, x| r.smoothness.jitter = 0.5 * x),
            (Part::Smoothness, |r, x| r.smoothness.error = 2.0 * x),
            (Part::Smoothness, |r, x| r.smoothness.on_target = 1.0 - x),
            (Part::Stillness, |r, x| r.stillness.drift = 2.0 * x),
            (Part::Stillness, |r, x| r.stillness.travel = 40.0 * x),
            (Part::Stillness, |r, x| r.stillness.moves = 5 + (10.0 * x) as u32),
            (Part::Stillness, |r, x| r.stillness.early_shots = (6.0 * x) as u32),
            (Part::Flick, |r, x| r.flicks.overshot = (12.0 * x) as u32),
            (Part::Flick, |r, x| r.flicks.corrections = (24.0 * x) as u32),
            (Part::Flick, |r, x| {
                r.flicks.shots = 24;
                r.flicks.moving_shots = (24.0 * x) as u32;
            }),
            (Part::Settle, |r, x| r.settling.secs = 0.6 * x),
            (Part::Settle, |r, x| r.settling.exits = (20.0 * x) as u32),
        ];
        for (index, (part, worsen)) in cases.into_iter().enumerate() {
            let scores: Vec<f32> = (0..=40)
                .map(|step| {
                    let mut result = calm();
                    worsen(&mut result, step as f32 / 40.0);
                    result.score(part)
                })
                .collect();
            assert!(scores.windows(2).all(|pair| pair[1] <= pair[0]), "case {index} ({part:?}): {scores:?}");
            assert!(scores[40] < scores[0], "case {index} ({part:?}) never costs anything: {scores:?}");
        }
        // More shots for the same kills.
        let mut wasteful = calm();
        wasteful.flicks.shots = 18;
        assert!(wasteful.score(Part::Flick) < calm().score(Part::Flick));
    }

    /// From the hand to the score, through the steadiness measures: a smooth follower scores
    /// 100 and a shaky one clearly less; a still wait scores 100 and a drifting one clearly less.
    #[test]
    fn synthetic_hands_score_as_they_move() {
        use std::f32::consts::TAU;

        use bevy::math::Vec2;

        use crate::metrics::steadiness::{self, Tracked};

        const FPS: f32 = 178.0;
        // Six seconds on a 7°/s dot of head size, with `shake` degrees of 8 Hz tremor.
        let following = |shake: f32| {
            let frames: Vec<Tracked> = (0..=(6.0 * FPS) as usize)
                .map(|i| {
                    let t = i as f32 / FPS;
                    let target = Vec2::new(7.0 * t, 0.0);
                    Tracked { t, aim: target + Vec2::new(0.0, shake * (TAU * 8.0 * t).sin()), target, radius: 1.15 }
                })
                .collect();
            let follow = steadiness::follow(&frames).unwrap();
            let smoothness = Smoothness {
                jitter: follow.jitter,
                with_target: follow.with_target,
                error: follow.error,
                on_target: follow.on_target,
            };
            CalmResult { smoothness, ..calm() }.score(Part::Smoothness)
        };
        assert_eq!(following(0.0), 100.0);
        let (shaky, shakier) = (following(0.25), following(0.4));
        assert!(shaky < 80.0 && shakier < shaky - 15.0, "{shaky}, {shakier}");

        // A three-second wait, sliding away at `slide` degrees per second.
        let waiting = |slide: f32| {
            let points: Vec<(f32, Vec2)> = (0..=(3.0 * FPS) as usize)
                .map(|i| {
                    let t = i as f32 / FPS;
                    (t, Vec2::new(4.0 + slide * t, 0.0))
                })
                .collect();
            let hold = steadiness::hold(&points).unwrap();
            let stillness = Stillness {
                waits: 1,
                wait_secs: hold.secs,
                drift: hold.drift,
                travel: hold.travel,
                ..calm().stillness
            };
            CalmResult { stillness, ..calm() }.score(Part::Stillness)
        };
        assert_eq!(waiting(0.0), 100.0);
        let (drifting, sliding) = (waiting(0.2), waiting(0.4));
        assert!(drifting < 95.0 && sliding < 75.0, "{drifting}, {sliding}");
    }

    #[test]
    fn jitter_only_counts_for_the_time_with_the_target() {
        // A crosshair that never moved has no jitter, and no claim to smoothness either.
        let parked = CalmResult {
            smoothness: Smoothness { jitter: 0.0, with_target: 0.1, error: 9.0, on_target: 0.03 },
            ..calm()
        };
        assert_eq!(parked.score(Part::Smoothness), 5.0);
        let absent = CalmResult { smoothness: Smoothness::default(), ..calm() };
        assert!(!absent.judged(Part::Smoothness));
        assert_eq!(absent.score(Part::Smoothness), 0.0);
    }

    #[test]
    fn too_little_to_judge_scores_nothing() {
        let mut result = calm();
        result.flicks = Flicks { kills: 2, shots: 2, ..Flicks::default() };
        result.settling = Settling { targets: 2, secs: 0.05, ..Settling::default() };
        result.stillness = Stillness { waits: 1, wait_secs: 1.5, ..Stillness::default() };
        for part in [Part::Stillness, Part::Flick, Part::Settle] {
            assert!(!result.judged(part), "{part:?}");
            assert_eq!(result.score(part), 0.0, "{part:?}");
        }
        assert_eq!(result.overall(), 25.0);
        // Waited calmly but never clicked a target: the move onto it can't have been clean.
        let waited = CalmResult { stillness: Stillness { targets: 0, moves: 0, ..calm().stillness }, ..calm() };
        assert_eq!(waited.score(Part::Stillness), 75.0);
        // No measure is ever not a number in the score.
        assert_eq!(CalmResult::default().overall(), 0.0);
    }

    #[test]
    fn bands_work_in_either_direction() {
        let less = Band { calm: 1.0, tense: 3.0 };
        assert_eq!((less.points(0.5), less.points(2.0), less.points(4.0)), (100.0, 50.0, 0.0));
        let more = Band { calm: 0.9, tense: 0.3 };
        assert_eq!((more.points(1.0), more.points(0.3), more.points(0.0)), (100.0, 0.0, 0.0));
        assert!((more.points(0.6) - 50.0).abs() < 1e-4);
        assert_eq!(less.points(f32::NAN), 0.0);
    }
}
