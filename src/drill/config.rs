//! Tunable drill parameters. Built-in drills use the defaults (plus the global target size and
//! decay settings); custom scenarios store their own copy.

use serde::{Deserialize, Serialize};

use super::{DrillKind, def::DecayRule, params::ParamId};
use crate::settings::Settings;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrillConfig {
    pub kind: DrillKind,
    /// Target size multiplier.
    pub size: f32,
    /// Seconds before a target expires (counts as a miss); 0 = never.
    pub lifetime: f32,
    // Gridshot
    pub grid_cols: u32,
    pub grid_rows: u32,
    /// Angle between neighbouring grid cells, in degrees.
    pub grid_spacing: f32,
    /// Targets visible at once.
    pub grid_active: u32,
    // Flick drills (Microflick, Head level, Peripheral)
    /// Targets appear within +- this many degrees left/right.
    pub yaw_range: f32,
    /// Targets appear within +- this many degrees up/down.
    pub pitch_range: f32,
    /// Minimum angle from the crosshair to a new target, in degrees.
    pub min_flick: f32,
    // Flick constraints (Microflick only; weak-spot training)
    /// Maximum angle from the crosshair to a new target, in degrees; 0 = no limit.
    pub max_flick: f32,
    /// On-screen direction new targets appear in, in degrees counter-clockwise from right
    /// (90 = up), +- `flick_spread`; `None` = any direction.
    pub flick_direction: Option<f32>,
    /// Half-width of the `flick_direction` sector, in degrees.
    pub flick_spread: f32,
    // Tracking
    pub strafe_min: f32,
    pub strafe_max: f32,
    /// Seconds between direction changes.
    pub turn_min: f32,
    pub turn_max: f32,
    /// Half-width of the strafe lane, in metres at 10 m.
    pub lane: f32,
    // Peeks
    pub peek_speed_min: f32,
    pub peek_speed_max: f32,
    /// Seconds a peek holds still before retreating.
    pub hold_min: f32,
    pub hold_max: f32,
    // Reaction
    pub delay_min: f32,
    pub delay_max: f32,
}

impl Default for DrillConfig {
    fn default() -> Self {
        Self::defaults(DrillKind::Gridshot)
    }
}

impl DrillConfig {
    /// The common defaults, adjusted by the kind's [`DrillDef::tune_defaults`](super::def::DrillDef).
    pub fn defaults(kind: DrillKind) -> Self {
        let mut config = Self {
            kind,
            size: 1.0,
            lifetime: 0.0,
            grid_cols: 3,
            grid_rows: 3,
            grid_spacing: 7.0,
            grid_active: 3,
            yaw_range: 30.0,
            pitch_range: 12.0,
            min_flick: 8.0,
            max_flick: 0.0,
            flick_direction: None,
            flick_spread: 22.5,
            // Walk-to-jog strafes (Valorant walk ~3 m/s, run 5.4) with readable direction changes.
            strafe_min: 1.5,
            strafe_max: 4.0,
            turn_min: 0.4,
            turn_max: 1.4,
            lane: 5.0,
            // Moderate swings that stay out long enough to react, aim and click.
            peek_speed_min: 2.5,
            peek_speed_max: 4.5,
            hold_min: 0.35,
            hold_max: 0.8,
            delay_min: 0.9,
            delay_max: 2.6,
        };
        (kind.def().tune_defaults)(&mut config);
        config
    }

    /// A built-in drill: defaults plus the global target size and (where it applies) decay.
    /// Use [`DrillSpec::builtin`] to also get the matching history name.
    pub fn builtin(kind: DrillKind, settings: &Settings) -> Self {
        let mut config = Self::defaults(kind);
        config.size = settings.target_scale;
        if kind.def().decay == DecayRule::Global {
            config.lifetime = settings.targets.lifetime;
        }
        config
    }

    pub fn supports_decay(&self) -> bool {
        self.kind.def().decay != DecayRule::None
    }

    /// Whether Microflick targets follow the direction/distance constraints instead of
    /// appearing anywhere in the yaw/pitch ranges.
    pub fn flick_constrained(&self) -> bool {
        self.kind == DrillKind::Microflick && (self.flick_direction.is_some() || self.max_flick > 0.0)
    }

    /// Distance range (degrees) of constrained flicks: `min_flick` up to `max_flick`, or up to
    /// the far side of the yaw/pitch area without a maximum.
    pub fn flick_distance_range(&self) -> (f32, f32) {
        let far = if self.max_flick > 0.0 { self.max_flick } else { 2.0 * self.yaw_range.max(self.pitch_range) };
        (self.min_flick, far.max(self.min_flick))
    }

    /// Clamps values into ranges the drills can actually run with: every parameter to its hard
    /// range in [`ParamId::spec`], then the rules between parameters. Non-finite values (NaN
    /// from a typed `nan` or a share code) fall back to the drill's defaults first, as `clamp`
    /// would otherwise pass NaN through or panic on a NaN bound.
    pub fn sanitize(&mut self) {
        let defaults = Self::defaults(self.kind);
        for param in ParamId::ALL {
            param.sanitize(self, &defaults);
        }
        // At least one free cell is needed so a kill can respawn somewhere new.
        let cells = self.grid_cols * self.grid_rows;
        self.grid_active = self.grid_active.min(cells.saturating_sub(1).max(1));
        self.min_flick = self.min_flick.min(self.yaw_range.max(self.pitch_range));
        if self.max_flick > 0.0 {
            self.max_flick = self.max_flick.max(self.min_flick);
        }
        self.flick_direction = self.flick_direction.filter(|a| a.is_finite()).map(|a| a.rem_euclid(360.0));
        // Every range stays ordered.
        self.strafe_max = self.strafe_max.max(self.strafe_min);
        self.turn_max = self.turn_max.max(self.turn_min);
        self.peek_speed_max = self.peek_speed_max.max(self.peek_speed_min);
        self.hold_max = self.hold_max.max(self.hold_min);
        self.delay_max = self.delay_max.max(self.delay_min);
    }
}

/// A playable drill: a display name (used for history and HUD) and its parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillSpec {
    pub name: String,
    pub config: DrillConfig,
}

impl DrillSpec {
    /// A built-in drill with the global target size and decay. Those change what a score
    /// means, so a non-default setting becomes part of the name (and thereby of the history the
    /// run is compared against), e.g. `Gridshot · decay 1.9s · size 0.75x`.
    pub fn builtin(kind: DrillKind, settings: &Settings) -> Self {
        let config = DrillConfig::builtin(kind, settings);
        let defaults = DrillConfig::defaults(kind);
        let mut name = kind.name().to_string();
        if config.lifetime != defaults.lifetime {
            name += &format!("{VARIANT_SEPARATOR}decay {:.1}s", config.lifetime);
        }
        if (config.size - defaults.size).abs() > 1e-3 {
            name += &format!("{VARIANT_SEPARATOR}size {}x", (config.size * 100.0).round() / 100.0);
        }
        Self { name, config }
    }
}

/// Joins a built-in drill's name and its non-default settings.
pub const VARIANT_SEPARATOR: &str = " · ";

/// Shortest and longest a drill (or playlist step) may last, in seconds.
pub const SECS_RANGE: (f32, f32) = (5.0, 600.0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_always_keeps_a_free_cell() {
        let mut config = DrillConfig { grid_cols: 2, grid_rows: 2, grid_active: 9, ..DrillConfig::defaults(DrillKind::Gridshot) };
        config.sanitize();
        assert_eq!(config.grid_active, 3);
        let mut single = DrillConfig { grid_cols: 1, grid_rows: 1, grid_active: 5, ..config };
        single.sanitize();
        assert_eq!(single.grid_active, 1);
    }

    #[test]
    fn ranges_stay_ordered() {
        let mut config = DrillConfig { strafe_min: 8.0, strafe_max: 2.0, hold_min: 1.0, hold_max: 0.1, ..DrillConfig::default() };
        config.sanitize();
        assert!(config.strafe_max >= config.strafe_min && config.hold_max >= config.hold_min);
    }

    #[test]
    fn non_finite_values_fall_back_to_defaults() {
        let nan = f32::NAN;
        let mut config = DrillConfig {
            kind: DrillKind::Tracking,
            size: f32::INFINITY,
            yaw_range: nan,
            pitch_range: nan,
            min_flick: nan,
            strafe_min: nan,
            strafe_max: nan,
            turn_min: nan,
            peek_speed_min: nan,
            hold_min: nan,
            delay_min: nan,
            delay_max: nan,
            ..DrillConfig::defaults(DrillKind::Tracking)
        };
        config.sanitize();
        let d = DrillConfig::defaults(DrillKind::Tracking);
        assert_eq!(config.strafe_min, d.strafe_min);
        assert_eq!(config.strafe_max, d.strafe_max);
        assert_eq!(config.yaw_range, d.yaw_range);
        assert_eq!(config.min_flick, d.min_flick);
        assert_eq!(config.size, d.size);
    }

    #[test]
    fn flick_constraints_are_sanitized() {
        let mut config = DrillConfig {
            kind: DrillKind::Microflick,
            min_flick: 20.0,
            max_flick: 10.0,
            flick_direction: Some(-90.0),
            flick_spread: f32::NAN,
            ..DrillConfig::defaults(DrillKind::Microflick)
        };
        config.sanitize();
        assert_eq!(config.max_flick, config.min_flick, "the range stays ordered");
        assert_eq!(config.flick_direction, Some(270.0));
        assert_eq!(config.flick_spread, 22.5);
        assert!(config.flick_constrained());

        let mut wild = DrillConfig { max_flick: f32::INFINITY, flick_direction: Some(f32::NAN), flick_spread: 900.0, ..config };
        wild.sanitize();
        assert_eq!(wild.max_flick, 0.0);
        assert_eq!(wild.flick_direction, None);
        assert_eq!(wild.flick_spread, 180.0);
        assert!(!wild.flick_constrained());
        // Other drills ignore the constraints.
        assert!(!DrillConfig { kind: DrillKind::Gridshot, max_flick: 20.0, ..DrillConfig::default() }.flick_constrained());
    }

    #[test]
    fn old_configs_without_flick_constraints_load_unconstrained() {
        let config: DrillConfig = ron::from_str("(kind: Microflick, yaw_range: 20.0, min_flick: 5.0)").unwrap();
        assert_eq!(config.flick_direction, None);
        assert_eq!(config.max_flick, 0.0);
        assert_eq!(config.flick_spread, 22.5);
        assert!(!config.flick_constrained());
    }

    #[test]
    fn builtin_uses_global_size_and_decay_only_where_it_applies() {
        let mut settings = Settings { target_scale: 0.8, ..Default::default() };
        settings.targets.lifetime = 1.5;
        assert_eq!(DrillConfig::builtin(DrillKind::Microflick, &settings).lifetime, 1.5);
        assert_eq!(DrillConfig::builtin(DrillKind::Tracking, &settings).lifetime, 0.0);
        // Peripheral always decays on its own schedule.
        assert_eq!(DrillConfig::builtin(DrillKind::Peripheral, &settings).lifetime, 1.1);
        assert_eq!(DrillConfig::builtin(DrillKind::Gridshot, &settings).size, 0.8);
    }

    #[test]
    fn builtin_variants_get_their_own_history_name() {
        let mut settings = Settings::default();
        assert_eq!(DrillSpec::builtin(DrillKind::Gridshot, &settings).name, "Gridshot");
        settings.targets.lifetime = 1.9;
        settings.target_scale = 0.75;
        assert_eq!(DrillSpec::builtin(DrillKind::Gridshot, &settings).name, "Gridshot · decay 1.9s · size 0.75x");
        // Tracking never decays, so only the size distinguishes it.
        assert_eq!(DrillSpec::builtin(DrillKind::Tracking, &settings).name, "Tracking · size 0.75x");
        // Peripheral's own decay is its normal mode, not a variant.
        settings.target_scale = 1.0;
        assert_eq!(DrillSpec::builtin(DrillKind::Peripheral, &settings).name, "Peripheral");
    }
}
