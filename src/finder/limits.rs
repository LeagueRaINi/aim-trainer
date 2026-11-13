//! The finder's foundation: what a player's hand and pad allow, measured, and the usable range
//! of sensitivities that follows. This is the main module of the finder; the design of all its
//! parts is written down here.
//!
//! # Why
//!
//! Aim is near-optimal over a wide range of sensitivities, about 20 to 80 cm/360, and only
//! extremes clearly hurt (Boudaoud, Spjut, Kim, NVIDIA, arXiv 2203.12050; they also threw away
//! the first half of every 10-minute session as adaptation). Scores therefore cannot single out
//! one best value, and 20 s trials mostly measure adaptation. The gain with the best fresh
//! throughput was the most fatiguing over 30 minutes (Ren et al. 2025, PubMed 40560231), while
//! 30 minutes of aim training fatigued the wrist without lowering scores (Forman et al. 2025,
//! PubMed 40117860): whether control fades late is individual and has to be measured. So the
//! finder's honest job is to rule out what does not fit the body, rule out what the hand cannot
//! hold steady (also when tired), prefer the conservative side of what remains, and change
//! nothing without evidence. It never claims an optimum.
//!
//! # 1. Calibration ([`probe`](super::probe), [`checkup`](super::checkup)), about two minutes
//!
//! Four steps, each started from a card and measured from raw mouse counts (`counts / DPI`
//! inches; the DPI setting must be right, and the result stores which was used):
//!
//! 1. **Arm sweep.** Left and right across the pad without lifting, 4 full sweeps. A sweep runs
//!    between two turning points; the hand has turned once it is 1 cm back from its furthest
//!    point; the first leg (from wherever the mouse lay) is not a sweep. Result: the median, cm.
//! 2. **Wrist sweep.** The same with the forearm planted: wrist and fingers only.
//! 3. **Hold.** The crosshair on a still dot for 8 s after a 2 s lead-in. The dot has the
//!    angular size of the assumed head (below). Result: the standard deviation of the
//!    crosshair's offset around its own mean, both axes pooled, converted to millimetres of
//!    hand travel (`degrees * cm_per_360 * 10 / 360`).
//! 4. **Follow.** The same dot swinging slowly from side to side (a sine, +-15 mm of hand
//!    travel, 8 s there and back, so at most 12 mm/s for the hand at any sensitivity; it starts
//!    at rest under the crosshair and swings wider over the lead-in) for 16 s after the
//!    lead-in. Result: the standard deviation of the *vertical* offset only, in mm.
//!    Along the dot's path the error is mostly pursuit lag, which is not the hand's noise.
//!
//! Hand noise is the larger of 3 and 4 (a mouse at rest on a pad often does not move at all, so
//! the follow usually decides). Assumption: this noise is a property of the hand in millimetres
//! and is the same at other sensitivities. It is measured at one sensitivity, with visual
//! feedback at that gain, so this is a first-order model, and the page says so.
//!
//! # 2. Limits and the range (this module)
//!
//! On the cm/360 axis (more centimetres = slower):
//!
//! - **Pad, the slow limit:** a 180° turn must fit in one sweep: `cm/360 <= 2 * sweep_cm`.
//! - **Hand noise, the fast limit:** [`NOISE_SDS`] = 2 standard deviations of noise must fit
//!   inside a head's angular radius, i.e. the wobble stays on the head about 95% of the time
//!   (one axis, normal noise): `cm/360 >= 36 * 2 * noise_mm / head_angle`. The head is a sphere
//!   of [`HEAD_RADIUS_M`] = 0.14 m (the trainer's own Head level target, a generous Valorant
//!   head) at [`HEAD_DISTANCE_M`] = 25 m (a common mid-range duel): 0.32°. Both are assumptions
//!   and are shown with the result. 0.10 mm of noise gives 22 cm/360, 0.20 mm gives 45.
//! - **Tired check:** when it has measured a clear loss of control, the fast limit is
//!   multiplied by that loss (section 3).
//! - **Wrist reach** is not a limit: at a given cm/360 the wrist sweep turns
//!   `360 * wrist_cm / cm_per_360` degrees. Covering the whole view reads as wrist-led aiming,
//!   under 60% of it as arm-led, between as a hybrid (community rules of thumb put wrist aimers
//!   at 20-32, hybrids at 30-45 and arm aimers at 45-70 cm/360, which matches a 7 to 8 cm wrist
//!   sweep). This is reported as a description of the geometry, never a verdict.
//!
//! The range is everything between the two limits. If they cross, the range is empty and is
//! reported as empty, with both limits; nothing is patched up and no pick is made.
//!
//! **Verdict and pick.** Inside the range the current sensitivity is kept: nothing measurable
//! is gained by moving on the plateau, and re-adapting costs. Outside it, one conservative pick:
//! errors scale with gain and travel is the only cost of going slower, so it sits on the slower
//! side, two thirds of the way (log scale) from the fast end to the slow end, where the slow
//! end is also capped by the wrist still covering [`WRIST_COVER_DEG`] = 30° (+-15° around the
//! crosshair, the corrections good crosshair placement leaves) and by the study plateau's 80
//! cm/360, and the fast end is raised to the plateau's 20, wherever those caps leave room.
//!
//! # 3. Tired check ([`tired`](super::tired)), about a minute
//!
//! A fixed battery that stresses fine control, not speed, at the current sensitivity: the hold
//! and the follow of the calibration, then 30 s of small Microflick targets (fixed size and
//! layout, no decay, no time pressure) of which only the first clicks' mean distance from the
//! target centre is used, in mm of hand travel. The check's number is its control error, the
//! geometric mean of hand noise and click error. The player marks each check fresh or tired
//! (after a match or a long session); every saved check is a row of the log. Once there are 3
//! of each at (within 10% of) the current sensitivity, the tired ones are compared with the
//! fresh ones on the logarithm of the control error with Welch's t test at the 1% level. Only
//! a clear drop moves the fast limit, by the measured ratio. Tested on synthetic players: with
//! no real loss and 20% scatter between checks, at most 2% of histories of 8 pairs ever show a
//! drop; a real 50% loss is found within 8 pairs in most (smaller losses need more checks, or
//! checks that scatter less).
//!
//! # 4. Monitor ([`monitor`](super::monitor))
//!
//! After every practice run of a flick drill, a summary from its recorded flicks
//! ([`analysis::flicks_in`](crate::analysis::flicks_in)) is appended to the monitor's log:
//! sensitivity, cm/360, flicks, median first-flick landing ratio, mean click bias, mean click
//! distance from the centre, corrections per kill, misses per flick, score. Time into the
//! session is worked out from the timestamps (runs less than 20 minutes apart are one session).
//!
//! The monitor looks at the unbroken block of runs at the current sensitivity (the latest 60 of
//! them) and may suggest ONE step of 5%. Silence is the default; a pattern must pass all of:
//!
//! - **minimum data:** 20 runs, 500 flicks, 4 separate (UTC) days at this sensitivity;
//! - **an effect that matters:** first flicks at least 5% long, or first clicks at least 0.15
//!   target radii past the centre (overshoot, slower; clicks *short* of the centre mean
//!   nothing, that is clicking as the crosshair enters the target); first flicks at least 12%
//!   short (undershoot, faster: a first flick a little short, finished by a correction, is how
//!   practised aiming looks, so the threshold is wider); at least 1.0 corrections per kill
//!   while first flicks are not more than 4% short (shaky, slower: short flicks need their
//!   corrections; the owner's logs show 0.0 to 0.3 on Gridshot, Microflick and Head level, and
//!   median landings within 3% of the target, so these levels are far from a practised
//!   player's normal); click error at least 10% larger in the late half of sessions of 15
//!   minutes or more, relative to each drill's own average, over at least 6 such sessions
//!   (fade, slower);
//! - **clear of the player's own noise:** besides reaching the threshold, the mean must stand
//!   at least 4 standard errors of the run-to-run scatter above *half* the threshold (and at
//!   least the 1% t value for the number of runs), so noise around a small real bias cannot
//!   carry the average over it;
//! - **on separate days:** the pattern shows in each of the latest 4 days' averages and in at
//!   least 80% of all days.
//!
//! Patterns asking for opposite steps cancel. A step is only suggested inside the measured
//! range: none without a calibration, none when the sensitivity is already outside the range
//! (the range's verdict comes first), cut to the limit when it would cross it. After a change
//! the new sensitivity starts from nothing; once it has its own minimum, the runs since (minus
//! the first 5, adaptation) are compared with the block before on click error and on score,
//! each run relative to its drill's average and with days as the unit (Welch's t over the daily
//! averages, 1% level): clearly more precise without clearly lower scores (or clearly higher
//! scores) helped; clearly less precise, or clearly lower scores without a gain in precision,
//! did not, and going back is suggested; anything else is said as it is. Tested on synthetic
//! players with run and day scatter, looked at after every run of 60 days: at most 1 in 100
//! players without a pattern (or with one below the thresholds) ever sees a suggestion; a 10%
//! overshoot is reported to nearly all within 12 days, never before the minimum; a change
//! without any real difference is called helpful or harmful in about 2 of 100 cases.
//!
//! What it cannot do: runs at one sensitivity say nothing about how another would have gone.
//! It suggests a step to try and checks it afterwards; it does not compute an optimum.
//!
//! # 5. Long comparison ([`compare`](super::compare))
//!
//! Two candidates (the current sensitivity and one more, offered inside the range) on 5 to 10
//! days. Each day: 4 blocks of 2 minutes in ABBA order (BAAB every other day), each block 45 s
//! unscored adaptation then 75 s scored plain Microflick with fixed settings. Per complete day
//! and candidate: mean click error (first clicks' distance from the centre, degrees) and
//! accuracy. The daily differences are tested against zero (paired t, 1% level for the number
//! of days) from day 5; a candidate wins when its click error is clearly lower, unless accuracy
//! clearly says the opposite (then: a trade-off, no winner); accuracy alone names no winner,
//! as a second way to win would be a second chance for noise at each of the six looks. After
//! day 10 without a clear difference the verdict is "no measurable difference: keep what you
//! have". Tested: two equal candidates get a winner in under 5% of comparisons; a real 20%
//! difference is found in most.
//!
//! # 6. Quick finders
//!
//! The blind A/B and score-model finders stay as a first look; their maths is unchanged. What
//! they recommend is moved to the nearest sensitivity inside the calibrated range, and their
//! result says that short trials mostly measure adaptation.
//!
//! # Files (the player's data, in the working directory)
//!
//! - `aim_trainer_calibration.ron`: `(latest: Some((unix_time, dpi, cm_per_360, sweep_cm,
//!   wrist_cm, hold_mm, follow_mm)))`, the latest calibration ([`CalibrationStore`]).
//! - `aim_trainer_tired.csv`: one row per saved tired check:
//!   `unix_time,state,cm_per_360,hold_mm,follow_mm,click_mm,clicks,accuracy`, `state` being
//!   `fresh` or `tired`.
//! - `aim_trainer_monitor.csv`: one row per practice run of a flick drill, keyed by the run's
//!   history timestamp:
//!   `unix_time,drill,kind,sens,cm_per_360,flicks,landing,bias,click_deg,corrections,misses,score`
//!   (empty where a run had too few flicks for a value).
//! - `aim_trainer_comparison.ron`: the running long comparison
//!   ([`CompareStore`](super::compare::CompareStore)): both candidates' cm/360 and every scored
//!   block (timestamp, side, click error, accuracy, clicks).
//!
//! The tired check's flick part and the comparison's blocks are runs with
//! [`RunPurpose::FinderTrial`](crate::drill::RunPurpose): like the quick finders' trials they
//! are logged in the history as finder trials, never as practice, so they touch no drill's
//! scores, averages or replays. Sweeps, hold and follow are not runs at all.

use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::drill::kinds::head_level;
use crate::persist::{LoadReport, Persisted, load_ron};
use crate::settings::Settings;

const CM_PER_INCH: f32 = 2.54;
/// Radius of a head, metres: the trainer's own Head level target, so "a head" means the same
/// thing on every screen.
pub const HEAD_RADIUS_M: f32 = head_level::DEF.base_radius;
/// The engagement distance the steadiness limit is worked out for, metres.
pub const HEAD_DISTANCE_M: f32 = 25.0;
/// Standard deviations of hand noise that must fit inside the head's radius: within 2 SD the
/// crosshair's wobble stays on the head about 95% of the time (one axis, normal noise).
pub const NOISE_SDS: f32 = 2.0;
/// The turn that must fit in one sweep without lifting, degrees.
pub const TURN_DEG: f32 = 180.0;
/// The angle the wrist and fingers alone should still cover (side to side) at the picked
/// sensitivity, degrees: +-15 around the crosshair, the corrections good crosshair placement
/// leaves. Only the pick respects it; arm aimers play slower than this by choice.
pub const WRIST_COVER_DEG: f32 = 30.0;
/// Where a study measured near-optimal aiming, cm/360 (Boudaoud, Spjut, Kim; arXiv 2203.12050).
pub const PLATEAU: (f32, f32) = (20.0, 80.0);
/// The middle half of Valorant professionals, cm/360 (community statistics, not a study).
pub const PROS: (f32, f32) = (47.0, 71.0);
/// Where in the range the pick sits, from its fast end (0) to its slow end (1) on a log scale.
const PICK_POSITION: f32 = 2.0 / 3.0;
/// Share of the view's width the wrist sweep must cover to call the aim wrist-led, and below
/// which it is arm-led.
const WRIST_STYLE: f32 = 1.0;
const ARM_STYLE: f32 = 0.6;

/// Mouse counts as centimetres of hand travel.
pub fn counts_to_cm(counts: f32, dpi: f32) -> f32 {
    counts / dpi * CM_PER_INCH
}

/// Millimetres of hand travel per degree of view rotation at a sensitivity.
pub fn mm_per_degree(cm_per_360: f32) -> f32 {
    cm_per_360 * 10.0 / 360.0
}

/// The sensitivity (in the current yaw and DPI) with this cm/360.
pub fn sens_for(settings: &Settings, cm_per_360: f32) -> f32 {
    settings.sens * settings.cm_per_360(settings.sens) / cm_per_360
}

/// Angular radius of the assumed head at the assumed distance, degrees.
pub fn head_angle() -> f32 {
    (HEAD_RADIUS_M / HEAD_DISTANCE_M).atan().to_degrees()
}

/// The slowest sensitivity (largest cm/360) at which [`TURN_DEG`] fits in one sweep.
pub fn pad_limit(sweep_cm: f32) -> f32 {
    sweep_cm * 360.0 / TURN_DEG
}

/// The fastest sensitivity (smallest cm/360) at which [`NOISE_SDS`] of hand noise still fit
/// inside the head: `noise_deg = noise_mm / mm_per_degree` must stay within [`head_angle`].
pub fn noise_limit(noise_mm: f32) -> f32 {
    NOISE_SDS * noise_mm * 36.0 / head_angle()
}

/// The slowest sensitivity at which the wrist sweep still covers [`WRIST_COVER_DEG`].
pub fn wrist_limit(wrist_cm: f32) -> f32 {
    wrist_cm * 360.0 / WRIST_COVER_DEG
}

/// Degrees of view the wrist sweep turns at a sensitivity.
pub fn wrist_cover(wrist_cm: f32, cm_per_360: f32) -> f32 {
    360.0 * wrist_cm / cm_per_360
}

/// Which joints a sensitivity leaves the aiming to, read off the wrist sweep. A description of
/// the geometry, not a verdict on how anyone should aim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// The wrist and fingers alone sweep the whole view.
    Wrist,
    /// They cover most of it; wide flicks need the forearm.
    Hybrid,
    /// They cover less than [`ARM_STYLE`] of it; most flicks come from the arm.
    Arm,
}

impl Style {
    /// From the share of the view's width (`hfov` degrees) the wrist sweep covers.
    pub fn of(wrist_cm: f32, cm_per_360: f32, hfov: f32) -> Self {
        match wrist_cover(wrist_cm, cm_per_360) / hfov {
            share if share >= WRIST_STYLE => Style::Wrist,
            share if share >= ARM_STYLE => Style::Hybrid,
            _ => Style::Arm,
        }
    }
}

/// One finished calibration: what the hand and the pad allow, in physical units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    /// When it was finished (unix seconds).
    pub unix_time: u64,
    /// The DPI setting the counts were converted with: the centimetres are only as right as it.
    pub dpi: f32,
    /// The sensitivity the steadiness was measured at.
    pub cm_per_360: f32,
    /// Comfortable sweep without lifting, pad and arm: median of the sweeps, cm.
    pub sweep_cm: f32,
    /// Comfortable sweep with the forearm planted, wrist and fingers: median, cm.
    pub wrist_cm: f32,
    /// Hand noise while holding on a still target: per-axis standard deviation, mm.
    pub hold_mm: f32,
    /// Hand noise across the path while following a slow target: standard deviation, mm.
    pub follow_mm: f32,
}

impl Calibration {
    /// The hand noise the fast limit is worked out from: the larger of the two measurements.
    pub fn noise_mm(&self) -> f32 {
        self.hold_mm.max(self.follow_mm)
    }
}

/// The saved calibration (`aim_trainer_calibration.ron`).
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalibrationStore {
    pub latest: Option<Calibration>,
}

impl Persisted for CalibrationStore {
    const PATH: &'static str = "aim_trainer_calibration.ron";
}

impl CalibrationStore {
    pub fn load() -> (Self, LoadReport) {
        Self::load_from(Path::new(Self::PATH))
    }

    fn load_from(path: &Path) -> (Self, LoadReport) {
        let (mut store, report): (Self, _) = load_ron(path);
        // A hand-edited file must not turn into NaN limits on the page.
        store.latest = store.latest.filter(|c| {
            [c.dpi, c.cm_per_360, c.sweep_cm, c.wrist_cm].iter().all(|v| v.is_finite() && *v > 0.0)
                && [c.hold_mm, c.follow_mm].iter().all(|v| v.is_finite() && *v >= 0.0)
        });
        (store, report)
    }
}

/// What caps the slow side of the pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlowCap {
    /// The pad: a 180° turn must fit in a sweep.
    Pad,
    /// The wrist must still cover [`WRIST_COVER_DEG`].
    Wrist,
    /// The slow end of the study's plateau: beyond it there is no evidence it stays as good.
    Plateau,
}

/// Where the current sensitivity sits relative to the range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    /// Inside the range: keep it.
    Keep,
    /// Faster than the fast limit: the hand's noise is bigger than a head.
    TooFast { pick: f32, cap: SlowCap },
    /// Slower than the slow limit: a 180° turn doesn't fit on the pad.
    TooSlow { pick: f32, cap: SlowCap },
    /// The limits cross: no sensitivity satisfies both. Reported as it is, never patched up.
    Empty,
}

/// A player's limits on the cm/360 axis (larger = slower).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Fastest usable sensitivity (smallest cm/360): hand noise, after the tired check's loss.
    pub fast: f32,
    /// The same limit when fresh, before the tired check moved it.
    pub fresh_fast: f32,
    /// Slowest usable sensitivity (largest cm/360): the pad.
    pub slow: f32,
    /// Slowest sensitivity at which the wrist still covers [`WRIST_COVER_DEG`]: a cap on the
    /// pick, not a limit of the range.
    pub wrist: f32,
}

impl Limits {
    /// The limits a calibration implies. `tired_loss` is the factor by which control error
    /// grows when tired (1.25 = 25% more), once the tired check has measured one: hand noise
    /// then needs that much more room, so the fast limit moves by the same factor.
    pub fn new(calibration: &Calibration, tired_loss: Option<f32>) -> Self {
        let fresh_fast = noise_limit(calibration.noise_mm());
        Self {
            fast: fresh_fast * tired_loss.map_or(1.0, |loss| loss.max(1.0)),
            fresh_fast,
            slow: pad_limit(calibration.sweep_cm),
            wrist: wrist_limit(calibration.wrist_cm),
        }
    }

    /// The usable range (fast end, slow end) in cm/360; `None` when the limits cross.
    pub fn range(&self) -> Option<(f32, f32)> {
        (self.fast <= self.slow).then_some((self.fast, self.slow))
    }

    /// `cm_per_360` moved into the range if it is outside; `None` when there is no range.
    pub fn clamp(&self, cm_per_360: f32) -> Option<f32> {
        self.range().map(|(fast, slow)| cm_per_360.clamp(fast, slow))
    }

    /// The conservative pick and what caps its slow side: [`PICK_POSITION`] of the way (on a
    /// log scale) from the fast end to the slow end of the range, where the slow end is also
    /// capped by the wrist's reach and the study plateau, and the fast end raised to the
    /// plateau's, wherever those leave room. `None` when there is no range.
    pub fn pick(&self) -> Option<(f32, SlowCap)> {
        let (fast, slow) = self.range()?;
        // Not below the plateau's fast end; with a pad too small for that, the slower half.
        let low = if PLATEAU.0 < slow { fast.max(PLATEAU.0) } else { fast.max(slow * 0.5) };
        let (mut high, mut cap) = (slow, SlowCap::Pad);
        for (limit, which) in [(self.wrist, SlowCap::Wrist), (PLATEAU.1, SlowCap::Plateau)] {
            if limit < high && limit > low {
                (high, cap) = (limit, which);
            }
        }
        Some((low * (high / low).powf(PICK_POSITION), cap))
    }

    /// The sensitivity `sens` moved to the nearest one inside the range, and whether that moved
    /// it: what a quick finder may recommend. Unchanged when there is no range.
    pub fn keep_inside(&self, sens: f32, settings: &Settings) -> (f32, bool) {
        let cm = settings.cm_per_360(sens);
        match self.clamp(cm) {
            Some(inside) if (inside / cm).ln().abs() > 1e-3 => (sens_for(settings, inside), true),
            _ => (sens, false),
        }
    }

    /// Whether the current sensitivity fits, and where to go if it doesn't.
    pub fn verdict(&self, current: f32) -> Verdict {
        let (Some((fast, slow)), Some((pick, cap))) = (self.range(), self.pick()) else { return Verdict::Empty };
        if current < fast {
            Verdict::TooFast { pick, cap }
        } else if current > slow {
            Verdict::TooSlow { pick, cap }
        } else {
            Verdict::Keep
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A calibration measured at 40 cm/360 and 800 DPI.
    pub(crate) fn calibration(sweep_cm: f32, wrist_cm: f32, noise_mm: f32) -> Calibration {
        Calibration {
            unix_time: 1_793_000_000,
            dpi: 800.0,
            cm_per_360: 40.0,
            sweep_cm,
            wrist_cm,
            hold_mm: noise_mm * 0.4,
            follow_mm: noise_mm,
        }
    }

    #[test]
    fn counts_become_centimetres() {
        // 800 counts at 800 DPI are one inch.
        assert!((counts_to_cm(800.0, 800.0) - 2.54).abs() < 1e-6);
        assert!((counts_to_cm(-1600.0, 400.0) + 10.16).abs() < 1e-5);
        // 36 cm/360 is exactly 1 mm per degree.
        assert!((mm_per_degree(36.0) - 1.0).abs() < 1e-6);
        // Valorant 0.4 at 800 DPI is 40.82 cm/360; the way back gives the sens again.
        let settings = Settings { sens: 0.4, dpi: 800.0, ..Settings::default() };
        assert!((sens_for(&settings, 40.82) - 0.4).abs() < 1e-4);
        assert!((sens_for(&settings, 2.0 * settings.cm_per_360(0.4)) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn limits_follow_from_the_stated_assumptions() {
        // A 14 cm head radius at 25 m is 0.32 degrees.
        assert!((head_angle() - 0.3209).abs() < 1e-3, "{}", head_angle());
        // A 180° turn is half of a 360: 29 cm of sweep allow up to 58 cm/360.
        assert_eq!(pad_limit(29.0), 58.0);
        // At the noise limit, 2 SD of noise are exactly the head's radius.
        let limit = noise_limit(0.12);
        assert!((NOISE_SDS * 0.12 / mm_per_degree(limit) - head_angle()).abs() < 1e-5);
        assert!((limit - 26.9).abs() < 0.1, "{limit}");
        // Twice the noise needs twice the centimetres.
        assert!((noise_limit(0.24) / limit - 2.0).abs() < 1e-5);
        // At the wrist limit the wrist sweep covers exactly 30 degrees.
        assert!((wrist_cover(7.0, wrist_limit(7.0)) - WRIST_COVER_DEG).abs() < 1e-4);
        assert_eq!(wrist_limit(7.0), 84.0);
    }

    #[test]
    fn style_is_read_off_the_wrist_coverage() {
        // 9 cm of wrist sweep at 24 cm/360 turn 135°: more than Valorant's 103° view.
        assert_eq!(Style::of(9.0, 24.0, 103.0), Style::Wrist);
        // 7.5 cm at 40 cm/360: 67.5°, about two thirds of the view.
        assert_eq!(Style::of(7.5, 40.0, 103.0), Style::Hybrid);
        // 6 cm at 55 cm/360: 39°.
        assert_eq!(Style::of(6.0, 55.0, 103.0), Style::Arm);
    }

    #[test]
    fn a_sensitivity_inside_the_range_is_kept() {
        // A wrist aimer on a small pad: 14 cm of sweep, steady hand.
        let wrist_aimer = Limits::new(&calibration(14.0, 9.0, 0.07), None);
        let (fast, slow) = wrist_aimer.range().unwrap();
        assert!((fast - 15.7).abs() < 0.1 && slow == 28.0, "{fast} to {slow}");
        assert_eq!(wrist_aimer.verdict(24.0), Verdict::Keep);
        // An arm aimer on a big pad.
        let arm_aimer = Limits::new(&calibration(45.0, 6.0, 0.1), None);
        assert_eq!(arm_aimer.range().map(|r| r.1), Some(90.0));
        assert_eq!(arm_aimer.verdict(55.0), Verdict::Keep);
        // The limits themselves are inside.
        assert_eq!(arm_aimer.verdict(90.0), Verdict::Keep);
        assert_eq!(arm_aimer.verdict(arm_aimer.fast), Verdict::Keep);
    }

    #[test]
    fn a_small_pad_bounds_the_slow_side() {
        let limits = Limits::new(&calibration(18.0, 8.0, 0.08), None);
        assert_eq!(limits.slow, 36.0);
        let Verdict::TooSlow { pick, cap } = limits.verdict(45.0) else { panic!("{:?}", limits.verdict(45.0)) };
        assert_eq!(cap, SlowCap::Pad);
        // Two thirds of the way from the plateau's 20 to the pad's 36, on a log scale.
        assert!((pick - 20.0 * (36.0f32 / 20.0).powf(2.0 / 3.0)).abs() < 1e-3, "{pick}");
        assert!(pick > 28.0 && pick < 36.0);
    }

    #[test]
    fn a_shaky_hand_bounds_the_fast_side() {
        let limits = Limits::new(&calibration(40.0, 8.0, 0.25), None);
        let (fast, slow) = limits.range().unwrap();
        assert!((fast - 56.1).abs() < 0.1 && slow == 80.0, "{fast} to {slow}");
        let Verdict::TooFast { pick, cap } = limits.verdict(40.0) else { panic!("{:?}", limits.verdict(40.0)) };
        // Slower side of 56 to 80; the pad and the plateau end at the same place here.
        assert!(pick > 65.0 && pick < 80.0, "{pick}");
        assert_eq!(cap, SlowCap::Pad);
    }

    #[test]
    fn the_pick_respects_wrist_reach_and_plateau() {
        // A huge pad (sweep 60 cm, limit 120) with short wrist reach (5 cm, limit 60).
        let short_reach = Limits::new(&calibration(60.0, 5.0, 0.1), None);
        let (pick, cap) = short_reach.pick().unwrap();
        assert_eq!(cap, SlowCap::Wrist);
        assert!(pick < 60.0 && pick > 22.0, "{pick}");
        // The same pad with a long reach (9 cm, limit 108): the plateau's 80 caps it.
        let long_reach = Limits::new(&calibration(60.0, 9.0, 0.1), None);
        let (pick, cap) = long_reach.pick().unwrap();
        assert_eq!(cap, SlowCap::Plateau);
        assert!(pick < 80.0, "{pick}");
        // A cap below the range's fast end can't apply: the range's own ends are used.
        let shaky = Limits::new(&calibration(60.0, 4.0, 0.3), None);
        assert!(shaky.wrist < shaky.fast);
        let (pick, cap) = shaky.pick().unwrap();
        assert_eq!(cap, SlowCap::Plateau);
        assert!(pick > shaky.fast && pick < 80.0, "{pick}");
        // The pick is always inside the range.
        for limits in [short_reach, long_reach, shaky] {
            let (fast, slow) = limits.range().unwrap();
            let pick = limits.pick().unwrap().0;
            assert!((fast..=slow).contains(&pick));
        }
        // A pad too small for the plateau: the slower half of what there is.
        let tiny = Limits::new(&calibration(8.0, 6.0, 0.01), None);
        let pick = tiny.pick().unwrap().0;
        assert!(pick > 8.0 && pick < 16.0, "{pick}");
    }

    #[test]
    fn an_empty_range_is_reported_with_both_limits() {
        // A shaky hand (fast limit 56) on a small pad (slow limit 40).
        let limits = Limits::new(&calibration(20.0, 7.0, 0.25), None);
        assert!(limits.fast > limits.slow);
        assert_eq!(limits.range(), None);
        assert_eq!(limits.pick(), None);
        assert_eq!(limits.clamp(45.0), None);
        for current in [30.0, 45.0, 70.0] {
            assert_eq!(limits.verdict(current), Verdict::Empty);
        }
    }

    #[test]
    fn tiredness_moves_only_the_fast_limit() {
        let calibration = calibration(29.0, 7.5, 0.12);
        let fresh = Limits::new(&calibration, None);
        let tired = Limits::new(&calibration, Some(1.25));
        assert!((tired.fast / fresh.fast - 1.25).abs() < 1e-5);
        assert_eq!((tired.fresh_fast, tired.slow, tired.wrist), (fresh.fast, fresh.slow, fresh.wrist));
        // A sensitivity that fits fresh but not tired.
        assert_eq!(fresh.verdict(30.0), Verdict::Keep);
        assert!(matches!(tired.verdict(30.0), Verdict::TooFast { .. }));
        // "Better when tired" never loosens the limit.
        assert_eq!(Limits::new(&calibration, Some(0.8)).fast, fresh.fast);
        assert_eq!(tired.clamp(20.0), Some(tired.fast));
        assert_eq!(tired.clamp(100.0), Some(58.0));
    }

    #[test]
    fn a_quick_finders_pick_is_moved_to_the_nearest_end_of_the_range() {
        // Range 26.9 to 58 cm/360; the player is at 0.4 (40.8 cm/360).
        let settings = Settings { sens: 0.4, dpi: 800.0, ..Settings::default() };
        let limits = Limits::new(&calibration(29.0, 7.5, 0.12), None);
        assert_eq!(limits.keep_inside(0.35, &settings), (0.35, false));
        // 0.7 is 23.3 cm/360, too fast: moved to the fast limit.
        let (sens, moved) = limits.keep_inside(0.7, &settings);
        assert!(moved && (settings.cm_per_360(sens) - limits.fast).abs() < 1e-2, "{sens}");
        // 0.25 is 65.3 cm/360, too slow for the pad: moved to 58.
        let (sens, moved) = limits.keep_inside(0.25, &settings);
        assert!(moved && (settings.cm_per_360(sens) - 58.0).abs() < 1e-2, "{sens}");
        // Crossed limits can't hold anything.
        let empty = Limits::new(&calibration(20.0, 7.0, 0.25), None);
        assert_eq!(empty.keep_inside(0.7, &settings), (0.7, false));
    }

    #[test]
    fn broken_calibration_files_are_dropped() {
        use crate::test_util::TempDir;
        let dir = TempDir::new("calibration_load");
        let path = dir.join("calibration.ron");
        let good = CalibrationStore { latest: Some(calibration(29.0, 7.5, 0.12)) };
        std::fs::write(&path, ron::ser::to_string(&good).unwrap()).unwrap();
        assert_eq!(CalibrationStore::load_from(&path).0, good);
        // A value no limit can be worked out from: the calibration counts as not done.
        let text = ron::ser::to_string(&good).unwrap().replace("sweep_cm:29.0", "sweep_cm:NaN");
        assert!(text.contains("NaN"));
        std::fs::write(&path, text).unwrap();
        assert_eq!(CalibrationStore::load_from(&path).0.latest, None);
    }
}
