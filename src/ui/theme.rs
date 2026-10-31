//! The UI theme: the Feathers dark theme plus a few game-specific color tokens, and the
//! spacing scale shared by every screen.

use bevy::{
    feathers::{
        dark_theme::create_dark_theme,
        theme::{SemanticToken, ThemeToken, UiTheme},
    },
    prelude::*,
};

// Spacing scale (logical px). Everything in the UI is laid out on these steps.
pub const SPACE_XS: f32 = 4.0;
pub const SPACE_S: f32 = 8.0;
pub const SPACE_M: f32 = 12.0;
pub const SPACE_L: f32 = 16.0;
pub const SPACE_XL: f32 = 24.0;

/// Emphasized text: headings, big numbers, HUD values.
pub const TEXT_STRONG: ThemeToken = ThemeToken::new_static("aim.text.strong");
/// Accent-colored text (prompts, highlights).
pub const TEXT_ACCENT: ThemeToken = ThemeToken::new_static("aim.text.accent");
/// Improvement / hit.
pub const TEXT_GOOD: ThemeToken = ThemeToken::new_static("aim.text.good");
/// Regression / miss.
pub const TEXT_BAD: ThemeToken = ThemeToken::new_static("aim.text.bad");
/// Personal bests and best trials.
pub const GOLD: ThemeToken = ThemeToken::new_static("aim.gold");
/// Bars and markers in charts.
pub const CHART_BAR: ThemeToken = ThemeToken::new_static("aim.chart.bar");
/// Plot area behind charts.
pub const CHART_BG: ThemeToken = ThemeToken::new_static("aim.chart.bg");
/// Translucent backing behind in-game HUD text.
pub const HUD_BG: ThemeToken = ThemeToken::new_static("aim.hud.bg");
/// Thin separators and the HUD timeline track.
pub const DIVIDER: ThemeToken = ThemeToken::new_static("aim.divider");

pub fn ui_theme() -> UiTheme {
    let mut props = create_dark_theme();
    for (token, color) in [
        (TEXT_STRONG, Color::srgb(0.95, 0.96, 0.98)),
        (TEXT_ACCENT, Color::srgb(0.42, 0.66, 1.0)),
        (TEXT_GOOD, Color::srgb(0.36, 0.86, 0.52)),
        (TEXT_BAD, Color::srgb(1.0, 0.45, 0.45)),
        (GOLD, Color::srgb(0.98, 0.76, 0.22)),
        (CHART_BAR, Color::srgba(0.3, 0.55, 1.0, 0.75)),
        (CHART_BG, Color::srgba(0.0, 0.0, 0.0, 0.25)),
        (HUD_BG, Color::srgba(0.05, 0.06, 0.08, 0.72)),
        (DIVIDER, Color::srgba(1.0, 1.0, 1.0, 0.12)),
    ] {
        let semantic = SemanticToken::new(token.to_string().into());
        props.semantic_base.insert(semantic.clone(), color);
        props.token_assignments.insert(token, semantic);
    }
    UiTheme(props)
}
