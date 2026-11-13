//! The live measuring stage of the calibration and the tired check: sweeps (how far the hand
//! travels without lifting) and steadiness (how much it wobbles on a still and on a slow
//! target), read from raw mouse counts. See [`limits`](super::limits) for what the numbers are
//! used for.
//!
//! A [`Probe`] is one measurement in progress. It lives in `AppState::Finder` (it has no
//! [`Run`](crate::drill::Run): nothing is scored, timed against a drill length or written to
//! the history) and brings its own ready gate, countdown and pause: Escape or losing focus
//! stops it, and nothing advances until it is resumed. A paused hold or follow goes on exactly
//! where it stopped, after the same countdown as a drill; a paused sweep keeps the sweeps
//! already counted and starts the one in progress again, since the hand has moved meanwhile.
//! All timing uses [`Time`] deltas.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::math::DVec2;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, WindowFocused};

use super::checkup::Checkup;
use super::limits::{HEAD_DISTANCE_M, HEAD_RADIUS_M, counts_to_cm, mm_per_degree};
use crate::AppState;
use crate::camera::{Look, apply_look};
use crate::drill::{FacesEye, KeepRunsOnFocusLoss, TargetAssets, facing_eye};
use crate::metrics::median;
use crate::settings::Settings;

/// Full sweeps a sweep measurement takes (the median of them is the result).
pub const SWEEPS: usize = 4;
/// Travel back from the furthest point (cm) that ends a sweep: more than the wobble of a hand
/// turning around, far less than any sweep.
const REVERSAL_CM: f32 = 1.0;
/// Unscored seconds at the start of a hold or follow: getting on the target and settling.
pub const LEAD_SECS: f32 = 2.0;
/// Scored seconds of a hold.
pub const HOLD_SECS: f32 = 8.0;
/// Scored seconds of a follow: two periods of the target's motion.
pub const FOLLOW_SECS: f32 = 16.0;
/// Seconds the follow target takes to go there and back.
const FOLLOW_PERIOD: f32 = 8.0;
/// How far the follow target swings to each side, in millimetres of hand travel, so the hand
/// does the same slow movement (12 mm/s at most while it is scored) at any sensitivity.
const FOLLOW_REACH_MM: f32 = 15.0;
/// The swing is never wider than this many degrees, so the target stays on screen at very fast
/// sensitivities.
const FOLLOW_MAX_DEG: f32 = 30.0;
/// As for a drill: the ready gate ignores input at first, and a countdown leads in.
const READY_ARM_SECS: f32 = 0.6;
const COUNTDOWN_SECS: f32 = 1.5;
/// Distance of the target sphere from the eye, metres (the drills' own).
const TARGET_DISTANCE: f32 = 10.0;

/// Counts the full sweeps of a hand going left and right without lifting, from horizontal mouse
/// counts. A sweep runs from one turning point to the next; the hand has turned once it has come
/// back [`REVERSAL_CM`] from the furthest point it reached. The first leg is not a sweep: it
/// starts wherever the mouse happened to lie.
#[derive(Clone, Debug)]
pub struct SweepMeter {
    /// Hand position, counts from where measuring started.
    position: f32,
    /// The lowest and highest position reached, for the live display.
    reach: (f32, f32),
    /// Direction of the leg in progress (+-1); 0 until the hand has moved clearly one way.
    direction: f32,
    /// Where the direction is being told from, while it is unknown.
    origin: f32,
    /// Furthest position of the leg in progress.
    extreme: f32,
    /// The turning point the leg in progress started at; `None` on the first leg.
    from: Option<f32>,
    /// Counts of travel back that make a turning point.
    reversal: f32,
    /// Finished sweeps, counts.
    sweeps: Vec<f32>,
}

impl SweepMeter {
    pub fn new(dpi: f32) -> Self {
        Self {
            position: 0.0,
            reach: (0.0, 0.0),
            direction: 0.0,
            origin: 0.0,
            extreme: 0.0,
            from: None,
            reversal: REVERSAL_CM / counts_to_cm(1.0, dpi),
            sweeps: Vec::new(),
        }
    }

    /// Feeds horizontal mouse counts.
    pub fn feed(&mut self, counts: f32) {
        self.position += counts;
        self.reach = (self.reach.0.min(self.position), self.reach.1.max(self.position));
        if self.direction == 0.0 {
            let moved = self.position - self.origin;
            if moved.abs() >= self.reversal {
                self.direction = moved.signum();
                self.extreme = self.position;
            }
        } else if (self.position - self.extreme) * self.direction > 0.0 {
            self.extreme = self.position;
        } else if (self.extreme - self.position) * self.direction >= self.reversal {
            if let Some(from) = self.from {
                self.sweeps.push((self.extreme - from).abs());
            }
            self.from = Some(self.extreme);
            self.direction = -self.direction;
            self.extreme = self.position;
        }
    }

    /// Forgets the leg in progress (the hand moved unseen, during a pause): the next sweep
    /// starts at the next turning point. Finished sweeps stay.
    pub fn interrupt(&mut self) {
        self.direction = 0.0;
        self.origin = self.position;
        self.from = None;
    }

    /// Finished sweeps so far, counts.
    pub fn sweeps(&self) -> &[f32] {
        &self.sweeps
    }

    /// The median finished sweep, counts.
    pub fn median(&self) -> Option<f32> {
        median(&mut self.sweeps.clone())
    }

    /// Where the hand is between the furthest points it has reached, 0 (left) to 1 (right), and
    /// the distance between those points in counts.
    pub fn span(&self) -> (f32, f32) {
        let width = self.reach.1 - self.reach.0;
        (if width > 0.0 { (self.position - self.reach.0) / width } else { 0.5 }, width)
    }
}

/// Spread of the hand's error around its own average, weighted by time so the frame rate
/// doesn't matter. Errors are in millimetres of hand travel.
#[derive(Clone, Copy, Debug, Default)]
pub struct SteadyMeter {
    time: f64,
    sum: DVec2,
    squares: DVec2,
}

impl SteadyMeter {
    pub fn sample(&mut self, error_mm: Vec2, dt: f32) {
        let (error, dt) = (error_mm.as_dvec2(), f64::from(dt));
        self.time += dt;
        self.sum += error * dt;
        self.squares += error * error * dt;
    }

    /// Variance per axis around the mean error (a constant offset is not noise), mm².
    fn variance(&self) -> Option<DVec2> {
        (self.time > 0.0).then(|| {
            let mean = self.sum / self.time;
            (self.squares / self.time - mean * mean).max(DVec2::ZERO)
        })
    }

    /// Standard deviation per axis with both axes pooled, mm: for a still target.
    pub fn both_axes(&self) -> Option<f32> {
        self.variance().map(|v| ((v.x + v.y) * 0.5).sqrt() as f32)
    }

    /// Standard deviation of the vertical error alone, mm: for a target moving sideways, where
    /// the error along its path is lag, not noise.
    pub fn vertical(&self) -> Option<f32> {
        self.variance().map(|v| v.y.sqrt() as f32)
    }
}

/// Which sweep is measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Whole arm on the pad.
    Arm,
    /// Forearm planted: wrist and fingers only.
    Wrist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeTask {
    Sweep(Reach),
    /// Keep the crosshair on a still head-size target.
    Hold,
    /// Keep it on one moving slowly from side to side.
    Follow,
}

impl ProbeTask {
    /// Whether the view turns and a target is shown.
    pub fn aims(self) -> bool {
        !matches!(self, ProbeTask::Sweep(_))
    }

    /// Scored seconds; `None` for sweeps, which end after [`SWEEPS`] sweeps.
    pub fn scored_secs(self) -> Option<f32> {
        match self {
            ProbeTask::Sweep(_) => None,
            ProbeTask::Hold => Some(HOLD_SECS),
            ProbeTask::Follow => Some(FOLLOW_SECS),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbePhase {
    /// Waiting for a click or Space.
    Ready,
    Countdown,
    Live,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProbePause {
    Running,
    /// The pause menu is up.
    Menu,
    /// Counting down back into a live hold or follow.
    Resuming(f32),
}

/// What a finished probe measured.
#[derive(Clone, Debug, PartialEq)]
pub enum ProbeResult {
    /// The median sweep, cm.
    Sweep(f32),
    /// Hand noise (standard deviation), mm.
    Steady(f32),
}

/// One measurement in progress.
#[derive(Resource, Clone, Debug)]
pub struct Probe {
    pub task: ProbeTask,
    phase: ProbePhase,
    pause: ProbePause,
    /// Seconds the ready gate has been up.
    ready_for: f32,
    countdown: f32,
    /// Live seconds.
    elapsed: f32,
    /// View angles (yaw, pitch), degrees.
    aim: Vec2,
    deg_per_count: f32,
    mm_per_deg: f32,
    dpi: f32,
    sweep: SweepMeter,
    steady: SteadyMeter,
}

impl Probe {
    /// A probe at the current sensitivity and DPI, at its ready gate.
    pub fn new(task: ProbeTask, settings: &Settings) -> Self {
        Self {
            task,
            phase: ProbePhase::Ready,
            pause: ProbePause::Running,
            ready_for: 0.0,
            countdown: COUNTDOWN_SECS,
            elapsed: 0.0,
            aim: Vec2::ZERO,
            deg_per_count: settings.deg_per_count(settings.sens),
            mm_per_deg: mm_per_degree(settings.cm_per_360(settings.sens)),
            dpi: settings.dpi,
            sweep: SweepMeter::new(settings.dpi),
            steady: SteadyMeter::default(),
        }
    }

    pub fn phase(&self) -> ProbePhase {
        self.phase
    }

    pub fn pause_state(&self) -> ProbePause {
        self.pause
    }

    /// Seconds left of whichever countdown is running.
    pub fn countdown(&self) -> Option<f32> {
        match (self.pause, self.phase) {
            (ProbePause::Resuming(left), _) => Some(left),
            (ProbePause::Running, ProbePhase::Countdown) => Some(self.countdown),
            _ => None,
        }
    }

    /// Seconds of a hold or follow still to go, lead-in included.
    pub fn time_left(&self) -> Option<f32> {
        self.task.scored_secs().map(|scored| (LEAD_SECS + scored - self.elapsed).max(0.0))
    }

    /// Whether a live hold or follow is past its lead-in.
    pub fn scoring(&self) -> bool {
        self.phase == ProbePhase::Live && self.task.aims() && self.elapsed >= LEAD_SECS
    }

    /// Finished sweeps, cm.
    pub fn sweeps_cm(&self) -> impl Iterator<Item = f32> + '_ {
        self.sweep.sweeps().iter().map(|&counts| counts_to_cm(counts, self.dpi))
    }

    /// The live sweep display: the hand's place between its furthest points (0 to 1) and their
    /// distance, cm.
    pub fn sweep_span(&self) -> (f32, f32) {
        let (place, width) = self.sweep.span();
        (place, counts_to_cm(width, self.dpi))
    }

    /// View angles (yaw, pitch), degrees.
    pub fn aim(&self) -> Vec2 {
        self.aim
    }

    /// Where the target is (yaw, pitch), degrees: straight ahead for a hold. For a follow it
    /// starts there too, at rest, and swings from side to side, wider and wider over the
    /// lead-in until it reaches its full swing: it never jumps or turns abruptly, and it starts
    /// where the crosshair is.
    pub fn target(&self) -> Vec2 {
        match self.task {
            ProbeTask::Follow => {
                let reach = (FOLLOW_REACH_MM / self.mm_per_deg).min(FOLLOW_MAX_DEG);
                let grown = (self.elapsed / LEAD_SECS).clamp(0.0, 1.0);
                let swing = reach * grown * grown * (3.0 - 2.0 * grown);
                Vec2::new(swing * (std::f32::consts::TAU * self.elapsed / FOLLOW_PERIOD).sin(), 0.0)
            },
            _ => Vec2::ZERO,
        }
    }

    /// Angular radius of the target, degrees: the head the steadiness limit is about, at its
    /// distance.
    pub fn target_radius() -> f32 {
        (HEAD_RADIUS_M / HEAD_DISTANCE_M).atan().to_degrees()
    }

    /// The crosshair's offset from the target, degrees on screen (x along the yaw axis).
    fn error(&self) -> Vec2 {
        let offset = self.aim - self.target();
        Vec2::new(offset.x * self.aim.y.to_radians().cos(), offset.y)
    }

    pub fn on_target(&self) -> bool {
        self.error().length() <= Self::target_radius()
    }

    /// A click or Space at the ready gate (ignored until it has been up for a moment). Sweeps
    /// start at once; a hold or follow counts down first.
    pub fn start(&mut self) {
        if self.phase == ProbePhase::Ready && self.pause == ProbePause::Running && self.ready_for >= READY_ARM_SECS {
            self.phase = if self.task.aims() { ProbePhase::Countdown } else { ProbePhase::Live };
        }
    }

    /// Escape, or the window lost focus.
    pub fn pause(&mut self) {
        if self.phase != ProbePhase::Done {
            self.pause = ProbePause::Menu;
            self.sweep.interrupt();
        }
    }

    /// Back in from the pause menu: a live hold or follow counts down first.
    pub fn resume(&mut self) {
        if self.pause == ProbePause::Menu {
            let live_aiming = self.phase == ProbePhase::Live && self.task.aims();
            self.pause = if live_aiming { ProbePause::Resuming(COUNTDOWN_SECS) } else { ProbePause::Running };
        }
    }

    /// Whether the mouse belongs to the probe (cursor hidden and locked) rather than to the
    /// pause menu.
    pub fn grabs_cursor(&self) -> bool {
        self.pause != ProbePause::Menu && self.phase != ProbePhase::Done
    }

    /// One frame: `dt` seconds and `motion` raw mouse counts. Returns the result on the frame
    /// the measurement completes. Does nothing while the pause menu is up, and only counts down
    /// while resuming.
    pub fn advance(&mut self, dt: f32, motion: Vec2) -> Option<ProbeResult> {
        match self.pause {
            ProbePause::Menu => return None,
            ProbePause::Resuming(left) => {
                self.pause = if left - dt <= 0.0 { ProbePause::Running } else { ProbePause::Resuming(left - dt) };
                return None;
            },
            ProbePause::Running => {},
        }
        if self.task.aims() && self.phase != ProbePhase::Done {
            // Raw counts, as in `camera::mouse_look`.
            const PITCH_LIMIT: f32 = 89.0;
            self.aim.x -= motion.x * self.deg_per_count;
            self.aim.y = (self.aim.y - motion.y * self.deg_per_count).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        }
        match self.phase {
            ProbePhase::Ready => self.ready_for += dt,
            ProbePhase::Countdown => {
                self.countdown -= dt;
                if self.countdown <= 0.0 {
                    self.phase = ProbePhase::Live;
                }
            },
            ProbePhase::Live => return self.measure(dt, motion),
            ProbePhase::Done => {},
        }
        None
    }

    fn measure(&mut self, dt: f32, motion: Vec2) -> Option<ProbeResult> {
        let Some(scored) = self.task.scored_secs() else {
            self.sweep.feed(motion.x);
            if self.sweep.sweeps().len() < SWEEPS {
                return None;
            }
            self.phase = ProbePhase::Done;
            return self.sweep.median().map(|counts| ProbeResult::Sweep(counts_to_cm(counts, self.dpi)));
        };
        self.elapsed += dt;
        if self.elapsed >= LEAD_SECS {
            self.steady.sample(self.error() * self.mm_per_deg, dt);
        }
        if self.elapsed < LEAD_SECS + scored {
            return None;
        }
        self.phase = ProbePhase::Done;
        let noise = if self.task == ProbeTask::Hold { self.steady.both_axes() } else { self.steady.vertical() };
        noise.map(ProbeResult::Steady)
    }
}

/// The target of a hold or follow.
#[derive(Component)]
struct ProbeTarget;

pub(super) fn register(app: &mut App) {
    app.add_systems(
        Update,
        drive_probe.before(apply_look).run_if(in_state(AppState::Finder)).run_if(resource_exists::<Probe>),
    )
    .add_systems(OnExit(AppState::Finder), release);
}

/// Position on the target sphere for view angles in degrees.
fn on_sphere(angles: Vec2) -> Vec3 {
    Quat::from_euler(EulerRot::YXZ, angles.x.to_radians(), angles.y.to_radians(), 0.0) * Vec3::NEG_Z * TARGET_DISTANCE
}

/// Input, one step of the probe, and what follows from it: the view, the target, the cursor, and
/// handing a finished measurement to the [`Checkup`] it belongs to.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn drive_probe(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut focus: MessageReader<WindowFocused>,
    keep_on_focus_loss: Option<Res<KeepRunsOnFocusLoss>>,
    settings: Res<Settings>,
    assets: Res<TargetAssets>,
    mut probe: ResMut<Probe>,
    mut checkup: Option<ResMut<Checkup>>,
    mut look: ResMut<Look>,
    mut cursor: Single<&mut CursorOptions>,
    mut targets: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>), With<ProbeTarget>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let lost_focus = focus.read().any(|event| !event.focused) && keep_on_focus_loss.is_none();
    let escape = keys.just_pressed(KeyCode::Escape);
    match probe.pause_state() {
        ProbePause::Menu if escape => probe.resume(),
        ProbePause::Menu if keys.just_pressed(KeyCode::KeyR) => {
            let task = probe.task;
            *probe = Probe::new(task, &settings);
        },
        ProbePause::Menu if keys.just_pressed(KeyCode::KeyQ) => {
            // Back to the step's card; nothing of this attempt is kept.
            commands.remove_resource::<Probe>();
            next_state.set(AppState::Finder);
            return;
        },
        ProbePause::Menu => {},
        _ if escape || lost_focus => probe.pause(),
        _ if buttons.just_pressed(MouseButton::Left) || keys.just_pressed(KeyCode::Space) => probe.start(),
        _ => {},
    }

    let result = probe.advance(time.delta_secs(), motion.delta);

    let grab = probe.grabs_cursor();
    if cursor.visible == grab {
        cursor.visible = !grab;
        cursor.grab_mode = if grab { CursorGrabMode::Locked } else { CursorGrabMode::None };
    }
    if probe.task.aims() {
        let aim = probe.aim();
        if look.angles() != (aim.x.to_radians(), aim.y.to_radians()) {
            look.set(aim.x.to_radians(), aim.y.to_radians());
        }
        let position = on_sphere(probe.target());
        let material = if probe.on_target() { &assets.hot } else { &assets.idle };
        match targets.single_mut() {
            Ok((mut transform, mut shown)) => {
                if transform.translation != position {
                    transform.translation = position;
                }
                if shown.0 != *material {
                    shown.0 = material.clone();
                }
            },
            Err(_) => {
                let radius = TARGET_DISTANCE * Probe::target_radius().to_radians().tan();
                commands.spawn((
                    ProbeTarget,
                    DespawnOnExit(AppState::Finder),
                    Mesh3d(assets.mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(position)
                        .with_rotation(facing_eye(position))
                        .with_scale(Vec3::splat(radius)),
                    FacesEye,
                ));
            },
        }
    }

    if probe.phase() == ProbePhase::Done {
        if let (Some(checkup), Some(result)) = (checkup.as_mut(), result) {
            checkup.record_probe(probe.task, result);
        }
        commands.remove_resource::<Probe>();
        // Re-enter to show the checkup's next card.
        next_state.set(AppState::Finder);
    }
}

/// Leaving the finder (or re-entering it for the next screen): the mouse and the view go back
/// to the menus.
fn release(mut cursor: Single<&mut CursorOptions>, mut look: ResMut<Look>, probe: Option<Res<Probe>>) {
    // A probe that is just starting keeps what `drive_probe` sets up on its first frame.
    if probe.is_some_and(|probe| probe.grabs_cursor()) {
        return;
    }
    cursor.visible = true;
    cursor.grab_mode = CursorGrabMode::None;
    if look.angles() != (0.0, 0.0) {
        look.set(0.0, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::finder::checkup::{CheckupKind, Step};

    const DPI: f32 = 800.0;

    fn cm(value: f32) -> f32 {
        value / counts_to_cm(1.0, DPI)
    }

    /// Moves the hand `distance` cm (signed) in small steps.
    fn sweep(meter: &mut SweepMeter, distance: f32) {
        let steps = 200;
        for _ in 0..steps {
            meter.feed(cm(distance) / steps as f32);
        }
    }

    #[test]
    fn sweeps_run_between_turning_points_and_skip_the_first_leg() {
        let mut meter = SweepMeter::new(DPI);
        // From somewhere in the middle to the right edge: not a sweep.
        sweep(&mut meter, 11.0);
        sweep(&mut meter, -28.0);
        assert!(meter.sweeps().is_empty(), "a sweep counts once the hand has turned back");
        sweep(&mut meter, 30.0);
        sweep(&mut meter, -29.0);
        // Turning back once more ends the third.
        sweep(&mut meter, 2.0);
        let sweeps: Vec<f32> = meter.sweeps().iter().map(|&c| counts_to_cm(c, DPI)).collect();
        assert_eq!(sweeps.len(), 3);
        for (measured, expected) in sweeps.iter().zip([28.0, 30.0, 29.0]) {
            assert!((measured - expected).abs() < 0.01, "{sweeps:?}");
        }
        assert!((counts_to_cm(meter.median().unwrap(), DPI) - 29.0).abs() < 0.01);
        let (place, width) = meter.span();
        assert!((counts_to_cm(width, DPI) - 30.0).abs() < 0.01 && (0.0..=1.0).contains(&place));
    }

    #[test]
    fn wobble_at_the_turn_does_not_end_a_sweep() {
        let mut meter = SweepMeter::new(DPI);
        sweep(&mut meter, 10.0);
        sweep(&mut meter, -20.0);
        // Half a centimetre back and forth at the edge, then on to the true end.
        sweep(&mut meter, 0.5);
        sweep(&mut meter, -0.5);
        sweep(&mut meter, -5.0);
        sweep(&mut meter, 25.0);
        sweep(&mut meter, -3.0);
        let sweeps: Vec<f32> = meter.sweeps().iter().map(|&c| counts_to_cm(c, DPI)).collect();
        assert_eq!(sweeps.len(), 2);
        assert!((sweeps[0] - 25.0).abs() < 0.01 && (sweeps[1] - 25.0).abs() < 0.01, "{sweeps:?}");
    }

    #[test]
    fn an_interrupted_sweep_is_dropped_and_finished_ones_stay() {
        let mut meter = SweepMeter::new(DPI);
        sweep(&mut meter, 10.0);
        sweep(&mut meter, -28.0);
        sweep(&mut meter, 28.0);
        sweep(&mut meter, -14.0);
        assert_eq!(meter.sweeps().len(), 2);
        // Paused mid-sweep; the hand is put down elsewhere and starts again.
        meter.interrupt();
        sweep(&mut meter, -6.0);
        sweep(&mut meter, 27.0);
        assert_eq!(meter.sweeps().len(), 2, "the leg after a pause starts anywhere: not a sweep");
        sweep(&mut meter, -27.0);
        sweep(&mut meter, 2.0);
        let sweeps: Vec<f32> = meter.sweeps().iter().map(|&c| counts_to_cm(c, DPI)).collect();
        assert_eq!(sweeps.len(), 4);
        assert!(sweeps[2..].iter().all(|s| (s - 27.0).abs() < 0.01), "{sweeps:?}");
    }

    #[test]
    fn steadiness_is_the_spread_around_the_mean_whatever_the_frame_rate() {
        // A constant offset is not noise.
        let mut parked = SteadyMeter::default();
        for _ in 0..500 {
            parked.sample(Vec2::new(0.3, -0.2), 0.004);
        }
        assert!(parked.both_axes().unwrap() < 1e-4 && parked.vertical().unwrap() < 1e-4);
        // A 0.2 mm vertical sine has a standard deviation of 0.2 / sqrt(2) mm, sampled evenly or
        // with every other frame three times as long.
        for uneven in [false, true] {
            let mut meter = SteadyMeter::default();
            let (mut t, mut frame) = (0.0f32, 0);
            while t < 10.0 {
                let dt = if uneven && frame % 2 == 0 { 0.006 } else { 0.002 };
                t += dt;
                frame += 1;
                meter.sample(Vec2::new(5.0 * t, 0.2 * (std::f32::consts::TAU * t).sin()), dt);
            }
            assert!((meter.vertical().unwrap() - 0.2 / 2f32.sqrt()).abs() < 2e-3, "{:?}", meter.vertical());
            // The drift along x counts for a still target, not for one moving along x.
            assert!(meter.both_axes().unwrap() > 5.0);
        }
        assert_eq!(SteadyMeter::default().vertical(), None);
    }

    fn settings(sens: f32) -> Settings {
        Settings { sens, dpi: DPI, ..Settings::default() }
    }

    const DT: f32 = 1.0 / 250.0;

    /// Starts a probe and runs its countdown down.
    fn started(task: ProbeTask, settings: &Settings) -> Probe {
        let mut probe = Probe::new(task, settings);
        probe.start();
        assert_eq!(probe.phase(), ProbePhase::Ready, "the ready gate ignores input at first");
        while probe.phase() == ProbePhase::Ready {
            probe.advance(DT, Vec2::ZERO);
            probe.start();
        }
        while probe.phase() == ProbePhase::Countdown {
            assert!(probe.countdown().is_some());
            probe.advance(DT, Vec2::ZERO);
        }
        assert_eq!(probe.phase(), ProbePhase::Live);
        probe
    }

    /// A hand that follows the follow target exactly sideways and wobbles `wobble_mm` (SD) up
    /// and down: the mouse counts for the frame ending at live time `t`.
    fn follow_counts(probe: &Probe, t: f32, wobble_mm: f32, settings: &Settings) -> Vec2 {
        let deg_per_count = settings.deg_per_count(settings.sens);
        let mm_per_deg = mm_per_degree(settings.cm_per_360(settings.sens));
        // Where the target will be at the end of this frame.
        let mut ahead = probe.clone();
        ahead.advance(DT, Vec2::ZERO);
        let pitch = wobble_mm * 2f32.sqrt() / mm_per_deg * (std::f32::consts::TAU * 3.0 * t).sin();
        let wanted = Vec2::new(ahead.target().x, pitch);
        // Turning right or up is negative counts times the gain, as in the camera.
        -(wanted - probe.aim()) / deg_per_count
    }

    /// Runs a follow to its end with `wobble_mm` of vertical noise; `pause_at` pauses once at
    /// that live time, for a while, with the mouse moving meanwhile.
    fn follow(sens: f32, wobble_mm: f32, pause_at: Option<f32>) -> f32 {
        let settings = settings(sens);
        let mut probe = started(ProbeTask::Follow, &settings);
        let (mut t, mut paused) = (0.0, false);
        loop {
            if pause_at.is_some_and(|at| t >= at) && !paused {
                paused = true;
                probe.pause();
                let frozen = (probe.aim(), probe.target(), probe.time_left());
                for _ in 0..500 {
                    assert_eq!(probe.advance(DT, Vec2::new(40.0, -25.0)), None);
                }
                assert_eq!((probe.aim(), probe.target(), probe.time_left()), frozen);
                probe.resume();
                assert!(matches!(probe.pause_state(), ProbePause::Resuming(_)));
                let mut frames = 0;
                while probe.pause_state() != ProbePause::Running {
                    probe.advance(DT, Vec2::new(-30.0, 10.0));
                    frames += 1;
                }
                assert!((frames as f32 * DT - COUNTDOWN_SECS).abs() < 0.02);
                assert_eq!((probe.aim(), probe.target(), probe.time_left()), frozen, "resumes where it stopped");
            }
            t += DT;
            let counts = follow_counts(&probe, t, wobble_mm, &settings);
            if let Some(result) = probe.advance(DT, counts) {
                let ProbeResult::Steady(mm) = result else { panic!("{result:?}") };
                assert_eq!(probe.phase(), ProbePhase::Done);
                assert!((t - (LEAD_SECS + FOLLOW_SECS)).abs() < 0.05, "{t}");
                return mm;
            }
        }
    }

    #[test]
    fn follow_noise_comes_out_in_millimetres_at_any_sensitivity() {
        // The same hand wobble at 0.25 and at 0.6 (65 and 27 cm/360) measures the same.
        for sens in [0.25, 0.4, 0.6] {
            let measured = follow(sens, 0.15, None);
            assert!((measured - 0.15).abs() < 0.01, "sens {sens}: {measured} mm");
        }
        assert!(follow(0.4, 0.0, None) < 0.005);
    }

    #[test]
    fn a_paused_follow_resumes_exactly_where_it_stopped() {
        let straight = follow(0.4, 0.15, None);
        let paused = follow(0.4, 0.15, Some(7.3));
        assert!((straight - paused).abs() < 1e-4, "{straight} vs {paused}");
    }

    #[test]
    fn the_follow_target_moves_smoothly_and_stays_in_view() {
        for sens in [0.1, 0.4, 2.5] {
            let settings = settings(sens);
            let mut probe = started(ProbeTask::Follow, &settings);
            // It starts where the crosshair is, at rest.
            assert_eq!(probe.target(), Vec2::ZERO);
            let mm_per_deg = mm_per_degree(settings.cm_per_360(sens));
            let (mut last, mut last_speed) = (Vec2::ZERO, 0.0);
            let (mut widest, mut fastest) = (0.0f32, 0.0f32);
            while probe.phase() == ProbePhase::Live {
                probe.advance(DT, Vec2::ZERO);
                let target = probe.target();
                let speed = (target.x - last.x) / DT;
                // No jumps in position or speed: a few times the swing's own acceleration at
                // most (a jump within one frame would be thousands of times it).
                assert!((speed - last_speed).abs() / DT <= 90.0, "sens {sens}: speed jumped");
                (last, last_speed) = (target, speed);
                widest = widest.max(target.x.abs());
                if probe.scoring() {
                    fastest = fastest.max(speed.abs());
                }
            }
            assert!((3.0..=FOLLOW_MAX_DEG + 1e-3).contains(&widest), "sens {sens}: {widest} degrees");
            // Slow for the hand at every sensitivity: 12 mm/s at most while it counts.
            assert!(fastest * mm_per_deg <= 12.0, "sens {sens}: {} mm/s", fastest * mm_per_deg);
        }
    }

    #[test]
    fn a_hold_measures_wobble_on_both_axes_after_the_lead_in() {
        let settings = settings(0.4);
        let mm_per_deg = mm_per_degree(settings.cm_per_360(0.4));
        let deg_per_count = settings.deg_per_count(0.4);
        let mut probe = started(ProbeTask::Hold, &settings);
        let mut t = 0.0;
        let result = loop {
            t += DT;
            // Far off during the lead-in, then a 0.1 mm (SD) circle around a point off-centre.
            let wanted = if t < LEAD_SECS - 0.1 {
                Vec2::new(3.0, 2.0)
            } else {
                let angle = std::f32::consts::TAU * 2.0 * t;
                Vec2::new(0.1, 0.05) + Vec2::from_angle(angle) * 0.1 * 2f32.sqrt() / mm_per_deg
            };
            if let Some(result) = probe.advance(DT, -(wanted - probe.aim()) / deg_per_count) {
                break result;
            }
            // (The frame the lead-in ends on depends on rounding.)
            if (t - LEAD_SECS).abs() > 0.02 {
                assert_eq!(probe.scoring(), t > LEAD_SECS, "{t}");
            }
        };
        let ProbeResult::Steady(mm) = result else { panic!("{result:?}") };
        assert!((mm - 0.1).abs() < 0.005, "{mm}");
        assert!((t - (LEAD_SECS + HOLD_SECS)).abs() < 0.05);
    }

    #[test]
    fn a_sweep_probe_ends_after_its_sweeps_and_survives_a_pause() {
        let settings = settings(0.4);
        let mut probe = Probe::new(ProbeTask::Sweep(Reach::Arm), &settings);
        while probe.phase() == ProbePhase::Ready {
            probe.advance(DT, Vec2::ZERO);
            probe.start();
        }
        // No countdown, no view turning.
        assert_eq!((probe.phase(), probe.countdown()), (ProbePhase::Live, None));
        let mut result = None;
        let mut legs = 0;
        let mut direction = 1.0;
        while result.is_none() {
            for _ in 0..100 {
                result = result.or(probe.advance(DT, Vec2::new(direction * cm(29.0) / 100.0, 3.0)));
            }
            direction = -direction;
            legs += 1;
            if legs == 3 {
                // One sweep counted, the second under way: a pause drops only that one.
                assert_eq!(probe.sweeps_cm().count(), 1);
                probe.pause();
                assert!(!probe.grabs_cursor());
                assert_eq!(probe.advance(DT, Vec2::new(5000.0, 0.0)), None);
                probe.resume();
                assert_eq!(probe.pause_state(), ProbePause::Running, "sweeps resume at once");
            }
            assert!(legs < 20, "never finished");
        }
        let Some(ProbeResult::Sweep(median)) = result else { panic!("{result:?}") };
        assert!((median - 29.0).abs() < 0.01, "{median}");
        assert_eq!(probe.aim(), Vec2::ZERO);
        assert_eq!(probe.sweeps_cm().count(), SWEEPS);
        // A leg to reach an edge, the first sweep, the one lost to the pause, a leg to reach an
        // edge again, three more sweeps, and the turn that ends the last.
        assert_eq!(legs, 8);
    }

    // -----------------------------------------------------------------------------------------
    // The system: keys, focus, cursor and the view around the probe.
    // -----------------------------------------------------------------------------------------

    const FRAME_MS: u64 = 10;

    /// A calibration whose next step is the hold, with its probe at the ready gate.
    fn probe_app() -> App {
        let settings = settings(0.4);
        let mut checkup = Checkup::new(CheckupKind::Calibration);
        checkup.record_probe(ProbeTask::Sweep(Reach::Arm), ProbeResult::Sweep(29.0));
        checkup.record_probe(ProbeTask::Sweep(Reach::Wrist), ProbeResult::Sweep(7.5));
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(AppState::Finder)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(FRAME_MS)))
            .insert_resource(Probe::new(ProbeTask::Hold, &settings))
            .insert_resource(settings)
            .insert_resource(checkup)
            .insert_resource(TargetAssets {
                mesh: Handle::default(),
                idle: Handle::default(),
                hot: Handle::default(),
                wait: Handle::default(),
                cover_mesh: Handle::default(),
                cover_material: Handle::default(),
            })
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<Look>()
            .add_message::<WindowFocused>();
        app.world_mut().spawn(CursorOptions::default());
        register(&mut app);
        app
    }

    /// One frame in which `key` goes down.
    fn press(app: &mut App, key: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    }

    /// `frames` frames with the mouse moving `counts` in each.
    fn run(app: &mut App, frames: usize, counts: Vec2) {
        app.world_mut().resource_mut::<AccumulatedMouseMotion>().delta = counts;
        for _ in 0..frames {
            app.update();
        }
    }

    fn cursor_locked(app: &mut App) -> bool {
        let world = app.world_mut();
        let mut cursors = world.query::<&CursorOptions>();
        let cursor = cursors.single(world).unwrap();
        assert_eq!(cursor.visible, cursor.grab_mode == CursorGrabMode::None);
        !cursor.visible
    }

    /// What a paused probe must keep: its time left and where the view points.
    fn frozen(app: &App) -> (Option<f32>, (f32, f32)) {
        (app.world().resource::<Probe>().time_left(), app.world().resource::<Look>().angles())
    }

    /// Through the ready gate and the countdown, into the live hold.
    fn go_live(app: &mut App) {
        run(app, 80, Vec2::ZERO);
        assert!(cursor_locked(app), "the probe takes the mouse from its first frame");
        press(app, KeyCode::Space);
        assert_eq!(app.world().resource::<Probe>().phase(), ProbePhase::Countdown);
        run(app, 160, Vec2::ZERO);
        assert_eq!(app.world().resource::<Probe>().phase(), ProbePhase::Live);
    }

    #[test]
    fn escape_and_focus_loss_pause_a_probe_and_it_resumes_where_it_stopped() {
        let mut app = probe_app();
        go_live(&mut app);
        // The view turns with the mouse: right is negative yaw, as in a drill.
        run(&mut app, 5, Vec2::new(10.0, 0.0));
        assert!(app.world().resource::<Look>().angles().0 < 0.0);
        press(&mut app, KeyCode::Escape);
        assert_eq!(app.world().resource::<Probe>().pause_state(), ProbePause::Menu);
        assert!(!cursor_locked(&mut app), "the pause menu needs the cursor");
        let paused = frozen(&app);
        // Paused for longer than the rest of the step, the mouse moving all the while.
        run(&mut app, 2000, Vec2::new(25.0, -10.0));
        assert_eq!(frozen(&app), paused);
        // Resuming counts down first, still standing still, with the mouse taken again.
        press(&mut app, KeyCode::Escape);
        assert!(matches!(app.world().resource::<Probe>().pause_state(), ProbePause::Resuming(_)));
        run(&mut app, 100, Vec2::new(25.0, -10.0));
        assert_eq!(frozen(&app), paused);
        assert!(cursor_locked(&mut app));
        run(&mut app, 100, Vec2::ZERO);
        assert_eq!(app.world().resource::<Probe>().pause_state(), ProbePause::Running);
        assert!(app.world().resource::<Probe>().time_left() < paused.0);
        // Losing focus pauses too.
        app.world_mut().write_message(WindowFocused { window: Entity::PLACEHOLDER, focused: false });
        app.update();
        assert_eq!(app.world().resource::<Probe>().pause_state(), ProbePause::Menu);
        // Unless runs are kept going, as for the screenshot tour.
        press(&mut app, KeyCode::Escape);
        run(&mut app, 200, Vec2::ZERO);
        app.insert_resource(KeepRunsOnFocusLoss);
        app.world_mut().write_message(WindowFocused { window: Entity::PLACEHOLDER, focused: false });
        app.update();
        assert_eq!(app.world().resource::<Probe>().pause_state(), ProbePause::Running);
    }

    #[test]
    fn a_finished_probe_hands_its_result_to_the_checkup_and_gives_the_mouse_back() {
        let mut app = probe_app();
        go_live(&mut app);
        // A little wobble around the dot for the whole step.
        for frame in 0..((LEAD_SECS + HOLD_SECS) * 1000.0) as u64 / FRAME_MS + 5 {
            run(&mut app, 1, if frame % 2 == 0 { Vec2::new(1.0, 1.0) } else { Vec2::new(-1.0, -1.0) });
        }
        assert!(app.world().get_resource::<Probe>().is_none());
        let checkup = app.world().resource::<Checkup>();
        assert_eq!(checkup.step(), Some(Step::Probe(ProbeTask::Follow)));
        // One count at 800 DPI is 0.03 mm; the wobble is half a count around its middle.
        let noise = checkup.hold_mm.expect("the hold was measured");
        assert!((noise - 0.016).abs() < 0.002, "{noise} mm");
        // The screen for the next step is entered with the cursor free and the view reset.
        run(&mut app, 2, Vec2::ZERO);
        assert!(!cursor_locked(&mut app));
        assert_eq!(app.world().resource::<Look>().angles(), (0.0, 0.0));
    }

    #[test]
    fn a_paused_probe_can_be_restarted_or_left_without_a_result() {
        let mut app = probe_app();
        go_live(&mut app);
        run(&mut app, 300, Vec2::ZERO);
        press(&mut app, KeyCode::Escape);
        press(&mut app, KeyCode::KeyR);
        let probe = app.world().resource::<Probe>();
        assert_eq!((probe.phase(), probe.pause_state()), (ProbePhase::Ready, ProbePause::Running));
        assert_eq!(probe.time_left(), Some(LEAD_SECS + HOLD_SECS));
        // Paused at the ready gate, then left: the step is still to do.
        press(&mut app, KeyCode::Escape);
        press(&mut app, KeyCode::KeyQ);
        run(&mut app, 2, Vec2::ZERO);
        assert!(app.world().get_resource::<Probe>().is_none());
        let checkup = app.world().resource::<Checkup>();
        assert_eq!((checkup.hold_mm, checkup.done()), (None, 2));
        assert!(!cursor_locked(&mut app));
    }
}
