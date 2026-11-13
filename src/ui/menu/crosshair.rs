//! Crosshair page: color, center dot, lines, outlines and a live preview.

use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{MenuTab, main_column, page, side_column};
use crate::crosshair::CrosshairPreview;
use crate::settings::{CrosshairSettings, SettingField, Settings};
use crate::ui::bindings::*;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Height of the preview box; with the Outlines card below it the column fits the menu.
const PREVIEW_H: f32 = 220.0;

pub(super) fn crosshair_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Crosshair, bsn_list! {
            @main_column()
            Children [
                @card("Color", bsn_list! {
                    @crosshair_color_choices()
                    --
                    @setting_row("Custom RGB", bsn_list! {
                        @number_input(SettingField::ColorR, settings, 72.0)
                        --
                        @number_input(SettingField::ColorG, settings, 72.0)
                        --
                        @number_input(SettingField::ColorB, settings, 72.0)
                    })
                })
                --
                @card("Center dot & lines", bsn_list! {
                    @on_off_row("Center dot", Choice::CenterDot(true), Choice::CenterDot(false))
                    --
                    @number_field(SettingField::DotThickness, settings)
                    --
                    @on_off_row("Inner lines", Choice::InnerLines(true), Choice::InnerLines(false))
                    --
                    @number_field(SettingField::LineLength, settings)
                    --
                    @number_field(SettingField::LineThickness, settings)
                    --
                    @number_field(SettingField::LineOffset, settings)
                })
            ]
            --
            @side_column()
            Children [
                @card("Preview", bsn_list! {
                    Node {
                        height: px(PREVIEW_H),
                        border_radius: BorderRadius::all(px(RADIUS_S)),
                    }
                    ThemeBackgroundColor(PREVIEW_BG)
                    CrosshairPreview
                    --
                    @row()
                    Children [
                        @small("Shown at its real size in pixels.")
                        Node { flex_grow: 1.0, flex_basis: px(0) }
                        --
                        @compact_button("Reset to Valorant default", Btn::Secondary)
                        Node { flex_shrink: 0.0 }
                        on(|_: On<Activate>, mut settings: ResMut<Settings>| {
                            settings.crosshair = CrosshairSettings::default();
                        })
                    ]
                })
                --
                @card("Outlines", bsn_list! {
                    @on_off_row("Outlines", Choice::Outlines(true), Choice::Outlines(false))
                    --
                    @number_field(SettingField::OutlineThickness, settings)
                    --
                    @number_field(SettingField::OutlineOpacity, settings)
                })
            ]
        })
    }
}
