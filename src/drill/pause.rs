//! Pausing a run. Escape, or the window losing focus, stops the drill where it is and brings up
//! the pause menu (`ui::screens::pause`); from there the run is resumed (a live one after a short
//! countdown), restarted or left. While paused or counting back in, none of the
//! [`DrillSet`](super::DrillSet) stages run and the view doesn't turn, so a resumed run goes on
//! exactly where it stopped.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::WindowFocused;

use super::run::COUNTDOWN_SECS;
use super::{Phase, Run, begin_run};
use crate::{AppState, PlayState};

/// Present while runs must go on when the window loses focus: the screenshot tour drives the game
/// in the background, so it must not pause there.
#[derive(Resource)]
pub struct KeepRunsOnFocusLoss;

/// Seconds until a resumed run goes on; counts down in [`PlayState::Resuming`].
#[derive(Resource, Default)]
pub struct ResumeCountdown(f32);

impl ResumeCountdown {
    pub fn left(&self) -> f32 {
        self.0
    }
}

/// What can be done with a paused run: for the pause menu's buttons and keys.
#[derive(SystemParam)]
pub struct PauseMenu<'w, 's> {
    commands: Commands<'w, 's>,
    run: Res<'w, Run>,
    countdown: ResMut<'w, ResumeCountdown>,
    next_play: ResMut<'w, NextState<PlayState>>,
    next_app: ResMut<'w, NextState<AppState>>,
}

impl PauseMenu<'_, '_> {
    /// Back into the run. A live one counts down first, so the player has the mouse in hand
    /// again when it goes on.
    pub fn resume(&mut self) {
        if self.run.phase() == Phase::Live {
            self.countdown.0 = COUNTDOWN_SECS;
            self.next_play.set(PlayState::Resuming);
        } else {
            self.next_play.set(PlayState::Running);
        }
    }

    /// The same run from its ready card; nothing of the paused attempt is recorded.
    pub fn restart(&mut self) {
        let run = self.run.restarted();
        begin_run(&mut self.commands, &mut self.next_app, run);
        // Re-entering `Playing` alone would leave it paused.
        self.next_play.set(PlayState::Running);
    }

    /// Leaves the run without recording it.
    pub fn quit(&mut self) {
        self.next_app.set(self.run.purpose.return_state());
    }
}

/// Escape or the window losing focus pauses; in the pause menu Escape resumes, R restarts and Q
/// leaves; a resumed live run goes on once its countdown is over.
pub(super) fn pause_control(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: MessageReader<WindowFocused>,
    keep_on_focus_loss: Option<Res<KeepRunsOnFocusLoss>>,
    state: Res<State<PlayState>>,
    mut menu: PauseMenu,
) {
    let lost_focus = focus.read().any(|event| !event.focused) && keep_on_focus_loss.is_none();
    let escape = keys.just_pressed(KeyCode::Escape);
    match state.get() {
        PlayState::Running | PlayState::Resuming if escape || lost_focus => menu.next_play.set(PlayState::Paused),
        PlayState::Running => {},
        PlayState::Resuming => {
            menu.countdown.0 -= time.delta_secs();
            if menu.countdown.0 <= 0.0 {
                menu.next_play.set(PlayState::Running);
            }
        },
        PlayState::Paused if escape => menu.resume(),
        PlayState::Paused if keys.just_pressed(KeyCode::KeyR) => menu.restart(),
        PlayState::Paused if keys.just_pressed(KeyCode::KeyQ) => menu.quit(),
        PlayState::Paused => {},
    }
}
