//! Video page: display mode, aspect, VSync, MSAA and the FPS cap.

use bevy::{
    feathers::display::label_dim,
    prelude::*,
};

use super::{MenuTab, main_column, page, side_column};
use crate::{
    settings::{AspectMode, DisplayMode, SettingField, Settings},
    ui::{bindings::*, widgets::*},
};

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
                })
                --
                @card("Performance", bsn_list! {
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
                })
            ]
            --
            @side_column()
            Children [
                @card("Tips", bsn_list! {
                    @label_dim("16:9 + black bars renders exactly what Valorant shows on an ultrawide or 16:10 screen. Fill screen behaves like CS2.")
                    --
                    @label_dim("For the lowest input lag: VSync off, Fullscreen, FPS cap 0 or well above your refresh rate.")
                })
            ]
        ]
    }
}

