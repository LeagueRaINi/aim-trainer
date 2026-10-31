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
    drill::{
        DrillKind, DrillSpec, Run, RunPurpose, begin_run,
        def::{Category, DecayRule},
    },
    routine::{ROUTINES, Routine, start_routine},
    settings::{SettingField, Settings},
    ui::{bindings::*, theme::*, widgets::*},
};

#[derive(Component, Clone, Copy, Default)]
struct PlayButton(DrillKind);

#[derive(Component, Clone, Copy, Default)]
struct RoutineButton(usize);

/// A built-in drill played on its own, with the global drill length.
fn practice_run(kind: DrillKind, settings: &Settings) -> Run {
    Run::new(
        DrillSpec::builtin(kind, settings),
        settings.sens,
        settings.run_secs,
        RunPurpose::Practice,
        kind.name().to_string(),
    )
}

/// The built-in drills grouped into one card per category.
fn drill_cards() -> Vec<impl Scene> {
    Category::ALL
        .into_iter()
        .map(|category| {
            let buttons: Vec<_> =
                DrillKind::ALL.into_iter().filter(|kind| kind.def().category == category).map(play_button).collect();
            card(category.title(), buttons)
        })
        .collect()
}

/// Explains the global decay setting and names the drills it applies to.
fn decay_note() -> String {
    let kinds: Vec<&str> =
        DrillKind::ALL.into_iter().filter(|kind| kind.def().decay == DecayRule::Global).map(DrillKind::name).collect();
    format!("Decay: targets shrink and count as a miss if not hit in time ({}). 0 = off.", kinds.join(", "))
}

pub(super) fn play_page(settings: &Settings) -> impl Scene {
    let cards = drill_cards();
    bsn! {
        @page(MenuTab::Play)
        Children [
            @main_column()
            Children [ {cards} ]
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
                    @label_dim(decay_note())
                    --
                    @label("Target color")
                    --
                    @target_color_choices()
                })
            ]
        ]
    }
}

fn play_button(kind: DrillKind) -> impl Scene {
    bsn! {
        @row()
        Node { column_gap: px(SPACE_M) }
        Children [
            @button(kind.name(), ButtonVariant::Normal)
            Node { width: px(140), flex_shrink: 0.0 }
            PlayButton(kind)
            on(play)
            --
            @label_dim(kind.blurb())
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
    if let Ok(&PlayButton(kind)) = buttons.get(activate.event_target()) {
        begin_run(&mut commands, &mut next_state, practice_run(kind, &settings));
    }
}

