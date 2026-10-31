//! Menu, results and sensitivity-finder screens, built with Bevy Feathers widgets.

mod custom;
mod finder;
mod history;
mod insights;
mod menu;
mod routine;
mod screens;
pub mod theme;
pub mod widgets;

use bevy::prelude::*;

use crate::{AppState, settings::Settings};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<menu::MenuTab>()
            .init_resource::<history::HistoryView>()
            .init_resource::<custom::CustomSelection>()
            .init_resource::<custom::PlaylistDraft>()
            .init_resource::<custom::CustomStatus>()
            .add_systems(OnEnter(AppState::Menu), menu::spawn_menu)
            .add_systems(OnExit(AppState::Menu), custom::clear_status)
            .add_systems(OnEnter(AppState::Results), screens::spawn_results)
            .add_systems(OnEnter(AppState::Finder), finder::spawn_finder_screen)
            .add_systems(OnEnter(AppState::Routine), routine::spawn_routine_screen)
            .add_systems(Update, finder::draw_smart_chart.run_if(in_state(AppState::Finder)))
            .add_systems(Update, routine::routine_break.run_if(in_state(AppState::Routine)))
            // Post-run screens live outside the menu, so the unlock timer runs in every state.
            .add_systems(Update, widgets::arm_buttons)
            .add_systems(
                Update,
                (
                    widgets::sync_fields.run_if(resource_changed::<Settings>),
                    widgets::highlight_choices,
                    menu::show_tab,
                    menu::update_sens_info,
                    history::rebuild_history,
                    insights::rebuild_insights,
                    custom::rebuild_custom,
                    custom::init_name_inputs,
                    custom::sync_item_names,
                    custom::sync_status,
                    menu::update_discord_status,
                )
                    .run_if(in_state(AppState::Menu)),
            );
    }
}
