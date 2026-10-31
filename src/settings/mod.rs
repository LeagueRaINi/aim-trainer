//! Persistent user settings: the saved types, their defaults and loading. Sensitivity math
//! lives in [`sens`], the numeric fields' limits (shared by the UI and [`Settings::sanitize`])
//! in [`fields`].

pub mod fields;
mod sens;

use std::path::Path;

use bevy::prelude::*;
pub use fields::SettingField;
pub use sens::PRESETS;
use serde::{Deserialize, Serialize};

use crate::drill::DrillKind;
use crate::persist::{LoadReport, Persisted, load_ron};

const SETTINGS_PATH: &str = "aim_trainer_settings.ron";

impl Persisted for Settings {
    const PATH: &'static str = SETTINGS_PATH;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayMode {
    #[default]
    Windowed,
    Borderless,
    /// Borderless fullscreen at the desktop resolution that minimizes when it loses focus,
    /// like exclusive fullscreen.
    Fullscreen,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectMode {
    /// Render a 16:9 image with black bars on wider/narrower screens (Valorant).
    #[default]
    Lock16x9,
    /// Fill the screen, keeping vertical FOV fixed so wider screens see more (CS2).
    Native,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSettings {
    pub display: DisplayMode,
    pub aspect: AspectMode,
    pub vsync: bool,
    /// Frames per second limit; 0 means unlimited.
    pub fps_cap: u32,
    /// MSAA sample count: 1 (off), 2, 4 or 8.
    pub msaa: u32,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self { display: DisplayMode::Windowed, aspect: AspectMode::Lock16x9, vsync: false, fps_cap: 0, msaa: 4 }
    }
}

/// Valorant-style crosshair. Sizes are in physical screen pixels, like in Valorant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrosshairSettings {
    pub color: [u8; 3],
    pub center_dot: bool,
    pub dot_thickness: f32,
    pub inner_lines: bool,
    pub line_length: f32,
    pub line_thickness: f32,
    pub line_offset: f32,
    pub outlines: bool,
    pub outline_thickness: f32,
    pub outline_opacity: f32,
}

impl Default for CrosshairSettings {
    /// Valorant's default crosshair.
    fn default() -> Self {
        Self {
            color: [255, 255, 255],
            center_dot: false,
            dot_thickness: 2.0,
            inner_lines: true,
            line_length: 6.0,
            line_thickness: 2.0,
            line_offset: 3.0,
            outlines: true,
            outline_thickness: 1.0,
            outline_opacity: 0.5,
        }
    }
}

/// Valorant's crosshair color choices.
pub const CROSSHAIR_COLORS: [(&str, [u8; 3]); 8] = [
    ("White", [255, 255, 255]),
    ("Green", [0, 255, 0]),
    ("Yellow Green", [127, 255, 0]),
    ("Green Yellow", [223, 255, 0]),
    ("Yellow", [255, 255, 0]),
    ("Cyan", [0, 255, 255]),
    ("Pink", [255, 0, 255]),
    ("Red", [255, 0, 0]),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HitSound {
    Off,
    #[default]
    Tick,
    Pop,
    Ding,
}

impl HitSound {
    pub const ALL: [HitSound; 4] = [HitSound::Off, HitSound::Tick, HitSound::Pop, HitSound::Ding];

    pub fn name(self) -> &'static str {
        match self {
            HitSound::Off => "Off",
            HitSound::Tick => "Tick",
            HitSound::Pop => "Pop",
            HitSound::Ding => "Ding",
        }
    }
}

/// Target ("orb") appearance and the optional decay mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TargetSettings {
    pub color: [u8; 3],
    /// Seconds before a target expires and counts as a miss; 0 disables decay.
    /// Applies to built-in drills whose decay rule is `Global` (see `drill::def::DecayRule`).
    pub lifetime: f32,
}

impl Default for TargetSettings {
    fn default() -> Self {
        Self { color: TARGET_COLORS[0].1, lifetime: 0.0 }
    }
}

pub const TARGET_COLORS: [(&str, [u8; 3]); 6] = [
    ("Cyan", [26, 217, 255]),
    ("Yellow", [255, 220, 40]),
    ("Red", [255, 60, 70]),
    ("Green", [60, 255, 110]),
    ("Magenta", [255, 70, 230]),
    ("White", [245, 245, 245]),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinderMethod {
    /// Fits a model to measured scores (plus your feel ratings) across many sensitivities.
    #[default]
    Smart,
    /// Blind A/B bisection: pick which of two sensitivities felt better, range halves each round.
    Classic,
}

/// What the smart finder optimises the sensitivity for. Optimal sensitivity depends on the task
/// (flicking favours lower, tracking higher), so a single all-round value is a compromise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinderFocus {
    /// Flicking and tracking equally: one sensitivity for everything.
    #[default]
    Balanced,
    /// Flicks only (Gridshot and Microflick): e.g. for tactical shooters.
    Flicking,
    /// Tracking only: e.g. for tracking-heavy games.
    Tracking,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub hit_sound: HitSound,
    /// Linear volume, 0..=1.
    pub volume: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { hit_sound: HitSound::Tick, volume: 0.5 }
    }
}

/// Discord Rich Presence (the Application ID is built in, see `presence`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscordSettings {
    pub enabled: bool,
}

impl Default for DiscordSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Updates from the GitHub releases (see `update`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Check at startup and install newer releases.
    pub auto: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { auto: true }
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub sens: f32,
    pub yaw: f32,
    pub dpi: f32,
    /// Horizontal FOV in degrees, measured at 16:9 (Hor+, like Valorant/CS2).
    pub hfov: f32,
    pub run_secs: f32,
    pub target_scale: f32,
    pub finder_rounds: u32,
    pub finder_trial_secs: f32,
    pub finder_drill: DrillKind,
    pub finder_method: FinderMethod,
    /// Which kind of aiming the smart finder optimises for.
    pub finder_focus: FinderFocus,
    pub targets: TargetSettings,
    pub video: VideoSettings,
    pub crosshair: CrosshairSettings,
    pub audio: AudioSettings,
    pub discord: DiscordSettings,
    pub updates: UpdateSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sens: 0.4,
            yaw: PRESETS[0].yaw,
            dpi: 800.0,
            hfov: PRESETS[0].hfov,
            run_secs: 60.0,
            target_scale: 1.0,
            finder_rounds: 7,
            finder_trial_secs: 20.0,
            finder_drill: DrillKind::Microflick,
            finder_method: FinderMethod::Smart,
            finder_focus: FinderFocus::Balanced,
            targets: TargetSettings::default(),
            video: VideoSettings::default(),
            crosshair: CrosshairSettings::default(),
            audio: AudioSettings::default(),
            discord: DiscordSettings::default(),
            updates: UpdateSettings::default(),
        }
    }
}

impl Settings {
    /// Reads the settings file. Must run before the app exists (the window is created from the
    /// settings); the report's problems are logged once the app is running.
    pub(crate) fn load() -> (Self, LoadReport) {
        let (mut settings, report): (Self, _) = load_ron(Path::new(SETTINGS_PATH));
        settings.sanitize();
        (settings, report)
    }

    /// Clamps values (e.g. from a hand-edited file) to the same hard limits the settings UI uses
    /// (see [`SettingField`]). Non-finite values fall back to the defaults.
    pub fn sanitize(&mut self) {
        let defaults = Self::default();
        for field in SettingField::ALL {
            field.sanitize(self, &defaults);
        }
        if ![1, 2, 4, 8].contains(&self.video.msaa) {
            self.video.msaa = defaults.video.msaa;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_util::TempDir;

    #[test]
    fn sanitize_clamps_to_ui_limits() {
        let mut settings = Settings { hfov: 179.0, sens: f32::NAN, dpi: 1.0, finder_rounds: 0, ..Settings::default() };
        settings.video.msaa = 3;
        settings.crosshair.outline_opacity = -1.0;
        settings.sanitize();
        assert_eq!(settings.hfov, 150.0);
        assert_eq!(settings.sens, Settings::default().sens);
        assert_eq!(settings.dpi, 50.0);
        assert_eq!(settings.finder_rounds, 1);
        assert_eq!(settings.video.msaa, 4);
        assert_eq!(settings.crosshair.outline_opacity, 0.0);
    }

    /// The settings `fixtures/settings.ron` was saved from (by v1.0.0). Settings added later
    /// stay at their defaults here.
    fn fixture_settings() -> Settings {
        let mut s = Settings {
            sens: 1.25,
            yaw: 0.022,
            dpi: 1600.0,
            hfov: 106.26,
            run_secs: 45.0,
            target_scale: 0.75,
            finder_rounds: 9,
            finder_trial_secs: 30.0,
            finder_drill: DrillKind::HeadLevel,
            finder_method: FinderMethod::Classic,
            finder_focus: FinderFocus::Tracking,
            ..Default::default()
        };
        s.targets.color = [255, 60, 70];
        s.targets.lifetime = 1.5;
        s.video.display = DisplayMode::Borderless;
        s.video.aspect = AspectMode::Native;
        s.video.vsync = true;
        s.video.fps_cap = 240;
        s.video.msaa = 2;
        s.crosshair.color = [0, 255, 0];
        s.crosshair.center_dot = true;
        s.crosshair.dot_thickness = 3.0;
        s.crosshair.inner_lines = false;
        s.crosshair.line_length = 8.0;
        s.crosshair.line_thickness = 1.0;
        s.crosshair.line_offset = 4.0;
        s.crosshair.outlines = false;
        s.crosshair.outline_thickness = 2.0;
        s.crosshair.outline_opacity = 0.75;
        s.audio.hit_sound = HitSound::Ding;
        s.audio.volume = 0.25;
        s.discord.enabled = false;
        s.updates.auto = false;
        s
    }

    /// Settings files saved by v1.0.0 keep loading: field and enum variant names, with settings
    /// added since (absent from the file) at their defaults. The fixture is frozen; never
    /// regenerate it.
    #[test]
    fn loads_v1_settings_fixture() {
        let fixture = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/settings.ron"));
        let dir = TempDir::new("settings_fixture");
        let path = dir.join("settings.ron");
        fs::write(&path, fixture).unwrap();
        let (mut loaded, report): (Settings, _) = load_ron(&path);
        assert!(report.problem.is_none());
        loaded.sanitize();
        assert_eq!(loaded, fixture_settings());
    }

    /// What the settings file is saved as loads back unchanged.
    #[test]
    fn saved_settings_round_trip() {
        let written = ron::ser::to_string_pretty(&fixture_settings(), ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(ron::from_str::<Settings>(&written).unwrap(), fixture_settings());
    }

    /// A file missing fields at every level (and keeping one since removed) loads with the
    /// defaults filled in, so each settings struct needs its `#[serde(default)]`.
    #[test]
    fn partial_settings_files_load_with_defaults() {
        let text = "(sens: 0.5, removed: (map: 0), targets: (lifetime: 1.0), video: (vsync: true), \
                    crosshair: (center_dot: true), audio: (volume: 0.3), discord: (), updates: ())";
        let mut expected = Settings { sens: 0.5, ..Settings::default() };
        expected.targets.lifetime = 1.0;
        expected.video.vsync = true;
        expected.crosshair.center_dot = true;
        expected.audio.volume = 0.3;
        assert_eq!(ron::from_str::<Settings>(text).unwrap(), expected);
    }
}
