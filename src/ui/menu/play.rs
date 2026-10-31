//! Play page: built-in drills by category and the warmup routines.

use bevy::{
    feathers::{
        controls::ButtonVariant,
        display::{label, label_dim},
    },
    prelude::*,
    ui_widgets::Activate,
};

use super::{MenuTab, main_column, page, side_column};
use crate::{
    AppState,
    routine::{ROUTINES, Routine, start_routine},
    scenario::{DrillSpec, Run, RunPurpose, Scenario, begin_run},
    settings::{SettingField, Settings},
    ui::{bindings::*, theme::*, widgets::*},
};

#[derive(Component, Clone, Copy, Default)]
struct PlayButton(Scenario);

#[derive(Component, Clone, Copy, Default)]
struct RoutineButton(usize);

/// A built-in drill played on its own, with the global drill length.
fn practice_run(scenario: Scenario, settings: &Settings) -> Run {
    Run::new(
        DrillSpec::builtin(scenario, settings),
        settings.sens,
        settings.run_secs,
        RunPurpose::Practice,
        scenario.name().to_string(),
    )
}


pub(super) fn play_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Play)
        Children [
            @main_column()
            Children [
                @card("Aim", bsn_list! {
                    @play_button(Scenario::Gridshot)
                    --
                    @play_button(Scenario::Microflick)
                    --
                    @play_button(Scenario::Tracking)
                })
                --
                @card("Valorant / CS2", bsn_list! {
                    @play_button(Scenario::HeadLevel)
                    --
                    @play_button(Scenario::Peek)
                })
                --
                @card("Eye training", bsn_list! {
                    @play_button(Scenario::Reaction)
                    --
                    @play_button(Scenario::Peripheral)
                })
            ]
            --
            @side_column()
            Children [
                @card("Warmup routines", bsn_list! {
                    @routine_button(0)
                    --
                    @routine_button(1)
                    --
                    @routine_button(2)
                })
                --
                @card("Drill settings", bsn_list! {
                    @number_field(SettingField::RunSecs, settings)
                    --
                    @number_field(SettingField::TargetScale, settings)
                    --
                    @number_field(SettingField::TargetLifetime, settings)
                    --
                    @label_dim("Decay: targets shrink and count as a miss if not hit in time (Gridshot, Microflick, Head level). 0 = off.")
                    --
                    @label("Target color")
                    --
                    @target_color_choices()
                })
            ]
        ]
    }
}

fn play_button(scenario: Scenario) -> impl Scene {
    bsn! {
        @row()
        Node { column_gap: px(SPACE_M) }
        Children [
            @button(scenario.name(), ButtonVariant::Normal)
            Node { width: px(140), flex_shrink: 0.0 }
            PlayButton(scenario)
            on(play)
            --
            @label_dim(scenario.blurb())
        ]
    }
}

fn routine_button(index: usize) -> impl Scene {
    let plan = &ROUTINES[index];
    bsn! {
        @row()
        Node { column_gap: px(SPACE_M) }
        Children [
            @button(plan.name, ButtonVariant::Primary)
            Node { width: px(140), flex_shrink: 0.0 }
            RoutineButton(index)
            on(|activate: On<Activate>, buttons: Query<&RoutineButton>, settings: Res<Settings>, mut commands: Commands, mut next: ResMut<NextState<AppState>>| {
                if let Ok(&RoutineButton(index)) = buttons.get(activate.event_target()) {
                    start_routine(&mut commands, &mut next, Routine::from_plan(&ROUTINES[index], &settings));
                }
            })
            --
            @label_dim(plan.blurb)
        ]
    }
}

fn play(
    activate: On<Activate>,
    buttons: Query<&PlayButton>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if let Ok(&PlayButton(scenario)) = buttons.get(activate.event_target()) {
        begin_run(&mut commands, &mut next_state, practice_run(scenario, &settings));
    }
}

