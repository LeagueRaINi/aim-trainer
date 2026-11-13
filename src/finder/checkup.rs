//! The two guided procedures built from [`probe`](super::probe) measurements: the calibration
//! (both sweeps, hold, follow) and the tired check (hold, follow, precise flicks). A [`Checkup`]
//! holds what has been measured so far; each step is started from its card on the finder screen
//! and comes back there. Nothing is saved until the player accepts the result.

use bevy::prelude::*;

use super::limits::{Calibration, mm_per_degree};
use super::probe::{ProbeResult, ProbeTask, Reach};
use super::tired::{Freshness, TiredEntry};
use crate::drill::{DrillConfig, DrillKind, DrillSpec, Run, RunPurpose, RunStats};
use crate::settings::Settings;

/// Length of the tired check's flick part, seconds.
pub const FLICK_SECS: f32 = 30.0;
/// First clicks the flick part must have for its click error to count.
pub const MIN_CLICKS: usize = 8;
/// The history name of the tired check's flick part (logged as a finder trial, not practice).
const FLICK_NAME: &str = "Tired check";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckupKind {
    Calibration,
    Tired(Freshness),
}

/// One step of a checkup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Probe(ProbeTask),
    /// 30 s of small Microflick targets without time pressure, played as a run.
    Flicks,
}

const CALIBRATION_STEPS: [Step; 4] = [
    Step::Probe(ProbeTask::Sweep(Reach::Arm)),
    Step::Probe(ProbeTask::Sweep(Reach::Wrist)),
    Step::Probe(ProbeTask::Hold),
    Step::Probe(ProbeTask::Follow),
];
const TIRED_STEPS: [Step; 3] = [Step::Probe(ProbeTask::Hold), Step::Probe(ProbeTask::Follow), Step::Flicks];

/// What the flick part measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlickPart {
    /// Mean distance of the first clicks from the target centre, degrees.
    pub click_deg: f32,
    pub clicks: u32,
    pub accuracy: f32,
}

impl FlickPart {
    /// `None` with fewer than [`MIN_CLICKS`] measured first clicks.
    pub fn from_stats(stats: &RunStats) -> Option<Self> {
        let offsets: Vec<f32> =
            stats.clicks.iter().filter(|c| c.first).filter_map(|c| Some(c.task_offset()?.length())).collect();
        (offsets.len() >= MIN_CLICKS).then(|| Self {
            click_deg: offsets.iter().sum::<f32>() / offsets.len() as f32,
            clicks: offsets.len() as u32,
            accuracy: stats.accuracy(),
        })
    }
}

/// A calibration or tired check in progress.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Checkup {
    pub kind: CheckupKind,
    /// Steps finished.
    done: usize,
    pub sweep_cm: Option<f32>,
    pub wrist_cm: Option<f32>,
    pub hold_mm: Option<f32>,
    pub follow_mm: Option<f32>,
    pub flicks: Option<FlickPart>,
    /// The flick part was played but had too few clicks to measure.
    pub flicks_too_few: bool,
}

impl Checkup {
    pub fn new(kind: CheckupKind) -> Self {
        Self {
            kind,
            done: 0,
            sweep_cm: None,
            wrist_cm: None,
            hold_mm: None,
            follow_mm: None,
            flicks: None,
            flicks_too_few: false,
        }
    }

    pub fn steps(&self) -> &'static [Step] {
        match self.kind {
            CheckupKind::Calibration => &CALIBRATION_STEPS,
            CheckupKind::Tired(_) => &TIRED_STEPS,
        }
    }

    /// Steps finished.
    pub fn done(&self) -> usize {
        self.done
    }

    /// The step to do next; `None` once all are done.
    pub fn step(&self) -> Option<Step> {
        self.steps().get(self.done).copied()
    }

    /// Stores a finished probe, if it is the step that was due.
    pub fn record_probe(&mut self, task: ProbeTask, result: ProbeResult) {
        if self.step() != Some(Step::Probe(task)) {
            return;
        }
        match (task, result) {
            (ProbeTask::Sweep(Reach::Arm), ProbeResult::Sweep(cm)) => self.sweep_cm = Some(cm),
            (ProbeTask::Sweep(Reach::Wrist), ProbeResult::Sweep(cm)) => self.wrist_cm = Some(cm),
            (ProbeTask::Hold, ProbeResult::Steady(mm)) => self.hold_mm = Some(mm),
            (ProbeTask::Follow, ProbeResult::Steady(mm)) => self.follow_mm = Some(mm),
            _ => return,
        }
        self.done += 1;
    }

    /// The run of the flick part: small Microflick targets close to the crosshair, with a fixed
    /// layout and size (the global target size and decay don't apply) so every check is the same
    /// task.
    pub fn flick_run(settings: &Settings) -> Run {
        let config = DrillConfig {
            size: 0.5,
            yaw_range: 20.0,
            pitch_range: 8.0,
            min_flick: 5.0,
            ..DrillConfig::defaults(DrillKind::Microflick)
        };
        let drill = DrillSpec { name: FLICK_NAME.to_string(), config };
        let title = "Tired check: precise flicks, take your time".to_string();
        Run::new(drill, settings.sens, FLICK_SECS, RunPurpose::FinderTrial, title)
    }

    /// Stores the finished flick part, if it was due and had enough clicks.
    pub fn record_flicks(&mut self, stats: &RunStats) {
        if self.step() != Some(Step::Flicks) {
            return;
        }
        self.flicks = FlickPart::from_stats(stats);
        self.flicks_too_few = self.flicks.is_none();
        self.done += usize::from(self.flicks.is_some());
    }

    /// The finished calibration, measured at the current sensitivity and DPI.
    pub fn calibration(&self, settings: &Settings, unix_time: u64) -> Option<Calibration> {
        if self.kind != CheckupKind::Calibration {
            return None;
        }
        Some(Calibration {
            unix_time,
            dpi: settings.dpi,
            cm_per_360: settings.cm_per_360(settings.sens),
            sweep_cm: self.sweep_cm?,
            wrist_cm: self.wrist_cm?,
            hold_mm: self.hold_mm?,
            follow_mm: self.follow_mm?,
        })
    }

    /// The finished tired check as a log entry.
    pub fn tired_entry(&self, settings: &Settings, unix_time: u64) -> Option<TiredEntry> {
        let CheckupKind::Tired(state) = self.kind else { return None };
        let flicks = self.flicks?;
        let cm_per_360 = settings.cm_per_360(settings.sens);
        Some(TiredEntry {
            unix_time,
            state,
            cm_per_360,
            hold_mm: self.hold_mm?,
            follow_mm: self.follow_mm?,
            click_mm: flicks.click_deg * mm_per_degree(cm_per_360),
            clicks: flicks.clicks,
            accuracy: flicks.accuracy,
        })
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;
    use crate::metrics::throughput::Click;

    /// Stats of a flick part whose first clicks land `off` degrees past the centre.
    fn flick_stats(clicks: usize, off: f32) -> RunStats {
        let click = Click {
            start: Vec2::ZERO,
            target: Vec2::new(-10.0, 0.0),
            radius: 0.57,
            size: 0.57,
            aim: Vec2::new(-10.0 - off, 0.0),
            time: 0.5,
            hit: true,
            first: true,
        };
        RunStats {
            kind: DrillKind::Microflick,
            drill: FLICK_NAME.to_string(),
            sens: 0.4,
            cm_per_360: 40.82,
            duration: FLICK_SECS,
            hits: clicks as u32,
            shots: clicks as u32 + 2,
            expired: 0,
            ttk_total: clicks as f32 * 0.6,
            best_ttk: 0.4,
            on_target: 0.0,
            landings: Vec::new(),
            // A correction click after each first one doesn't count.
            clicks: (0..clicks)
                .flat_map(|_| [click, Click { first: false, aim: Vec2::new(-3.0, 0.0), ..click }])
                .collect(),
            rating: None,
            rounds_hit: 0,
        }
    }

    #[test]
    fn a_calibration_takes_its_four_steps_in_order() {
        let settings = Settings { sens: 0.4, dpi: 800.0, ..Settings::default() };
        let mut checkup = Checkup::new(CheckupKind::Calibration);
        assert_eq!(checkup.step(), Some(Step::Probe(ProbeTask::Sweep(Reach::Arm))));
        // A result for another step (a stale probe) changes nothing.
        checkup.record_probe(ProbeTask::Hold, ProbeResult::Steady(0.1));
        checkup.record_probe(ProbeTask::Sweep(Reach::Arm), ProbeResult::Steady(0.1));
        assert_eq!((checkup.done(), checkup.hold_mm), (0, None));
        assert_eq!(checkup.calibration(&settings, 5), None);
        checkup.record_probe(ProbeTask::Sweep(Reach::Arm), ProbeResult::Sweep(29.0));
        checkup.record_probe(ProbeTask::Sweep(Reach::Wrist), ProbeResult::Sweep(7.5));
        checkup.record_probe(ProbeTask::Hold, ProbeResult::Steady(0.04));
        assert_eq!(checkup.step(), Some(Step::Probe(ProbeTask::Follow)));
        checkup.record_probe(ProbeTask::Follow, ProbeResult::Steady(0.12));
        assert_eq!((checkup.done(), checkup.step()), (4, None));
        let calibration = checkup.calibration(&settings, 5).unwrap();
        assert_eq!(
            (calibration.sweep_cm, calibration.wrist_cm, calibration.hold_mm, calibration.follow_mm),
            (29.0, 7.5, 0.04, 0.12)
        );
        assert_eq!((calibration.unix_time, calibration.dpi), (5, 800.0));
        assert!((calibration.cm_per_360 - 40.82).abs() < 0.01);
        assert_eq!(checkup.tired_entry(&settings, 5), None);
    }

    #[test]
    fn a_tired_check_measures_click_error_in_hand_millimetres() {
        let settings = Settings { sens: 0.4, dpi: 800.0, ..Settings::default() };
        let mut checkup = Checkup::new(CheckupKind::Tired(Freshness::Tired));
        checkup.record_probe(ProbeTask::Hold, ProbeResult::Steady(0.03));
        checkup.record_probe(ProbeTask::Follow, ProbeResult::Steady(0.11));
        assert_eq!(checkup.step(), Some(Step::Flicks));
        // Too few clicks: the part has to be played again.
        checkup.record_flicks(&flick_stats(MIN_CLICKS - 1, 0.3));
        assert!(checkup.flicks_too_few && checkup.step() == Some(Step::Flicks));
        checkup.record_flicks(&flick_stats(20, 0.3));
        assert!(!checkup.flicks_too_few && checkup.step().is_none());
        let entry = checkup.tired_entry(&settings, 9).unwrap();
        assert_eq!((entry.state, entry.clicks, entry.hold_mm, entry.follow_mm), (Freshness::Tired, 20, 0.03, 0.11));
        // 0.3 degrees at 40.82 cm/360 (1.134 mm per degree).
        assert!((entry.click_mm - 0.3 * 40.82 / 36.0).abs() < 1e-3, "{}", entry.click_mm);
        assert!((entry.accuracy - 20.0 / 22.0).abs() < 1e-5);
        assert_eq!(checkup.calibration(&settings, 9), None);
    }

    #[test]
    fn the_flick_part_is_a_fixed_task() {
        let mut settings = Settings::default();
        let plain = Checkup::flick_run(&settings);
        settings.target_scale = 0.6;
        settings.targets.lifetime = 1.2;
        let scaled = Checkup::flick_run(&settings);
        assert_eq!(plain.drill, scaled.drill, "global size and decay don't change the battery");
        assert_eq!((plain.purpose, plain.duration), (RunPurpose::FinderTrial, FLICK_SECS));
        assert_eq!(plain.drill.name, FLICK_NAME);
    }
}
