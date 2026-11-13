//! Online page: Discord Rich Presence and automatic updates, with their live status.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{MenuTab, main_column, note_card, page, side_column};
use crate::presence::{Connection, PresenceLink};
use crate::settings::Settings;
use crate::ui::bindings::*;
use crate::ui::theme::SPACE_M;
use crate::ui::widgets::*;
use crate::update::{UpdateStatus, Updater, current_version};

pub(super) fn online_page() -> impl Scene {
    bsn! {
        @page(MenuTab::Online, bsn_list! {
            @main_column()
            Children [
                @card("Discord Rich Presence", bsn_list! {
                    @on_off_row_hint(
                        "Show activity",
                        "Needs the Discord desktop app running on this PC.",
                        Choice::Discord(true),
                        Choice::Discord(false)
                    )
                    --
                    @label_strong("")
                    DiscordStatus
                })
                --
                @card("Updates", bsn_list! {
                    @on_off_row_hint(
                        "Install updates automatically",
                        "Checked at startup, installed at the next start.",
                        Choice::AutoUpdate(true),
                        Choice::AutoUpdate(false)
                    )
                    --
                    @row()
                    Node { column_gap: px(SPACE_M) }
                    Children [
                        @label_strong("")
                        Node { flex_grow: 1.0, flex_basis: px(0) }
                        UpdateStatusLabel
                        --
                        @restart_button()
                    ]
                })
            ]
            --
            @side_column()
            Children [
                @note_card("What Discord shows", bsn_list! {
                    @small("Your Discord profile shows \"Playing Aim Trainer\" with what you're doing: the drill and a countdown, your warmup routine step, the sens finder, or your last score.")
                    --
                    @small("Your sensitivity is never shown.")
                })
                --
                @note_card("How updates work", bsn_list! {
                    @small("At startup the trainer checks GitHub for a newer release and downloads it in the background. Only a download signed with the release key is installed, at the next start or when you restart.")
                    --
                    @small("Your settings, history, replays and custom drills stay as they are.")
                })
            ]
        })
    }
}

/// Restarts into the downloaded update; only shown once there is one.
pub(super) fn restart_button() -> impl Scene {
    bsn! {
        @button("Restart to update", Btn::Primary)
        Node { display: Display::None, min_width: px(0) }
        RestartButton
        on(|_: On<Activate>, updater: Res<Updater>, mut exit: MessageWriter<AppExit>| {
            // The update goes in once the app has quit and saved (see `main`).
            updater.request_restart();
            exit.write(AppExit::Success);
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

/// Keeps the update status line and the restart buttons in step with the update check and the
/// setting; rebuilds them only when either changed, or one of them was just spawned (the menu,
/// or the Online page in it).
#[allow(clippy::type_complexity)]
pub fn update_update_status(
    settings: Res<Settings>,
    updater: Res<Updater>,
    mut shown: Local<Option<(UpdateStatus, bool)>>,
    spawned: Query<(), Or<(Added<UpdateStatusLabel>, Added<RestartButton>)>>,
    mut labels: Query<&mut Text, With<UpdateStatusLabel>>,
    mut buttons: Query<&mut Node, With<RestartButton>>,
) {
    let status = updater.status();
    let auto = settings.updates.auto;
    if spawned.is_empty() && shown.as_ref().is_some_and(|(last, last_auto)| *last == *status && *last_auto == auto) {
        return;
    }
    let current = current_version();
    let ready = matches!(*status, UpdateStatus::Ready(_));
    let wanted = match &*status {
        // A downloaded update is still offered after switching updates off.
        _ if !auto && !ready => format!("Off: staying on v{current}."),
        UpdateStatus::Idle | UpdateStatus::Checking => "Checking for updates...".to_string(),
        UpdateStatus::Off => format!("Off: staying on v{current}."),
        UpdateStatus::Unavailable(reason) => format!("v{current}, not updated automatically: {reason}."),
        UpdateStatus::UpToDate => format!("Up to date (v{current})."),
        UpdateStatus::NoReleases => format!("v{current}: no releases found on GitHub. Trying again at the next start."),
        UpdateStatus::Downloading(version) => format!("Downloading v{version}..."),
        UpdateStatus::Ready(version) => format!("v{version} is downloaded. Restart to use it."),
        UpdateStatus::Failed(err) => format!("Update failed: {err}. Trying again at the next start."),
    };
    *shown = Some((status.clone(), auto));
    drop(status);
    for mut text in &mut labels {
        if text.0 != wanted {
            text.0.clone_from(&wanted);
        }
    }
    let display = if ready { Display::Flex } else { Display::None };
    for mut node in &mut buttons {
        if node.display != display {
            node.display = display;
        }
    }
}
