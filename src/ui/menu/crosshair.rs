//! Crosshair page: color, center dot, lines, outlines and a live preview.

use bevy::feathers::controls::ButtonVariant;
use bevy::feathers::display::label_dim;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{MenuTab, main_column, page, side_column};
use crate::crosshair::CrosshairPreview;
use crate::settings::{CrosshairSettings, SettingField, Settings};
use crate::ui::bindings::*;
use crate::ui::theme::*;
use crate::ui::widgets::*;

pub(super) fn crosshair_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Crosshair)
        Children [
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
                @row()
                Node { align_items: AlignItems::Start, column_gap: px(SPACE_L) }
                Children [
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
                    Node { flex_grow: 1.0, flex_basis: px(0) }
                    --
                    @card("Outlines", bsn_list! {
                        @on_off_row("Outlines", Choice::Outlines(true), Choice::Outlines(false))
                        --
                        @number_field(SettingField::OutlineThickness, settings)
                        --
                        @number_field(SettingField::OutlineOpacity, settings)
                    })
                    Node { flex_grow: 1.0, flex_basis: px(0) }
                ]
            ]
            --
            @side_column()
            Children [
                @card("Preview", bsn_list! {
                    Node {
                        height: px(220),
                        border_radius: BorderRadius::all(px(6)),
                    }
                    BackgroundColor(Color::srgb_u8(44, 47, 56))
                    CrosshairPreview
                    --
                    @label_dim("Actual pixel size.")
                    --
                    @actions(bsn_list! {
                        @button("Reset to Valorant default", ButtonVariant::Normal)
                        on(|_: On<Activate>, mut settings: ResMut<Settings>| {
                            settings.crosshair = CrosshairSettings::default();
                        })
                    })
                })
            ]
        ]
    }
}
