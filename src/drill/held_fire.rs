//! Held fire: in drills where a target is tracked rather than clicked ([`ScoreModel::held_fire`]),
//! the fire button is a gun that shoots at a fixed rate while it is held, and only what it does
//! counts. A crosshair that rests on a target without firing scores nothing and hurts nothing,
//! and a round fired off the target is a miss, so the accuracy says how much of the fire the
//! player chose to give was on target.
//!
//! [`pull`] works the gun once a frame, in `DrillSet::Input`; the drills read the frame's
//! [`HeldFire`] in `DrillSet::Score`, after their targets have moved.
//!
//! [`ScoreModel::held_fire`]: super::stats::ScoreModel::held_fire

use bevy::prelude::*;

use super::{DrillSet, Run};
use crate::AppState;
use crate::replay::Recorder;

/// Rounds per second while the fire button is held: about a Valorant rifle's (Vandal 9.75,
/// Phantom 11).
pub(crate) const ROUNDS_PER_SEC: f32 = 10.0;

/// The gun and what it did this frame.
#[derive(Resource, Default)]
pub(crate) struct HeldFire {
    /// Seconds until the next round may be fired; 0 or less means now.
    cooldown: f32,
    held: bool,
    rounds: u32,
}

impl HeldFire {
    /// A frame of `dt` seconds with the button `held` or not; returns the rounds fired in it.
    /// The first round of a press goes off at once; while the button stays down, what a frame
    /// is late by is carried over, so the rate is the same at any frame rate (a long frame
    /// fires all the rounds that were due in it). Tapping can't beat the rate.
    pub(crate) fn pull(&mut self, held: bool, dt: f32) -> u32 {
        self.held = held;
        self.rounds = 0;
        if held {
            while self.cooldown <= 0.0 {
                self.rounds += 1;
                self.cooldown += 1.0 / ROUNDS_PER_SEC;
            }
            self.cooldown -= dt;
        } else {
            self.cooldown = (self.cooldown - dt).max(0.0);
        }
        self.rounds
    }

    /// Whether the gun is firing this frame.
    pub(crate) fn held(&self) -> bool {
        self.held
    }

    /// The rounds that went off this frame.
    pub(crate) fn rounds(&self) -> u32 {
        self.rounds
    }

    /// Books this frame's fire at a single target the crosshair is `on_target` of or not: every
    /// round is a shot and, on target, a hit; time on target counts while firing.
    pub(crate) fn book(&self, run: &mut Run, recorder: &mut Recorder, on_target: bool, dt: f32) {
        for _ in 0..self.rounds {
            run.shot();
            if on_target {
                run.round_hit();
            }
        }
        if self.rounds > 0 {
            recorder.mark_shot(on_target);
        }
        if self.held && on_target {
            run.track_on_target(dt);
        }
    }
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<HeldFire>()
        .add_systems(OnEnter(AppState::Playing), |mut fire: ResMut<HeldFire>| *fire = HeldFire::default())
        .add_systems(Update, pull.in_set(DrillSet::Input));
}

/// Works the gun for this frame: it fires only in a live run of a held-fire drill.
pub(crate) fn pull(time: Res<Time>, mouse: Res<ButtonInput<MouseButton>>, run: Res<Run>, mut fire: ResMut<HeldFire>) {
    let held = run.live() && run.kind().def().score.held_fire() && mouse.pressed(MouseButton::Left);
    fire.pull(held, time.delta_secs());
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Frame rates the timing must not depend on: slower than the gun, around it, far above.
    pub(crate) const FRAME_RATES: [f32; 5] = [8.0, 30.0, 60.0, 144.0, 500.0];

    /// The rounds due in `frames` frames at `fps` with the button held from the first.
    pub(crate) fn rounds_due(frames: u32, fps: f32) -> u32 {
        ((frames - 1) as f32 / fps * ROUNDS_PER_SEC + 1e-3).floor() as u32 + 1
    }

    /// Presses or releases the fire button of a test app.
    pub(crate) fn hold(app: &mut App, held: bool) {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        if held {
            mouse.press(MouseButton::Left);
        } else {
            mouse.release(MouseButton::Left);
        }
    }

    /// Held for a while, the gun fires its rate whatever the frame rate, starting at once; and
    /// tapping faster than the rate fires no more than holding.
    #[test]
    fn the_gun_fires_at_its_rate_at_any_frame_rate() {
        for fps in FRAME_RATES {
            let dt = 1.0 / fps;
            let mut gun = HeldFire::default();
            assert_eq!(gun.pull(true, dt), 1, "{fps} fps: no round on the press");
            let mut fired = 1;
            // 3 s of frames: a round was due every 0.1 s up to the start of the last one, give
            // or take the one due right at a frame's start.
            let frames = (3.0 * fps).round() as u32;
            for _ in 1..frames {
                fired += gun.pull(true, dt);
            }
            let due = rounds_due(frames, fps);
            assert!(fired.abs_diff(due) <= 1, "{fps} fps: {fired} rounds in 3 s, {due} due");

            // A new press every other frame.
            let mut tapping = HeldFire::default();
            let tapped: u32 = (0..frames).map(|frame| tapping.pull(frame % 2 == 0, dt)).sum();
            assert!(tapped <= due + 1, "{fps} fps: tapping fired {tapped} rounds in 3 s, holding {due}");
        }
        // Released, the gun is ready again after one interval and not before.
        let mut gun = HeldFire::default();
        assert_eq!(gun.pull(true, 0.01), 1);
        assert_eq!(gun.pull(false, 0.05), 0);
        assert_eq!(gun.pull(true, 0.01), 0);
        assert_eq!(gun.pull(false, 0.05), 0);
        assert_eq!(gun.pull(true, 0.01), 1);
        assert!(gun.held() && gun.rounds() == 1);
    }
}
