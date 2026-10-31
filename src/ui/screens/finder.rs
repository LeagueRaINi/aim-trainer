//! Sensitivity finder screens: classic blind A/B and the smart score-model finder.

use bevy::{
    feathers::{
        controls::ButtonVariant,
        display::{label, label_dim},
        theme::ThemeBackgroundColor,
        tokens,
    },
    prelude::*,
    ui::InteractionDisabled,
    ui_widgets::Activate,
};

use crate::ui::{
    charts::*,
    screens::score::{go_to_menu, screen_title},
    theme::*,
    widgets::*,
};
use crate::{
    AppState,
    finder::{
        classic::{Finder, FinderStep},
        smart::{PLATEAU, SmartFinder, SmartStep, WARM_IN},
    },
    drill::{DrillSpec, Run, RunPurpose, begin_run},
    settings::{FinderMethod, Settings},
};

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
) {
    match (classic, smart) {
        (Some(finder), _) => spawn_classic(commands, finder, settings),
        (_, Some(finder)) => spawn_smart(commands, finder, settings),
        _ => {}
    }
}

/// Width of the finder screens.
const FINDER_W: f32 = 660.0;

// ---------------------------------------------------------------------------------------------
// Classic: blind A/B bisection
// ---------------------------------------------------------------------------------------------

fn spawn_classic(mut commands: Commands, finder: Res<Finder>, settings: Res<Settings>) {
    let context = format!("Sens finder  ·  Round {} / {}", (finder.round + 1).min(finder.rounds), finder.rounds);
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
                        @label_dim(body)
                        --
                        @row()
                        Node { width: px(200) }
                        Children [ {previous} ]
                        --
                        @actions(bsn_list! {
                            @button(start, ButtonVariant::Primary)
                            on(start_finder_trial)
                            --
                            @button("Cancel finder", ButtonVariant::Normal)
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        }
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
                        @label_dim("Trust feel over score.")
                        --
                        @row()
                        Node { align_items: AlignItems::Stretch }
                        Children [ {tiles} ]
                        --
                        @actions(bsn_list! {
                            @button("A felt better", ButtonVariant::Primary)
                            ArmDelay
                            InteractionDisabled
                            on(pick_a)
                            --
                            @button("B felt better", ButtonVariant::Primary)
                            ArmDelay
                            InteractionDisabled
                            on(pick_b)
                            --
                            @button("Redo round", ButtonVariant::Normal)
                            ArmDelay
                            InteractionDisabled
                            on(redo_round)
                            --
                            @button("Cancel finder", ButtonVariant::Normal)
                            ArmDelay
                            InteractionDisabled
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        }
        FinderStep::Done => {
            let recommended = finder.recommendation();
            const ROUND_W: f32 = 70.0;
            const VALUE_W: f32 = 110.0;
            let mut rows = vec![table_row(
                ["Round", "Low", "High", "Picked", "Scores"]
                    .into_iter()
                    .zip([ROUND_W, VALUE_W, VALUE_W, VALUE_W, VALUE_W])
                    .map(|(text, width)| cell(text.to_string(), width, tokens::TEXT_DIM))
                    .collect::<Vec<_>>(),
                false,
            )];
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
            let started = format!("Started at {:.3}", finder.start_sens);
            let sens = settings.describe_sens(recommended);
            commands.spawn_scene(bsn! {
                @overlay()
                Children [
                    @panel()
                    Node { width: px(FINDER_W) }
                    Children [
                        @screen_title("Sens finder", "Result")
                        --
                        @recommendation(recommended, started, sens)
                        --
                        @surface()
                        Node { row_gap: px(2) }
                        Children [ {rows} ]
                        --
                        @actions(bsn_list! {
                            @button("Apply", ButtonVariant::Primary)
                            on(apply_finder_result)
                            --
                            @button("Discard", ButtonVariant::Normal)
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        }
    };
    entity.insert(DespawnOnExit(AppState::Finder));
}

/// The recommended sensitivity as a big number with its context beside it.
fn recommendation(recommended: f32, started: String, sens: String) -> impl Scene {
    bsn! {
        @row()
        Node { column_gap: px(SPACE_XL), align_items: AlignItems::Center }
        Children [
            @hero("Recommended sensitivity", format!("{recommended:.3}"))
            Node { flex_shrink: 0.0 }
            --
            @vertical_divider()
            --
            @column()
            Node { flex_grow: 1.0, flex_basis: px(0), row_gap: px(SPACE_XS) }
            Children [ @label_dim(started) -- @label_dim(sens) ]
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
    mut settings: ResMut<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    settings.sens = (finder.recommendation() * 1000.0).round() / 1000.0;
    commands.remove_resource::<Finder>();
    next_state.set(AppState::Menu);
}

// ---------------------------------------------------------------------------------------------
// Smart: score model
// ---------------------------------------------------------------------------------------------

const CHART_W: f32 = 560.0;
const CHART_H: f32 = 160.0;

#[derive(Component, Clone, Copy, Default)]
struct FeelButton(u8);

/// Container the result chart is drawn into once spawned.
#[derive(Component, Clone, Copy, Default)]
pub struct SmartChart;

fn spawn_smart(mut commands: Commands, finder: Res<SmartFinder>, settings: Res<Settings>) {
    let done = finder.trials.len();
    let total = finder.total_trials();
    let context = format!("Smart sens finder  ·  Trial {} / {total}", done + 1);
    let mut entity = match finder.step {
        SmartStep::Trial => {
            let phase = if done < crate::finder::smart::PROBES {
                "Probing 5 sensitivities from 0.6x to 1.6x yours, in random order."
            } else {
                "Refining around the predicted best sensitivity."
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
                        @trial_progress(done, total)
                        --
                        @label(phase)
                        --
                        @label_dim(body)
                        --
                        @actions(bsn_list! {
                            @button("Start trial", ButtonVariant::Primary)
                            on(start_smart_trial)
                            --
                            @button("Cancel finder", ButtonVariant::Normal)
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
                        @label_dim("Rate control and comfort over both drills, not the score: could you stop precisely on targets?")
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
                            @button("Redo trial", ButtonVariant::Normal)
                            ArmDelay
                            InteractionDisabled
                            on(|_: On<Activate>, mut finder: ResMut<SmartFinder>, mut next: ResMut<NextState<AppState>>| {
                                finder.redo();
                                next.set(AppState::Finder);
                            })
                            --
                            @button("Cancel finder", ButtonVariant::Normal)
                            ArmDelay
                            InteractionDisabled
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        }
        SmartStep::Done => {
            let recommended = finder.recommendation();
            let (lo, hi) = finder.plateau();
            let keep = (recommended / finder.start_sens - 1.0).abs() < 0.005;
            let basis = if finder.is_flat() {
                "Your results barely changed across everything tested, so sensitivity isn't what limits you here: keep yours."
            } else if finder.uses_precision() {
                "Inside your best range, where your aim is clearly on target: first flicks land on the target and clicks land on its centre."
            } else if keep {
                "Your current sens is already in your best range: changing it would cost re-adaptation for no measurable gain."
            } else {
                "Your current sens scored measurably worse; this is the nearest sens in your best range."
            };
            let started = format!("Started at {:.3}. {basis}", finder.start_sens);
            let sens = settings.describe_sens(recommended);
            let mut notes = vec![format!(
                "Best range {lo:.3} to {hi:.3}: these all score within the trial noise ({:.0}% or more) of the best (peak {:.3}).",
                PLATEAU * 100.0,
                finder.peak_sens(),
            )];
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
            let notes: Vec<_> = notes.into_iter().map(|note| bsn! { @label_dim(note) }).collect();
            commands.spawn_scene(bsn! {
                @overlay()
                Children [
                    @panel()
                    Node { width: px(FINDER_W) }
                    Children [
                        @screen_title("Smart sens finder", "Result")
                        --
                        @recommendation(recommended, started, sens)
                        --
                        @column()
                        Node { row_gap: px(SPACE_XS) }
                        Children [ {notes} ]
                        --
                        @surface()
                        SmartChart
                        --
                        @actions(bsn_list! {
                            @button("Apply", ButtonVariant::Primary)
                            on(apply_smart_result)
                            --
                            @button("Discard", ButtonVariant::Normal)
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        }
    };
    entity.insert(DespawnOnExit(AppState::Finder));
}

/// One bar segment per trial: finished ones filled with the accent color.
fn trial_progress(done: usize, total: usize) -> impl Scene {
    let pips: Vec<_> = (0..total)
        .map(|index| {
            let color = if index < done { tokens::BUTTON_PRIMARY_BG } else { tokens::GROUP_BG };
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
        @button(text, ButtonVariant::Primary)
        Node { min_width: px(0), flex_grow: 1.0, flex_basis: px(0), height: px(36) }
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
    mut settings: ResMut<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    settings.sens = (finder.recommendation() * 1000.0).round() / 1000.0;
    commands.remove_resource::<SmartFinder>();
    next_state.set(AppState::Menu);
}

/// Trials as dots (gold = best feel-adjusted result), fitted curve as a line, the performance
/// plateau shaded, the recommendation as a green marker and the sens where your aim is on target
/// (first flicks land on it, clicks on its centre) as a blue one. X axis is log-scaled sensitivity.
pub fn draw_smart_chart(mut commands: Commands, finder: Option<Res<SmartFinder>>, charts: Query<Entity, Added<SmartChart>>) {
    let Some(finder) = finder else { return };
    for chart in &charts {
        let curve = finder.curve(60);
        let objectives = finder.objectives();
        let all_y: Vec<f32> = curve.iter().map(|p| p.1).chain(objectives.iter().copied()).collect();
        let (min, max) = all_y.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let pad = ((max - min) * 0.15).max(0.01);
        let (lo, hi) = (min - pad, max + pad);
        let sens_lo = finder.start_sens * 0.55;
        let sens_hi = finder.start_sens * 1.75;
        let x = |sens: f32| (sens / sens_lo).ln() / (sens_hi / sens_lo).ln() * CHART_W;
        let y = |value: f32| CHART_H - (value - lo) / (hi - lo) * CHART_H;

        commands.spawn_scene(overline("Result + feel by sensitivity")).insert(ChildOf(chart));
        let plot = commands
            .spawn((
                ChildOf(chart),
                Node { width: px(CHART_W), height: px(CHART_H), border_radius: BorderRadius::all(px(4)), ..default() },
                ThemeBackgroundColor(CHART_BG),
            ))
            .id();
        let (plateau_lo, plateau_hi) = finder.plateau();
        if plateau_hi > plateau_lo {
            let left = x(plateau_lo);
            commands.spawn((ChildOf(plot), absolute(left, 0.0, x(plateau_hi) - left, CHART_H), ThemeBackgroundColor(DIVIDER)));
        }

        let points: Vec<Vec2> = curve.iter().map(|&(sens, value)| Vec2::new(x(sens), y(value))).collect();
        polyline(&mut commands, plot, &points, tokens::TEXT_MAIN);

        if let Some(on_target) = finder.precision_sens().filter(|s| (sens_lo..=sens_hi).contains(s)) {
            commands.spawn((ChildOf(plot), absolute(x(on_target) - 1.0, 0.0, 2.0, CHART_H), ThemeBackgroundColor(TEXT_ACCENT)));
        }
        let recommended = x(finder.recommendation());
        commands.spawn((
            ChildOf(plot),
            absolute(recommended - 1.0, 0.0, 2.0, CHART_H),
            ThemeBackgroundColor(TEXT_GOOD),
        ));

        let best = objectives.iter().copied().fold(f32::MIN, f32::max);
        for (trial, &value) in finder.trials.iter().zip(&objectives) {
            let size = 9.0;
            let color = if value >= best { GOLD } else { CHART_BAR };
            commands.spawn((
                ChildOf(plot),
                Node { border_radius: BorderRadius::MAX, ..absolute(x(trial.sens) - size * 0.5, y(value) - size * 0.5, size, size) },
                ThemeBackgroundColor(color),
            ));
        }

        text(
            &mut commands,
            chart,
            format!(
                "Sensitivity {sens_lo:.3} to {sens_hi:.3} (log scale).  Dots: trials (throughput or score, + feel).  Line: fitted model.  Shaded: best range (within the trial noise, at least {:.0}%, of the peak).  Green: recommendation.  Blue: aim on target (flick landing / click position).",
                PLATEAU * 100.0
            ),
            11.0,
            tokens::TEXT_DIM,
            Node::default(),
        );
    }
}
