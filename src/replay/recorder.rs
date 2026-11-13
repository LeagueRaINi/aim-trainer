//! Recording a live drill into a [`Replay`].

use bevy::prelude::*;

use super::format::{Replay, ReplayFrame, Shot};
use crate::drill::DrillKind;

/// Recording rate. Shots are always recorded on the exact frame they happen.
pub(super) const SAMPLE_INTERVAL: f32 = 1.0 / 240.0;

/// Collects frames while a drill is live. Owned by the drill systems.
#[derive(Resource, Default)]
pub struct Recorder {
    frames: Vec<ReplayFrame>,
    pending_shot: Shot,
}

impl Recorder {
    pub fn mark_shot(&mut self, hit: bool) {
        self.pending_shot = if hit { Shot::Hit } else { Shot::Miss };
    }

    /// Records a frame if enough time has passed since the last one, or a shot happened.
    pub fn sample(&mut self, t: f32, (yaw, pitch): (f32, f32), targets: impl Iterator<Item = (Vec3, f32)>) {
        let due = self.frames.last().is_none_or(|last| t - last.t >= SAMPLE_INTERVAL);
        if !due && self.pending_shot == Shot::None {
            return;
        }
        let shot = std::mem::take(&mut self.pending_shot);
        self.frames.push(ReplayFrame { t, yaw, pitch, shot, targets: targets.collect() });
    }

    /// The frames recorded so far: a finished run can be measured before it is saved.
    pub fn frames(&self) -> &[ReplayFrame] {
        &self.frames
    }

    pub fn finish(&mut self, kind: DrillKind, name: &str, sens: f32, duration: f32) -> Replay {
        Replay { kind, name: name.to_string(), sens, duration, frames: std::mem::take(&mut self.frames) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::format::tests::sample_replay;

    #[test]
    fn recorder_throttles_but_keeps_shot_frames() {
        let replay = sample_replay();
        let times: Vec<f32> = replay.frames.iter().map(|f| f.t).collect();
        assert_eq!(times, [0.0, 0.002, 0.01]);
        assert_eq!(replay.frames[1].shot, Shot::Hit);
        assert_eq!(replay.frames[2].shot, Shot::None);
    }
}
