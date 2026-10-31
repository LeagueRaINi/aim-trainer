//! Warmup routine screens: the self-paced break between steps and the final summary.

use std::time::Duration;

use bevy::{
    feathers::{
        constants::fonts,
        controls::ButtonVariant,
        display::label_dim,
        theme::{ThemeBackgroundColor, ThemeToken},
        tokens,
    },
    prelude::*,
    input_focus::InputFocus,
    ui_widgets::Activate,
};

use super::{
    screens::{go_to_menu, score_summary, screen_title},
    theme::*,
    widgets::*,
};
use crate::{
    AppState,
    routine::{Routine, StepResult, start_routine},
    settings::Settings,
};

/// When the current break started (real time).
#[derive(Resource)]
pub struct BreakStart(Duration);

#[derive(Component, Clone, Copy, Default)]
pub struct BreakTimer;

const DRILL_W: f32 = 230.0;
const VALUE_W: f32 = 110.0;

pub fn spawn_routine_screen(
    mut commands: Commands,
    routine: Res<Routine>,
    time: Res<Time<Real>>,
) {
    commands.insert_resource(BreakStart(time.elapsed()));
    let total = routine.steps.len();
    let mut entity = match routine.next_step() {
        Some(step) => {
            let context = format!("{}  ·  Step {} / {total}", routine.name, routine.next + 1);
            let title = format!("Next: {}", step.drill.name);
            let next = format!("{:.0} s  ·  {}", step.secs, step.drill.config.kind.blurb());
            let pips: Vec<_> = (0..total)
                .map(|index| {
                    progress_pip(match index.cmp(&routine.next) {
                        std::cmp::Ordering::Less => tokens::BUTTON_PRIMARY_BG,
                        std::cmp::Ordering::Equal => TEXT_STRONG,
                        std::cmp::Ordering::Greater => tokens::GROUP_BG,
                    })
                })
                .collect();
            let last: Vec<_> = routine.results.last().map(last_step).into_iter().collect();
            let intro: Vec<_> = (routine.results.is_empty() && !routine.blurb.is_empty())
                .then(|| label_dim(routine.blurb.clone()))
                .into_iter()
                .collect();
            commands.spawn_scene(bsn! {
                @overlay()
                Children [
                    @panel()
                    Node { width: px(660) }
                    Children [
                        @screen_title(context, title)
                        --
                        @row()
                        Node { column_gap: px(SPACE_XS) }
                        Children [ {pips} ]
                        --
                        @label_dim(next)
                        --
                        {intro}
                        --
                        {last}
                        --
                        @row()
                        Node { column_gap: px(SPACE_M), align_items: AlignItems::Baseline }
                        Children [
                            @overline("Break")
                            --
                            @styled("0:00", fonts::BOLD, 22.0, TEXT_STRONG)
                            BreakTimer
                            --
                            @label_dim("Take as long as you like; press Space or Start next when ready.")
                        ]
                        --
                        @actions(bsn_list! {
                            @button("Start next", ButtonVariant::Primary)
                            on(|_: On<Activate>, routine: Res<Routine>, settings: Res<Settings>, mut commands: Commands, mut next: ResMut<NextState<AppState>>| {
                                routine.start_next(settings.sens, &mut commands, &mut next);
                            })
                            --
                            @button("Skip step", ButtonVariant::Normal)
                            on(|_: On<Activate>, mut routine: ResMut<Routine>, mut next: ResMut<NextState<AppState>>| {
                                routine.skip();
                                next.set(AppState::Routine);
                            })
                            --
                            @button("End routine", ButtonVariant::Normal)
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        }
        None => {
            let mut rows = vec![table_row(
                vec![
                    cell("Drill".to_string(), DRILL_W, tokens::TEXT_DIM),
                    cell("Score".to_string(), VALUE_W, tokens::TEXT_DIM),
                    cell("vs avg".to_string(), VALUE_W, tokens::TEXT_DIM),
                    cell("vs PB".to_string(), VALUE_W, tokens::TEXT_DIM),
                ],
                false,
            )];
            for (index, result) in routine.results.iter().enumerate() {
                let score = result.stats.score();
                let delta = |reference: Option<f32>| match reference.filter(|r| *r > 0.0) {
                    Some(r) => {
                        let percent = (score / r - 1.0) * 100.0;
                        cell(format!("{percent:+.1}%"), VALUE_W, delta_color(percent))
                    }
                    None => cell("-".to_string(), VALUE_W, tokens::TEXT_DIM),
                };
                rows.push(table_row(
                    vec![
                        cell(result.stats.drill.clone(), DRILL_W, tokens::TEXT_MAIN),
                        cell(format!("{score}"), VALUE_W, TEXT_STRONG),
                        delta(result.previous.recent_avg),
                        delta(result.previous.best),
                    ],
                    index % 2 == 0,
                ));
            }
            let skipped = total - routine.results.len();
            let skipped: Vec<_> = (skipped > 0).then(|| label_dim(format!("{skipped} step(s) skipped."))).into_iter().collect();
            commands.spawn_scene(bsn! {
                @overlay()
                Children [
                    @panel()
                    Node { width: px(660) }
                    Children [
                        @screen_title("Routine complete", routine.name.clone())
                        --
                        @surface()
                        Node { row_gap: px(2) }
                        Children [ {rows} ]
                        --
                        {skipped}
                        --
                        @actions(bsn_list! {
                            @button("Run again", ButtonVariant::Primary)
                            on(|_: On<Activate>, routine: Res<Routine>, mut commands: Commands, mut next: ResMut<NextState<AppState>>| {
                                start_routine(&mut commands, &mut next, routine.restarted());
                            })
                            --
                            @button("Menu", ButtonVariant::Normal)
                            on(go_to_menu)
                        })
                    ]
                ]
            })
        }
    };
    entity.insert(DespawnOnExit(AppState::Routine));
}

/// One segment of the step progress bar.
fn progress_pip(color: ThemeToken) -> impl Scene {
    bsn! {
        Node { flex_grow: 1.0, height: px(4), border_radius: BorderRadius::all(px(2)) }
        ThemeBackgroundColor(color)
    }
}

/// The result of the step just played.
fn last_step(last: &StepResult) -> impl Scene {
    bsn! {
        @surface()
        Node { row_gap: px(SPACE_M) }
        Children [
            @overline(format!("Last: {}", last.stats.drill))
            --
            @score_summary(&last.stats, &last.previous)
        ]
    }
}

/// Shows how long the break has lasted; Space starts the next step.
#[allow(clippy::too_many_arguments)]
pub fn routine_break(
    time: Res<Time<Real>>,
    start: Res<BreakStart>,
    keys: Res<ButtonInput<KeyCode>>,
    routine: Res<Routine>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next: ResMut<NextState<AppState>>,
    mut timers: Query<&mut Text, With<BreakTimer>>,
    focus: Res<InputFocus>,
    existing: Query<()>,
) {
    if routine.finished() {
        return;
    }
    let secs = (time.elapsed() - start.0).as_secs();
    let wanted = format!("{}:{:02}", secs / 60, secs % 60);
    for mut text in &mut timers {
        if text.0 != wanted {
            text.0 = wanted.clone();
        }
    }
    // A focused button (Tab navigation, or one you clicked) handles Space itself; acting here too
    // would run two actions for one key press.
    let button_has_focus = focus.get().is_some_and(|entity| existing.contains(entity));
    if keys.just_pressed(KeyCode::Space) && !button_has_focus {
        routine.start_next(settings.sens, &mut commands, &mut next);
    }
}
