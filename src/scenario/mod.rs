//! Drills: the run lifecycle (ready gate, countdown, timer), scoring, HUD and recording.
//! Per-drill target behaviour lives in [`drills`].

mod config;
mod drills;
pub mod flick_probe;

use std::borrow::Cow;

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    feathers::{constants::fonts, tokens},
    prelude::*,
    window::WindowFocused,
};
use serde::{Deserialize, Serialize};

pub use config::{DrillConfig, DrillSpec, VARIANT_SEPARATOR};
pub use drills::{TargetAssets, spawn_cover};
use flick_probe::FlickProbe;

use crate::{
    AppState,
    arena::{Look, apply_look},
    finder::Finder,
    history::{Comparison, History},
    replay::Recorder,
    routine::Routine,
    settings::Settings,
    smart_finder::SmartFinder,
    ui::{
        theme::{TEXT_ACCENT, TEXT_STRONG},
        widgets::{hud_anchor, hud_bar, styled, vertical_divider},
    },
};

const COUNTDOWN_SECS: f32 = 1.5;
const READY_ARM_SECS: f32 = 0.6;

pub struct ScenarioPlugin;

impl Plugin for ScenarioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, drills::create_target_assets)
            .add_systems(Update, drills::sync_target_colors.run_if(resource_changed::<Settings>))
            .add_systems(OnEnter(AppState::Playing), (spawn_hud, drills::spawn_static, |mut commands: Commands| {
                commands.insert_resource(Recorder::default())
            }))
            .add_systems(
                Update,
                (
                    // First, so an abort on the run's final frame wins over finishing it.
                    abort_run,
                    ready_gate,
                    (
                        drills::shoot,
                        drills::reaction,
                        drills::decay,
                        drills::probe_flicks,
                        drills::spawn_pending,
                        drills::move_peeks,
                        drills::move_strafers,
                        drills::track,
                    )
                        .chain()
                        .after(apply_look),
                    record_frame,
                    tick_run,
                    update_hud,
                )
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scenario {
    #[default]
    Gridshot,
    Microflick,
    Tracking,
    HeadLevel,
    Peek,
    Reaction,
    Peripheral,
}

impl Scenario {
    /// Order matters: replays store the index. Only ever append.
    pub const ALL: [Scenario; 7] = [
        Scenario::Gridshot,
        Scenario::Microflick,
        Scenario::Tracking,
        Scenario::HeadLevel,
        Scenario::Peek,
        Scenario::Reaction,
        Scenario::Peripheral,
    ];

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Scenario::Gridshot => "Gridshot",
            Scenario::Microflick => "Microflick",
            Scenario::Tracking => "Tracking",
            Scenario::HeadLevel => "Head level",
            Scenario::Peek => "Peeks",
            Scenario::Reaction => "Reaction",
            Scenario::Peripheral => "Peripheral",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Scenario::Gridshot => "3 dots on a 3x3 grid. Speed + accuracy.",
            Scenario::Microflick => "One small dot at a time. Flick precision.",
            Scenario::Tracking => "Keep the crosshair on a strafing dot.",
            Scenario::HeadLevel => "Head-sized dots, always at head height.",
            Scenario::Peek => "Hold an angle: enemies swing out of cover.",
            Scenario::Reaction => "Click the instant the dot lights up.",
            Scenario::Peripheral => "Brief dots far off-center. Spot + flick.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunPurpose {
    Practice,
    FinderTrial,
    /// A step of a warmup routine; logged as practice.
    Routine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
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
    elapsed: f32,
    /// Live seconds before which nothing counts (finder warm-in); 0 for everything else.
    scoring_from: f32,
    hits: u32,
    shots: u32,
    /// Targets that expired (decay, retreating peeks, ignored reaction cues).
    expired: u32,
    ttk_total: f32,
    best_ttk: f32,
    last_kill: f32,
    on_target: f32,
    /// First-flick landing ratios (Microflick), see [`FlickProbe`].
    landings: Vec<f32>,
    /// Measures the current Microflick target's first flick.
    probe: Option<FlickProbe>,
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
            probe: None,
            aborted: false,
        }
    }

    /// The first `secs` of live time play normally but don't count toward the stats.
    pub fn with_warm_in(mut self, secs: f32) -> Self {
        self.scoring_from = secs;
        self
    }

    pub fn scenario(&self) -> Scenario {
        self.drill.config.kind
    }

    fn live(&self) -> bool {
        self.phase == Phase::Live && !self.aborted
    }

    /// Seconds left while the run is live; `None` before the start (ready gate, countdown).
    pub fn time_left(&self) -> Option<f32> {
        self.live().then(|| (self.duration - self.elapsed).max(0.0))
    }

    /// Whether events count toward the stats (warm-in is over).
    fn counting(&self) -> bool {
        self.elapsed >= self.scoring_from
    }

    fn shot(&mut self) {
        if self.counting() {
            self.shots += 1;
        }
    }

    /// Records a kill `ttk` seconds after the target appeared (or after the previous kill).
    fn kill(&mut self, ttk: f32) {
        self.last_kill = self.elapsed;
        if self.counting() {
            self.hits += 1;
            self.ttk_total += ttk;
            self.best_ttk = self.best_ttk.min(ttk);
        }
    }

    fn expire(&mut self) {
        if self.counting() {
            self.expired += 1;
        }
    }

    fn track_on_target(&mut self, dt: f32) {
        if self.counting() {
            self.on_target += dt;
        }
    }

    fn land(&mut self, ratio: f32) {
        if self.counting() {
            self.landings.push(ratio);
        }
    }

    fn stats(&self, settings: &Settings) -> RunStats {
        RunStats {
            scenario: self.scenario(),
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

#[derive(Clone, Debug)]
pub struct RunStats {
    /// Kind of drill, which decides scoring.
    pub scenario: Scenario,
    /// Drill name: the built-in name or a custom scenario's name. History is grouped by it.
    pub drill: String,
    pub sens: f32,
    pub cm_per_360: f32,
    /// Counted seconds: live time after the warm-in.
    pub duration: f32,
    pub hits: u32,
    pub shots: u32,
    pub expired: u32,
    pub ttk_total: f32,
    pub best_ttk: f32,
    pub on_target: f32,
    /// First-flick landing ratios (1 = landed on target); Microflick only.
    pub landings: Vec<f32>,
}

impl RunStats {
    /// Hits over everything that could have been a hit: shots fired plus targets that expired.
    pub fn accuracy(&self) -> f32 {
        let attempts = self.shots + self.expired;
        if attempts == 0 { 0.0 } else { self.hits as f32 / attempts as f32 }
    }

    /// Average time to kill: from appearance (or the previous kill, in Gridshot).
    pub fn avg_kill_ms(&self) -> f32 {
        if self.hits == 0 { 0.0 } else { self.ttk_total / self.hits as f32 * 1000.0 }
    }

    pub fn on_target_ratio(&self) -> f32 {
        self.on_target / self.duration
    }

    /// Single number used to compare runs, independent of drill length.
    /// Flick drills: accuracy-weighted kills per minute (x100). Tracking: time on target in 0.01%
    /// steps. Reaction: 100 000 / average reaction ms, weighted by accuracy.
    pub fn score(&self) -> f32 {
        match self.scenario {
            Scenario::Tracking => (self.on_target_ratio() * 10_000.0).round(),
            Scenario::Reaction if self.hits == 0 => 0.0,
            Scenario::Reaction => (100_000.0 / self.avg_kill_ms() * self.accuracy()).round(),
            _ => (self.hits as f32 * 100.0 * self.accuracy() * 60.0 / self.duration).round(),
        }
    }

    /// The secondary stats shown next to the score, as (caption, value).
    pub fn key_stats(&self) -> Vec<(&'static str, String)> {
        let best_ms = if self.hits == 0 { 0.0 } else { self.best_ttk * 1000.0 };
        match self.scenario {
            Scenario::Tracking => vec![("On target", format!("{:.1}%", self.on_target_ratio() * 100.0))],
            Scenario::Reaction => vec![
                ("Avg reaction", format!("{:.0} ms", self.avg_kill_ms())),
                ("Best", format!("{best_ms:.0} ms")),
                ("Hits", self.hits.to_string()),
                ("Early / late", (self.shots + self.expired - self.hits).to_string()),
                ("Accuracy", format!("{:.1}%", self.accuracy() * 100.0)),
            ],
            _ => {
                let mut stats = vec![
                    ("Hits", format!("{} / {}", self.hits, self.shots)),
                    ("Accuracy", format!("{:.1}%", self.accuracy() * 100.0)),
                    ("Avg kill", format!("{:.0} ms", self.avg_kill_ms())),
                    ("Best kill", format!("{best_ms:.0} ms")),
                    ("Kills / s", format!("{:.2}", self.hits as f32 / self.duration)),
                ];
                if self.expired > 0 {
                    stats.insert(1, ("Expired", self.expired.to_string()));
                }
                stats
            }
        }
    }
}

/// The most recent finished practice run, shown on the results screen.
#[derive(Resource)]
pub struct LastRun {
    pub stats: RunStats,
    /// Earlier practice runs of the same drill, not including this one.
    pub previous: Comparison,
    /// History timestamp, which also identifies the run's replay.
    pub timestamp: u64,
    /// What was played, so "Retry" replays the same (possibly custom) drill.
    pub drill: DrillSpec,
    pub duration: f32,
}

/// Starts a drill. The `Run` resource is read by `OnEnter(Playing)` systems.
pub fn begin_run(commands: &mut Commands, next_state: &mut NextState<AppState>, run: Run) {
    commands.insert_resource(run);
    next_state.set(AppState::Playing);
}

/// Text slots of the in-game HUD, filled by `update_hud`.
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
enum HudField {
    #[default]
    Title,
    Time,
    Detail,
    Info,
    ReadyTitle,
    ReadyBlurb,
    Countdown,
}

/// The "click to start" card, shown only while waiting in the Ready phase.
#[derive(Component, Clone, Copy, Default)]
struct ReadyCard;

/// Starts the countdown on click or Space, then the drill once it reaches zero.
fn ready_gate(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<Run>,
    assets: Res<TargetAssets>,
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
                drills::spawn_initial(&mut commands, &assets, &run);
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

fn record_frame(
    run: Res<Run>,
    look: Res<Look>,
    mut recorder: ResMut<Recorder>,
    targets: Query<(&Transform, Option<&drills::ReactionCue>), With<drills::Target>>,
) {
    if run.live() {
        // A reaction cue that hasn't lit up yet isn't a target the player should react to.
        let targets = targets
            .iter()
            .filter(|(_, cue)| cue.is_none_or(|cue| cue.is_lit()))
            .map(|(transform, _)| (transform.translation, transform.scale.x));
        recorder.sample(run.elapsed, look.angles(), targets);
    }
}

#[allow(clippy::too_many_arguments)]
fn tick_run(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    settings: Res<Settings>,
    mut history: ResMut<History>,
    mut recorder: ResMut<Recorder>,
    finder: Option<ResMut<Finder>>,
    smart_finder: Option<ResMut<SmartFinder>>,
    routine: Option<ResMut<Routine>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if !run.live() {
        return;
    }
    run.elapsed += time.delta_secs();
    if run.elapsed < run.duration {
        return;
    }

    let stats = run.stats(&settings);
    let previous = history.compare(&stats.drill);
    let logged_as = match run.purpose {
        RunPurpose::FinderTrial => RunPurpose::FinderTrial,
        RunPurpose::Practice | RunPurpose::Routine => RunPurpose::Practice,
    };
    let timestamp = history.record(&stats, logged_as, &settings);
    if logged_as == RunPurpose::Practice {
        recorder.finish(stats.scenario, &stats.drill, stats.sens, stats.duration).save(timestamp);
    }
    match run.purpose {
        RunPurpose::FinderTrial => {
            if let Some(mut finder) = finder {
                finder.record_trial(stats.score());
            }
            if let Some(mut smart) = smart_finder {
                smart.record_segment(stats.scenario, stats.score(), &stats.landings);
                // Straight on to the trial's next part, if there is one.
                if let Some(next) = smart.segment_run(&settings) {
                    begin_run(&mut commands, &mut next_state, next);
                    return;
                }
            }
            next_state.set(AppState::Finder);
        }
        RunPurpose::Routine => {
            if let Some(mut routine) = routine {
                routine.record(stats, previous);
            }
            next_state.set(AppState::Routine);
        }
        RunPurpose::Practice => {
            commands.insert_resource(LastRun { stats, previous, timestamp, drill: run.drill.clone(), duration: run.duration });
            next_state.set(AppState::Results);
        }
    }
}

/// Escape or alt-tab abandons the run without recording it. Runs before everything else, so the
/// rest of the frame's systems (including `tick_run`) see the run as no longer live.
fn abort_run(
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
    next_state.set(match run.purpose {
        RunPurpose::Practice => AppState::Menu,
        RunPurpose::FinderTrial => AppState::Finder,
        RunPurpose::Routine => AppState::Routine,
    });
}

fn spawn_hud(mut commands: Commands) {
    commands
        .spawn_scene(bsn! {
            @hud_anchor(px(14))
            Children [
                @hud_bar()
                Children [
                    @styled("", fonts::BOLD, 15.0, tokens::TEXT_MAIN)
                    HudField::Title
                    --
                    @vertical_divider()
                    --
                    Node { min_width: px(72), justify_content: JustifyContent::Center }
                    Children [ @styled("", fonts::BOLD, 26.0, TEXT_STRONG) HudField::Time ]
                    --
                    @vertical_divider()
                    --
                    @styled("", fonts::REGULAR, 16.0, TEXT_STRONG)
                    HudField::Detail
                    --
                    @vertical_divider()
                    --
                    @styled("", fonts::REGULAR, 12.0, tokens::TEXT_DIM)
                    HudField::Info
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Playing));
    commands
        .spawn_scene(bsn! {
            @hud_anchor(percent(24))
            Children [
                @hud_bar()
                ReadyCard
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    padding: UiRect::axes(px(36), px(22)),
                    border_radius: BorderRadius::all(px(14)),
                }
                Children [
                    @styled("", fonts::BOLD, 30.0, TEXT_STRONG)
                    HudField::ReadyTitle
                    --
                    @styled("", fonts::REGULAR, 16.0, tokens::TEXT_MAIN)
                    HudField::ReadyBlurb
                    --
                    @styled("CLICK OR PRESS SPACE TO START", fonts::BOLD, 14.0, TEXT_ACCENT)
                    Node { margin: UiRect::top(px(10)) }
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Playing));
    commands
        .spawn_scene(bsn! {
            @hud_anchor(percent(28))
            Children [ @styled("", fonts::BOLD, 96.0, TEXT_STRONG) HudField::Countdown ]
        })
        .insert(DespawnOnExit(AppState::Playing));
}

fn update_hud(
    run: Res<Run>,
    settings: Res<Settings>,
    diagnostics: Res<DiagnosticsStore>,
    mut fields: Query<(&HudField, &mut Text)>,
    mut ready_card: Single<&mut Node, With<ReadyCard>>,
) {
    let stats = run.stats(&settings);
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed())
        .unwrap_or(0.0);
    for (field, mut text) in &mut fields {
        let wanted: Cow<str> = match field {
            HudField::Title | HudField::ReadyTitle => Cow::Borrowed(&run.title),
            HudField::Time => Cow::Owned(format!("{:.1}s", (run.duration - run.elapsed).max(0.0))),
            HudField::Detail if !run.counting() => {
                Cow::Owned(format!("Warm-in {:.1}s (not scored)", run.scoring_from - run.elapsed))
            }
            HudField::Detail => Cow::Owned(match run.scenario() {
                Scenario::Tracking => format!("On target {:.1}%", stats.on_target_ratio() * 100.0),
                Scenario::Reaction => format!("Hits {}   Avg {:.0} ms", stats.hits, stats.avg_kill_ms()),
                _ => format!("Hits {}   Acc {:.0}%", stats.hits, stats.accuracy() * 100.0),
            }),
            HudField::Info => Cow::Owned(format!("{fps:.0} fps   Esc to quit")),
            HudField::ReadyBlurb => Cow::Borrowed(run.scenario().blurb()),
            HudField::Countdown => match run.phase {
                Phase::Countdown => Cow::Owned(format!("{}", run.countdown.ceil() as u32)),
                Phase::Ready | Phase::Live => Cow::Borrowed(""),
            },
        };
        if text.0 != wanted {
            text.0 = wanted.into_owned();
        }
    }
    let display = if run.phase == Phase::Ready { Display::Flex } else { Display::None };
    if ready_card.display != display {
        ready_card.display = display;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(scenario: Scenario, hits: u32, shots: u32, expired: u32, duration: f32) -> RunStats {
        RunStats {
            scenario,
            drill: scenario.name().to_string(),
            sens: 0.4,
            cm_per_360: 40.8,
            duration,
            hits,
            shots,
            expired,
            ttk_total: hits as f32 * 0.25,
            best_ttk: 0.2,
            on_target: 0.0,
            landings: Vec::new(),
        }
    }

    #[test]
    fn warm_in_is_not_scored() {
        let settings = Settings::default();
        let drill = DrillSpec::builtin(Scenario::Microflick, &settings);
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
    fn flick_score_does_not_depend_on_drill_length() {
        // Same pace and accuracy: a 20 s finder trial must score like a 60 s practice run.
        let short = stats(Scenario::Gridshot, 40, 50, 0, 20.0);
        let long = stats(Scenario::Gridshot, 120, 150, 0, 60.0);
        assert_eq!(short.score(), long.score());
    }

    #[test]
    fn expired_targets_count_as_misses() {
        let clean = stats(Scenario::Microflick, 50, 50, 0, 60.0);
        let decayed = stats(Scenario::Microflick, 50, 50, 10, 60.0);
        assert!((decayed.accuracy() - 50.0 / 60.0).abs() < 1e-6);
        assert!(decayed.score() < clean.score());
    }

    #[test]
    fn reaction_score_rewards_faster_and_more_accurate() {
        let mut fast = stats(Scenario::Reaction, 20, 20, 0, 30.0);
        fast.ttk_total = 20.0 * 0.2;
        let mut slow = fast.clone();
        slow.ttk_total = 20.0 * 0.3;
        let mut sloppy = fast.clone();
        sloppy.shots = 25; // five early clicks
        assert_eq!(fast.score(), 500.0);
        assert!(slow.score() < fast.score());
        assert!(sloppy.score() < fast.score());
        assert_eq!(stats(Scenario::Reaction, 0, 3, 0, 30.0).score(), 0.0);
    }

    #[test]
    fn names_round_trip_for_the_csv() {
        for scenario in Scenario::ALL {
            assert_eq!(Scenario::from_name(scenario.name()), Some(scenario));
        }
    }
}
