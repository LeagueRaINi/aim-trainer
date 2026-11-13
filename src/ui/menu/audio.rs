//! Audio page: hit sound and volume.

use bevy::prelude::*;

use super::{MenuTab, main_column, page};
use crate::settings::{HitSound, SettingField, Settings};
use crate::ui::bindings::*;
use crate::ui::widgets::*;

pub(super) fn audio_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Audio, bsn_list! {
            @main_column()
            Children [
                @card("Hit sound", bsn_list! {
                    @setting_row_hint("Sound", "Click one to hear it.", bsn_list! {
                        @choice(HitSound::ALL[0].name(), Choice::HitSound(HitSound::ALL[0]))
                        --
                        @choice(HitSound::ALL[1].name(), Choice::HitSound(HitSound::ALL[1]))
                        --
                        @choice(HitSound::ALL[2].name(), Choice::HitSound(HitSound::ALL[2]))
                        --
                        @choice(HitSound::ALL[3].name(), Choice::HitSound(HitSound::ALL[3]))
                        --
                        @choice(HitSound::ALL[4].name(), Choice::HitSound(HitSound::ALL[4]))
                    })
                    --
                    @number_field_hint(SettingField::Volume, "Changing it plays the sound.", settings)
                })
            ]
        })
    }
}
