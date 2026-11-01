//! Play page: built-in drills by category, their adaptive modes and the warmup routines.

use bevy::feathers::controls::ButtonVariant;
use bevy::feathers::display::label_dim;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{MAIN_COLUMN_W, MenuTab, page};
use crate::AppState;
use crate::drill::adaptive::MEASURE_SECS;
use crate::drill::def::{Category, DecayRule};
use crate::drill::{ADAPTIVE_TIME_LIMIT, DrillConfig, DrillKind, DrillSpec, Run, RunPurpose, begin_run};
use crate::metrics::quest::QuestMode;
use crate::routine::{ROUTINES, Routine, start_routine};
use crate::settings::{SettingField, Settings};
use crate::ui::bindings::*;
use crate::ui::theme::*;
use crate::ui::widgets::*;

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

/// One card per category, its drills as tiles two to a row: more drills only add rows, and the
/// column scrolls once they outgrow the menu.
fn category_card(category: Category) -> impl Scene {
    let tiles: Vec<_> =
        DrillKind::ALL.into_iter().filter(|kind| kind.def().category == category).map(drill_tile).collect();
    bsn! {
        @card(category.title(), bsn_list! {
            @wrap_row()
            Node { column_gap: px(SPACE_S), align_items: AlignItems::Stretch }
            Children [ {tiles} ]
        })
    }
}

/// Explains the global decay setting and names the drills it applies to.
fn decay_note() -> String {
    let kinds: Vec<&str> =
        DrillKind::ALL.into_iter().filter(|kind| kind.def().decay == DecayRule::Global).map(DrillKind::name).collect();
    format!("Decay: targets shrink and count as a miss if not hit in time ({}). 0 = off.", kinds.join(", "))
}

/// What the adaptive buttons do.
fn adaptive_note() -> String {
    format!(
        "Measure / Train on a drill start adaptive mode: targets resize to your limit and must be hit in the decay time ({ADAPTIVE_TIME_LIMIT} s with decay off). Measure runs {MEASURE_SECS:.0} s, Train the drill length. θ75 = radius you hit 75% of the time; lower is better."
    )
}

pub(super) fn play_page(settings: &Settings) -> impl Scene {
    let cards: Vec<_> = Category::ALL.into_iter().map(category_card).collect();
    bsn! {
        @page(MenuTab::Play)
        Children [
            // Wider by the scrollbar gutter, so two tiles still fit side by side while it shows.
            @scroll_column(bsn_list! { {cards} })
            Node { width: px(MAIN_COLUMN_W + SCROLLBAR_GUTTER), flex_shrink: 0.0 }
            --
            @scroll_column(bsn_list! {
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
                })
                --
                @card("Adaptive mode", bsn_list! { @label_dim(adaptive_note()) })
            })
            Node { flex_grow: 1.0, flex_basis: px(0) }
        ]
    }
}

/// Two tiles and the gap between them fill a card on the main column.
const TILE_W: f32 = (MAIN_COLUMN_W - 2.0 * SPACE_L - SPACE_S) / 2.0 - 1.0;

/// A drill's play button and blurb, plus its adaptive modes if it has them (at the bottom, so
/// tiles side by side line up).
fn drill_tile(kind: DrillKind) -> impl Scene {
    let adaptive: Vec<_> = kind.def().adaptive.then(|| adaptive_row(kind)).into_iter().collect();
    bsn! {
        @tile()
        Node { width: px(TILE_W), row_gap: px(SPACE_S), padding: px(SPACE_M) }
        Children [
            @button(kind.name(), ButtonVariant::Normal)
            PlayButton(kind)
            on(play)
            --
            @label_dim(DrillConfig::defaults(kind).blurb())
            Node { flex_grow: 1.0 }
            --
            {adaptive}
        ]
    }
}

/// A drill's adaptive modes as a slim row at the bottom of its tile.
fn adaptive_row(kind: DrillKind) -> impl Scene {
    let (measure, train) = (QuestMode::Measure, QuestMode::Train);
    bsn! {
        @row()
        Node { column_gap: px(SPACE_S) }
        Children [
            @overline("Adaptive")
            --
            @compact_button(measure.name(), ButtonVariant::Normal)
            AdaptiveButton(kind, measure)
            on(play_adaptive)
            --
            @compact_button(train.name(), ButtonVariant::Normal)
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
