//! Menu, post-run screens and in-game overlays, built with Bevy Feathers widgets.

mod bindings;
mod calm_hud;
pub(crate) mod capture;
mod charts;
mod cue_hud;
mod finder_tour;
mod hud;
mod menu;
mod preview;
mod range;
mod replay_hud;
pub(crate) mod screens;
pub mod theme;
pub mod widgets;

mod tap_hud;

use bevy::picking::PickingSystems;
use bevy::prelude::*;

use crate::drill::DrillSet;
use crate::settings::Settings;
use crate::{AppState, PlayState};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((preview::PreviewPlugin, capture::CapturePlugin))
            .init_resource::<menu::MenuTab>()
            .init_resource::<menu::play::PlaySelection>()
            .init_resource::<menu::history::HistoryView>()
            .init_resource::<menu::custom::CustomSelection>()
            .init_resource::<menu::custom::PlaylistDraft>()
            .init_resource::<menu::custom::CustomStatus>()
            .init_resource::<menu::finder::FinderView>()
            .init_resource::<screens::compare::ComparePick>()
            .add_systems(OnEnter(AppState::Menu), menu::spawn_menu)
            .add_systems(OnExit(AppState::Menu), menu::custom::clear_status)
            .add_systems(OnEnter(AppState::Playing), hud::spawn_hud)
            .add_systems(Update, hud::update_hud.after(DrillSet::Finish).run_if(in_state(AppState::Playing)))
            .add_systems(OnEnter(AppState::Playing), tap_hud::spawn_tap_hud)
            .add_systems(
                Update,
                tap_hud::update_tap_hud
                    .after(DrillSet::Finish)
                    .run_if(in_state(AppState::Playing).and_then(resource_exists::<crate::drill::rifle::TapRun>)),
            )
            .add_systems(OnEnter(PlayState::Paused), screens::pause::spawn_pause_menu)
            .add_systems(OnEnter(AppState::Playing), calm_hud::spawn_calm_hud)
            .add_systems(
                Update,
                calm_hud::update_calm_hud
                    .after(DrillSet::Finish)
                    .run_if(in_state(AppState::Playing))
                    .run_if(any_with_component::<calm_hud::PartStrip>),
            )
            .add_systems(Update, screens::results::show_results.in_set(DrillSet::Outcome))
            .add_systems(OnEnter(AppState::Replay), replay_hud::spawn_replay_hud)
            .add_systems(
                Update,
                replay_hud::update_replay_hud
                    .after(crate::replay::player::advance_playback)
                    .run_if(in_state(AppState::Replay)),
            )
            .add_systems(OnEnter(AppState::Results), screens::results::spawn_results)
            .add_systems(OnEnter(AppState::Finder), screens::finder::spawn_finder_screen)
            .add_systems(
                OnEnter(AppState::Finder),
                (screens::checkup::spawn_checkup_screen, screens::compare::spawn_compare_screen),
            )
            .add_systems(
                Update,
                screens::probe::update_probe_hud
                    .run_if(in_state(AppState::Finder))
                    .run_if(resource_exists::<crate::finder::probe::Probe>),
            )
            .add_systems(OnEnter(AppState::Routine), screens::routine::spawn_routine_screen)
            .add_systems(Update, screens::finder::draw_smart_chart.run_if(in_state(AppState::Finder)))
            .add_systems(Update, screens::results::draw_hit_scatter.run_if(in_state(AppState::Results)))
            .add_systems(
                Update,
                cue_hud::show_cue_banner
                    .after(DrillSet::Finish)
                    .run_if(in_state(AppState::Playing))
                    .run_if(resource_changed::<crate::drill::cue::CueBanner>),
            )
            .add_systems(Update, screens::routine::routine_break.run_if(in_state(AppState::Routine)))
            // Post-run screens live outside the menu, so the unlock timer runs in every state.
            .add_systems(Update, widgets::arm_buttons)
            // With the other widgets' styling, right after hover and press states are updated.
            .add_systems(PreUpdate, widgets::style_buttons.in_set(PickingSystems::Last))
            // Systems that queue commands on a page's entities run after `show_tab` (with its
            // commands applied in between): it may be replacing that page this very frame.
            .add_systems(
                Update,
                (
                    bindings::sync_fields.after(menu::show_tab).run_if(resource_changed::<Settings>),
                    bindings::highlight_choices,
                    menu::show_tab,
                    menu::play::rebuild_play_detail.after(menu::show_tab),
                    menu::update_sens_info,
                    menu::history::rebuild_history.after(menu::show_tab),
                    menu::insights::rebuild_insights.after(menu::show_tab),
                    menu::custom::rebuild_custom.after(menu::show_tab),
                    menu::custom::init_name_inputs,
                    menu::custom::sync_item_names,
                    menu::custom::sync_scenario_blurbs,
                    menu::custom::sync_status,
                    menu::update_discord_status,
                    menu::update_update_status,
                    menu::finder::rebuild_finder.after(menu::show_tab),
                )
                    .run_if(in_state(AppState::Menu)),
            )
            .add_systems(
                Update,
                (
                    menu::plan::refresh_plan.after(menu::show_tab),
                    menu::plan::rebuild_plan.after(menu::plan::refresh_plan),
                    menu::benchmark::rebuild_benchmark.after(menu::show_tab),
                )
                    .run_if(in_state(AppState::Menu)),
            )
            .add_systems(OnEnter(AppState::Routine), screens::benchmark::spawn_benchmark_result);
    }
}
