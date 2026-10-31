//! Online page: Discord Rich Presence and automatic updates, with their live status.

use bevy::feathers::controls::ButtonVariant;
use bevy::feathers::display::{label, label_dim};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{MenuTab, main_column, page, side_column};
use crate::presence::{Connection, PresenceLink};
use crate::settings::Settings;
use crate::ui::bindings::*;
use crate::ui::theme::SPACE_M;
use crate::ui::widgets::*;
use crate::update::{UpdateStatus, Updater, current_version};

pub(super) fn online_page() -> impl Scene {
    bsn! {
        @page(MenuTab::Online)
        Children [
            @main_column()
            Children [
                @card("Discord Rich Presence", bsn_list! {
                    @on_off_row("Show activity", Choice::Discord(true), Choice::Discord(false))
                    --
                    @label("")
                    DiscordStatus
                })
                --
                @card("Updates", bsn_list! {
                    @on_off_row("Install updates automatically", Choice::AutoUpdate(true), Choice::AutoUpdate(false))
                    --
                    @row()
                    Node { column_gap: px(SPACE_M) }
                    Children [
                        @label("")
                        UpdateStatusLabel
                        --
                        @spacer()
                        --
                        @restart_button()
                    ]
                })
            ]
            --
            @side_column()
            Children [
                @card("What Discord shows", bsn_list! {
                    @label_dim("Your Discord profile shows \"Playing Aim Trainer\" with what you're doing: the drill and a countdown, your warmup routine step, the sens finder, or your last score. Your sensitivity is never shown.")
                    --
                    @label_dim("The Discord desktop app must be running on this PC.")
                })
                --
                @card("How updates work", bsn_list! {
                    @label_dim("At startup the trainer checks GitHub for a newer release and downloads it in the background, checked against the checksum GitHub publishes for it. It replaces this exe and runs from the next start.")
                    --
                    @label_dim("Your settings, history, replays and custom drills stay as they are.")
                })
            ]
        ]
    }
}

/// Restarts into the installed update; only shown once there is one.
pub(super) fn restart_button() -> impl Scene {
    bsn! {
        @button("Restart to update", ButtonVariant::Primary)
        Node { display: Display::None }
        RestartButton
        on(|_: On<Activate>, updater: Res<Updater>, mut exit: MessageWriter<AppExit>| {
            if updater.restart() {
                exit.write(AppExit::Success);
            }
        })
    }
}

#[derive(Component, Clone, Copy, Default)]
pub struct DiscordStatus;

#[derive(Component, Clone, Copy, Default)]
pub struct UpdateStatusLabel;

#[derive(Component, Clone, Copy, Default)]
pub struct RestartButton;

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

/// Keeps the update status line and the restart buttons in step with the update check.
pub fn update_update_status(
    updater: Res<Updater>,
    mut labels: Query<&mut Text, With<UpdateStatusLabel>>,
    mut buttons: Query<&mut Node, With<RestartButton>>,
) {
    let status = updater.status();
    let current = current_version();
    let wanted = match &status {
        UpdateStatus::Idle | UpdateStatus::Checking => "Checking for updates...".to_string(),
        UpdateStatus::Off => format!("Off: staying on v{current}."),
        UpdateStatus::Unavailable(reason) => format!("v{current}, not updated automatically: {reason}."),
        UpdateStatus::UpToDate => format!("Up to date (v{current})."),
        UpdateStatus::Downloading(version) => format!("Downloading v{version}..."),
        UpdateStatus::Ready(version) => format!("v{version} is installed. Restart to use it."),
        UpdateStatus::Failed(err) => format!("Update failed: {err}. Trying again at the next start."),
    };
    for mut text in &mut labels {
        if text.0 != wanted {
            text.0.clone_from(&wanted);
        }
    }
    let display = if matches!(status, UpdateStatus::Ready(_)) { Display::Flex } else { Display::None };
    for mut node in &mut buttons {
        if node.display != display {
            node.display = display;
        }
    }
}
