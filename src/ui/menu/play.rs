//! Play page: built-in drills by category, their adaptive modes and the warmup routines.

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
        ADAPTIVE_TIME_LIMIT, DrillKind, DrillSpec, Run, RunPurpose,
        adaptive::MEASURE_SECS,
        begin_run,
        def::{Category, DecayRule},
    },
    metrics::quest::QuestMode,
    routine::{ROUTINES, Routine, start_routine},
    settings::{SettingField, Settings},
    ui::{bindings::*, theme::*, widgets::*},
};

#[derive(Component, Clone, Copy, Default)]
struct PlayButton(DrillKind);

#[derive(Component, Clone, Copy, Default)]
struct RoutineButton(usize);

#[derive(Component, Clone, Copy, Default)]
struct AdaptiveButton(DrillKind, QuestMode);

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

/// A built-in drill in adaptive mode: Measure runs long enough for a solid estimate, Train for
/// the global drill length.
fn adaptive_run(kind: DrillKind, mode: QuestMode, settings: &Settings) -> Run {
    let drill = DrillSpec::adaptive(kind, mode, settings);
    let secs = match mode {
        QuestMode::Measure => MEASURE_SECS,
        QuestMode::Train => settings.run_secs,
    };
    let title = drill.name.clone();
    Run::new(drill, settings.sens, secs, RunPurpose::Practice, title)
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

/// One row per drill with an adaptive mode, and how it works.
fn adaptive_card() -> impl Scene {
    let rows: Vec<_> = DrillKind::ALL.into_iter().filter(|kind| kind.def().adaptive).map(adaptive_row).collect();
    let note = format!(
        "Each target's size adapts to you, and it counts only if hit before its time runs out (the decay time above, or {ADAPTIVE_TIME_LIMIT} s with decay off). Measure ({MEASURE_SECS:.0} s): {} Train: {} Results show θ75, the radius you hit 75% of the time; smaller is better.",
        QuestMode::Measure.blurb(),
        QuestMode::Train.blurb()
    );
    bsn! {
        @card("Adaptive", bsn_list! {
            {rows}
            --
            @label_dim(note)
        })
    }
}

pub(super) fn play_page(settings: &Settings) -> impl Scene {
    let cards = drill_cards();
    bsn! {
        @page(MenuTab::Play)
        Children [
            @main_column()
            Children [ {cards} -- @adaptive_card() ]
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

/// A drill's adaptive modes as two buttons.
fn adaptive_row(kind: DrillKind) -> impl Scene {
    let (measure, train) = (QuestMode::Measure, QuestMode::Train);
    bsn! {
        @row()
        Node { column_gap: px(SPACE_M), align_items: AlignItems::Center }
        Children [
            @label(kind.name())
            Node { width: px(140), flex_shrink: 0.0 }
            --
            @button(measure.name(), ButtonVariant::Primary)
            AdaptiveButton(kind, measure)
            on(play_adaptive)
            --
            @button(train.name(), ButtonVariant::Normal)
            AdaptiveButton(kind, train)
            on(play_adaptive)
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

fn play_adaptive(
    activate: On<Activate>,
    buttons: Query<&AdaptiveButton>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if let Ok(&AdaptiveButton(kind, mode)) = buttons.get(activate.event_target()) {
        begin_run(&mut commands, &mut next_state, adaptive_run(kind, mode, &settings));
    }
}

