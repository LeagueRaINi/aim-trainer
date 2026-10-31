//! The results screen after a practice run.

use bevy::{
    feathers::{controls::ButtonVariant, display::label_dim},
    prelude::*,
    ui_widgets::Activate,
};

use crate::{
    AppState,
    replay::{Replay, watch},
    scenario::{LastRun, Run, RunPurpose, begin_run},
    settings::Settings,
    ui::widgets::*,
};

use super::score::*;

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
