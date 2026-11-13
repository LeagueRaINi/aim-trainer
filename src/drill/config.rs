//! Tunable drill parameters. Built-in drills use the defaults (plus the global target size and
//! decay settings); custom scenarios store their own copy.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use super::DrillKind;
use super::def::{Category, DecayRule};
use super::kinds::gridshot::MAX_GRID_HEIGHT;
use super::params::ParamId;
use crate::metrics::quest::QuestMode;
use crate::settings::Settings;

/// Time limit of an adaptive target when the config has no lifetime of its own, seconds.
pub const ADAPTIVE_TIME_LIMIT: f32 = 1.5;

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
    // Survival. Written only when not at their defaults, so files and share codes saved before
    // they existed (all at the defaults) stay byte-identical.
    /// Targets that may reach the player before the run ends.
    #[serde(skip_serializing_if = "is_default_lives")]
    pub lives: u32,
    /// Speed targets start approaching at, m/s; it rises as the run goes on.
    #[serde(skip_serializing_if = "is_default_approach_speed")]
    pub approach_speed: f32,
    /// Seconds between new targets at the start; it shrinks as the run goes on.
    #[serde(skip_serializing_if = "is_default_spawn_interval")]
    pub spawn_interval: f32,
    // Switch tracking. Written only when not at its default, like the Survival fields.
    /// Seconds the crosshair has to stay on a target to kill it.
    #[serde(skip_serializing_if = "is_default_track_time")]
    pub track_time: f32,
    // Smooth tracking. Written only when not at its default, like the Survival fields.
    /// Speed the target glides round its loop at, m/s at 10 m.
    #[serde(skip_serializing_if = "is_default_sweep_speed")]
    pub sweep_speed: f32,
    // Adaptive (drills whose `DrillDef::adaptive` is set)
    /// QUEST+ picks every target's size (see [`crate::metrics::quest`]) and a target counts as a
    /// hit only if killed within [`time_limit`](Self::time_limit); `None` = fixed size. Written
    /// only when set, so files and share codes without it stay byte-identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<QuestMode>,
}

const DEFAULT_LIVES: u32 = 3;
const DEFAULT_APPROACH_SPEED: f32 = 2.0;
const DEFAULT_SPAWN_INTERVAL: f32 = 1.2;
const DEFAULT_TRACK_TIME: f32 = 0.5;
const DEFAULT_SWEEP_SPEED: f32 = 2.5;

fn is_default_lives(lives: &u32) -> bool {
    *lives == DEFAULT_LIVES
}

fn is_default_approach_speed(speed: &f32) -> bool {
    *speed == DEFAULT_APPROACH_SPEED
}

fn is_default_spawn_interval(interval: &f32) -> bool {
    *interval == DEFAULT_SPAWN_INTERVAL
}

fn is_default_track_time(secs: &f32) -> bool {
    *secs == DEFAULT_TRACK_TIME
}

fn is_default_sweep_speed(speed: &f32) -> bool {
    *speed == DEFAULT_SWEEP_SPEED
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
            // Four seconds from appearing to arriving, a new one every 1.2 s: easy at first.
            lives: DEFAULT_LIVES,
            approach_speed: DEFAULT_APPROACH_SPEED,
            spawn_interval: DEFAULT_SPAWN_INTERVAL,
            // Half a second on target: long enough to need real tracking, short enough to keep
            // switching.
            track_time: DEFAULT_TRACK_TIME,
            // A jog seen from 20 m (14°/s): fast enough that standing still loses the dot at
            // once, slow enough to stay on it once the hand moves at its speed.
            sweep_speed: DEFAULT_SWEEP_SPEED,
            adaptive: None,
        };
        (kind.def().tune_defaults)(&mut config);
        config
    }

    /// A built-in drill: defaults plus the global target size (scaling the drill's own default
    /// size) and (where it applies) decay. Use [`DrillSpec::builtin`] to also get the matching
    /// history name.
    pub fn builtin(kind: DrillKind, settings: &Settings) -> Self {
        let mut config = Self::defaults(kind);
        // Tests have a fixed procedure: the global target size and decay don't apply to them.
        if kind.def().category == Category::Tests {
            return config;
        }
        config.size *= settings.target_scale;
        if kind.def().decay == DecayRule::Global {
            config.lifetime = settings.targets.lifetime;
        }
        config
    }

    /// One line describing this config (see [`DrillDef::blurb`](super::def::DrillDef)).
    pub fn blurb(&self) -> Cow<'static, str> {
        (self.kind.def().blurb)(self)
    }

    pub fn supports_decay(&self) -> bool {
        self.kind.def().decay != DecayRule::None
    }

    /// The adaptive mode, if this config has one and its kind supports it.
    pub fn adaptive(&self) -> Option<QuestMode> {
        self.adaptive.filter(|_| self.kind.def().adaptive)
    }

    /// Seconds an adaptive target has to be hit in: the lifetime, or [`ADAPTIVE_TIME_LIMIT`].
    pub fn time_limit(&self) -> f32 {
        if self.lifetime > 0.0 { self.lifetime } else { ADAPTIVE_TIME_LIMIT }
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
        // Every row between floor and ceiling: the spacing (in the editor's 0.5° steps) shrinks
        // to fit a tall grid.
        if self.grid_rows > 1 {
            let fits = (MAX_GRID_HEIGHT / (self.grid_rows - 1) as f32 * 2.0).floor() * 0.5;
            self.grid_spacing = self.grid_spacing.min(fits);
        }
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
        // Only drills that support it keep an adaptive mode.
        self.adaptive = self.adaptive();
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
        if (config.size / defaults.size - 1.0).abs() > 1e-3 {
            name += &format!("{VARIANT_SEPARATOR}size {}x", (settings.target_scale * 100.0).round() / 100.0);
        }
        Self { name, config }
    }

    /// A built-in drill in adaptive mode, with the global decay as its time limit (see
    /// [`DrillConfig::time_limit`]); the size is chosen per target. Named
    /// `Microflick · adaptive · measure`, with a non-default time limit before the mode
    /// (`Microflick · adaptive · limit 1.2s · train`), so each mode has its own history.
    pub fn adaptive(kind: DrillKind, mode: QuestMode, settings: &Settings) -> Self {
        let mut config = DrillConfig::builtin(kind, settings);
        config.size = 1.0;
        config.adaptive = Some(mode);
        let mut name = format!("{}{VARIANT_SEPARATOR}adaptive", kind.name());
        if config.time_limit() != ADAPTIVE_TIME_LIMIT {
            name += &format!("{VARIANT_SEPARATOR}limit {:.1}s", config.time_limit());
        }
        name += &format!("{VARIANT_SEPARATOR}{}", mode.name().to_lowercase());
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
        let mut config =
            DrillConfig { grid_cols: 2, grid_rows: 2, grid_active: 9, ..DrillConfig::defaults(DrillKind::Gridshot) };
        config.sanitize();
        assert_eq!(config.grid_active, 3);
        let mut single = DrillConfig { grid_cols: 1, grid_rows: 1, grid_active: 5, ..config };
        single.sanitize();
        assert_eq!(single.grid_active, 1);
    }

    #[test]
    fn ranges_stay_ordered() {
        let mut config =
            DrillConfig { strafe_min: 8.0, strafe_max: 2.0, hold_min: 1.0, hold_max: 0.1, ..DrillConfig::default() };
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

        let mut wild =
            DrillConfig { max_flick: f32::INFINITY, flick_direction: Some(f32::NAN), flick_spread: 900.0, ..config };
        wild.sanitize();
        assert_eq!(wild.max_flick, 0.0);
        assert_eq!(wild.flick_direction, None);
        assert_eq!(wild.flick_spread, 180.0);
        assert!(!wild.flick_constrained());
        // Other drills ignore the constraints.
        assert!(
            !DrillConfig { kind: DrillKind::Gridshot, max_flick: 20.0, ..DrillConfig::default() }.flick_constrained()
        );
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
        // The global size scales each drill's own default size.
        assert_eq!(DrillConfig::builtin(DrillKind::Microflick, &settings).size, 0.8);
        let gridshot = DrillConfig::defaults(DrillKind::Gridshot).size;
        assert!((DrillConfig::builtin(DrillKind::Gridshot, &settings).size - gridshot * 0.8).abs() < 1e-6);
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

    #[test]
    fn adaptive_is_optional_in_files() {
        // Old custom files (no `adaptive` field) load with fixed-size targets.
        let old: DrillConfig = ron::from_str("(kind: Microflick, lifetime: 1.2, min_flick: 5.0)").unwrap();
        assert_eq!(old.adaptive, None);
        // Unset, it isn't written at all, so existing files and share codes stay byte-identical.
        let fixed = DrillConfig::defaults(DrillKind::Microflick);
        assert!(!ron::ser::to_string(&fixed).unwrap().contains("adaptive"));
        let adaptive = DrillConfig { adaptive: Some(QuestMode::Train), ..fixed };
        let text = ron::ser::to_string(&adaptive).unwrap();
        assert!(text.contains("adaptive:Some(Train)"), "{text}");
        assert_eq!(ron::from_str::<DrillConfig>(&text).unwrap(), adaptive);
    }

    #[test]
    fn adaptive_only_where_supported() {
        let mut micro =
            DrillConfig { adaptive: Some(QuestMode::Measure), ..DrillConfig::defaults(DrillKind::Microflick) };
        micro.sanitize();
        assert_eq!(micro.adaptive(), Some(QuestMode::Measure));
        assert_eq!(micro.time_limit(), ADAPTIVE_TIME_LIMIT);
        assert_eq!(DrillConfig { lifetime: 1.1, ..micro }.time_limit(), 1.1);
        let mut tracking =
            DrillConfig { adaptive: Some(QuestMode::Train), ..DrillConfig::defaults(DrillKind::Tracking) };
        assert_eq!(tracking.adaptive(), None);
        tracking.sanitize();
        assert_eq!(tracking.adaptive, None);
    }

    #[test]
    fn adaptive_builtins_get_their_own_history_name() {
        let mut settings = Settings { target_scale: 0.75, ..Default::default() };
        let measure = DrillSpec::adaptive(DrillKind::Microflick, QuestMode::Measure, &settings);
        // The global size doesn't apply: each target's size is chosen.
        assert_eq!(measure.name, "Microflick · adaptive · measure");
        assert_eq!((measure.config.size, measure.config.adaptive()), (1.0, Some(QuestMode::Measure)));
        settings.targets.lifetime = 1.2;
        let train = DrillSpec::adaptive(DrillKind::Microflick, QuestMode::Train, &settings);
        assert_eq!(train.name, "Microflick · adaptive · limit 1.2s · train");
        assert_eq!(train.config.time_limit(), 1.2);
    }

    #[test]
    fn tests_ignore_the_global_size_and_decay() {
        let mut settings = Settings { target_scale: 0.75, ..Default::default() };
        settings.targets.lifetime = 1.9;
        let test = DrillSpec::builtin(DrillKind::ReactionTest, &settings);
        assert_eq!(test.name, "Reaction test");
        assert_eq!(test.config, DrillConfig::defaults(DrillKind::ReactionTest));
        // Drills keep both in their name.
        assert_eq!(DrillSpec::builtin(DrillKind::Gridshot, &settings).name, "Gridshot · decay 1.9s · size 0.75x");
    }
}
