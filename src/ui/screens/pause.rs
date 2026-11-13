//! The pause menu over a paused run: resume, restart or leave (`drill::pause`).

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::score::screen_title;
use crate::PlayState;
use crate::drill::pause::PauseMenu;
use crate::drill::{Phase, Run, RunPurpose};
use crate::ui::theme::*;
use crate::ui::widgets::*;

pub fn spawn_pause_menu(mut commands: Commands, run: Res<Run>) {
    let title = run.title.clone();
    const LIVE: &str = "Resume continues where you stopped, after a short countdown. Restarting or leaving doesn't record this attempt.";
    let (progress, resumes) = match (run.phase(), run.time_left()) {
        (Phase::Live, Some(left)) => (format!("{left:.1} s left"), LIVE),
        (Phase::Live, None) => (format!("{:.1} s played", run.elapsed()), LIVE),
        (Phase::Ready | Phase::Countdown, _) => ("Not started yet".to_string(), "Nothing is recorded if you leave."),
    };
    let leave = match run.purpose {
        RunPurpose::Practice => "Quit to menu",
        RunPurpose::FinderTrial => "Back to finder",
        RunPurpose::Routine => "Back to routine",
    };
    commands
        .spawn_scene(bsn! {
            @overlay()
            // Above the HUD, whichever was spawned first.
            GlobalZIndex(1)
            Children [
                @panel()
                Node { width: px(400), min_width: px(0), row_gap: px(SPACE_S) }
                Children [
                    @screen_title("Paused", title)
                    --
                    @column()
                    Node { row_gap: px(2), margin: UiRect::bottom(px(SPACE_S)) }
                    Children [ @label_strong(progress) -- @small(resumes) ]
                    --
                    @pause_action("Resume", "Esc", Btn::Primary)
                    on(resume)
                    --
                    @pause_action("Restart", "R", Btn::Secondary)
                    on(restart)
                    --
                    @pause_action(leave, "Q", Btn::Secondary)
                    on(quit)
                ]
            ]
        })
        .insert(DespawnOnExit(PlayState::Paused));
}

/// A full-width button of the pause menu with the key that does the same.
fn pause_action(text: &'static str, key: &'static str, kind: Btn) -> impl Scene {
    bsn! {
        @button_box(kind, 44.0)
        Node { justify_content: JustifyContent::SpaceBetween }
        Children [ @caption(text) -- @keycap(key) ]
    }
}

fn resume(_: On<Activate>, mut menu: PauseMenu) {
    menu.resume();
}

fn restart(_: On<Activate>, mut menu: PauseMenu) {
    menu.restart();
}

fn quit(_: On<Activate>, mut menu: PauseMenu) {
    menu.quit();
}
