//! Drills: the run lifecycle (ready gate, countdown, timer), scoring, HUD and recording.
//! Each built-in drill is described by a [`DrillDef`](def::DrillDef) in [`kinds`]; per-drill
//! target behaviour lives in [`drills`].

mod config;
pub mod def;
mod drills;
pub mod flick_probe;
mod kind;
pub mod params;
pub mod kinds;
pub mod stats;

use std::borrow::Cow;

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    feathers::{constants::fonts, tokens},
    prelude::*,
    window::WindowFocused,
};

pub use config::{DrillConfig, DrillSpec, SECS_RANGE, VARIANT_SEPARATOR};
pub use drills::{TargetAssets, spawn_cover};
use flick_probe::FlickProbe;
pub use kind::DrillKind;
pub use stats::RunStats;

use crate::{
    AppState,
    camera::{ActiveSens, Look, apply_look, mouse_look},
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

pub struct DrillPlugin;

impl Plugin for DrillPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, drills::create_target_assets)
            .add_systems(Update, drills::sync_target_colors.run_if(resource_changed::<Settings>))
            .add_systems(OnEnter(AppState::Playing), (spawn_hud, drills::spawn_static, |mut commands: Commands| {
                commands.insert_resource(Recorder::default())
            }))
            // Mouse look turns at the run's sensitivity (read every frame: a finder trial's next
            // part may replace the run without leaving `Playing`).
            .add_systems(
                Update,
                (|run: Res<Run>, mut sens: ResMut<ActiveSens>| sens.0 = run.sens)
                    .before(mouse_look)
                    .run_if(in_state(AppState::Playing)),
            )
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

    pub fn kind(&self) -> DrillKind {
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
        recorder.finish(stats.kind, &stats.drill, stats.sens, stats.duration).save(timestamp);
    }
    match run.purpose {
        RunPurpose::FinderTrial => {
            if let Some(mut finder) = finder {
                finder.record_trial(stats.score());
            }
            if let Some(mut smart) = smart_finder {
                smart.record_segment(stats.kind, stats.score(), &stats.landings);
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
            HudField::Detail => Cow::Owned(run.kind().def().score.hud_detail(&stats)),
            HudField::Info => Cow::Owned(format!("{fps:.0} fps   Esc to quit")),
            HudField::ReadyBlurb => Cow::Borrowed(run.kind().blurb()),
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
}
