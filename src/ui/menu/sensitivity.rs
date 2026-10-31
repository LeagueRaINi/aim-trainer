//! Sensitivity page: game presets, sensitivity, DPI, yaw and FOV.

use bevy::{
    feathers::display::{label, label_dim},
    prelude::*,
};

use super::{MenuTab, main_column, page, side_column};
use crate::{
    settings::{SettingField, Settings},
    ui::{bindings::*, theme::*, widgets::*},
};

#[derive(Component, Clone, Copy, Default)]
pub struct SensInfo;

pub(super) fn sensitivity_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Sensitivity)
        Children [
            @main_column()
            Children [
                @card("Sensitivity", bsn_list! {
                    @setting_row("Game", bsn_list! {
                        @choice("Valorant", Choice::Preset(0))
                        --
                        @choice("CS2", Choice::Preset(1))
                    })
                    --
                    @number_field(SettingField::Sens, settings)
                    --
                    @number_field(SettingField::Dpi, settings)
                    --
                    @number_field(SettingField::Yaw, settings)
                    --
                    @number_field(SettingField::Hfov, settings)
                    --
                    @tile()
                    Node { margin: UiRect::top(px(SPACE_XS)), row_gap: px(SPACE_XS) }
                    Children [
                        @overline("Equivalent")
                        --
                        @label("")
                        SensInfo
                    ]
                })
            ]
            --
            @side_column()
            Children [
                @card("How it maps", bsn_list! {
                    @label_dim("Mouse input is raw (unaccelerated) counts, like both games. One count turns the view by sens x yaw degrees.")
                    --
                    @label_dim("Valorant: yaw 0.07, 103 deg horizontal at 16:9, black bars on other aspect ratios.")
                    --
                    @label_dim("CS2: yaw 0.022 (m_yaw), fov 90 at 4:3 = 106.26 deg at 16:9, wider screens see more.")
                    --
                    @label_dim("Switching game keeps your cm/360. No preset highlighted = custom yaw/FOV.")
                })
            ]
        ]
    }
}


pub fn update_sens_info(settings: Res<Settings>, mut info: Query<&mut Text, With<SensInfo>>) {
    let wanted = settings.describe_sens(settings.sens).replace("  |  ", "\n");
    for mut text in &mut info {
        if text.0 != wanted {
            text.0.clone_from(&wanted);
        }
    }
}
