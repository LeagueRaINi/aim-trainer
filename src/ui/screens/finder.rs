//! The quick finders' screens: classic blind A/B and the smart score-model finder. Their
//! results are a first look: short blind trials mostly measure adaptation, so whatever they
//! pick is kept inside the calibrated range ([`RangeGuard`]) and presented as a hint.

use bevy::ecs::system::SystemParam;
use bevy::feathers::constants::fonts;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;

use crate::AppState;
use crate::drill::{DrillSpec, Run, RunPurpose, begin_run};
use crate::finder::classic::{Finder, FinderStep};
use crate::finder::limits::{CalibrationStore, Limits};
use crate::finder::smart::{MAX_TRIALS, MIN_TRIALS, OPENING, PLATEAU, SmartFinder, SmartStep, WARM_IN};
use crate::finder::tired::TiredLog;
use crate::settings::{FinderMethod, Settings};
use crate::ui::charts::*;
use crate::ui::screens::score::{go_to_menu, screen_title};
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Starts the finder with the method chosen in settings.
pub fn start_finder(
    _: On<Activate>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    match settings.finder_method {
        FinderMethod::Smart => commands.insert_resource(SmartFinder::new(settings.sens, settings.finder_focus)),
        FinderMethod::Classic => commands.insert_resource(Finder::new(settings.sens, settings.finder_rounds)),
    }
    next_state.set(AppState::Finder);
}

pub fn spawn_finder_screen(
    commands: Commands,
    classic: Option<Res<Finder>>,
    smart: Option<Res<SmartFinder>>,
    settings: Res<Settings>,
    guard: RangeGuard,
) {
    match (classic, smart) {
        (Some(finder), _) => spawn_classic(commands, finder, settings, guard),
        (_, Some(finder)) => spawn_smart(commands, finder, settings, guard),
        _ => {},
    }
}

/// The calibrated range, as far as the quick finders need it: what they recommend never leaves
/// it.
#[derive(SystemParam)]
pub struct RangeGuard<'w> {
    calibration: Res<'w, CalibrationStore>,
    tired: Res<'w, TiredLog>,
}

impl RangeGuard<'_> {
    /// `recommended` moved to the nearest sensitivity inside the range, and whether that moved
    /// it. Unchanged without a calibration, or when the calibration's limits leave no range.
    pub fn keep_inside(&self, recommended: f32, settings: &Settings) -> (f32, bool) {
        let current = settings.cm_per_360(settings.sens);
        let limits = self.calibration.latest.as_ref().map(|c| Limits::new(c, self.tired.verdict(current).loss()));
        limits.map_or((recommended, false), |limits| limits.keep_inside(recommended, settings))
    }
}

/// What a quick finder's result can and cannot tell, shown with every result.
const FIRST_LOOK: &str = "Short blind trials mostly measure how fast you adapt, and aim is near-equal over a wide range of sensitivities. Treat this as a hint, and check a change with the long comparison before relying on it.";

/// Width of the finder screens.
const FINDER_W: f32 = 660.0;

// ---------------------------------------------------------------------------------------------
// Classic: blind A/B bisection
// ---------------------------------------------------------------------------------------------

fn spawn_classic(mut commands: Commands, finder: Res<Finder>, settings: Res<Settings>, guard: RangeGuard) {
    let context = format!("Quick finder  ·  Round {} / {}", (finder.round + 1).min(finder.rounds), finder.rounds);
    let mut entity = match finder.step {
        FinderStep::TrialA | FinderStep::TrialB => {
            let title = format!("Trial {}", finder.trial_label());
            let body = format!(
                "{}, {:.0} s. Sensitivities stay hidden until you have chosen.",
                settings.finder_drill.name(),
                settings.finder_trial_secs,
            );
            let previous: Vec<_> = (finder.step == FinderStep::TrialB)
                .then(|| stat("Trial A score", finder.score_a.to_string()))
                .into_iter()
                .collect();
            let start = if finder.step == FinderStep::TrialA { "Start trial A" } else { "Start trial B" };
            commands.spawn_scene(bsn! {
                @overlay()
                Children [
                    @panel()
                    Node { width: px(FINDER_W) }
                    Children [
                        @screen_title(context, title)
                        --
                        @round_progress(finder.round, finder.rounds)
                        --
                        @label(body)
                        --
                        @row()
                        Node { width: px(200) }
                        Children [ {previous} ]
                        --
                        @actions(bsn_list! {
                            @button(start, Btn::Primary)
                            on(start_finder_trial)
                            --
                            @button("Cancel finder", Btn::Secondary)
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        },
        FinderStep::Choose => {
            let higher_a = finder.score_a >= finder.score_b;
            let caption = |trial: &str, higher: bool| {
                if higher { format!("Trial {trial}  ·  higher score") } else { format!("Trial {trial}") }
            };
            let tiles = vec![
                stat(caption("A", higher_a), finder.score_a.to_string()),
                stat(caption("B", !higher_a), finder.score_b.to_string()),
            ];
            commands.spawn_scene(bsn! {
                @overlay()
                Children [
                    @panel()
                    Node { width: px(FINDER_W) }
                    Children [
                        @screen_title(context, "Which one felt better?")
                        --
                        @round_progress(finder.round, finder.rounds)
                        --
                        @label("Trust feel over score.")
                        --
                        @row()
                        Node { align_items: AlignItems::Stretch }
                        Children [ {tiles} ]
                        --
                        @actions(bsn_list! {
                            @button("A felt better", Btn::Secondary)
                            ArmDelay
                            InteractionDisabled
                            on(pick_a)
                            --
                            @button("B felt better", Btn::Secondary)
                            ArmDelay
                            InteractionDisabled
                            on(pick_b)
                            --
                            @spacer()
                            --
                            @button("Redo round", Btn::Ghost)
                            Node { min_width: px(0) }
                            ArmDelay
                            InteractionDisabled
                            on(redo_round)
                            --
                            @button("Cancel finder", Btn::Ghost)
                            Node { min_width: px(0) }
                            ArmDelay
                            InteractionDisabled
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        },
        FinderStep::Done => {
            let (recommended, moved) = guard.keep_inside(finder.recommendation(), &settings);
            const ROUND_W: f32 = 70.0;
            const VALUE_W: f32 = 110.0;
            let header: Vec<_> = ["Round", "Low", "High", "Picked", "Scores (low / high)"]
                .into_iter()
                .zip([ROUND_W, VALUE_W, VALUE_W, VALUE_W, VALUE_W + 40.0])
                .map(|(text, width)| header_cell(text, width))
                .collect();
            let mut rows = Vec::new();
            for (i, round) in finder.history.iter().enumerate() {
                let picked = if round.picked_high { round.high } else { round.low };
                rows.push(table_row(
                    vec![
                        cell(format!("{}", i + 1), ROUND_W, tokens::TEXT_DIM),
                        cell(format!("{:.3}", round.low), VALUE_W, tokens::TEXT_MAIN),
                        cell(format!("{:.3}", round.high), VALUE_W, tokens::TEXT_MAIN),
                        cell(format!("{picked:.3}"), VALUE_W, TEXT_STRONG),
                        cell(format!("{} / {}", round.low_score, round.high_score), VALUE_W, tokens::TEXT_MAIN),
                    ],
                    i % 2 == 0,
                ));
            }
            let mut lines = vec![format!("Started at {:.3}", finder.start_sens), settings.describe_sens(recommended)];
            if moved {
                lines.push(format!(
                    "The rounds ended at {:.3}, outside your calibrated range: this is the nearest sensitivity inside it.",
                    finder.recommendation()
                ));
            }
            commands.spawn_scene(bsn! {
                @overlay()
                Children [
                    @panel()
                    Node { width: px(FINDER_W) }
                    Children [
                        @screen_title("Quick finder  ·  Classic", "A first look")
                        --
                        @recommendation(recommended, lines)
                        --
                        @callout(TEXT_ACCENT, "A hint, not a verdict", FIRST_LOOK)
                        --
                        @surface()
                        Node { row_gap: px(2) }
                        Children [ @table_header(header) -- {rows} ]
                        --
                        @actions(bsn_list! {
                            @button("Apply", Btn::Primary)
                            on(apply_finder_result)
                            --
                            @button("Discard", Btn::Secondary)
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        },
    };
    entity.insert(DespawnOnExit(AppState::Finder));
}

/// The recommended sensitivity as a big number with its context beside it.
fn recommendation(recommended: f32, lines: Vec<String>) -> impl Scene {
    let lines: Vec<_> = lines.into_iter().map(label_dim).collect();
    bsn! {
        @row()
        Node { column_gap: px(SPACE_XL), align_items: AlignItems::Center }
        Children [
            @hero("The quick finder's pick", format!("{recommended:.3}"))
            Node { flex_shrink: 0.0 }
            --
            @vertical_divider()
            --
            @column()
            Node { flex_grow: 1.0, flex_basis: px(0), row_gap: px(SPACE_XS) }
            Children [ {lines} ]
        ]
    }
}

fn start_finder_trial(
    _: On<Activate>,
    finder: Res<Finder>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Some(sens) = finder.trial_sens() else { return };
    let title = format!("Finder {}/{} - trial {}", finder.round + 1, finder.rounds, finder.trial_label());
    let drill = DrillSpec::builtin(settings.finder_drill, &settings);
    let run = Run::new(drill, sens, settings.finder_trial_secs, RunPurpose::FinderTrial, title);
    begin_run(&mut commands, &mut next_state, run);
}

fn pick_a(_: On<Activate>, finder: ResMut<Finder>, next_state: ResMut<NextState<AppState>>) {
    choose(finder, next_state, true);
}

fn pick_b(_: On<Activate>, finder: ResMut<Finder>, next_state: ResMut<NextState<AppState>>) {
    choose(finder, next_state, false);
}

fn choose(mut finder: ResMut<Finder>, mut next_state: ResMut<NextState<AppState>>, picked_a: bool) {
    finder.choose(picked_a);
    // Re-enter the state to rebuild the screen for the next step.
    next_state.set(AppState::Finder);
}

fn redo_round(_: On<Activate>, mut finder: ResMut<Finder>, mut next_state: ResMut<NextState<AppState>>) {
    finder.redo_round();
    next_state.set(AppState::Finder);
}

fn apply_finder_result(
    _: On<Activate>,
    finder: Res<Finder>,
    guard: RangeGuard,
    mut settings: ResMut<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let (recommended, _) = guard.keep_inside(finder.recommendation(), &settings);
    settings.sens = (recommended * 1000.0).round() / 1000.0;
    commands.remove_resource::<Finder>();
    next_state.set(AppState::Menu);
}

// ---------------------------------------------------------------------------------------------
// Smart: score model
// ---------------------------------------------------------------------------------------------

/// Width of the smart finder's result screen: the verdict column, then the chart in its card.
const VERDICT_W: f32 = 372.0;
const CHART_W: f32 = 520.0;
const CHART_H: f32 = 230.0;
const SMART_RESULT_W: f32 = 2.0 * SPACE_XL + VERDICT_W + SPACE_L + CHART_W + 2.0 * SPACE_L + 2.0;
/// Width of the strips the chart's band is filled with; whole pixels at the usual UI scales.
const STRIP_W: f32 = 2.0;
/// Width of a label centred on a point of the chart's x axis.
const AXIS_LABEL_W: f32 = 250.0;
/// Diameter of a trial's dot.
const TRIAL_DOT: f32 = 11.0;

#[derive(Component, Clone, Copy, Default)]
struct FeelButton(u8);

/// Container the result chart is drawn into once spawned.
#[derive(Component, Clone, Copy, Default)]
pub struct SmartChart;

fn spawn_smart(mut commands: Commands, finder: Res<SmartFinder>, settings: Res<Settings>, guard: RangeGuard) {
    let done = finder.trials.len();
    let context = format!("Quick finder  ·  Smart  ·  Trial {} (up to {MAX_TRIALS})", done + 1);
    let mut entity = match finder.step {
        SmartStep::Trial => {
            let phase = if done < OPENING {
                "Opening: your sens, 0.6x and 1.6x of it, in random order.".to_string()
            } else {
                format!(
                    "Testing where the model expects to learn most about your best range. It stops once that range settles ({MIN_TRIALS} to {MAX_TRIALS} trials)."
                )
            };
            let names: Vec<&str> = finder.segment_order().iter().map(|drill| drill.name()).collect();
            let (each, parts) = if names.len() > 1 { (" each", "of each aren't") } else { ("", "aren't") };
            let body = format!(
                "Next: {}, {:.0} s{each} at the same hidden sensitivity (the first {WARM_IN:.0} s {parts} scored). Then you rate how it felt.",
                names.join(" then "),
                settings.finder_trial_secs,
            );
            commands.spawn_scene(bsn! {
                @overlay()
                Children [
                    @panel()
                    Node { width: px(FINDER_W) }
                    Children [
                        @screen_title(context, "Next trial")
                        --
                        @trial_progress(done)
                        --
                        @label(body)
                        --
                        @small(phase)
                        --
                        @actions(bsn_list! {
                            @button("Start trial", Btn::Primary)
                            on(start_smart_trial)
                            --
                            @button("Cancel finder", Btn::Secondary)
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        }
        SmartStep::Rate => {
            commands.spawn_scene(bsn! {
                @overlay()
                Children [
                    @panel()
                    Node { width: px(FINDER_W) }
                    Children [
                        @screen_title(context, "How did it feel?")
                        --
                        @trial_progress(done)
                        --
                        @label("Rate control and comfort over both drills, not the score: could you stop precisely on targets?")
                        --
                        @row()
                        Children [
                            @feel_button("1 Awful", 1)
                            --
                            @feel_button("2", 2)
                            --
                            @feel_button("3 OK", 3)
                            --
                            @feel_button("4", 4)
                            --
                            @feel_button("5 Great", 5)
                        ]
                        --
                        @actions(bsn_list! {
                            @button("Redo trial", Btn::Ghost)
                            ArmDelay
                            InteractionDisabled
                            on(|_: On<Activate>, mut finder: ResMut<SmartFinder>, mut next: ResMut<NextState<AppState>>| {
                                finder.redo();
                                next.set(AppState::Finder);
                            })
                            --
                            @button("Cancel finder", Btn::Ghost)
                            ArmDelay
                            InteractionDisabled
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        }
        SmartStep::Done => {
            let (recommended, moved) = guard.keep_inside(finder.recommendation(), &settings);
            let (lo, hi) = finder.best_range();
            let keep = (recommended / finder.start_sens - 1.0).abs() < 0.005;
            let (tone, headline, basis) = if moved {
                (
                    TEXT_WARN,
                    "Kept inside your calibrated range",
                    "The model's own pick is outside what your hand and pad allow, so this is the nearest sensitivity inside your range.",
                )
            } else if finder.is_flat() {
                (
                    TEXT_GOOD,
                    "Keep your sensitivity",
                    "Your results barely changed across everything tested, so sensitivity isn't what limits you here.",
                )
            } else if finder.uses_precision() {
                (
                    TEXT_ACCENT,
                    "Picked where your aim lands on target",
                    "Inside your best range, where your aim is clearly on target: first flicks land on the target and clicks land on its centre.",
                )
            } else if keep {
                (
                    TEXT_GOOD,
                    "Keep your sensitivity",
                    "Your current sens is already in your best range: changing it would cost re-adaptation for no measurable gain.",
                )
            } else {
                (
                    TEXT_ACCENT,
                    "A nearby sensitivity suits you better",
                    "Your current sens is probably more than 3% worse than your best; this is the nearest sens that is probably (70%+) within 3% of it.",
                )
            };
            let confidence = finder.confidence().unwrap_or(0.0) * 100.0;
            let mut notes = vec![
                format!(
                    "Best range {lo:.3} to {hi:.3} ({confidence:.0}% sure your best sens is in it): each is at least even odds to perform within {:.0}% of your best (peak about {:.3}).",
                    PLATEAU * 100.0,
                    finder.peak_sens(),
                ),
                format!(
                    "{} trials. Trial-to-trial noise about {:.1}%; warm-up drift {:+.1}% per trial, factored out.",
                    finder.trials.len(),
                    finder.noise().unwrap_or(0.0) * 100.0,
                    finder.drift().unwrap_or(0.0) * 100.0,
                ),
            ];
            notes.push(match finder.landing_at_start() {
                Some(landing) => {
                    let verdict = match landing {
                        l if l < 0.95 => "undershooting",
                        l if l > 1.05 => "overshooting",
                        _ => "on target",
                    };
                    let on_target = finder
                        .landing_sens()
                        .map(|s| format!(" They would land on target at about {s:.3}."))
                        .unwrap_or_default();
                    format!(
                        "At your current sens your first flicks land at {:.0}% of the distance: {verdict}.{on_target}",
                        landing * 100.0
                    )
                }
                None => "Not enough flicks to tell where your first flicks land.".to_string(),
            });
            if let Some(bias) = finder.bias_at_start() {
                let side = if bias >= 0.0 { "past" } else { "short of" };
                let centred =
                    finder.bias_sens().map(|s| format!(" They would land on the centre at about {s:.3}.")).unwrap_or_default();
                notes.push(format!(
                    "Your clicks land {:.0}% of a target radius {side} the centre along the flick.{centred}",
                    bias.abs() * 100.0
                ));
            }
            let lines: Vec<_> = [
                format!("Started at {:.3}", finder.start_sens),
                settings.describe_sens(recommended).replace("  |  ", "  ·  "),
                format!("Best range {lo:.3} to {hi:.3}, {confidence:.0}% sure"),
            ]
            .into_iter()
            .map(label_dim)
            .collect();
            let notes = notes.join("\n");
            commands.spawn_scene(bsn! {
                @overlay()
                Children [
                    @panel()
                    Node { width: px(SMART_RESULT_W) }
                    Children [
                        @screen_title("Quick finder  ·  Smart", "A first look")
                        --
                        // The verdict on the left, the evidence for it on the right.
                        @row()
                        Node { column_gap: px(SPACE_L), align_items: AlignItems::Stretch }
                        Children [
                            @column()
                            Node { width: px(VERDICT_W), flex_shrink: 0.0, row_gap: px(SPACE_M) }
                            Children [
                                @hero("The quick finder's pick", format!("{recommended:.3}"))
                                --
                                @column()
                                Node { row_gap: px(SPACE_XS) }
                                Children [ {lines} ]
                                --
                                @callout(tone, headline, basis)
                                --
                                @small(FIRST_LOOK)
                                --
                                @details("The numbers behind it", notes)
                                --
                                @spacer()
                                --
                                @actions(bsn_list! {
                                    @button("Apply", Btn::Primary)
                                    on(apply_smart_result)
                                    --
                                    @button("Discard", Btn::Secondary)
                                    on(go_to_menu)
                                })
                            ]
                            --
                            @surface()
                            Node { row_gap: px(SPACE_XS) }
                            SmartChart
                        ]
                    ]
                ]
            })
        }
    };
    entity.insert(DespawnOnExit(AppState::Finder));
}

/// One bar segment per possible trial: finished ones filled with the accent color, the ones the
/// finder may skip (after `MIN_TRIALS`, once the best range settles) fainter.
fn trial_progress(done: usize) -> impl Scene {
    let pips: Vec<_> = (0..MAX_TRIALS)
        .map(|index| {
            let color = match index {
                i if i < done => ACCENT,
                i if i == done => TEXT_STRONG,
                i if i < MIN_TRIALS => BORDER_STRONG,
                _ => BORDER,
            };
            bsn! {
                Node { flex_grow: 1.0, height: px(4), border_radius: BorderRadius::all(px(2)) }
                ThemeBackgroundColor(color)
            }
        })
        .collect();
    bsn! {
        @row()
        Node { column_gap: px(SPACE_XS) }
        Children [ {pips} ]
    }
}

/// One bar segment per round of the classic finder: finished ones filled, the current one white.
fn round_progress(round: u32, rounds: u32) -> impl Scene {
    let pips: Vec<_> = (0..rounds)
        .map(|index| {
            let color = match index.cmp(&round) {
                std::cmp::Ordering::Less => ACCENT,
                std::cmp::Ordering::Equal => TEXT_STRONG,
                std::cmp::Ordering::Greater => BORDER_STRONG,
            };
            bsn! {
                Node { flex_grow: 1.0, height: px(4), border_radius: BorderRadius::all(px(2)) }
                ThemeBackgroundColor(color)
            }
        })
        .collect();
    bsn! {
        @row()
        Node { column_gap: px(SPACE_XS) }
        Children [ {pips} ]
    }
}

fn feel_button(text: &'static str, feel: u8) -> impl Scene {
    bsn! {
        @button(text, Btn::Secondary)
        Node { min_width: px(0), flex_grow: 1.0, flex_basis: px(0), height: px(44) }
        FeelButton(feel)
        ArmDelay
        InteractionDisabled
        on(|activate: On<Activate>, buttons: Query<&FeelButton>, mut finder: ResMut<SmartFinder>, mut next: ResMut<NextState<AppState>>| {
            if let Ok(&FeelButton(feel)) = buttons.get(activate.event_target()) {
                finder.rate(feel);
                // Re-enter to show the next step.
                next.set(AppState::Finder);
            }
        })
    }
}

fn start_smart_trial(
    _: On<Activate>,
    mut finder: ResMut<SmartFinder>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    finder.begin_trial();
    let Some(run) = finder.segment_run(&settings) else { return };
    begin_run(&mut commands, &mut next_state, run);
}

fn apply_smart_result(
    _: On<Activate>,
    finder: Res<SmartFinder>,
    guard: RangeGuard,
    mut settings: ResMut<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let (recommended, _) = guard.keep_inside(finder.recommendation(), &settings);
    settings.sens = (recommended * 1000.0).round() / 1000.0;
    commands.remove_resource::<SmartFinder>();
    next_state.set(AppState::Menu);
}

/// A row above or below the plot with labels centred on points of its x axis.
fn axis_row(commands: &mut Commands, chart: Entity) -> Entity {
    commands.spawn((ChildOf(chart), Node { width: px(CHART_W), height: px(16), flex_shrink: 0.0, ..default() })).id()
}

/// A label centred on `x` in an [`axis_row`], kept inside the plot's width.
fn axis_label(commands: &mut Commands, row: Entity, x: f32, value: String, font: &'static str, color: ThemeToken) {
    let left = (x - AXIS_LABEL_W * 0.5).clamp(0.0, CHART_W - AXIS_LABEL_W);
    let justify = if left <= 0.0 {
        JustifyContent::FlexStart
    } else if left >= CHART_W - AXIS_LABEL_W {
        JustifyContent::FlexEnd
    } else {
        JustifyContent::Center
    };
    let slot = commands
        .spawn((ChildOf(row), Node { justify_content: justify, ..absolute(left, 0.0, AXIS_LABEL_W, 16.0) }))
        .id();
    spawn_in(commands, slot, styled(value, font, FONT_SMALL, color));
}

/// The smart finder's evidence: how you did (result + feel, warm-up drift removed) at each
/// sensitivity, on a log-scaled x axis. The model's estimate is a line inside the filled band it
/// lies in with 90% probability; trials are dots (gold = best); the best range is tinted; the
/// recommendation is a labelled green marker, and the sens where your aim is on target (first
/// flicks land on it, clicks on its centre) an accent-colored one.
pub fn draw_smart_chart(
    mut commands: Commands,
    finder: Option<Res<SmartFinder>>,
    settings: Res<Settings>,
    guard: RangeGuard,
    charts: Query<Entity, Added<SmartChart>>,
) {
    let Some(finder) = finder else { return };
    for chart in &charts {
        let commands = &mut commands;
        let band = finder.band();
        let objectives = finder.corrected_objectives();
        let span = |values: &mut dyn Iterator<Item = f32>| {
            values.fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
        };
        let (min, max) = span(&mut band.iter().map(|p| p.1).chain(objectives.iter().copied()));
        // The band widens away from the trials: show it, but not at the cost of squashing the
        // mean and the dots (it's clipped at the plot edges instead).
        let (band_min, band_max) = span(&mut band.iter().flat_map(|p| [p.2, p.3]));
        let reach = (max - min).max(0.02);
        let pad = reach * 0.15;
        let (lo, hi) = (band_min.max(min - reach).min(min) - pad, band_max.min(max + reach).max(max) + pad);
        let sens_lo = finder.start_sens * 0.55;
        let sens_hi = finder.start_sens * 1.75;
        let x = |sens: f32| (sens / sens_lo).ln() / (sens_hi / sens_lo).ln() * CHART_W;
        let y = |value: f32| (CHART_H - (value - lo) / (hi - lo) * CHART_H).clamp(0.0, CHART_H);
        let (recommended, _) = guard.keep_inside(finder.recommendation(), &settings);

        spawn_in(commands, chart, overline("How you did at each sensitivity"));
        let flags = axis_row(commands, chart);
        let on_target = finder.precision_sens().filter(|s| (sens_lo..=sens_hi).contains(s));
        let picked_on_target = on_target.is_some_and(|s| (x(s) - x(recommended)).abs() < 3.0);
        let flag = if picked_on_target {
            format!("{recommended:.3} picked: aim on target")
        } else {
            format!("{recommended:.3} picked")
        };
        axis_label(commands, flags, x(recommended), flag, fonts::BOLD, TEXT_GOOD);
        let plot = plot(commands, chart, CHART_W, CHART_H);

        // Grid: the sensitivities named on the x axis, and thirds of the height.
        let ticks = [0.6, 0.8, 1.0, 1.25, 1.6].map(|factor: f32| finder.start_sens * factor);
        for tick in ticks {
            commands.spawn((ChildOf(plot), absolute(x(tick), 0.0, 1.0, CHART_H), ThemeBackgroundColor(CHART_GRID)));
        }
        for fraction in [1.0 / 3.0, 2.0 / 3.0] {
            let line = absolute(0.0, CHART_H * fraction, CHART_W, 1.0);
            commands.spawn((ChildOf(plot), line, ThemeBackgroundColor(CHART_GRID)));
        }

        let (range_lo, range_hi) = finder.best_range();
        if range_hi > range_lo {
            let (left, right) = (x(range_lo), x(range_hi));
            commands.spawn((
                ChildOf(plot),
                Node {
                    border: UiRect::horizontal(px(1)),
                    align_items: AlignItems::FlexEnd,
                    justify_content: JustifyContent::Center,
                    ..absolute(left, 0.0, right - left, CHART_H)
                },
                ThemeBackgroundColor(CHART_RANGE),
                ThemeBorderColor(CHART_RANGE_EDGE),
            ));
        }

        // The 90% band, filled strip by strip between its bounds.
        let bounds_at = |at: f32| {
            let after = band.iter().position(|p| x(p.0) >= at)?;
            let (a, b) = (band[after.checked_sub(1)?], band[after]);
            let t = (at - x(a.0)) / (x(b.0) - x(a.0)).max(f32::EPSILON);
            Some((a.2 + (b.2 - a.2) * t, a.3 + (b.3 - a.3) * t))
        };
        for strip in 0..(CHART_W / STRIP_W) as usize {
            let left = strip as f32 * STRIP_W;
            let Some((lower, upper)) = bounds_at(left + STRIP_W * 0.5) else { continue };
            let top = y(upper);
            let fill = absolute(left, top, STRIP_W, (y(lower) - top).max(1.0));
            commands.spawn((ChildOf(plot), fill, ThemeBackgroundColor(CHART_BAND)));
        }
        let mean: Vec<Vec2> = band.iter().map(|p| Vec2::new(x(p.0), y(p.1))).collect();
        polyline(commands, plot, &mean, TEXT_STRONG);

        // Where your aim is on target, unless that is the recommendation itself (its flag says so).
        if let Some(on_target) = on_target.filter(|_| !picked_on_target) {
            let at = x(on_target);
            commands.spawn((ChildOf(plot), absolute(at - 1.0, 0.0, 2.0, CHART_H), ThemeBackgroundColor(TEXT_ACCENT)));
            // Beside the line, on the side with more room.
            let (left, justify) = if at < CHART_W * 0.5 {
                (at + 6.0, JustifyContent::FlexStart)
            } else {
                (at - 6.0 - AXIS_LABEL_W, JustifyContent::FlexEnd)
            };
            let slot = commands
                .spawn((
                    ChildOf(plot),
                    Node { justify_content: justify, ..absolute(left, CHART_H - 20.0, AXIS_LABEL_W, 16.0) },
                ))
                .id();
            spawn_in(commands, slot, styled("aim on target", fonts::BOLD, FONT_SMALL, TEXT_ACCENT));
        }
        commands.spawn((
            ChildOf(plot),
            absolute(x(recommended) - 1.0, 0.0, 2.0, CHART_H),
            ThemeBackgroundColor(TEXT_GOOD),
        ));

        let best = objectives.iter().copied().fold(f32::MIN, f32::max);
        for (trial, &value) in finder.trials.iter().zip(&objectives) {
            let color = if value >= best { GOLD } else { TEXT_ACCENT };
            let half = TRIAL_DOT * 0.5;
            commands.spawn((
                ChildOf(plot),
                Node {
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::MAX,
                    ..absolute(x(trial.sens) - half, y(value) - half, TRIAL_DOT, TRIAL_DOT)
                },
                ThemeBackgroundColor(color),
                ThemeBorderColor(SURFACE_SUNKEN),
            ));
        }

        let axis = axis_row(commands, chart);
        for tick in ticks {
            let (value, color) = if tick == finder.start_sens {
                (format!("{tick:.3} now"), TEXT_STRONG)
            } else {
                (format!("{tick:.3}"), tokens::TEXT_DIM)
            };
            axis_label(commands, axis, x(tick), value, fonts::REGULAR, color);
        }

        let legend = spawn_in(
            commands,
            chart,
            bsn! { @wrap_row() Node { column_gap: px(0), row_gap: px(2), margin: UiRect::top(px(SPACE_S)) } },
        );
        for (color, meaning) in [
            (TEXT_ACCENT, "a trial"),
            (GOLD, "your best trial"),
            (TEXT_STRONG, "the model's estimate"),
            (CHART_BAND, "where it lies, 90% sure"),
            (CHART_RANGE_EDGE, "best range"),
        ] {
            spawn_in(commands, legend, legend_item(color, meaning));
        }
        let note = format!(
            "Higher is better: each dot is a trial's result (throughput or score) plus how it felt, with warm-up drift removed. Sensitivity runs left to right on a log scale. Best range: every sens with at least even odds to be within {:.0}% of your best. Aim on target: where your first flicks land on the target and your clicks on its centre.",
            PLATEAU * 100.0
        );
        spawn_in(commands, chart, details("How to read the chart", note));
    }
}
