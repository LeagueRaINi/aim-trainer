//! Audio page: hit sound and volume.

use bevy::feathers::display::label_dim;
use bevy::prelude::*;

use super::{MenuTab, main_column, page};
use crate::settings::{HitSound, SettingField, Settings};
use crate::ui::bindings::*;
use crate::ui::widgets::*;

pub(super) fn audio_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Audio)
        Children [
            @main_column()
            Children [
                @card("Sound", bsn_list! {
                    @setting_row("Hit sound", bsn_list! {
                        @choice(HitSound::ALL[0].name(), Choice::HitSound(HitSound::ALL[0]))
                        --
                        @choice(HitSound::ALL[1].name(), Choice::HitSound(HitSound::ALL[1]))
                        --
                        @choice(HitSound::ALL[2].name(), Choice::HitSound(HitSound::ALL[2]))
                        --
                        @choice(HitSound::ALL[3].name(), Choice::HitSound(HitSound::ALL[3]))
                    })
                    --
                    @number_field(SettingField::Volume, settings)
                    --
                    @label_dim("Picking a sound or changing the volume plays a preview.")
                })
            ]
        ]
    }
}
