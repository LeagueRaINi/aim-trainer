//! Video page: display mode, aspect, VSync, MSAA and the FPS cap; the targets' style and color;
//! and the scene, with a live preview of all three.

use bevy::feathers::display::{label, label_dim};
use bevy::prelude::*;

use super::{MenuTab, main_column, page};
use crate::settings::{AspectMode, DisplayMode, SettingField, Settings};
use crate::ui::bindings::*;
use crate::ui::preview::PreviewView;
use crate::ui::widgets::*;

pub(super) fn video_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Video)
        Children [
            @main_column()
            Children [
                @card("Display", bsn_list! {
                    @setting_row("Display mode", bsn_list! {
                        @choice("Windowed", Choice::Display(DisplayMode::Windowed))
                        --
                        @choice("Borderless", Choice::Display(DisplayMode::Borderless))
                        --
                        @choice("Fullscreen", Choice::Display(DisplayMode::Fullscreen))
                    })
                    --
                    @setting_row("Aspect ratio", bsn_list! {
                        @choice("16:9 + black bars", Choice::Aspect(AspectMode::Lock16x9))
                        --
                        @choice("Fill screen (Hor+)", Choice::Aspect(AspectMode::Native))
                    })
                    --
                    @on_off_row("VSync", Choice::Vsync(true), Choice::Vsync(false))
                    --
                    @setting_row("Anti-aliasing (MSAA)", bsn_list! {
                        @choice("Off", Choice::Msaa(1))
                        --
                        @choice("2x", Choice::Msaa(2))
                        --
                        @choice("4x", Choice::Msaa(4))
                        --
                        @choice("8x", Choice::Msaa(8))
                    })
                    --
                    @number_field(SettingField::FpsCap, settings)
                    --
                    @number_field(SettingField::UiScale, settings)
                })
                --
                @card("Targets", bsn_list! {
                    @setting_row("Style", bsn_list! { @target_style_choices() })
                    --
                    @label("Color")
                    --
                    @target_color_choices()
                    --
                    @label_dim("Glow adds a bloom pass, which costs some FPS.")
                })
            ]
            --
            @scroll_column(bsn_list! {
                @card("Scene", bsn_list! {
                    // The image fills a 16:9 box rather than sizing it, so its 640 px don't widen
                    // the column.
                    Node {
                        width: percent(100),
                        aspect_ratio: {Some(16.0 / 9.0)},
                        overflow: Overflow::clip(),
                        border_radius: BorderRadius::all(px(6)),
                    }
                    Children [
                        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100) }
                        PreviewView
                    ]
                    --
                    @label("Environment")
                    --
                    @scene_choices()
                    --
                    @setting_row("Weapon in drills", weapon_choices())
                    --
                    @setting_row("Weapon side", weapon_side_choices())
                })
                --
                @card("Tips", bsn_list! {
                    @label_dim("16:9 + black bars renders exactly what Valorant shows on an ultrawide or 16:10 screen. Fill screen behaves like CS2.")
                    --
                    @label_dim("For the lowest input lag: VSync off, Fullscreen, FPS cap 0 or well above your refresh rate.")
                })
            })
            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
        ]
    }
}
