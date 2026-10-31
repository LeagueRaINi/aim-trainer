//! Discord page: Rich Presence toggle and connection status.

use bevy::feathers::display::{label, label_dim};
use bevy::prelude::*;

use super::{MenuTab, main_column, page, side_column};
use crate::presence::{Connection, PresenceLink};
use crate::settings::Settings;
use crate::ui::bindings::*;
use crate::ui::widgets::*;

pub(super) fn discord_page() -> impl Scene {
    bsn! {
        @page(MenuTab::Discord)
        Children [
            @main_column()
            Children [
                @card("Rich Presence", bsn_list! {
                    @setting_row("Show activity", bsn_list! {
                        @choice("On", Choice::Discord(true))
                        --
                        @choice("Off", Choice::Discord(false))
                    })
                    --
                    @label("")
                    DiscordStatus
                })
            ]
            --
            @side_column()
            Children [
                @card("What it shows", bsn_list! {
                    @label_dim("Your Discord profile shows \"Playing Aim Trainer\" with what you're doing: the drill and a countdown, your warmup routine step, the sens finder, or your last score. Your sensitivity is never shown.")
                    --
                    @label_dim("The Discord desktop app must be running on this PC.")
                })
            ]
        ]
    }
}

#[derive(Component, Clone, Copy, Default)]
pub struct DiscordStatus;

pub fn update_discord_status(
    settings: Res<Settings>,
    link: Res<PresenceLink>,
    mut labels: Query<&mut Text, With<DiscordStatus>>,
) {
    let wanted = match (settings.discord.enabled, link.connection()) {
        (false, _) => "Off.",
        (true, Connection::Connected) => "Connected: your profile shows what you're playing.",
        (true, Connection::Searching | Connection::Off) => {
            "Not connected: is the Discord app running? Retrying every 10 s."
        },
    };
    for mut text in &mut labels {
        if text.0 != wanted {
            text.0 = wanted.to_string();
        }
    }
}
