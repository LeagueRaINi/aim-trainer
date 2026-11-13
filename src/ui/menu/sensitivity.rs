//! Sensitivity page: game presets, sensitivity, DPI, yaw and FOV, and what they come to.

use bevy::feathers::constants::fonts;
use bevy::prelude::*;

use super::{MenuTab, main_column, note_card, page, side_column};
use crate::settings::{SettingField, Settings};
use crate::ui::bindings::*;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Text slots filled by [`update_sens_info`].
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub enum SensInfo {
    /// The mouse distance for a full turn.
    #[default]
    Distance,
    /// The same sensitivity in each game, one per line.
    Games,
}

pub(super) fn sensitivity_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Sensitivity, bsn_list! {
            @main_column()
            Children [
                @card("Your game", bsn_list! {
                    @setting_row_hint("Game", "Sets yaw and FOV. Switching keeps your cm/360.", bsn_list! {
                        @choice("Valorant", Choice::Preset(0))
                        --
                        @choice("CS2", Choice::Preset(1))
                    })
                    --
                    @number_field_hint(SettingField::Sens, "Your in-game sensitivity.", settings)
                    --
                    @number_field_hint(SettingField::Dpi, "Your mouse's DPI, for cm/360.", settings)
                })
                --
                @card("Advanced", bsn_list! {
                    @number_field_hint(SettingField::Yaw, "Valorant 0.07, CS2 0.022 (m_yaw).", settings)
                    --
                    @number_field_hint(SettingField::Hfov, "Valorant 103, CS2 106.26 (fov 90 at 4:3).", settings)
                    --
                    @small("No game highlighted above means a custom yaw or FOV.")
                })
            ]
            --
            @side_column()
            Children [
                @card("This comes to", bsn_list! {
                    @styled("", fonts::BOLD, FONT_H1, TEXT_STRONG)
                    SensInfo::Distance
                    --
                    @small("Mouse travel for a full turn: the number to compare between games.")
                    --
                    @rule()
                    Node { margin: UiRect::vertical(px(SPACE_XS)) }
                    --
                    @overline("The same aim in")
                    --
                    @styled("", fonts::REGULAR, FONT_H2, TEXT_STRONG)
                    SensInfo::Games
                })
                --
                @note_card("How it maps", bsn_list! {
                    @small("Mouse input is raw (unaccelerated) counts, like both games. One count turns the view by sens x yaw degrees.")
                    --
                    @small("Valorant: 103 deg horizontal at 16:9, black bars on other aspect ratios.")
                    --
                    @small("CS2: fov 90 at 4:3 is 106.26 deg at 16:9, and wider screens see more.")
                })
            ]
        })
    }
}

pub fn update_sens_info(settings: Res<Settings>, mut info: Query<(&SensInfo, &mut Text)>) {
    if info.is_empty() {
        return;
    }
    let described = settings.describe_sens(settings.sens);
    let (distance, games) = described.split_once("  |  ").unwrap_or((&described, ""));
    let games = games.replace("  |  ", "\n");
    for (slot, mut text) in &mut info {
        let wanted = match slot {
            SensInfo::Distance => distance,
            SensInfo::Games => &games,
        };
        if text.0 != wanted {
            text.0 = wanted.to_string();
        }
    }
}
