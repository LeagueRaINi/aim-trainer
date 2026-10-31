//! Results screen, plus the score presentation shared with the routine screens.

use bevy::{
    feathers::{constants::fonts, controls::ButtonVariant, display::label_dim, tokens},
    prelude::*,
    ui_widgets::Activate,
};

use super::{theme::*, widgets::*};
use crate::{
    AppState,
    finder::Finder,
    history::Comparison,
    replay::{Replay, watch},
    routine::Routine,
    scenario::{LastRun, Run, RunPurpose, RunStats, begin_run},
    settings::Settings,
    smart_finder::SmartFinder,
};

/// Back to the menu, ending any finder or routine in progress.
pub fn go_to_menu(_: On<Activate>, mut commands: Commands, mut next_state: ResMut<NextState<AppState>>) {
    commands.remove_resource::<Finder>();
    commands.remove_resource::<SmartFinder>();
    commands.remove_resource::<Routine>();
    next_state.set(AppState::Menu);
}

// ---------------------------------------------------------------------------------------------
// Score presentation
// ---------------------------------------------------------------------------------------------

/// How this score compares to earlier practice runs of the same drill: an optional headline
/// (first run / new personal best) and one row per reference with the percent change.
pub fn comparison(score: f32, previous: &Comparison) -> impl Scene {
    // A zero reference (e.g. a Reaction run without hits) has no meaningful percent change.
    let percent = |reference: f32| (reference > 0.0).then(|| (score / reference - 1.0) * 100.0);
    let mut headline = Vec::new();
    let mut rows = Vec::new();
    // With a single earlier run, "best" and "previous run" are the same run: show it once.
    let only_one = previous.recent_count == 1;
    match previous.best {
        None => headline.push(styled("First run of this drill: baseline set.", fonts::REGULAR, 15.0, tokens::TEXT_DIM)),
        Some(best) if score > best => {
            headline.push(styled("NEW PERSONAL BEST!", fonts::BOLD, 16.0, GOLD));
            let label = if only_one { "Your only previous run" } else { "Previous best" };
            rows.push(comparison_row(label.into(), best, percent(best)));
        }
        Some(best) => {
            let label = if only_one { "Your only previous run" } else { "Personal best" };
            rows.push(comparison_row(label.into(), best, percent(best)));
        }
    }
    if let Some(avg) = previous.recent_avg.filter(|_| !only_one) {
        let runs = format!("Avg of your last {} runs", previous.recent_count);
        rows.push(comparison_row(runs, avg, percent(avg)));
    }
    bsn! {
        @column()
        Node { row_gap: px(SPACE_XS) }
        Children [ {headline} -- {rows} ]
    }
}

fn comparison_row(text: String, reference: f32, percent: Option<f32>) -> impl Scene {
    let change = percent.map_or("-".to_string(), |p| format!("{p:+.1}%"));
    let color = percent.map_or(tokens::TEXT_DIM, delta_color);
    bsn! {
        @row()
        Node { column_gap: px(SPACE_M) }
        Children [
            @label_dim(text)
            --
            @spacer()
            --
            @styled(format!("{reference:.0}"), fonts::BOLD, 15.0, TEXT_STRONG)
            --
            Node { width: px(64), justify_content: JustifyContent::End }
            Children [ @styled(change, fonts::BOLD, 15.0, color) ]
        ]
    }
}

/// Big score with the comparison next to it, and the secondary stats as tiles below.
pub fn score_summary(stats: &RunStats, previous: &Comparison) -> impl Scene {
    let score = stats.score();
    let tiles: Vec<_> = stats.key_stats().into_iter().map(|(caption, value)| stat(caption, value)).collect();
    bsn! {
        @column()
        Node { row_gap: px(SPACE_L) }
        Children [
            @row()
            Node { column_gap: px(SPACE_XL), align_items: AlignItems::Center }
            Children [
                @hero("Score", format!("{score}"))
                Node { min_width: px(160) }
                --
                @vertical_divider()
                --
                @comparison(score, previous)
                Node { flex_grow: 1.0 }
            ]
            --
            @row()
            Node { align_items: AlignItems::Stretch }
            Children [ {tiles} ]
        ]
    }
}

/// Small caps context line above a screen title.
pub fn screen_title(context: impl Into<String>, title: impl Into<String>) -> impl Scene {
    bsn! {
        @column()
        Node { row_gap: px(2) }
        Children [ @overline(context) -- @heading(title) ]
    }
}

// ---------------------------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------------------------

pub fn spawn_results(mut commands: Commands, last: Res<LastRun>, settings: Res<Settings>) {
    let stats = &last.stats;
    let kind = stats.scenario.name();
    let context = if stats.drill == kind { "Results".to_string() } else { format!("Results  ·  {kind}") };
    let sens = format!("Sensitivity {:.3}  ·  {}", stats.sens, settings.describe_sens(stats.sens));
    commands
        .spawn_scene(bsn! {
            @overlay()
            Children [
                @panel()
                Node { width: px(660) }
                Children [
                    @screen_title(context, stats.drill.clone())
                    --
                    @score_summary(stats, &last.previous)
                    --
                    @label_dim(sens)
                    --
                    @actions(bsn_list! {
                        @button("Retry", ButtonVariant::Primary)
                        on(retry)
                        --
                        @button("Watch replay", ButtonVariant::Normal)
                        on(watch_last_replay)
                        --
                        @button("Menu", ButtonVariant::Normal)
                        on(go_to_menu)
                    })
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Results));
}

fn retry(
    _: On<Activate>,
    last: Res<LastRun>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let run = Run::new(last.drill.clone(), settings.sens, last.duration, RunPurpose::Practice, last.drill.name.clone());
    begin_run(&mut commands, &mut next_state, run);
}

fn watch_last_replay(
    _: On<Activate>,
    last: Res<LastRun>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    match Replay::load(last.timestamp) {
        Some(replay) => watch(&mut commands, &mut next_state, replay, AppState::Results),
        None => warn!("Replay for this run could not be loaded"),
    }
}
