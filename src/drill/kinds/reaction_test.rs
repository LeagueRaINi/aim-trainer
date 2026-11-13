//! Reaction test: a measurement of simple visual reaction time, with a fixed procedure instead
//! of a drill length. A few practice trials, then a fixed number of scored ones; in each a large
//! dot appears in the middle of the view after a random wait and any click answers it.
//!
//! The procedure follows Woods et al. 2015, "Factors influencing the latency of simple reaction
//! time" (Front. Hum. Neurosci. 9:131): practice first, a large high-contrast stimulus, a gap
//! before each stimulus that is neither short (short gaps slow reactions) nor predictable, and
//! responses under 110 ms or over 1000 ms are not reactions. A click before the dot or within
//! 110 ms of it is a false start and the trial is taken again; no click within a second is a
//! lapse, which is counted but not averaged.
//!
//! The run's state is [`ReactionTest`]; `reaction_log` keeps every finished test. How a click is
//! timed is described in [`cue`](crate::drill::cue).

use bevy::prelude::*;

use crate::camera::PlayerCamera;
use crate::drill::cue::{ANTICIPATION_SECS, Cue, CueBanner, Frame, Tone, ahead};
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_respawn, no_spawn_next, no_tuning};
use crate::drill::geometry::{TARGET_DISTANCE, random_range};
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{Dormant, SpawnCtx};
use crate::drill::{DrillSet, Run};
use crate::metrics::reaction::{Summary, score};
use crate::replay::Recorder;
use crate::settings::Settings;
use crate::sfx::HitSounds;
use crate::{AppState, PlayState};

/// Trials before the scored ones, to settle into the task: Woods et al. used practice trials
/// too, and the first reactions of a session are slower than the rest.
pub const PRACTICE_TRIALS: u32 = 5;
/// Scored trials. With a trial-to-trial SD around 40 ms, 30 of them give the median a standard
/// error of about 9 ms; the whole test takes about a minute and a half.
pub const SCORED_TRIALS: u32 = 30;
/// Trials started at most, counting those taken again after a false start: a run of false
/// starts (a held or bouncing button) still ends, with the trials it got.
pub const MAX_ATTEMPTS: u32 = 60;
/// Seconds from the previous answer to the next dot, drawn evenly from this range. Not under
/// 1.5 s: reactions are slower when the next stimulus follows within about a second. Up to 3.5 s
/// so that the dot can't be timed: clicking on a guess is a false start far more often than a hit.
pub const WAIT_SECS: (f32, f32) = (1.5, 3.5);
/// A dot not clicked within this many seconds is a lapse (attention was elsewhere), not a slow
/// reaction.
pub const LAPSE_SECS: f32 = 1.0;
/// With fewer valid scored trials than this a median is too rough to compare tests by.
pub const MIN_VALID: usize = 20;

pub const DEF: DrillDef = DrillDef {
    name: "Reaction test",
    blurb: |_| {
        format!(
            "Click the moment the dot appears: anywhere, no aiming. {PRACTICE_TRIALS} practice trials, then {SCORED_TRIALS} timed ones."
        )
        .into()
    },
    category: Category::Tests,
    // Large: 17° across, about a quarter of the screen's height.
    base_radius: 1.5,
    decay: DecayRule::None,
    score: ScoreModel::ReactionTest,
    ttk_from: TtkFrom::Spawn,
    click_kills: false,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    endless: true,
    params: &[],
    tune_defaults: no_tuning,
    spawn_initial,
    respawn: no_respawn,
    spawn_next: no_spawn_next,
};

pub(super) fn register(app: &mut App) {
    app.init_resource::<ReactionTest>()
        .add_systems(OnEnter(AppState::Playing), |mut test: ResMut<ReactionTest>| *test = ReactionTest::default())
        .add_systems(Update, run_test.in_set(DrillSet::Input))
        .add_systems(OnEnter(PlayState::Paused), void_shown_trial);
}

/// The dot to react to: hidden (and [`Dormant`]) except while a trial shows it.
#[derive(Component)]
struct Stimulus;

/// A scored trial.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trial {
    /// Answered in this many milliseconds.
    Valid(f32),
    /// Not answered in time.
    Lapse,
}

/// How a trial (practice or scored) ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// A click before the dot, or too soon after it to be a reaction: the trial is taken again.
    FalseStart,
    /// Milliseconds from the dot to the click.
    Reacted(f32),
    Lapse,
}

impl Outcome {
    /// What a click `secs` after the dot came on screen is.
    fn of_click(secs: f32) -> Self {
        if secs < ANTICIPATION_SECS {
            Outcome::FalseStart
        } else if secs > LAPSE_SECS {
            Outcome::Lapse
        } else {
            Outcome::Reacted(secs * 1000.0)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Stage {
    /// The run isn't live yet.
    #[default]
    Idle,
    /// Nothing on screen until this run time.
    Waiting {
        until: f32,
    },
    /// The dot is up.
    Shown(Cue),
    Done,
}

/// What a frame of the procedure did.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Event {
    /// The first trial began.
    Began,
    /// The dot appears this frame.
    Show,
    /// A trial ended (`scored`: after the practice trials); the dot, if up, goes away.
    Ended { outcome: Outcome, scored: bool },
}

/// The procedure of the current (or last finished) reaction test and what it measured.
#[derive(Resource, Debug, Default)]
pub struct ReactionTest {
    stage: Stage,
    /// Trials started so far, including those taken again.
    attempts: u32,
    practice_done: u32,
    /// The scored trials so far, in order.
    trials: Vec<Trial>,
    /// False starts in scored trials.
    false_starts: u32,
    last: Option<Outcome>,
    /// Live frames and their total time, for the timing resolution.
    frames: u32,
    frame_secs: f32,
}

impl ReactionTest {
    /// Advances the procedure by one live frame in which the button was `clicked` or not.
    fn step(&mut self, frame: Frame, clicked: bool) -> Option<Event> {
        self.frames += 1;
        self.frame_secs += frame.dt;
        match self.stage {
            Stage::Idle => {
                self.begin_trial(frame.now);
                Some(Event::Began)
            },
            // A click in the frame the dot would appear in came before it could be seen.
            Stage::Waiting { .. } if clicked => Some(self.end_trial(Outcome::FalseStart, frame.now)),
            Stage::Waiting { until } if frame.now >= until => {
                self.stage = Stage::Shown(Cue::Drawn);
                Some(Event::Show)
            },
            Stage::Waiting { .. } | Stage::Done => None,
            Stage::Shown(mut cue) => {
                let onset = cue.onset(frame);
                self.stage = Stage::Shown(cue);
                let outcome = if clicked {
                    Outcome::of_click(frame.input() - onset)
                } else if frame.started() - onset >= LAPSE_SECS {
                    Outcome::Lapse
                } else {
                    return None;
                };
                Some(self.end_trial(outcome, frame.now))
            },
        }
    }

    /// The next trial's wait starts now.
    fn begin_trial(&mut self, now: f32) {
        self.attempts += 1;
        self.stage = Stage::Waiting { until: now + random_range(WAIT_SECS.0, WAIT_SECS.1) };
    }

    fn end_trial(&mut self, outcome: Outcome, now: f32) -> Event {
        let scored = self.practice_done >= PRACTICE_TRIALS;
        match (outcome, scored) {
            (Outcome::FalseStart, true) => self.false_starts += 1,
            (Outcome::FalseStart, false) => {},
            (_, false) => self.practice_done += 1,
            (Outcome::Reacted(ms), true) => self.trials.push(Trial::Valid(ms)),
            (Outcome::Lapse, true) => self.trials.push(Trial::Lapse),
        }
        self.last = Some(outcome);
        if self.trials.len() >= SCORED_TRIALS as usize || self.attempts >= MAX_ATTEMPTS {
            self.stage = Stage::Done;
        } else {
            self.begin_trial(now);
        }
        Event::Ended { outcome, scored }
    }

    /// A trial whose dot is up is taken again, without counting for or against anything: the
    /// run was paused over it, so the player has seen the dot for as long as they liked. Returns
    /// whether there was one.
    fn void_shown(&mut self, now: f32) -> bool {
        let shown = matches!(self.stage, Stage::Shown(_));
        if shown {
            self.stage = Stage::Waiting { until: now + random_range(WAIT_SECS.0, WAIT_SECS.1) };
            self.last = None;
        }
        shown
    }

    /// The dot appears on the next live frame instead of after the wait: for the screenshot
    /// tour, which can't wait a random time.
    pub(crate) fn cue_now(&mut self) {
        if let Stage::Waiting { until } = &mut self.stage {
            *until = f32::NEG_INFINITY;
        }
    }

    /// Whether every scored trial was taken or the attempts ran out.
    pub fn finished(&self) -> bool {
        self.stage == Stage::Done
    }

    /// The scored trials so far, in order.
    pub fn trials(&self) -> &[Trial] {
        &self.trials
    }

    /// False starts in scored trials.
    pub fn false_starts(&self) -> u32 {
        self.false_starts
    }

    /// The run's mean frame time, milliseconds: twice the resolution of a reaction (see
    /// [`cue`](crate::drill::cue)).
    pub fn frame_ms(&self) -> f32 {
        if self.frames == 0 { 0.0 } else { self.frame_secs / self.frames as f32 * 1000.0 }
    }

    /// The history score: [`score`] of the median valid reaction, 0 without one.
    pub fn score(&self) -> f32 {
        summary(&self.trials).map_or(0.0, |summary| score(summary.median))
    }

    /// What the banner says after the latest event.
    fn banner(&self) -> CueBanner {
        let caption = if self.finished() {
            "Test complete".to_string()
        } else if self.practice_done < PRACTICE_TRIALS {
            format!("Practice {} / {PRACTICE_TRIALS}", self.practice_done + 1)
        } else {
            format!("Trial {} / {SCORED_TRIALS}", self.trials.len() + 1)
        };
        let (result, tone, hint) = match self.last {
            None => (String::new(), Tone::Neutral, "Click the moment the dot appears. No need to aim."),
            Some(Outcome::Reacted(ms)) => (format!("{ms:.0} ms"), Tone::Neutral, ""),
            Some(Outcome::FalseStart) => {
                ("Too early".to_string(), Tone::Bad, "False start: wait for the dot. This trial starts over.")
            },
            Some(Outcome::Lapse) => ("Too slow".to_string(), Tone::Warn, "No click within a second: a lapse."),
        };
        CueBanner { caption, result, tone, hint }
    }
}

/// The valid reaction times among `trials`, milliseconds.
pub fn valid_ms(trials: &[Trial]) -> Vec<f32> {
    trials
        .iter()
        .filter_map(|trial| match trial {
            Trial::Valid(ms) => Some(*ms),
            Trial::Lapse => None,
        })
        .collect()
}

/// Median, quartiles, best, mean and SD of the valid reactions among `trials`.
pub fn summary(trials: &[Trial]) -> Option<Summary> {
    Summary::of(&valid_ms(trials))
}

/// The dot, hidden until the first trial shows it.
fn spawn_initial(ctx: &mut SpawnCtx) {
    ctx.spawn_target(Vec3::new(0.0, 0.0, -TARGET_DISTANCE), None).insert((Stimulus, Dormant, Visibility::Hidden));
}

/// Runs the procedure: shows the dot when a wait is over, takes a click as the answer (aimed
/// anywhere), and ends the run after the last scored trial.
///
/// In the run's counters a scored reaction is a hit (its time the kill time), a scored false
/// start a shot without a hit and a scored lapse an expired target, so the HUD and the history
/// can show them; practice trials only fire the gun.
#[allow(clippy::too_many_arguments)]
fn run_test(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut run: ResMut<Run>,
    mut test: ResMut<ReactionTest>,
    mut banner: ResMut<CueBanner>,
    sounds: Res<HitSounds>,
    settings: Res<Settings>,
    mut recorder: ResMut<Recorder>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Stimulus>)>,
    stimulus: Single<(Entity, &mut Transform, &mut Visibility), With<Stimulus>>,
) {
    if !run.live() {
        return;
    }
    let clicked = mouse.just_pressed(MouseButton::Left);
    let Some(event) = test.step(Frame::of(&run, &time), clicked) else { return };
    let (entity, mut transform, mut visibility) = stimulus.into_inner();
    match event {
        Event::Began => {},
        Event::Show => {
            transform.translation = ahead(camera.forward().as_vec3(), transform.scale.x);
            *visibility = Visibility::Inherited;
            commands.entity(entity).remove::<Dormant>();
        },
        Event::Ended { outcome, scored } => {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
                commands.entity(entity).insert(Dormant);
            }
            let reacted = matches!(outcome, Outcome::Reacted(_));
            if clicked {
                recorder.mark_shot(reacted);
                // A click after the dot went stale is the lapse it would have been anyway.
                if scored && outcome != Outcome::Lapse { run.shot() } else { run.dry_fire() }
            }
            if reacted {
                sounds.play(&mut commands, &settings.audio);
            }
            match outcome {
                Outcome::Reacted(ms) if scored => run.kill(ms / 1000.0),
                Outcome::Lapse if scored => run.expire(),
                _ => {},
            }
            if scored {
                run.rate(test.score());
            }
            if test.finished() {
                run.end();
            }
        },
    }
    *banner = test.banner();
}

/// Pausing while the dot is up voids that trial: otherwise the pause would be a way to look at
/// the dot for as long as one likes before "reacting" to it. The trial is taken again after the
/// run resumes; a pause during a wait changes nothing (the wait is in run time).
fn void_shown_trial(
    mut commands: Commands,
    run: Res<Run>,
    mut test: ResMut<ReactionTest>,
    mut banner: ResMut<CueBanner>,
    mut stimulus: Query<(Entity, &mut Visibility), With<Stimulus>>,
) {
    if !test.void_shown(run.elapsed()) {
        return;
    }
    for (entity, mut visibility) in &mut stimulus {
        *visibility = Visibility::Hidden;
        commands.entity(entity).insert(Dormant);
    }
    *banner = CueBanner { hint: "Paused with the dot up: this trial starts over.", ..test.banner() };
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy::window::WindowFocused;

    use super::*;
    use crate::drill::pause::{self, ResumeCountdown};
    use crate::drill::run::{RunFinished, tick_run};
    use crate::drill::{DrillKind, DrillSpec, RunPurpose, cue};

    /// Simulated frame time, seconds.
    const DT: f32 = 0.01;

    /// Drives the procedure alone, one 10 ms frame per call.
    struct Bench {
        test: ReactionTest,
        frame: u32,
    }

    impl Bench {
        fn new() -> Self {
            let mut bench = Self { test: ReactionTest::default(), frame: 0 };
            assert_eq!(bench.frame(false), Some(Event::Began));
            bench
        }

        fn frame(&mut self, clicked: bool) -> Option<Event> {
            let frame = Frame { now: self.frame as f32 * DT, dt: DT };
            self.frame += 1;
            self.test.step(frame, clicked)
        }

        /// Frames without a click until the dot appears.
        fn until_shown(&mut self) {
            for _ in 0..(WAIT_SECS.1 / DT) as u32 + 2 {
                match self.frame(false) {
                    None => {},
                    Some(Event::Show) => return,
                    other => panic!("{other:?} while waiting"),
                }
            }
            panic!("the dot never appeared");
        }

        /// `frames` frames with the dot up and no click, then one with a click.
        fn click_after(&mut self, frames: u32) -> Option<Event> {
            for _ in 0..frames {
                assert_eq!(self.frame(false), None);
            }
            self.frame(true)
        }

        /// A whole trial, its click read `frames` frames after the one that drew the dot.
        fn react(&mut self, frames: u32) -> Option<Event> {
            self.until_shown();
            self.click_after(frames - 1)
        }

        fn practice(&mut self) {
            for _ in 0..PRACTICE_TRIALS {
                assert!(matches!(self.react(20), Some(Event::Ended { outcome: Outcome::Reacted(_), scored: false })));
            }
        }
    }

    /// The reaction (ms) of a click read `frames` 10 ms frames after the frame that drew the dot:
    /// the dot was on screen from the next frame's start, the click arrived half a frame ago.
    fn reaction_ms(frames: u32) -> f32 {
        (frames as f32 - 1.5) * DT * 1000.0
    }

    #[test]
    fn practice_comes_first_and_is_not_scored() {
        let mut bench = Bench::new();
        assert_eq!(bench.test.banner().caption, format!("Practice 1 / {PRACTICE_TRIALS}"));
        bench.practice();
        assert!(bench.test.trials().is_empty());
        assert_eq!(bench.test.banner().caption, format!("Trial 1 / {SCORED_TRIALS}"));
        let Some(Event::Ended { outcome: Outcome::Reacted(ms), scored: true }) = bench.react(22) else {
            panic!("a scored reaction")
        };
        assert!((ms - reaction_ms(22)).abs() < 1e-2, "{ms}");
        assert_eq!(bench.test.trials(), [Trial::Valid(ms)]);
        assert_eq!(bench.test.score(), score(ms));
    }

    #[test]
    fn waits_are_random_within_the_range() {
        // The same draws every time.
        fastrand::seed(7);
        let mut bench = Bench::new();
        let mut waits = Vec::new();
        for _ in 0..12 {
            let started = bench.frame;
            bench.until_shown();
            waits.push((bench.frame - started) as f32 * DT);
            bench.click_after(20);
        }
        assert!(waits.iter().all(|wait| (WAIT_SECS.0..=WAIT_SECS.1 + 2.0 * DT).contains(wait)), "{waits:?}");
        let (shortest, longest) = waits.iter().fold((f32::MAX, f32::MIN), |(lo, hi), w| (lo.min(*w), hi.max(*w)));
        assert!(longest - shortest > 0.2, "waits don't vary: {waits:?}");
    }

    #[test]
    fn false_starts_repeat_the_trial_and_are_counted() {
        let mut bench = Bench::new();
        bench.practice();
        // A click during the wait.
        assert_eq!(bench.frame(false), None);
        assert_eq!(bench.frame(true), Some(Event::Ended { outcome: Outcome::FalseStart, scored: true }));
        assert_eq!((bench.test.false_starts(), bench.test.trials().len()), (1, 0));
        // The trial starts over with a full new wait.
        let Stage::Waiting { until } = bench.test.stage else { panic!("waiting again") };
        assert!(until > bench.frame as f32 * DT + WAIT_SECS.0 - 2.0 * DT);
        // A click in the very frame the dot is due came before it could be seen.
        bench.test.cue_now();
        assert_eq!(bench.frame(true), Some(Event::Ended { outcome: Outcome::FalseStart, scored: true }));
        // So did one read in the first frame after the dot was drawn (it is only just on screen),
        // and one 105 ms after it is too soon to be a reaction.
        assert_eq!(bench.react(1), Some(Event::Ended { outcome: Outcome::FalseStart, scored: true }));
        assert!(reaction_ms(12) < ANTICIPATION_SECS * 1000.0);
        assert_eq!(bench.react(12), Some(Event::Ended { outcome: Outcome::FalseStart, scored: true }));
        assert_eq!((bench.test.false_starts(), bench.test.trials().len()), (4, 0));
        assert_eq!(bench.test.banner().caption, format!("Trial 1 / {SCORED_TRIALS}"));
        // 115 ms can be one, and counts.
        assert!(matches!(bench.react(13), Some(Event::Ended { outcome: Outcome::Reacted(_), scored: true })));
        assert_eq!((bench.test.false_starts(), bench.test.trials().len()), (4, 1));
        // False starts in practice trials repeat them too, but are not part of the result.
        let mut bench = Bench::new();
        assert_eq!(bench.frame(true), Some(Event::Ended { outcome: Outcome::FalseStart, scored: false }));
        assert_eq!((bench.test.false_starts(), bench.test.practice_done), (0, 0));
    }

    #[test]
    fn lapses_are_counted_and_excluded() {
        let mut bench = Bench::new();
        bench.practice();
        bench.react(25);
        // No click: the dot goes away after a second, and the next trial begins.
        bench.until_shown();
        let mut frames = 0;
        let lapse = loop {
            frames += 1;
            if let Some(event) = bench.frame(false) {
                break event;
            }
        };
        assert_eq!(lapse, Event::Ended { outcome: Outcome::Lapse, scored: true });
        // On screen from the frame after the one that drew it, for a second.
        assert!((101..=102).contains(&frames), "{frames} frames");
        bench.react(35);
        let trials = bench.test.trials();
        assert_eq!(trials.len(), 3);
        assert_eq!(trials[1], Trial::Lapse);
        assert_eq!(valid_ms(trials).len(), 2);
        // The median is of the two reactions; the lapse is not a slow one among them.
        let expected = (reaction_ms(25) + reaction_ms(35)) / 2.0;
        assert!((summary(trials).unwrap().median - expected).abs() < 1e-2);
        assert_eq!(bench.test.score(), score(summary(trials).unwrap().median));
    }

    #[test]
    fn the_attempt_cap_ends_a_run_of_false_starts() {
        let mut bench = Bench::new();
        let mut clicks = 0;
        while !bench.test.finished() {
            assert_eq!(bench.frame(true), Some(Event::Ended { outcome: Outcome::FalseStart, scored: false }));
            clicks += 1;
            assert!(clicks <= MAX_ATTEMPTS, "never ends");
        }
        assert_eq!(clicks, MAX_ATTEMPTS);
        assert_eq!((bench.test.trials().len(), bench.test.score()), (0, 0.0));
        assert_eq!(bench.frame(true), None, "over");
    }

    /// The system in an app with the run lifecycle around it: pausing, the timer and the end of
    /// the run. No rendering; frames are 10 ms.
    fn app() -> App {
        let drill = DrillSpec::builtin(DrillKind::ReactionTest, &Settings::default());
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(AppState::Playing)
            .add_sub_state::<PlayState>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(DT)))
            .insert_resource(Settings::default())
            .insert_resource(Run::new(drill, 0.4, 60.0, RunPurpose::Practice, String::new()).started())
            .init_resource::<HitSounds>()
            .init_resource::<Recorder>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ResumeCountdown>()
            .add_message::<WindowFocused>()
            .add_message::<RunFinished>()
            .configure_sets(
                Update,
                (DrillSet::Start, DrillSet::Input, DrillSet::Finish).chain().run_if(in_state(PlayState::Running)),
            )
            .add_systems(
                Update,
                (
                    pause::pause_control.before(DrillSet::Start).run_if(in_state(AppState::Playing)),
                    tick_run.in_set(DrillSet::Finish),
                ),
            );
        cue::register(&mut app);
        register(&mut app);
        app.world_mut().spawn((PlayerCamera, Transform::default()));
        app.world_mut().spawn((Stimulus, Dormant, Visibility::Hidden, Transform::from_scale(Vec3::splat(1.5))));
        // The first frame starts the first trial.
        app.update();
        app
    }

    /// One frame; with `click`, the button goes down in it.
    fn frame(app: &mut App, click: bool) {
        if click {
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        }
        app.update();
        // No input plugin here to do this between frames.
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
    }

    fn press(app: &mut App, key: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    }

    fn shown(app: &mut App) -> bool {
        let mut stimulus = app.world_mut().query_filtered::<(&Visibility, Has<Dormant>), With<Stimulus>>();
        let (visibility, dormant) = stimulus.single(app.world()).unwrap();
        assert_eq!(*visibility == Visibility::Hidden, dormant, "hidden dots aren't recorded, shown ones are");
        !dormant
    }

    /// Frames without a click until the dot is up (also through a resume countdown).
    fn until_shown(app: &mut App) {
        for _ in 0..((WAIT_SECS.1 + 2.0) / DT) as u32 {
            if shown(app) {
                return;
            }
            frame(app, false);
        }
        panic!("the dot never appeared");
    }

    /// A whole trial, its click read `frames` frames after the one that drew the dot.
    fn react(app: &mut App, frames: u32) {
        until_shown(app);
        for _ in 0..frames - 1 {
            frame(app, false);
        }
        frame(app, true);
        assert!(!shown(app), "the dot goes away with the answer");
    }

    fn elapsed(app: &App) -> f32 {
        app.world().resource::<Run>().elapsed()
    }

    #[test]
    fn the_run_ends_itself_after_the_scored_trials() {
        let mut app = app();
        for _ in 0..PRACTICE_TRIALS {
            react(&mut app, 20);
        }
        let run = app.world().resource::<Run>();
        assert_eq!((run.fired(), run.stats(&Settings::default()).shots), (PRACTICE_TRIALS, 0), "practice isn't scored");
        // A false start, then 28 reactions from 175 to 465 ms and two lapses.
        frame(&mut app, false);
        frame(&mut app, true);
        for trial in 0..SCORED_TRIALS {
            assert!(app.world().resource::<Messages<RunFinished>>().is_empty(), "over before trial {trial}");
            if trial == 7 || trial == 19 {
                until_shown(&mut app);
                for _ in 0..(LAPSE_SECS / DT) as u32 + 3 {
                    frame(&mut app, false);
                }
                assert!(!shown(&mut app), "a lapsed dot goes away");
            } else {
                react(&mut app, 19 + trial);
            }
        }
        let finished: Vec<_> = app.world_mut().resource_mut::<Messages<RunFinished>>().drain().collect();
        assert_eq!(finished.len(), 1, "the last trial ends the run");
        let stats = &finished[0].stats;
        assert_eq!((stats.hits, stats.shots, stats.expired), (SCORED_TRIALS - 2, SCORED_TRIALS - 1, 2));
        assert!(stats.duration < 200.0, "not the drill length or the cap: {} s", stats.duration);
        let test = app.world().resource::<ReactionTest>();
        assert!(test.finished());
        assert_eq!((test.trials().len(), test.false_starts()), (SCORED_TRIALS as usize, 1));
        assert_eq!((test.trials()[7], test.trials()[19]), (Trial::Lapse, Trial::Lapse));
        // The score is the median's, and the history's average is the mean of the valid trials.
        let summary = summary(test.trials()).unwrap();
        assert_eq!(summary.count, 28);
        assert_eq!(stats.score(), score(summary.median));
        assert!((stats.avg_kill_ms() - summary.mean).abs() < 0.01);
        assert!((test.frame_ms() - DT * 1000.0).abs() < 0.01);
        let banner = app.world().resource::<CueBanner>();
        assert_eq!(banner.caption, "Test complete");
    }

    #[test]
    fn pausing_mid_wait_neither_shows_the_dot_nor_shortens_the_wait() {
        let mut app = app();
        let Stage::Waiting { until } = app.world().resource::<ReactionTest>().stage else { panic!("waiting") };
        for _ in 0..50 {
            frame(&mut app, false);
        }
        press(&mut app, KeyCode::Escape);
        let paused_at = elapsed(&app);
        assert!(paused_at < until);
        // Paused for far longer than the wait, with clicks on the pause menu.
        for step in 0..1000 {
            frame(&mut app, step % 100 == 0);
            assert!(!shown(&mut app));
        }
        assert_eq!(elapsed(&app), paused_at);
        // Resumed: through the countdown and the rest of the wait, nothing shows.
        press(&mut app, KeyCode::Escape);
        while elapsed(&app) < until {
            assert!(!shown(&mut app), "shown {} s early", until - elapsed(&app));
            frame(&mut app, false);
        }
        frame(&mut app, false);
        assert!(shown(&mut app), "the dot appears once the wait is over in run time");
        let test = app.world().resource::<ReactionTest>();
        assert_eq!((test.false_starts(), test.practice_done, test.attempts), (0, 0, 1));
    }

    #[test]
    fn pausing_with_the_dot_up_takes_the_trial_again() {
        let mut app = app();
        until_shown(&mut app);
        frame(&mut app, false);
        press(&mut app, KeyCode::Escape);
        frame(&mut app, false);
        assert!(!shown(&mut app), "the dot is gone behind the pause menu");
        press(&mut app, KeyCode::Escape);
        let resumed_at = elapsed(&app);
        let Stage::Waiting { until } = app.world().resource::<ReactionTest>().stage else { panic!("waiting again") };
        assert!(until >= resumed_at + WAIT_SECS.0, "a full new wait");
        until_shown(&mut app);
        assert!(elapsed(&app) >= until);
        let test = app.world().resource::<ReactionTest>();
        assert_eq!((test.practice_done, test.false_starts()), (0, 0), "the voided trial counted for nothing");
    }
}
