//! The tunable numeric drill parameters and their limits: one table used by the custom
//! scenario editor (labels, steps, soft/hard ranges) and by [`DrillConfig::sanitize`], so the
//! two can't disagree.

use super::DrillConfig;

/// A numeric [`DrillConfig`] field. Which ones a drill's editor shows is listed in its
/// [`DrillDef::params`](super::def::DrillDef::params).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamId {
    Size,
    Lifetime,
    GridCols,
    GridRows,
    GridSpacing,
    GridActive,
    YawRange,
    PitchRange,
    MinFlick,
    MaxFlick,
    FlickSpread,
    StrafeMin,
    StrafeMax,
    TurnMin,
    TurnMax,
    Lane,
    PeekSpeedMin,
    PeekSpeedMax,
    HoldMin,
    HoldMax,
    DelayMin,
    DelayMax,
    Lives,
    ApproachSpeed,
    SpawnInterval,
    TrackTime,
}

/// How a parameter is shown and limited.
pub struct ParamSpec {
    pub label: &'static str,
    /// Decimals shown in the editor.
    pub precision: i32,
    pub step: f64,
    /// Range of the editor's drag/arrow keys.
    pub soft: (f32, f32),
    /// Values outside this range are clamped (when edited, loaded or imported).
    pub hard: (f32, f32),
}

impl ParamId {
    pub const ALL: [ParamId; 26] = [
        ParamId::Size,
        ParamId::Lifetime,
        ParamId::GridCols,
        ParamId::GridRows,
        ParamId::GridSpacing,
        ParamId::GridActive,
        ParamId::YawRange,
        ParamId::PitchRange,
        ParamId::MinFlick,
        ParamId::MaxFlick,
        ParamId::FlickSpread,
        ParamId::StrafeMin,
        ParamId::StrafeMax,
        ParamId::TurnMin,
        ParamId::TurnMax,
        ParamId::Lane,
        ParamId::PeekSpeedMin,
        ParamId::PeekSpeedMax,
        ParamId::HoldMin,
        ParamId::HoldMax,
        ParamId::DelayMin,
        ParamId::DelayMax,
        ParamId::Lives,
        ParamId::ApproachSpeed,
        ParamId::SpawnInterval,
        ParamId::TrackTime,
    ];

    pub fn spec(self) -> ParamSpec {
        let spec = |label, precision, step, soft, hard| ParamSpec { label, precision, step, soft, hard };
        match self {
            ParamId::Size => spec("Target size (x)", 2, 0.05, (0.2, 3.0), (0.1, 5.0)),
            ParamId::Lifetime => spec("Decay (s, 0 = off)", 2, 0.1, (0.0, 3.0), (0.0, 10.0)),
            ParamId::GridCols => spec("Grid columns", 0, 1.0, (1.0, 9.0), (1.0, 9.0)),
            ParamId::GridRows => spec("Grid rows", 0, 1.0, (1.0, 9.0), (1.0, 9.0)),
            ParamId::GridSpacing => spec("Cell spacing (deg)", 1, 0.5, (2.0, 20.0), (1.0, 30.0)),
            // Further limited to the grid's cell count minus one.
            ParamId::GridActive => spec("Targets at once", 0, 1.0, (1.0, 8.0), (1.0, 80.0)),
            ParamId::YawRange => spec("Horizontal range (+- deg)", 0, 1.0, (0.0, 70.0), (0.0, 80.0)),
            ParamId::PitchRange => spec("Vertical range (+- deg)", 0, 1.0, (0.0, 30.0), (0.0, 40.0)),
            // Further limited to the larger of the two ranges.
            ParamId::MinFlick => spec("Min flick distance (deg)", 0, 1.0, (0.0, 40.0), (0.0, 80.0)),
            ParamId::MaxFlick => spec("Max flick distance (deg, 0 = none)", 0, 1.0, (0.0, 60.0), (0.0, 160.0)),
            ParamId::FlickSpread => spec("Direction spread (+- deg)", 0, 1.0, (5.0, 90.0), (1.0, 180.0)),
            ParamId::StrafeMin => spec("Strafe speed min (m/s)", 1, 0.5, (0.5, 12.0), (0.1, 20.0)),
            ParamId::StrafeMax => spec("Strafe speed max (m/s)", 1, 0.5, (0.5, 12.0), (0.1, 20.0)),
            ParamId::TurnMin => spec("Direction change min (s)", 2, 0.05, (0.05, 3.0), (0.05, 5.0)),
            ParamId::TurnMax => spec("Direction change max (s)", 2, 0.05, (0.05, 3.0), (0.05, 5.0)),
            ParamId::Lane => spec("Lane half-width (m)", 1, 0.5, (1.0, 12.0), (0.5, 15.0)),
            ParamId::PeekSpeedMin => spec("Peek speed min (m/s)", 1, 0.5, (1.0, 12.0), (0.5, 20.0)),
            ParamId::PeekSpeedMax => spec("Peek speed max (m/s)", 1, 0.5, (1.0, 12.0), (0.5, 20.0)),
            ParamId::HoldMin => spec("Hold min (s)", 2, 0.05, (0.0, 1.5), (0.0, 3.0)),
            ParamId::HoldMax => spec("Hold max (s)", 2, 0.05, (0.0, 1.5), (0.0, 3.0)),
            ParamId::DelayMin => spec("Cue delay min (s)", 2, 0.1, (0.2, 5.0), (0.2, 10.0)),
            ParamId::DelayMax => spec("Cue delay max (s)", 2, 0.1, (0.2, 5.0), (0.2, 10.0)),
            ParamId::Lives => spec("Lives", 0, 1.0, (1.0, 10.0), (1.0, 99.0)),
            ParamId::ApproachSpeed => spec("Approach speed (m/s)", 1, 0.5, (0.5, 8.0), (0.1, 20.0)),
            ParamId::SpawnInterval => spec("Time between targets (s)", 2, 0.1, (0.3, 3.0), (0.1, 10.0)),
            ParamId::TrackTime => spec("Time on target to kill (s)", 2, 0.05, (0.1, 2.0), (0.05, 10.0)),
        }
    }

    /// The stored value (integers widened to `f32`).
    pub fn get(self, config: &DrillConfig) -> f32 {
        match self {
            ParamId::Size => config.size,
            ParamId::Lifetime => config.lifetime,
            ParamId::GridCols => config.grid_cols as f32,
            ParamId::GridRows => config.grid_rows as f32,
            ParamId::GridSpacing => config.grid_spacing,
            ParamId::GridActive => config.grid_active as f32,
            ParamId::YawRange => config.yaw_range,
            ParamId::PitchRange => config.pitch_range,
            ParamId::MinFlick => config.min_flick,
            ParamId::MaxFlick => config.max_flick,
            ParamId::FlickSpread => config.flick_spread,
            ParamId::StrafeMin => config.strafe_min,
            ParamId::StrafeMax => config.strafe_max,
            ParamId::TurnMin => config.turn_min,
            ParamId::TurnMax => config.turn_max,
            ParamId::Lane => config.lane,
            ParamId::PeekSpeedMin => config.peek_speed_min,
            ParamId::PeekSpeedMax => config.peek_speed_max,
            ParamId::HoldMin => config.hold_min,
            ParamId::HoldMax => config.hold_max,
            ParamId::DelayMin => config.delay_min,
            ParamId::DelayMax => config.delay_max,
            ParamId::Lives => config.lives as f32,
            ParamId::ApproachSpeed => config.approach_speed,
            ParamId::SpawnInterval => config.spawn_interval,
            ParamId::TrackTime => config.track_time,
        }
    }

    /// Stores `value` clamped to the hard range; integer parameters round. Call
    /// [`DrillConfig::sanitize`] afterwards for the rules between parameters.
    pub fn set(self, config: &mut DrillConfig, value: f32) {
        let spec = self.spec();
        self.store(config, value.clamp(spec.hard.0, spec.hard.1));
    }

    /// Stores `value` as is; integer parameters round.
    fn store(self, config: &mut DrillConfig, value: f32) {
        match self {
            ParamId::Size => config.size = value,
            ParamId::Lifetime => config.lifetime = value,
            ParamId::GridCols => config.grid_cols = value.round() as u32,
            ParamId::GridRows => config.grid_rows = value.round() as u32,
            ParamId::GridSpacing => config.grid_spacing = value,
            ParamId::GridActive => config.grid_active = value.round() as u32,
            ParamId::YawRange => config.yaw_range = value,
            ParamId::PitchRange => config.pitch_range = value,
            ParamId::MinFlick => config.min_flick = value,
            ParamId::MaxFlick => config.max_flick = value,
            ParamId::FlickSpread => config.flick_spread = value,
            ParamId::StrafeMin => config.strafe_min = value,
            ParamId::StrafeMax => config.strafe_max = value,
            ParamId::TurnMin => config.turn_min = value,
            ParamId::TurnMax => config.turn_max = value,
            ParamId::Lane => config.lane = value,
            ParamId::PeekSpeedMin => config.peek_speed_min = value,
            ParamId::PeekSpeedMax => config.peek_speed_max = value,
            ParamId::HoldMin => config.hold_min = value,
            ParamId::HoldMax => config.hold_max = value,
            ParamId::DelayMin => config.delay_min = value,
            ParamId::DelayMax => config.delay_max = value,
            ParamId::Lives => config.lives = value.round() as u32,
            ParamId::ApproachSpeed => config.approach_speed = value,
            ParamId::SpawnInterval => config.spawn_interval = value,
            ParamId::TrackTime => config.track_time = value,
        }
    }

    /// Clamps a loaded value to the hard range (integers stay whole, floats aren't rounded);
    /// non-finite values become the default's.
    pub(super) fn sanitize(self, config: &mut DrillConfig, defaults: &DrillConfig) {
        let spec = self.spec();
        let value = self.get(config);
        let value = if value.is_finite() { value.clamp(spec.hard.0, spec.hard.1) } else { self.get(defaults) };
        self.store(config, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::DrillKind;

    #[test]
    fn editor_limits_match_sanitize() {
        for kind in DrillKind::ALL {
            for param in ParamId::ALL {
                let spec = param.spec();
                let mut config = DrillConfig::defaults(kind);
                param.set(&mut config, spec.hard.1 + 1.0);
                config.sanitize();
                assert!(param.get(&config) <= spec.hard.1, "{kind:?} {param:?}");
                let mut config = DrillConfig::defaults(kind);
                param.set(&mut config, spec.hard.0 - 1.0);
                config.sanitize();
                assert!(param.get(&config) >= spec.hard.0, "{kind:?} {param:?}");
            }
        }
    }

    #[test]
    fn loading_clamps_without_rounding() {
        let mut config = DrillConfig { grid_cols: 40, yaw_range: 30.4, ..DrillConfig::default() };
        config.sanitize();
        assert_eq!((config.grid_cols, config.yaw_range), (9, 30.4));
    }
}
