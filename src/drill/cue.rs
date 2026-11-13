//! Cues the player answers with a click (Reaction test, Go / no-go): how the answer is timed,
//! where a cue appears, and the [`CueBanner`] that tells the player how the last one went.
//!
//! # Timing
//!
//! A reaction is timed between frames, so it can only be as fine as the frame time. Two things
//! are known about a frame and used here rather than left in the number:
//!
//! - a cue made visible during a frame is on screen when that frame is presented, which is when
//!   the next frame starts, not when the drill decided to show it;
//! - a click read in a frame arrived some time since the previous frame read its input: on
//!   average half a frame ago.
//!
//! So a reaction runs from the start of the frame after the one that drew the cue to the middle
//! of the frame interval the click arrived in: within half a frame of the truth either way,
//! whatever the frame rate. What stays in the number is what the game can't know: the display's
//! and the mouse's own delay (the `latency_offset_ms` setting).
//!
//! All of it is in run time ([`Run::elapsed`]), which stands still while the run is paused. Run
//! time is advanced at the end of a frame by that frame's delta, so during a frame's stages it
//! reads when the previous frame started; the frame itself started at `now + dt`.

use bevy::prelude::*;

use super::Run;
use super::geometry::{TARGET_DISTANCE, direction};
use crate::AppState;
use crate::metrics::flick_probe::angles;

/// A click this soon after a cue appeared (or before it) can't be a reaction to it: seeing it
/// alone takes longer. The cutoff of Woods et al. 2015, "Factors influencing the latency of
/// simple reaction time".
pub const ANTICIPATION_SECS: f32 = 0.110;

/// Height of the floor and the ceiling relative to the eye, metres (see `level`).
const FLOOR: f32 = -2.5;
const CEILING: f32 = 7.5;

/// One frame of a live run, in run time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// [`Run::elapsed`] during this frame's stages.
    pub now: f32,
    /// Seconds since the previous frame started.
    pub dt: f32,
}

impl Frame {
    pub fn of(run: &Run, time: &Time) -> Self {
        Self { now: run.elapsed(), dt: time.delta_secs() }
    }

    /// When this frame started, which is when the previous frame's image was handed to the
    /// display and when this frame's input had all arrived.
    pub fn started(self) -> f32 {
        self.now + self.dt
    }

    /// The middle of the interval this frame's input arrived in.
    pub fn input(self) -> f32 {
        self.now + self.dt * 0.5
    }
}

/// A cue that is visible, and since when.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cue {
    /// Made visible this frame: on screen once the frame is presented.
    Drawn,
    /// On screen since this run time.
    Shown(f32),
}

impl Cue {
    /// The run time the cue came on screen. Call it every frame after the one that drew the cue:
    /// the first call is what fixes the onset, at the start of that frame.
    pub fn onset(&mut self, frame: Frame) -> f32 {
        match *self {
            Cue::Shown(onset) => onset,
            Cue::Drawn => {
                let onset = frame.started();
                *self = Cue::Shown(onset);
                onset
            },
        }
    }
}

/// Where a cue of `radius` metres appears: straight ahead of the view (`forward`), so it is in
/// the middle of the screen wherever the player looks, but never sunk into the floor or the
/// ceiling.
pub fn ahead(forward: Vec3, radius: f32) -> Vec3 {
    let view = angles(forward);
    let limit = |height: f32| (height / TARGET_DISTANCE).clamp(-1.0, 1.0).asin();
    let pitch = view.y.to_radians().clamp(limit(FLOOR + radius), limit(CEILING - radius));
    direction(view.x.to_radians(), pitch) * TARGET_DISTANCE
}

/// How the last cue went, shown in the upper part of the screen until the next one is answered
/// (`ui::cue_hud`). Empty in drills without cues. Nothing in it changes when a cue is about to
/// appear, so it can't be used to anticipate one.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct CueBanner {
    /// Small capitals above the result: where the run is (trial 7 / 30) or what to do.
    pub caption: String,
    /// The result itself: a reaction time, or what went wrong.
    pub result: String,
    pub tone: Tone,
    /// One line under the result saying what it means for the player.
    pub hint: &'static str,
}

/// What a [`CueBanner`]'s result means, which decides its color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Neutral,
    Good,
    /// Something to look at: a cue that went unanswered.
    Warn,
    /// A click that should not have happened.
    Bad,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<CueBanner>()
        // A new run starts without the previous one's result.
        .add_systems(OnEnter(AppState::Playing), |mut banner: ResMut<CueBanner>| *banner = CueBanner::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A run at `fps` whose frames are numbered from 0: frame `index` in run time.
    fn frame(index: u32, fps: f32) -> Frame {
        Frame { now: index as f32 / fps, dt: 1.0 / fps }
    }

    /// A cue drawn in frame 10 and clicked 200 ms after it came on screen is timed as 200 ms at
    /// any frame rate, within half a frame: the frame that drew it and the wait for the click to
    /// be read are not part of the reaction.
    #[test]
    fn reactions_do_not_depend_on_the_frame_rate() {
        for fps in [60.0, 144.0, 500.0] {
            let dt = 1.0 / fps;
            let mut cue = Cue::Drawn;
            // Frame 10 drew the cue; it is on screen when frame 11 starts.
            let onset = cue.onset(frame(11, fps));
            assert!((onset - frame(11, fps).started()).abs() < 1e-6);
            assert_eq!(cue, Cue::Shown(onset));
            // The click arrives 200 ms later, and is read by the first frame that starts after it.
            let clicked_at = onset + 0.2;
            let reader = (0..).map(|index| frame(index, fps)).find(|f| f.started() >= clicked_at).unwrap();
            let reaction = reader.input() - cue.onset(reader);
            assert!((reaction - 0.2).abs() <= dt * 0.5 + 1e-5, "{fps} fps: {reaction}");
            // Timing from the frame that drew the cue to the frame that read the click, as the
            // drill sees them, would be a frame or two long instead.
            let naive = reader.now - frame(10, fps).now;
            assert!(naive - 0.2 >= dt - 1e-5, "{fps} fps: {naive}");
        }
    }

    #[test]
    fn cues_appear_ahead_but_clear_of_floor_and_ceiling() {
        let level = ahead(direction(0.7, 0.1), 1.5);
        assert!((level.length() - TARGET_DISTANCE).abs() < 1e-4);
        assert!(level.normalize().angle_between(direction(0.7, 0.1)) < 1e-4, "in the middle of the view");
        // Looking at the floor or the ceiling: the cue stays whole, at the same yaw.
        let low = ahead(direction(0.7, -1.2), 1.5);
        assert!((low.y - 1.5 - FLOOR).abs() < 1e-3, "{low}");
        assert!((angles(low).x - angles(level).x).abs() < 1e-3);
        let high = ahead(direction(0.7, 1.4), 1.5);
        assert!((high.y + 1.5 - CEILING).abs() < 1e-3, "{high}");
    }
}
