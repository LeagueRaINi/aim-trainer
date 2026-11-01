//! The numeric settings and their limits: one table used by the settings UI (labels, steps,
//! soft/hard ranges) and by [`Settings::sanitize`], so the two can't disagree.

use super::Settings;
use crate::drill::SECS_RANGE;

/// A numeric setting edited through a number input.
#[derive(bevy::prelude::Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingField {
    #[default]
    Sens,
    Yaw,
    Dpi,
    Hfov,
    RunSecs,
    TargetScale,
    FinderRounds,
    FinderTrialSecs,
    FpsCap,
    Volume,
    DotThickness,
    LineLength,
    LineThickness,
    LineOffset,
    OutlineThickness,
    OutlineOpacity,
    ColorR,
    ColorG,
    ColorB,
    TargetLifetime,
    UiScale,
}

/// How a field is shown and limited. Ranges are in displayed units.
pub struct FieldSpec {
    pub label: &'static str,
    /// Decimals shown; 0 means the field only holds whole numbers.
    pub precision: i32,
    pub step: f64,
    /// Range of the input's drag/arrow keys.
    pub soft: (f32, f32),
    /// Values outside this range are clamped (when typed and when loaded).
    pub hard: (f32, f32),
    /// Displayed value per stored unit (the volume is shown in percent, stored as 0..1).
    pub scale: f32,
}

impl FieldSpec {
    /// `value` clamped to the hard range.
    pub fn clamp(&self, value: f32) -> f32 {
        value.clamp(self.hard.0, self.hard.1)
    }
}

impl SettingField {
    pub const ALL: [SettingField; 21] = [
        SettingField::Sens,
        SettingField::Yaw,
        SettingField::Dpi,
        SettingField::Hfov,
        SettingField::RunSecs,
        SettingField::TargetScale,
        SettingField::FinderRounds,
        SettingField::FinderTrialSecs,
        SettingField::FpsCap,
        SettingField::Volume,
        SettingField::DotThickness,
        SettingField::LineLength,
        SettingField::LineThickness,
        SettingField::LineOffset,
        SettingField::OutlineThickness,
        SettingField::OutlineOpacity,
        SettingField::ColorR,
        SettingField::ColorG,
        SettingField::ColorB,
        SettingField::TargetLifetime,
        SettingField::UiScale,
    ];

    pub fn spec(self) -> FieldSpec {
        let spec = |label, precision, step, soft, hard| FieldSpec { label, precision, step, soft, hard, scale: 1.0 };
        let px = |label| spec(label, 0, 1.0, (0.0, 20.0), (0.0, 20.0));
        let channel = |label| spec(label, 0, 1.0, (0.0, 255.0), (0.0, 255.0));
        match self {
            SettingField::Sens => spec("Sensitivity", 3, 0.01, (0.05, 4.0), (0.001, 100.0)),
            SettingField::Yaw => spec("Yaw (deg / count)", 4, 0.001, (0.001, 0.1), (0.0001, 1.0)),
            SettingField::Dpi => spec("Mouse DPI", 0, 50.0, (100.0, 3200.0), (50.0, 32000.0)),
            SettingField::Hfov => spec("Horizontal FOV (16:9)", 2, 1.0, (70.0, 130.0), (40.0, 150.0)),
            SettingField::RunSecs => spec("Drill length (s)", 0, 5.0, (10.0, 180.0), SECS_RANGE),
            SettingField::TargetScale => spec("Target size (x)", 2, 0.05, (0.3, 2.0), (0.1, 4.0)),
            SettingField::FinderRounds => spec("Rounds", 0, 1.0, (1.0, 12.0), (1.0, 12.0)),
            SettingField::FinderTrialSecs => spec("Trial length (s)", 0, 5.0, (5.0, 60.0), (5.0, 120.0)),
            SettingField::FpsCap => spec("FPS cap (0 = unlimited)", 0, 10.0, (0.0, 500.0), (0.0, 1000.0)),
            SettingField::Volume => {
                FieldSpec { scale: 100.0, ..spec("Volume (%)", 0, 5.0, (0.0, 100.0), (0.0, 100.0)) }
            },
            SettingField::DotThickness => spec("Dot thickness", 0, 1.0, (1.0, 10.0), (1.0, 10.0)),
            SettingField::LineLength => px("Line length"),
            SettingField::LineThickness => spec("Line thickness", 0, 1.0, (1.0, 10.0), (1.0, 10.0)),
            SettingField::LineOffset => px("Line offset"),
            SettingField::OutlineThickness => spec("Outline thickness", 0, 1.0, (1.0, 6.0), (1.0, 6.0)),
            SettingField::OutlineOpacity => spec("Outline opacity", 2, 0.05, (0.0, 1.0), (0.0, 1.0)),
            SettingField::ColorR => channel("R"),
            SettingField::ColorG => channel("G"),
            SettingField::ColorB => channel("B"),
            SettingField::TargetLifetime => spec("Target decay (s)", 2, 0.1, (0.0, 3.0), (0.0, 10.0)),
            SettingField::UiScale => {
                FieldSpec { scale: 100.0, ..spec("UI size (%)", 0, 5.0, (50.0, 100.0), (50.0, 100.0)) }
            },
        }
    }

    /// The stored value (integers widened to `f32`; the volume as 0..1).
    fn stored(self, settings: &Settings) -> f32 {
        let crosshair = &settings.crosshair;
        match self {
            SettingField::Sens => settings.sens,
            SettingField::Yaw => settings.yaw,
            SettingField::Dpi => settings.dpi,
            SettingField::Hfov => settings.hfov,
            SettingField::RunSecs => settings.run_secs,
            SettingField::TargetScale => settings.target_scale,
            SettingField::FinderRounds => settings.finder_rounds as f32,
            SettingField::FinderTrialSecs => settings.finder_trial_secs,
            SettingField::FpsCap => settings.video.fps_cap as f32,
            SettingField::Volume => settings.audio.volume,
            SettingField::DotThickness => crosshair.dot_thickness,
            SettingField::LineLength => crosshair.line_length,
            SettingField::LineThickness => crosshair.line_thickness,
            SettingField::LineOffset => crosshair.line_offset,
            SettingField::OutlineThickness => crosshair.outline_thickness,
            SettingField::OutlineOpacity => crosshair.outline_opacity,
            SettingField::ColorR => crosshair.color[0] as f32,
            SettingField::ColorG => crosshair.color[1] as f32,
            SettingField::ColorB => crosshair.color[2] as f32,
            SettingField::TargetLifetime => settings.targets.lifetime,
            SettingField::UiScale => settings.video.ui_scale,
        }
    }

    /// Stores `value` as is (integer fields truncate).
    fn store(self, settings: &mut Settings, value: f32) {
        let crosshair = &mut settings.crosshair;
        match self {
            SettingField::Sens => settings.sens = value,
            SettingField::Yaw => settings.yaw = value,
            SettingField::Dpi => settings.dpi = value,
            SettingField::Hfov => settings.hfov = value,
            SettingField::RunSecs => settings.run_secs = value,
            SettingField::TargetScale => settings.target_scale = value,
            SettingField::FinderRounds => settings.finder_rounds = value as u32,
            SettingField::FinderTrialSecs => settings.finder_trial_secs = value,
            SettingField::FpsCap => settings.video.fps_cap = value as u32,
            SettingField::Volume => settings.audio.volume = value,
            SettingField::DotThickness => crosshair.dot_thickness = value,
            SettingField::LineLength => crosshair.line_length = value,
            SettingField::LineThickness => crosshair.line_thickness = value,
            SettingField::LineOffset => crosshair.line_offset = value,
            SettingField::OutlineThickness => crosshair.outline_thickness = value,
            SettingField::OutlineOpacity => crosshair.outline_opacity = value,
            SettingField::ColorR => crosshair.color[0] = value as u8,
            SettingField::ColorG => crosshair.color[1] = value as u8,
            SettingField::ColorB => crosshair.color[2] = value as u8,
            SettingField::TargetLifetime => settings.targets.lifetime = value,
            SettingField::UiScale => settings.video.ui_scale = value,
        }
    }

    /// The value as displayed. Scaled fields show whole units.
    pub fn get(self, settings: &Settings) -> f32 {
        let spec = self.spec();
        let value = self.stored(settings);
        if spec.scale == 1.0 { value } else { (value * spec.scale).round() }
    }

    /// Sets the displayed value, clamped to the hard range; integer fields round.
    pub fn set(self, settings: &mut Settings, value: f32) {
        let spec = self.spec();
        let value = spec.clamp(value);
        // Integer fields (precision 0) only ever hold whole numbers.
        let value = if spec.precision == 0 { value.round() } else { value };
        self.store(settings, value / spec.scale);
    }

    /// Clamps a loaded value to the hard range without rounding; non-finite values become the
    /// default's.
    pub(super) fn sanitize(self, settings: &mut Settings, defaults: &Settings) {
        let spec = self.spec();
        let value = self.stored(settings);
        let value = if value.is_finite() {
            value.clamp(spec.hard.0 / spec.scale, spec.hard.1 / spec.scale)
        } else {
            self.stored(defaults)
        };
        self.store(settings, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_fields_round_and_clamp() {
        let mut settings = Settings::default();
        SettingField::ColorR.set(&mut settings, 300.4);
        assert_eq!(settings.crosshair.color[0], 255);
        SettingField::LineLength.set(&mut settings, 4.6);
        assert_eq!(settings.crosshair.line_length, 5.0);
        SettingField::Volume.set(&mut settings, 40.0);
        assert!((settings.audio.volume - 0.4).abs() < 1e-6);
        assert_eq!(SettingField::Volume.get(&settings), 40.0);
    }

    #[test]
    fn sanitize_uses_field_limits() {
        let defaults = Settings::default();
        for field in SettingField::ALL {
            let spec = field.spec();
            let mut settings = Settings::default();
            field.store(&mut settings, (spec.hard.1 + 1.0) / spec.scale);
            settings.sanitize();
            assert_eq!(field.get(&settings), spec.hard.1, "{field:?} above");

            let mut settings = Settings::default();
            field.store(&mut settings, (spec.hard.0 - 1.0) / spec.scale);
            settings.sanitize();
            assert_eq!(field.get(&settings), spec.hard.0, "{field:?} below");

            // Integer fields can't hold NaN in the first place.
            let mut settings = Settings::default();
            field.store(&mut settings, f32::NAN);
            if field.stored(&settings).is_nan() {
                settings.sanitize();
                assert_eq!(field.get(&settings), field.get(&defaults), "{field:?} NaN");
            }
        }
    }

    #[test]
    fn sanitize_keeps_in_range_values_exactly() {
        // Loading never rounds: an in-range fractional value survives as is.
        let mut settings = Settings { dpi: 812.5, ..Settings::default() };
        settings.audio.volume = 0.253;
        settings.sanitize();
        assert_eq!(settings.dpi, 812.5);
        assert_eq!(settings.audio.volume, 0.253);
    }
}
