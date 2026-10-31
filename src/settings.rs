//! Persistent user settings and sensitivity math.
//!
//! Sensitivity is modelled the same way Source/Valorant-style games do it:
//! every raw mouse count rotates the view by `sens * yaw` degrees.

use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    persist::{LoadReport, Persisted, load_ron},
    scenario::Scenario,
};

const SETTINGS_PATH: &str = "aim_trainer_settings.ron";
const CM_PER_INCH: f32 = 2.54;

impl Persisted for Settings {
    const PATH: &'static str = SETTINGS_PATH;
}

/// A game whose yaw/FOV we can mimic so in-game sensitivity numbers transfer 1:1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GamePreset {
    pub name: &'static str,
    /// Degrees of rotation per mouse count at sensitivity 1.0.
    pub yaw: f32,
    /// Horizontal FOV in degrees at 16:9.
    pub hfov: f32,
    /// How the game handles screens that aren't 16:9.
    pub aspect: AspectMode,
}

pub const PRESETS: [GamePreset; 2] = [
    // Valorant: 103° at 16:9 (70.53° vertical); wider screens get black bars.
    GamePreset { name: "Valorant", yaw: 0.07, hfov: 103.0, aspect: AspectMode::Lock16x9 },
    // CS2: 90° at 4:3, which is 106.26° at 16:9; wider screens see more (Hor+).
    GamePreset { name: "CS2", yaw: 0.022, hfov: 106.26, aspect: AspectMode::Native },
];

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
    /// Applies to Gridshot, Microflick and Head level.
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

impl FinderFocus {
    /// The drills each trial plays.
    pub fn drills(self) -> &'static [Scenario] {
        match self {
            FinderFocus::Balanced => &[Scenario::Microflick, Scenario::Tracking],
            FinderFocus::Flicking => &[Scenario::Gridshot, Scenario::Microflick],
            FinderFocus::Tracking => &[Scenario::Tracking],
        }
    }
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
    pub finder_drill: Scenario,
    pub finder_method: FinderMethod,
    /// Which kind of aiming the smart finder optimises for.
    pub finder_focus: FinderFocus,
    pub targets: TargetSettings,
    pub video: VideoSettings,
    pub crosshair: CrosshairSettings,
    pub audio: AudioSettings,
    pub discord: DiscordSettings,
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
            finder_drill: Scenario::Microflick,
            finder_method: FinderMethod::Smart,
            finder_focus: FinderFocus::Balanced,
            targets: TargetSettings::default(),
            video: VideoSettings::default(),
            crosshair: CrosshairSettings::default(),
            audio: AudioSettings::default(),
            discord: DiscordSettings::default(),
        }
    }
}

impl Settings {
    /// Degrees of view rotation per raw mouse count.
    pub fn deg_per_count(&self, sens: f32) -> f32 {
        sens * self.yaw
    }

    /// Physical mouse travel for a full 360° turn.
    pub fn cm_per_360(&self, sens: f32) -> f32 {
        360.0 / (self.deg_per_count(sens) * self.dpi) * CM_PER_INCH
    }

    /// Converts `sens` (in the current yaw) to the equivalent sensitivity in `preset`.
    pub fn sens_in(&self, sens: f32, preset: &GamePreset) -> f32 {
        sens * self.yaw / preset.yaw
    }

    /// Vertical FOV in radians. Fixed regardless of window aspect, so wider windows see more (Hor+).
    pub fn vfov_rad(&self) -> f32 {
        let half_h = (self.hfov.to_radians() * 0.5).tan();
        2.0 * (half_h / (16.0 / 9.0)).atan()
    }

    /// The preset whose yaw and FOV match the current values, if any.
    pub fn matching_preset(&self) -> Option<&'static GamePreset> {
        PRESETS
            .iter()
            .find(|p| (p.yaw - self.yaw).abs() < 1e-6 && (p.hfov - self.hfov).abs() < 0.01)
    }

    /// Switches to a game's yaw/FOV/aspect handling, converting `sens` so cm/360 stays the same.
    pub fn apply_preset(&mut self, preset: &GamePreset) {
        self.sens = self.sens_in(self.sens, preset);
        self.yaw = preset.yaw;
        self.hfov = preset.hfov;
        self.video.aspect = preset.aspect;
    }

    /// One-line summary of the current sensitivity, used across screens.
    pub fn describe_sens(&self, sens: f32) -> String {
        let equivalents = PRESETS
            .iter()
            .map(|p| format!("{} {:.3}", p.name, self.sens_in(sens, p)))
            .collect::<Vec<_>>()
            .join("  |  ");
        format!(
            "{:.2} cm/360  ({:.2} in)  |  {}",
            self.cm_per_360(sens),
            self.cm_per_360(sens) / CM_PER_INCH,
            equivalents
        )
    }

    /// Reads the settings file. Must run before the app exists (the window is created from the
    /// settings); the report's problems are logged once the app is running.
    pub(crate) fn load() -> (Self, LoadReport) {
        let (mut settings, report): (Self, _) = load_ron(Path::new(SETTINGS_PATH));
        settings.sanitize();
        (settings, report)
    }

    /// Clamps values (e.g. from a hand-edited file) to the same hard limits the settings UI uses.
    /// Non-finite values fall back to the defaults.
    pub fn sanitize(&mut self) {
        let d = Self::default();
        let fit = |value: &mut f32, default: f32, min: f32, max: f32| {
            *value = if value.is_finite() { value.clamp(min, max) } else { default };
        };
        fit(&mut self.sens, d.sens, 0.001, 100.0);
        fit(&mut self.yaw, d.yaw, 0.0001, 1.0);
        fit(&mut self.dpi, d.dpi, 50.0, 32000.0);
        fit(&mut self.hfov, d.hfov, 40.0, 150.0);
        fit(&mut self.run_secs, d.run_secs, 5.0, 600.0);
        fit(&mut self.target_scale, d.target_scale, 0.1, 4.0);
        fit(&mut self.finder_trial_secs, d.finder_trial_secs, 5.0, 120.0);
        fit(&mut self.targets.lifetime, d.targets.lifetime, 0.0, 10.0);
        fit(&mut self.audio.volume, d.audio.volume, 0.0, 1.0);
        self.finder_rounds = self.finder_rounds.clamp(1, 12);
        self.video.fps_cap = self.video.fps_cap.min(1000);
        if ![1, 2, 4, 8].contains(&self.video.msaa) {
            self.video.msaa = d.video.msaa;
        }
        let (c, dc) = (&mut self.crosshair, &d.crosshair);
        fit(&mut c.dot_thickness, dc.dot_thickness, 1.0, 10.0);
        fit(&mut c.line_length, dc.line_length, 0.0, 20.0);
        fit(&mut c.line_thickness, dc.line_thickness, 1.0, 10.0);
        fit(&mut c.line_offset, dc.line_offset, 0.0, 20.0);
        fit(&mut c.outline_thickness, dc.outline_thickness, 1.0, 6.0);
        fit(&mut c.outline_opacity, dc.outline_opacity, 0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn cm_per_360_matches_known_valorant_value() {
        // 0.4 @ 800 DPI in Valorant is a well known ~40.82 cm/360.
        let settings = Settings { sens: 0.4, dpi: 800.0, ..Settings::default() };
        assert!((settings.cm_per_360(0.4) - 40.82).abs() < 0.01);
    }

    #[test]
    fn converts_between_games_at_equal_cm_per_360() {
        let valorant = Settings::default();
        let cs2_sens = valorant.sens_in(0.4, &PRESETS[1]);
        assert!((cs2_sens - 1.2727).abs() < 1e-3);

        let mut cs2 = valorant.clone();
        cs2.apply_preset(&PRESETS[1]);
        assert!((cs2.sens - cs2_sens).abs() < 1e-6);
        assert!((cs2.cm_per_360(cs2.sens) - valorant.cm_per_360(0.4)).abs() < 1e-3);
    }

    #[test]
    fn vertical_fov_of_cs2_preset_is_4_3_ninety() {
        // CS2's 106.26° @ 16:9 corresponds to 73.74° vertical.
        let mut settings = Settings::default();
        settings.apply_preset(&PRESETS[1]);
        assert!((settings.vfov_rad().to_degrees() - 73.74).abs() < 0.01);
    }

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

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aim_trainer_test_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The settings `fixtures/settings.ron` was saved from.
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
            finder_drill: Scenario::HeadLevel,
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
        s
    }

    /// Pins the settings file format: field names, enum variant names and the pretty writer.
    #[test]
    fn loads_saved_settings_fixture() {
        let fixture = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/settings.ron"));
        let dir = temp_dir("settings_fixture");
        let path = dir.join("settings.ron");
        fs::write(&path, fixture).unwrap();
        let (mut loaded, report): (Settings, _) = load_ron(&path);
        assert!(report.problem.is_none());
        loaded.sanitize();
        assert_eq!(loaded, fixture_settings());
        let written = ron::ser::to_string_pretty(&fixture_settings(), ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(written.replace("\r\n", "\n"), fixture.replace("\r\n", "\n"));

        // Files from builds with the arena still carry its key, and lack newer fields.
        let legacy = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/settings_legacy_arena.ron"));
        fs::write(&path, legacy).unwrap();
        let (loaded, report): (Settings, _) = load_ron(&path);
        assert!(report.problem.is_none() && report.can_save);
        assert_eq!(loaded, Settings { sens: 0.35, ..Settings::default() });
        let _ = fs::remove_dir_all(&dir);
    }
}
