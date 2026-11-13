//! Go / no-go: after a random wait a cue appears in the middle of the view in one of two looks.
//! An enemy (a ball in the target color and style) is to be shot as fast as possible; a friendly
//! (a dim grey ring) is to be left alone. Most cues are enemies, so the click is the habit and
//! holding it back is the work: the decision Valorant asks for when a teammate swings into the
//! crosshair.
//!
//! The two looks differ in shape (filled ball, hollow ring) and in brightness, not only in hue,
//! so they can be told apart without color vision. Any click answers a cue; aiming is not part
//! of it. How a click is timed is described in [`cue`](crate::drill::cue).

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use crate::camera::PlayerCamera;
use crate::drill::cue::{ANTICIPATION_SECS, Cue, CueBanner, Frame, Tone, ahead};
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_respawn, no_spawn_next};
use crate::drill::geometry::{TARGET_DISTANCE, random_range};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{Dormant, SpawnCtx};
use crate::drill::{DrillConfig, DrillSet, Run};
use crate::replay::Recorder;
use crate::settings::Settings;
use crate::sfx::HitSounds;
use crate::{AppState, PlayState};

/// Seconds a cue stays up: an enemy not shot by then is missed, a friendly not shot is spared.
/// Long enough for a considered answer (choice reactions run 300 to 500 ms), short enough that
/// waiting to be sure costs enemies.
pub const WINDOW_SECS: f32 = 0.75;
/// Enemies and friendlies in every block of eight cues, in random order: three in four are
/// enemies, so the click becomes the habit, and a run's share of friendlies is never down to
/// luck.
const BLOCK: (u32, u32) = (6, 2);

pub const DEF: DrillDef = DrillDef {
    name: "Go / no-go",
    blurb: |_| "Shoot the ball (enemy) the moment it appears; hold fire on the grey ring (friendly). No aiming.".into(),
    category: Category::Eyes,
    base_radius: 0.8,
    decay: DecayRule::None,
    score: ScoreModel::GoNoGo,
    ttk_from: TtkFrom::Spawn,
    click_kills: false,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    endless: false,
    params: &[ParamId::DelayMin, ParamId::DelayMax],
    tune_defaults: tune,
    spawn_initial,
    respawn: no_respawn,
    spawn_next: no_spawn_next,
};

/// Shorter waits than the Reaction drill's, for more decisions per minute: 0.8 to 2 s between
/// an answer and the next cue.
fn tune(config: &mut DrillConfig) {
    (config.delay_min, config.delay_max) = (0.8, 2.0);
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<GoNoGo>()
        .add_systems(Startup, create_ring)
        .add_systems(OnEnter(AppState::Playing), |mut drill: ResMut<GoNoGo>| *drill = GoNoGo::default())
        .add_systems(Update, run_cues.in_set(DrillSet::Input))
        .add_systems(OnEnter(PlayState::Paused), void_shown_cue);
}

/// The friendly look's mesh: a ring facing the eye.
#[derive(Resource)]
struct FriendlyRing(Handle<Mesh>);

fn create_ring(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    // Bevy's torus lies flat; targets face the eye with their +Z pole.
    let ring = Mesh::from(Torus::new(0.56, 1.0)).rotated_by(Quat::from_rotation_x(FRAC_PI_2));
    commands.insert_resource(FriendlyRing(meshes.add(ring)));
}

/// One of the two cues: hidden except while it is the one on screen.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    Enemy,
    Friendly,
}

/// How a cue, or a click without one, ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// An enemy shot, this many milliseconds after it appeared.
    Hit(f32),
    /// An enemy not shot in time.
    Miss,
    /// A friendly left alone.
    Held,
    /// A click on a friendly.
    ShotFriendly,
    /// A click with nothing on screen, or too soon after an enemy appeared to have seen it.
    ShotNothing,
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
    Shown(Look, Cue),
}

/// What a frame did.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Event {
    /// The first wait began.
    Began,
    /// This cue appears this frame.
    Show(Look),
    /// A cue (or a click without one) ended; the cue, if up, goes away.
    Ended(Outcome),
}

/// The current (or last finished) Go / no-go run: what is on screen and the tally the score is
/// worked out from.
#[derive(Resource, Debug, Default)]
pub struct GoNoGo {
    stage: Stage,
    /// Enemies and friendlies left in the current block.
    block: (u32, u32),
    /// The look of the next cue, if the screenshot tour asked for one.
    staged: Option<Look>,
    hits: u32,
    misses: u32,
    held: u32,
    /// Clicks without an enemy to shoot: on a friendly, or on nothing.
    false_alarms: u32,
    /// Sum of the hits' reaction times, ms.
    reaction_ms: f32,
    last: Option<Outcome>,
}

impl GoNoGo {
    /// Advances by one live frame in which the button was `clicked` or not; `delay` is the
    /// range the waits are drawn from.
    fn step(&mut self, frame: Frame, clicked: bool, delay: (f32, f32)) -> Option<Event> {
        let outcome = match self.stage {
            Stage::Idle => {
                self.begin_wait(frame.now, delay);
                return Some(Event::Began);
            },
            // A click in the frame the cue would appear in came before it could be seen.
            Stage::Waiting { .. } if clicked => Outcome::ShotNothing,
            Stage::Waiting { until } if frame.now >= until => {
                let look = self.next_look();
                self.stage = Stage::Shown(look, Cue::Drawn);
                return Some(Event::Show(look));
            },
            Stage::Waiting { .. } => return None,
            Stage::Shown(look, mut cue) => {
                let onset = cue.onset(frame);
                self.stage = Stage::Shown(look, cue);
                let reaction = frame.input() - onset;
                match (look, clicked) {
                    (Look::Friendly, true) => Outcome::ShotFriendly,
                    (Look::Enemy, true) if reaction < ANTICIPATION_SECS => Outcome::ShotNothing,
                    (Look::Enemy, true) => Outcome::Hit(reaction * 1000.0),
                    (_, false) if frame.started() - onset < WINDOW_SECS => return None,
                    (Look::Enemy, false) => Outcome::Miss,
                    (Look::Friendly, false) => Outcome::Held,
                }
            },
        };
        match outcome {
            Outcome::Hit(ms) => {
                self.hits += 1;
                self.reaction_ms += ms;
            },
            Outcome::Miss => self.misses += 1,
            Outcome::Held => self.held += 1,
            Outcome::ShotFriendly | Outcome::ShotNothing => self.false_alarms += 1,
        }
        self.last = Some(outcome);
        self.begin_wait(frame.now, delay);
        Some(Event::Ended(outcome))
    }

    fn begin_wait(&mut self, now: f32, delay: (f32, f32)) {
        self.stage = Stage::Waiting { until: now + random_range(delay.0, delay.1) };
    }

    /// Draws the next cue's look from the current block without putting it back.
    fn next_look(&mut self) -> Look {
        if self.block == (0, 0) {
            self.block = BLOCK;
        }
        let (enemies, friendlies) = self.block;
        let look = match self.staged.take() {
            Some(look) => look,
            None if fastrand::u32(0..enemies + friendlies) < enemies => Look::Enemy,
            None => Look::Friendly,
        };
        match look {
            Look::Enemy => self.block.0 = enemies.saturating_sub(1),
            Look::Friendly => self.block.1 = friendlies.saturating_sub(1),
        }
        look
    }

    /// A cue that is up goes away without counting for or against anything: the run was paused
    /// over it. Returns whether there was one.
    fn void_shown(&mut self, now: f32, delay: (f32, f32)) -> bool {
        let shown = matches!(self.stage, Stage::Shown(..));
        if shown {
            self.begin_wait(now, delay);
            self.last = None;
        }
        shown
    }

    /// The next cue appears on the next live frame, as a friendly or an enemy: for the
    /// screenshot tour, which can wait for neither the delay nor the draw.
    pub(crate) fn cue_now(&mut self, friendly: bool) {
        if let Stage::Waiting { until } = &mut self.stage {
            *until = f32::NEG_INFINITY;
            self.staged = Some(if friendly { Look::Friendly } else { Look::Enemy });
        }
    }

    /// The run's score so far, see [`score`].
    pub fn score(&self) -> f32 {
        let mean_ms = if self.hits == 0 { 0.0 } else { self.reaction_ms / self.hits as f32 };
        score(mean_ms, self.hits, self.misses, self.held, self.false_alarms)
    }

    fn banner(&self) -> CueBanner {
        let (result, tone, hint) = match self.last {
            None => (String::new(), Tone::Neutral, "Any click shoots. No need to aim."),
            Some(Outcome::Hit(ms)) => (format!("{ms:.0} ms"), Tone::Good, ""),
            Some(Outcome::Miss) => ("Missed".to_string(), Tone::Warn, "The enemy got away: shoot the ball sooner."),
            Some(Outcome::Held) => ("Held".to_string(), Tone::Good, "Friendly spared."),
            Some(Outcome::ShotFriendly) => {
                ("Friendly fire".to_string(), Tone::Bad, "False alarm: the grey ring is a teammate.")
            },
            Some(Outcome::ShotNothing) => {
                ("Too early".to_string(), Tone::Bad, "False alarm: there was nothing to shoot yet.")
            },
        };
        CueBanner { caption: "Ball: shoot  ·  Ring: hold".to_string(), result, tone, hint }
    }
}

/// The Go / no-go score: fast and correct, where correct means both shooting what is hostile
/// and holding on what is not.
///
/// `100 000 / mean reaction ms on the enemies hit` (the Reaction drill's speed scale: 200 ms =
/// 500) `x hits / enemies shown x holds / (holds + false alarms)`, rounded. A hold is a friendly
/// left alone; a false alarm is a click without an enemy to shoot (on a friendly, or on nothing).
/// Without any hold or false alarm yet the last factor is 1. Shooting everything scores 0 (no
/// holds), and so does shooting nothing (no hits): only telling the two apart scores, which
/// accuracy over shots (the Reaction drill's score) would not ask for. Persisted in the
/// history: never change it.
pub fn score(mean_ms: f32, hits: u32, misses: u32, held: u32, false_alarms: u32) -> f32 {
    if hits == 0 || mean_ms <= 0.0 {
        return 0.0;
    }
    let hit_share = hits as f32 / (hits + misses) as f32;
    let hold_share = if held + false_alarms == 0 { 1.0 } else { held as f32 / (held + false_alarms) as f32 };
    (100_000.0 / mean_ms * hit_share * hold_share).round()
}

/// Both cues, hidden until one is shown. The friendly is the grey of the Reaction drill's unlit
/// cue: dimmer than every target color, and visible in every scene. It stays [`Dormant`] even
/// while shown: replays only know targets, and it must not be replayed as one.
fn spawn_initial(ctx: &mut SpawnCtx) {
    let centre = Vec3::new(0.0, 0.0, -TARGET_DISTANCE);
    ctx.spawn_target(centre, None).insert((Look::Enemy, Dormant, Visibility::Hidden));
    let grey = ctx.assets.wait.clone();
    ctx.spawn_target(centre, None).insert((Look::Friendly, Dormant, Visibility::Hidden, MeshMaterial3d(grey))).queue(
        |mut cue: EntityWorldMut| {
            if let Some(ring) = cue.get_resource::<FriendlyRing>() {
                let mesh = Mesh3d(ring.0.clone());
                cue.insert(mesh);
            }
        },
    );
}

/// Shows a cue when a wait is over and takes a click (aimed anywhere) as the answer.
///
/// In the run's counters every click is a shot, a hit a kill (its time the kill time) and a
/// missed enemy an expired target, so `shots - hits` are the false alarms; the score, which
/// also needs the friendlies held, is set through [`Run::rate`].
#[allow(clippy::too_many_arguments)]
fn run_cues(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut run: ResMut<Run>,
    mut drill: ResMut<GoNoGo>,
    mut banner: ResMut<CueBanner>,
    sounds: Res<HitSounds>,
    settings: Res<Settings>,
    mut recorder: ResMut<Recorder>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Look>)>,
    mut cues: Populated<(Entity, &Look, &mut Transform, &mut Visibility)>,
) {
    if !run.live() {
        return;
    }
    let clicked = mouse.just_pressed(MouseButton::Left);
    let delay = (run.drill.config.delay_min, run.drill.config.delay_max);
    let Some(event) = drill.step(Frame::of(&run, &time), clicked, delay) else { return };
    match event {
        Event::Began => {},
        Event::Show(shown) => {
            for (entity, look, mut transform, mut visibility) in cues.iter_mut() {
                if *look != shown {
                    continue;
                }
                transform.translation = ahead(camera.forward().as_vec3(), transform.scale.x);
                *visibility = Visibility::Inherited;
                if shown == Look::Enemy {
                    commands.entity(entity).remove::<Dormant>();
                }
            }
        },
        Event::Ended(outcome) => {
            for (entity, _, _, mut visibility) in cues.iter_mut() {
                if *visibility != Visibility::Hidden {
                    *visibility = Visibility::Hidden;
                    commands.entity(entity).insert(Dormant);
                }
            }
            if clicked {
                run.shot();
                recorder.mark_shot(matches!(outcome, Outcome::Hit(_)));
            }
            match outcome {
                Outcome::Hit(ms) => {
                    run.kill(ms / 1000.0);
                    sounds.play(&mut commands, &settings.audio);
                },
                Outcome::Miss => run.expire(),
                Outcome::Held | Outcome::ShotFriendly | Outcome::ShotNothing => {},
            }
            run.rate(drill.score());
        },
    }
    *banner = drill.banner();
}

/// Pausing while a cue is up voids it: the pause would otherwise be all the time in the world
/// to decide. The next cue comes after a new wait once the run resumes.
fn void_shown_cue(
    mut commands: Commands,
    run: Res<Run>,
    mut drill: ResMut<GoNoGo>,
    mut banner: ResMut<CueBanner>,
    mut cues: Query<(Entity, &mut Visibility), With<Look>>,
) {
    let delay = (run.drill.config.delay_min, run.drill.config.delay_max);
    if !drill.void_shown(run.elapsed(), delay) {
        return;
    }
    for (entity, mut visibility) in &mut cues {
        *visibility = Visibility::Hidden;
        commands.entity(entity).insert(Dormant);
    }
    *banner = CueBanner { hint: "Paused with a cue up: it doesn't count.", ..drill.banner() };
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::drill::run::{RunFinished, tick_run};
    use crate::drill::{DrillKind, DrillSpec, RunPurpose};

    /// Simulated frame time, seconds.
    const DT: f32 = 0.01;
    const DELAY: (f32, f32) = (0.8, 2.0);

    /// Drives the procedure alone, one 10 ms frame per call.
    struct Bench {
        drill: GoNoGo,
        frame: u32,
    }

    impl Bench {
        fn new() -> Self {
            let mut bench = Self { drill: GoNoGo::default(), frame: 0 };
            assert_eq!(bench.frame(false), Some(Event::Began));
            bench
        }

        fn frame(&mut self, clicked: bool) -> Option<Event> {
            let frame = Frame { now: self.frame as f32 * DT, dt: DT };
            self.frame += 1;
            self.drill.step(frame, clicked, DELAY)
        }

        /// The next cue, shown as `look` on the next frame.
        fn show(&mut self, look: Look) {
            self.drill.cue_now(look == Look::Friendly);
            assert_eq!(self.frame(false), Some(Event::Show(look)));
        }

        /// Frames without a click until something happens.
        fn wait(&mut self) -> Event {
            for _ in 0..1000 {
                if let Some(event) = self.frame(false) {
                    return event;
                }
            }
            panic!("nothing happened");
        }

        /// A click read `frames` frames after the one that drew the cue.
        fn click_after(&mut self, frames: u32) -> Option<Event> {
            for _ in 0..frames - 1 {
                assert_eq!(self.frame(false), None);
            }
            self.frame(true)
        }

        fn tally(&self) -> (u32, u32, u32, u32) {
            (self.drill.hits, self.drill.misses, self.drill.held, self.drill.false_alarms)
        }
    }

    #[test]
    fn counts_hits_misses_holds_and_false_alarms() {
        let mut bench = Bench::new();
        // An enemy shot 30 frames after it was drawn: on screen 28.5 frames before the click.
        bench.show(Look::Enemy);
        let Some(Event::Ended(Outcome::Hit(ms))) = bench.click_after(30) else { panic!("a hit") };
        assert!((ms - 285.0).abs() < 0.01, "{ms}");
        assert_eq!(bench.tally(), (1, 0, 0, 0));
        // An enemy left alone is missed once its window is over.
        bench.show(Look::Enemy);
        let shown_at = bench.frame;
        assert_eq!(bench.wait(), Event::Ended(Outcome::Miss));
        let window = (bench.frame - shown_at) as f32 * DT;
        assert!((window - WINDOW_SECS).abs() <= 2.0 * DT, "missed after {window} s");
        assert_eq!(bench.tally(), (1, 1, 0, 0));
        // A friendly left alone is held; one shot is a false alarm, however late.
        bench.show(Look::Friendly);
        assert_eq!(bench.wait(), Event::Ended(Outcome::Held));
        bench.show(Look::Friendly);
        assert_eq!(bench.click_after(60), Some(Event::Ended(Outcome::ShotFriendly)));
        assert_eq!(bench.tally(), (1, 1, 1, 1));
        // So is a click with nothing on screen, and one too soon after an enemy to have seen it.
        assert_eq!(bench.frame(true), Some(Event::Ended(Outcome::ShotNothing)));
        bench.show(Look::Enemy);
        assert_eq!(bench.click_after(10), Some(Event::Ended(Outcome::ShotNothing)));
        assert_eq!(bench.tally(), (1, 1, 1, 3));
        // 285 ms, half the enemies hit, one hold in four chances to.
        assert_eq!(bench.drill.score(), (100_000.0f32 / 285.0 * 0.5 * 0.25).round());
        // Every answer is followed by a wait from the configured range.
        let ended = bench.frame;
        assert!(matches!(bench.wait(), Event::Show(_)));
        let waited = (bench.frame - ended) as f32 * DT;
        assert!((DELAY.0..=DELAY.1 + 2.0 * DT).contains(&waited), "{waited}");
    }

    #[test]
    fn most_cues_are_enemies_in_every_block() {
        // The same draws every time.
        fastrand::seed(7);
        let mut drill = GoNoGo::default();
        let mut friendly_first = 0;
        for _ in 0..50 {
            let block: Vec<Look> = (0..BLOCK.0 + BLOCK.1).map(|_| drill.next_look()).collect();
            assert_eq!(block.iter().filter(|look| **look == Look::Enemy).count() as u32, BLOCK.0, "{block:?}");
            friendly_first += u32::from(block[0] == Look::Friendly);
        }
        // The order within a block is random: a friendly leads one block in four.
        assert!((2..=30).contains(&friendly_first), "{friendly_first} of 50 blocks start with a friendly");
    }

    #[test]
    fn score_wants_speed_and_both_kinds_of_correct() {
        let clean = score(300.0, 20, 0, 6, 0);
        assert_eq!(clean, 333.0);
        assert!(score(250.0, 20, 0, 6, 0) > clean, "faster is better");
        assert!(score(300.0, 18, 2, 6, 0) < clean, "missed enemies cost");
        assert!(score(300.0, 20, 0, 5, 1) < clean, "shot friendlies cost");
        // Shooting everything is fast, and worth nothing; so is shooting nothing.
        assert_eq!(score(200.0, 20, 0, 0, 6), 0.0);
        assert_eq!(score(0.0, 0, 20, 6, 0), 0.0);
        // One friendly shot costs more than one enemy missed: there are fewer of them.
        assert!(score(300.0, 20, 0, 5, 1) < score(300.0, 19, 1, 6, 0));
        // No friendly yet, no false alarm: nothing to hold against the run.
        assert_eq!(score(300.0, 3, 0, 0, 0), clean);
    }

    /// The system with the run timer: counters and the score reach the finished run, and the
    /// run lasts the drill length.
    #[test]
    fn a_run_is_time_boxed_and_rated() {
        let settings = Settings::default();
        let drill = DrillSpec::builtin(DrillKind::GoNoGo, &settings);
        assert_eq!((drill.config.delay_min, drill.config.delay_max), DELAY);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(DT)))
            .insert_resource(settings.clone())
            .insert_resource(Run::new(drill, 0.4, 5.0, RunPurpose::Practice, String::new()).started())
            .init_resource::<GoNoGo>()
            .init_resource::<CueBanner>()
            .init_resource::<HitSounds>()
            .init_resource::<Recorder>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<RunFinished>()
            .add_systems(Update, (run_cues, tick_run).chain());
        app.world_mut().spawn((PlayerCamera, Transform::default()));
        let enemy = app.world_mut().spawn((Look::Enemy, Dormant, Visibility::Hidden, Transform::default())).id();
        let friendly = app.world_mut().spawn((Look::Friendly, Dormant, Visibility::Hidden, Transform::default())).id();
        let frame = |app: &mut App, click: bool| {
            if click {
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            }
            app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        };
        let visible = |app: &App, cue: Entity| *app.world().get::<Visibility>(cue).unwrap() != Visibility::Hidden;
        let show = |app: &mut App, look: Look| {
            app.world_mut().resource_mut::<GoNoGo>().cue_now(look == Look::Friendly);
            frame(app, false);
        };
        frame(&mut app, false);

        // An enemy, shot 250 ms after the frame that drew it.
        show(&mut app, Look::Enemy);
        assert!(visible(&app, enemy) && !visible(&app, friendly));
        assert!(app.world().get::<Dormant>(enemy).is_none(), "a shown enemy is recorded in the replay");
        for _ in 0..24 {
            frame(&mut app, false);
        }
        frame(&mut app, true);
        assert!(!visible(&app, enemy) && app.world().get::<Dormant>(enemy).is_some());
        // A friendly, shot too.
        show(&mut app, Look::Friendly);
        assert!(visible(&app, friendly) && !visible(&app, enemy));
        assert!(app.world().get::<Dormant>(friendly).is_some(), "a friendly is never replayed as a target");
        frame(&mut app, true);
        assert!(!visible(&app, friendly));
        // A friendly held, then an enemy missed.
        show(&mut app, Look::Friendly);
        for _ in 0..(WINDOW_SECS / DT) as u32 + 3 {
            frame(&mut app, false);
        }
        assert!(!visible(&app, friendly));
        show(&mut app, Look::Enemy);
        for _ in 0..(WINDOW_SECS / DT) as u32 + 3 {
            frame(&mut app, false);
        }
        assert!(!visible(&app, enemy));

        let mut frames = 0;
        while app.world().resource::<Messages<RunFinished>>().is_empty() {
            // Whatever comes up until the time is over is shot in the next frame, before it can
            // have been seen: all false alarms.
            let up = visible(&app, enemy) || visible(&app, friendly);
            frame(&mut app, up);
            frames += 1;
            assert!(frames < 1000, "the run never ended");
        }
        let finished: Vec<_> = app.world_mut().resource_mut::<Messages<RunFinished>>().drain().collect();
        let stats = &finished[0].stats;
        assert!((stats.duration - 5.0).abs() < 0.05, "{} s", stats.duration);
        let tally = app.world().resource::<GoNoGo>();
        assert_eq!((tally.hits, tally.misses, tally.held), (1, 1, 1));
        assert!(tally.false_alarms >= 1);
        // The counters the HUD and the history read: hits, misses, and false alarms as the
        // shots that hit nothing.
        assert_eq!((stats.hits, stats.expired, stats.shots - stats.hits), (1, 1, tally.false_alarms));
        assert!((stats.avg_kill_ms() - 235.0).abs() < 0.01, "{}", stats.avg_kill_ms());
        assert_eq!(stats.score(), score(235.0, 1, 1, 1, tally.false_alarms));
        assert_eq!(stats.score(), tally.score());
    }
}
