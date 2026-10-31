//! The run lifecycle: the [`Run`] resource and its stats, the ready gate and countdown, the
//! timer, aborting, and announcing a finished run as [`RunFinished`]. What happens next
//! (history, replay, finders, routines, results) is up to the systems reading that message.

use bevy::{prelude::*, window::WindowFocused};

use super::{DrillKind, DrillSpec, RunStats, TargetAssets, targets};
use crate::{AppState, settings::Settings};

const COUNTDOWN_SECS: f32 = 1.5;
const READY_ARM_SECS: f32 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunPurpose {
    Practice,
    FinderTrial,
    /// A step of a warmup routine; logged as practice.
    Routine,
}

impl RunPurpose {
    /// How the run is logged in the history.
    pub fn logged_as(self) -> RunPurpose {
        match self {
            RunPurpose::FinderTrial => RunPurpose::FinderTrial,
            RunPurpose::Practice | RunPurpose::Routine => RunPurpose::Practice,
        }
    }

    /// Persisted: the history CSV's `purpose` column. Never change these strings.
    pub fn csv_name(self) -> &'static str {
        match self.logged_as() {
            RunPurpose::FinderTrial => "FinderTrial",
            _ => "Practice",
        }
    }

    /// Reads [`csv_name`](Self::csv_name) back.
    pub fn from_csv(name: &str) -> Option<RunPurpose> {
        match name {
            "Practice" => Some(RunPurpose::Practice),
            "FinderTrial" => Some(RunPurpose::FinderTrial),
            _ => None,
        }
    }

    /// Where Escape or alt-tab leaves to.
    pub fn return_state(self) -> AppState {
        match self {
            RunPurpose::Practice => AppState::Menu,
            RunPurpose::FinderTrial => AppState::Finder,
            RunPurpose::Routine => AppState::Routine,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for the player to click/press Space. Nothing starts on its own.
    Ready,
    Countdown,
    Live,
}

/// The drill currently being played.
#[derive(Resource)]
pub struct Run {
    pub drill: DrillSpec,
    pub sens: f32,
    pub duration: f32,
    pub purpose: RunPurpose,
    /// Shown in the HUD. Finder trials use a blind label so the sens isn't revealed.
    pub title: String,
    phase: Phase,
    countdown: f32,
    /// Seconds the ready gate has been shown; input is ignored until [`READY_ARM_SECS`].
    ready_for: f32,
    pub(super) elapsed: f32,
    /// Live seconds before which nothing counts (finder warm-in); 0 for everything else.
    scoring_from: f32,
    hits: u32,
    shots: u32,
    /// Targets that expired (decay, retreating peeks, ignored reaction cues).
    expired: u32,
    ttk_total: f32,
    best_ttk: f32,
    pub(super) last_kill: f32,
    on_target: f32,
    /// First-flick landing ratios (Microflick), see [`FlickProbe`](crate::metrics::flick_probe::FlickProbe).
    landings: Vec<f32>,
    /// Set by Escape/alt-tab; nothing else happens to the run while the state changes.
    aborted: bool,
}

impl Run {
    pub fn new(drill: DrillSpec, sens: f32, duration: f32, purpose: RunPurpose, title: String) -> Self {
        let mut drill = drill;
        drill.config.sanitize();
        Self {
            drill,
            sens,
            duration,
            purpose,
            title,
            phase: Phase::Ready,
            countdown: COUNTDOWN_SECS,
            ready_for: 0.0,
            elapsed: 0.0,
            scoring_from: 0.0,
            hits: 0,
            shots: 0,
            expired: 0,
            ttk_total: 0.0,
            best_ttk: f32::MAX,
            last_kill: 0.0,
            on_target: 0.0,
            landings: Vec::new(),
            aborted: false,
        }
    }

    /// The first `secs` of live time play normally but don't count toward the stats.
    pub fn with_warm_in(mut self, secs: f32) -> Self {
        self.scoring_from = secs;
        self
    }

    pub fn kind(&self) -> DrillKind {
        self.drill.config.kind
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Seconds left in the countdown.
    pub fn countdown(&self) -> f32 {
        self.countdown
    }

    /// Live seconds played so far.
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// Seconds of warm-in left; 0 or less once events count.
    pub fn warm_in_left(&self) -> f32 {
        self.scoring_from - self.elapsed
    }

    pub(crate) fn live(&self) -> bool {
        self.phase == Phase::Live && !self.aborted
    }

    /// Seconds left while the run is live; `None` before the start (ready gate, countdown).
    pub fn time_left(&self) -> Option<f32> {
        self.live().then(|| (self.duration - self.elapsed).max(0.0))
    }

    /// Whether events count toward the stats (warm-in is over).
    pub fn counting(&self) -> bool {
        self.elapsed >= self.scoring_from
    }

    pub(crate) fn shot(&mut self) {
        if self.counting() {
            self.shots += 1;
        }
    }

    /// Records a kill `ttk` seconds after the target appeared (or after the previous kill).
    pub(crate) fn kill(&mut self, ttk: f32) {
        self.last_kill = self.elapsed;
        if self.counting() {
            self.hits += 1;
            self.ttk_total += ttk;
            self.best_ttk = self.best_ttk.min(ttk);
        }
    }

    pub(crate) fn expire(&mut self) {
        if self.counting() {
            self.expired += 1;
        }
    }

    pub(crate) fn track_on_target(&mut self, dt: f32) {
        if self.counting() {
            self.on_target += dt;
        }
    }

    pub(crate) fn land(&mut self, ratio: f32) {
        if self.counting() {
            self.landings.push(ratio);
        }
    }

    pub fn stats(&self, settings: &Settings) -> RunStats {
        RunStats {
            kind: self.kind(),
            drill: self.drill.name.clone(),
            sens: self.sens,
            cm_per_360: settings.cm_per_360(self.sens),
            duration: (self.elapsed - self.scoring_from).max(f32::EPSILON),
            hits: self.hits,
            shots: self.shots,
            expired: self.expired,
            ttk_total: self.ttk_total,
            best_ttk: self.best_ttk,
            on_target: self.on_target,
            landings: self.landings.clone(),
        }
    }
}

/// A run played to the end (not aborted).
#[derive(Message, Clone, Debug)]
pub struct RunFinished {
    pub stats: RunStats,
    pub purpose: RunPurpose,
    /// What was played, so it can be played again.
    pub drill: DrillSpec,
    pub duration: f32,
}

/// Starts a drill. The `Run` resource is read by `OnEnter(Playing)` systems.
pub fn begin_run(commands: &mut Commands, next_state: &mut NextState<AppState>, run: Run) {
    commands.insert_resource(run);
    next_state.set(AppState::Playing);
}

/// Starts the countdown on click or Space, then the drill once it reaches zero.
pub(super) fn ready_gate(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<Run>,
    assets: Res<TargetAssets>,
    mut next: ResMut<targets::NextSpawn>,
) {
    if run.aborted {
        return;
    }
    match run.phase {
        Phase::Ready => {
            // Clicks still in flight from the previous run (shooting at the buzzer) must not
            // skip this gate: only accept input once it has been up for a moment.
            run.ready_for += time.delta_secs();
            if run.ready_for >= READY_ARM_SECS && (mouse.just_pressed(MouseButton::Left) || keys.just_pressed(KeyCode::Space)) {
                run.phase = Phase::Countdown;
                targets::spawn_initial(&mut commands, &assets, &mut next, &run);
            }
        }
        Phase::Countdown => {
            run.countdown -= time.delta_secs();
            if run.countdown <= 0.0 {
                run.phase = Phase::Live;
            }
        }
        Phase::Live => {}
    }
}

/// Advances the timer and announces the run once it's over.
pub(super) fn tick_run(time: Res<Time>, mut run: ResMut<Run>, settings: Res<Settings>, mut finished: MessageWriter<RunFinished>) {
    if !run.live() {
        return;
    }
    run.elapsed += time.delta_secs();
    if run.elapsed < run.duration {
        return;
    }
    finished.write(RunFinished {
        stats: run.stats(&settings),
        purpose: run.purpose,
        drill: run.drill.clone(),
        duration: run.duration,
    });
}

/// Escape or alt-tab abandons the run without recording it. Runs before everything else, so the
/// rest of the frame's systems (including `tick_run`) see the run as no longer live.
pub(super) fn abort_run(
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: MessageReader<WindowFocused>,
    mut run: ResMut<Run>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let lost_focus = focus.read().any(|event| !event.focused);
    if run.aborted || (!keys.just_pressed(KeyCode::Escape) && !lost_focus) {
        return;
    }
    run.aborted = true;
    next_state.set(run.purpose.return_state());
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bevy::state::app::StatesPlugin;

    use super::*;
    use crate::{
        drill::DrillSet,
        finder::Finder,
        history::{self, History, LastRun},
        replay::{self, Recorder, ReplayDir},
        routine::{self, Routine, RoutineStep},
        settings::FinderFocus,
        smart_finder::{self, SmartFinder},
        ui::screens::results,
    };

    #[test]
    fn warm_in_is_not_scored() {
        let settings = Settings::default();
        let drill = DrillSpec::builtin(DrillKind::Microflick, &settings);
        let mut run = Run::new(drill, 0.4, 13.0, RunPurpose::FinderTrial, String::new()).with_warm_in(3.0);
        // During the warm-in: none of this counts.
        run.elapsed = 1.0;
        run.shot();
        run.kill(0.3);
        run.shot();
        run.expire();
        run.land(1.4);
        run.track_on_target(0.5);
        // After it: one clean kill.
        run.elapsed = 5.0;
        run.shot();
        run.kill(0.4);
        run.land(0.9);
        run.elapsed = 13.0;
        let stats = run.stats(&settings);
        assert_eq!((stats.hits, stats.shots, stats.expired), (1, 1, 0));
        assert_eq!(stats.landings, vec![0.9]);
        assert_eq!(stats.on_target, 0.0);
        // Rates use the 10 counted seconds: 1 kill at 100% = 6 kills/min.
        assert!((stats.duration - 10.0).abs() < 1e-5);
        assert_eq!(stats.score(), 600.0);
        assert!((stats.avg_kill_ms() - 400.0).abs() < 1e-3);
    }

    #[test]
    fn purpose_csv_names_are_stable() {
        assert_eq!(RunPurpose::Practice.csv_name(), "Practice");
        assert_eq!(RunPurpose::Routine.csv_name(), "Practice");
        assert_eq!(RunPurpose::FinderTrial.csv_name(), "FinderTrial");
        for purpose in [RunPurpose::Practice, RunPurpose::FinderTrial] {
            assert_eq!(RunPurpose::from_csv(purpose.csv_name()), Some(purpose));
        }
        assert_eq!(RunPurpose::from_csv("Routine"), None);
    }

    /// A temp directory removed again when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let dir = std::env::temp_dir().join(format!("aim_trainer_{name}_{}_{nanos}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The run lifecycle and every reaction to a finished run, without rendering or the drills'
    /// target systems. History and replays go to `dir`.
    fn lifecycle_app(dir: &TempDir) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(AppState::Playing)
            .insert_resource(Settings::default())
            .insert_resource(History::load(&dir.0.join("results.csv")))
            .insert_resource(ReplayDir(dir.0.join("replays")))
            .insert_resource(Recorder::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<WindowFocused>()
            .add_message::<RunFinished>()
            .add_message::<history::RunRecorded>()
            .configure_sets(
                Update,
                (DrillSet::Start, DrillSet::Finish, DrillSet::Log, DrillSet::Outcome)
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                Update,
                (
                    abort_run.in_set(DrillSet::Start),
                    tick_run.in_set(DrillSet::Finish),
                    history::record_finished.in_set(DrillSet::Log),
                    (
                        replay::save_finished_replay,
                        smart_finder::finder_trial_finished,
                        routine::routine_step_finished,
                        results::show_results,
                    )
                        .in_set(DrillSet::Outcome),
                ),
            );
        app
    }

    /// `run`, live and on its final frame.
    fn finishing(mut run: Run) -> Run {
        run.phase = Phase::Live;
        run.elapsed = run.duration;
        run
    }

    fn state(app: &App) -> AppState {
        *app.world().resource::<State<AppState>>().get()
    }

    fn finished_count(app: &App) -> usize {
        app.world().resource::<Messages<RunFinished>>().len()
    }

    #[test]
    fn finished_practice_is_logged_saved_and_shown() {
        let dir = TempDir::new("practice");
        let mut app = lifecycle_app(&dir);
        let drill = DrillSpec::builtin(DrillKind::Gridshot, &Settings::default());
        app.insert_resource(finishing(Run::new(drill, 0.4, 30.0, RunPurpose::Practice, "Gridshot".into())));
        app.update();
        assert_eq!(finished_count(&app), 1);
        let last = app.world().resource::<LastRun>();
        assert_eq!((last.stats.kind, last.duration), (DrillKind::Gridshot, 30.0));
        let timestamp = last.timestamp;
        app.update();
        assert_eq!(state(&app), AppState::Results);
        let csv = std::fs::read_to_string(dir.0.join("results.csv")).unwrap();
        assert_eq!(csv.lines().count(), 2, "header + one row");
        assert!(csv.lines().nth(1).unwrap().starts_with(&format!("{timestamp},Practice,Gridshot,")));
        assert!(dir.0.join("replays").join(format!("{timestamp}.replay")).is_file());
    }

    #[test]
    fn smart_finder_trial_chains_to_its_next_part() {
        let dir = TempDir::new("smart");
        let mut app = lifecycle_app(&dir);
        let settings = Settings::default();
        let smart = SmartFinder::new(0.4, FinderFocus::Balanced);
        let first = smart.segment_run(&settings).unwrap();
        assert!(first.title.ends_with("part 1/2: Microflick"), "{}", first.title);
        app.insert_resource(smart).insert_resource(finishing(first));
        app.update();
        let next = app.world().resource::<Run>();
        assert!(next.title.ends_with("part 2/2: Tracking"), "{}", next.title);
        assert_eq!(next.phase(), Phase::Ready);
        app.update();
        assert_eq!(state(&app), AppState::Playing);
        // Finder trials are logged, but never saved as replays.
        let csv = std::fs::read_to_string(dir.0.join("results.csv")).unwrap();
        assert!(csv.lines().nth(1).unwrap().contains(",FinderTrial,"));
        assert!(!dir.0.join("replays").exists());
    }

    #[test]
    fn classic_finder_trial_returns_to_the_finder() {
        let dir = TempDir::new("classic");
        let mut app = lifecycle_app(&dir);
        let finder = Finder::new(0.4, 3);
        let sens = finder.trial_sens().unwrap();
        let drill = DrillSpec::builtin(DrillKind::Gridshot, &Settings::default());
        app.insert_resource(finder)
            .insert_resource(finishing(Run::new(drill, sens, 20.0, RunPurpose::FinderTrial, String::new())));
        app.update();
        app.update();
        assert_eq!(state(&app), AppState::Finder);
        assert_ne!(app.world().resource::<Finder>().trial_sens(), Some(sens), "moved on to trial B");
    }

    #[test]
    fn routine_step_is_recorded() {
        let dir = TempDir::new("routine");
        let mut app = lifecycle_app(&dir);
        let settings = Settings::default();
        let step = RoutineStep { drill: DrillSpec::builtin(DrillKind::Microflick, &settings), secs: 30.0 };
        app.insert_resource(Routine::new("Test".into(), String::new(), vec![step.clone(), step]));
        let drill = DrillSpec::builtin(DrillKind::Microflick, &settings);
        app.insert_resource(finishing(Run::new(drill, 0.4, 30.0, RunPurpose::Routine, String::new())));
        app.update();
        app.update();
        assert_eq!(state(&app), AppState::Routine);
        let routine = app.world().resource::<Routine>();
        assert_eq!((routine.next, routine.results.len()), (1, 1));
        let csv = std::fs::read_to_string(dir.0.join("results.csv")).unwrap();
        assert!(csv.lines().nth(1).unwrap().contains(",Practice,"), "routine steps are logged as practice");
    }

    #[test]
    fn escape_aborts_without_finishing() {
        for (purpose, back) in [
            (RunPurpose::Practice, AppState::Menu),
            (RunPurpose::FinderTrial, AppState::Finder),
            (RunPurpose::Routine, AppState::Routine),
        ] {
            let dir = TempDir::new("abort");
            let mut app = lifecycle_app(&dir);
            let drill = DrillSpec::builtin(DrillKind::Gridshot, &Settings::default());
            app.insert_resource(finishing(Run::new(drill, 0.4, 30.0, purpose, String::new())));
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
            app.update();
            assert_eq!(finished_count(&app), 0, "{purpose:?}");
            app.update();
            assert_eq!(state(&app), back, "{purpose:?}");
            assert!(!dir.0.join("results.csv").exists());
        }
    }
}
