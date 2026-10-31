//! Sensitivity math and game presets.
//!
//! Sensitivity is modelled the same way Source/Valorant-style games do it:
//! every raw mouse count rotates the view by `sens * yaw` degrees.

use super::{AspectMode, Settings};

const CM_PER_INCH: f32 = 2.54;

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
        PRESETS.iter().find(|p| (p.yaw - self.yaw).abs() < 1e-6 && (p.hfov - self.hfov).abs() < 0.01)
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
}

#[cfg(test)]
mod tests {
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
}
