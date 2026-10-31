//! Menu, post-run screens and in-game overlays, built with Bevy Feathers widgets.

mod bindings;
mod charts;
mod hud;
mod menu;
mod replay_hud;
pub(crate) mod screens;
pub mod theme;
pub mod widgets;

use bevy::prelude::*;

use crate::AppState;
use crate::drill::DrillSet;
use crate::settings::Settings;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<menu::MenuTab>()
            .init_resource::<menu::history::HistoryView>()
            .init_resource::<menu::custom::CustomSelection>()
            .init_resource::<menu::custom::PlaylistDraft>()
            .init_resource::<menu::custom::CustomStatus>()
            .add_systems(OnEnter(AppState::Menu), menu::spawn_menu)
            .add_systems(OnExit(AppState::Menu), menu::custom::clear_status)
            .add_systems(OnEnter(AppState::Playing), hud::spawn_hud)
            .add_systems(Update, hud::update_hud.after(DrillSet::Finish).run_if(in_state(AppState::Playing)))
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
            .add_systems(OnEnter(AppState::Routine), screens::routine::spawn_routine_screen)
            .add_systems(Update, screens::finder::draw_smart_chart.run_if(in_state(AppState::Finder)))
            .add_systems(Update, screens::results::draw_hit_scatter.run_if(in_state(AppState::Results)))
            .add_systems(Update, screens::routine::routine_break.run_if(in_state(AppState::Routine)))
            // Post-run screens live outside the menu, so the unlock timer runs in every state.
            .add_systems(Update, widgets::arm_buttons)
            .add_systems(
                Update,
                (
                    bindings::sync_fields.run_if(resource_changed::<Settings>),
                    bindings::highlight_choices,
                    menu::show_tab,
                    menu::update_sens_info,
                    menu::history::rebuild_history,
                    menu::insights::rebuild_insights,
                    menu::custom::rebuild_custom,
                    menu::custom::init_name_inputs,
                    menu::custom::sync_item_names,
                    menu::custom::sync_status,
                    menu::update_discord_status,
                )
                    .run_if(in_state(AppState::Menu)),
            );
    }
}
