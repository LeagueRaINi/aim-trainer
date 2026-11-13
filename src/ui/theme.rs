//! The UI theme: one palette, the spacing and type scales, and the color tokens every screen
//! uses. The Feathers dark theme supplies the token table for its controls (number inputs, text
//! inputs, scrollbars); its semantic colors are replaced with this palette so they match.

use bevy::feathers::dark_theme::create_dark_theme;
pub use bevy::feathers::theme::ThemeToken;
use bevy::feathers::theme::{SemanticToken, UiTheme};
use bevy::feathers::tokens::{self, semantic};
use bevy::prelude::*;

// Spacing scale (logical px). Everything in the UI is laid out on these steps.
pub const SPACE_XS: f32 = 4.0;
pub const SPACE_S: f32 = 8.0;
pub const SPACE_M: f32 = 12.0;
pub const SPACE_L: f32 = 16.0;
pub const SPACE_XL: f32 = 24.0;

// Corner radii: controls, cards, windows.
pub const RADIUS_S: f32 = 6.0;
pub const RADIUS_M: f32 = 10.0;
pub const RADIUS_L: f32 = 14.0;

// Type scale (logical px). Explanations are never smaller than `FONT_SMALL`; `FONT_MICRO` is for
// capitals (section titles, key caps) and chart labels only.
/// The headline number of a screen.
pub const FONT_DISPLAY: f32 = 56.0;
/// Page and screen titles.
pub const FONT_H1: f32 = 24.0;
/// Card headings, the selected item's name.
pub const FONT_H2: f32 = 17.0;
/// Stat values in tiles.
pub const FONT_STAT: f32 = 19.0;
pub const FONT_BODY: f32 = 14.0;
pub const FONT_SMALL: f32 = 12.5;
pub const FONT_MICRO: f32 = 11.0;
// In game, read at a glance over the 3D scene.
/// The countdown into a run.
pub const FONT_COUNTDOWN: f32 = 96.0;
/// The drill's name on its ready card.
pub const FONT_READY: f32 = 30.0;
/// The HUD's timer, and the replay HUD's (which also shows the length).
pub const FONT_HUD_TIME: f32 = 26.0;
pub const FONT_REPLAY_TIME: f32 = 22.0;
/// Live stats and the ready card's description.
pub const FONT_HUD: f32 = 16.0;

// Text.
/// Emphasized text: headings, big numbers, HUD values.
pub const TEXT_STRONG: ThemeToken = ThemeToken::new_static("aim.text.strong");
/// Accent-colored text (prompts, highlights, selected items).
pub const TEXT_ACCENT: ThemeToken = ThemeToken::new_static("aim.text.accent");
/// Improvement / hit.
pub const TEXT_GOOD: ThemeToken = ThemeToken::new_static("aim.text.good");
/// Regression / miss, and destructive actions.
pub const TEXT_BAD: ThemeToken = ThemeToken::new_static("aim.text.bad");
/// Something to look at that is neither good nor bad yet.
pub const TEXT_WARN: ThemeToken = ThemeToken::new_static("aim.text.warn");
/// Text on an accent-filled control.
pub const TEXT_ON_ACCENT: ThemeToken = ThemeToken::new_static("aim.text.on_accent");
/// Text of a disabled control.
pub const TEXT_DISABLED: ThemeToken = ThemeToken::new_static("aim.text.disabled");
/// Personal bests and best trials.
pub const GOLD: ThemeToken = ThemeToken::new_static("aim.gold");

// Surfaces, darkest to lightest.
/// The navigation rail and other recessed areas.
pub const SURFACE_SUNKEN: ThemeToken = ThemeToken::new_static("aim.surface.sunken");
/// Cards on a window.
pub const SURFACE_CARD: ThemeToken = ThemeToken::new_static("aim.surface.card");
/// Tiles and list rows on a card.
pub const SURFACE_TILE: ThemeToken = ThemeToken::new_static("aim.surface.tile");
pub const SURFACE_TILE_HOVER: ThemeToken = ThemeToken::new_static("aim.surface.tile.hover");
/// Outlines of windows, cards and neutral controls.
pub const BORDER: ThemeToken = ThemeToken::new_static("aim.border");
pub const BORDER_STRONG: ThemeToken = ThemeToken::new_static("aim.border.strong");
/// Thin separators and the HUD timeline track.
pub const DIVIDER: ThemeToken = ThemeToken::new_static("aim.divider");
/// No fill, for controls that only show one on hover.
pub const CLEAR: ThemeToken = ThemeToken::new_static("aim.clear");

// Control fills.
/// The one primary action of a screen.
pub const ACCENT: ThemeToken = ThemeToken::new_static("aim.accent");
pub const ACCENT_HOVER: ThemeToken = ThemeToken::new_static("aim.accent.hover");
pub const ACCENT_PRESSED: ThemeToken = ThemeToken::new_static("aim.accent.pressed");
/// Tinted fill and outline of the selected choice, tab or list item.
pub const ACCENT_SOFT: ThemeToken = ThemeToken::new_static("aim.accent.soft");
pub const ACCENT_SOFT_HOVER: ThemeToken = ThemeToken::new_static("aim.accent.soft.hover");
pub const ACCENT_LINE: ThemeToken = ThemeToken::new_static("aim.accent.line");
/// Neutral buttons.
pub const FILL: ThemeToken = ThemeToken::new_static("aim.fill");
pub const FILL_HOVER: ThemeToken = ThemeToken::new_static("aim.fill.hover");
pub const FILL_PRESSED: ThemeToken = ThemeToken::new_static("aim.fill.pressed");
pub const FILL_DISABLED: ThemeToken = ThemeToken::new_static("aim.fill.disabled");
/// Hover and pressed fills of buttons without a resting fill.
pub const GHOST_HOVER: ThemeToken = ThemeToken::new_static("aim.ghost.hover");
pub const GHOST_PRESSED: ThemeToken = ThemeToken::new_static("aim.ghost.pressed");
/// Hover fill of destructive buttons.
pub const BAD_SOFT: ThemeToken = ThemeToken::new_static("aim.bad.soft");

// Charts and the in-game HUD.
/// Bars and markers in charts.
pub const CHART_BAR: ThemeToken = ThemeToken::new_static("aim.chart.bar");
/// Faint grid lines of a plot.
pub const CHART_GRID: ThemeToken = ThemeToken::new_static("aim.chart.grid");
/// The filled band an estimate lies in.
pub const CHART_BAND: ThemeToken = ThemeToken::new_static("aim.chart.band");
/// A highlighted stretch of a chart's x axis (the finder's best range) and its edges.
pub const CHART_RANGE: ThemeToken = ThemeToken::new_static("aim.chart.range");
pub const CHART_RANGE_EDGE: ThemeToken = ThemeToken::new_static("aim.chart.range.edge");
/// Plot area behind charts.
pub const CHART_BG: ThemeToken = ThemeToken::new_static("aim.chart.bg");
/// Translucent backing behind in-game HUD text.
pub const HUD_BG: ThemeToken = ThemeToken::new_static("aim.hud.bg");
/// Backdrop of the crosshair preview: about as bright as a wall in a drill.
pub const PREVIEW_BG: ThemeToken = ThemeToken::new_static("aim.preview.bg");
/// Dimmed backdrop behind windows over the 3D scene.
pub const SCRIM: ThemeToken = ThemeToken::new_static("aim.scrim");
/// Insights direction wheel: a direction at your average, clearly weaker, clearly stronger, and
/// one without enough flicks.
pub const HEAT_NEUTRAL: ThemeToken = ThemeToken::new_static("aim.heat.neutral");
pub const HEAT_WEAK: ThemeToken = ThemeToken::new_static("aim.heat.weak");
pub const HEAT_STRONG: ThemeToken = ThemeToken::new_static("aim.heat.strong");
pub const HEAT_EMPTY: ThemeToken = ThemeToken::new_static("aim.heat.empty");

// Drill groups on the Play page.
pub const GROUP_AIM: ThemeToken = ThemeToken::new_static("aim.group.aim");
pub const GROUP_TACTICAL: ThemeToken = ThemeToken::new_static("aim.group.tactical");
pub const GROUP_EYES: ThemeToken = ThemeToken::new_static("aim.group.eyes");
pub const GROUP_TESTS: ThemeToken = ThemeToken::new_static("aim.group.tests");
pub const GROUP_ROUTINE: ThemeToken = ThemeToken::new_static("aim.group.routine");

const fn rgb(hex: u32) -> Color {
    Color::srgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(hex: u32, alpha: f32) -> Color {
    Color::srgba(
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        alpha,
    )
}

// The palette: blue-black surfaces, one cyan accent, and green / red / gold kept for meaning.
const INK_0: Color = rgb(0x0B0E13);
const INK_1: Color = rgb(0x11151C);
const INK_2: Color = rgb(0x181D26);
const INK_3: Color = rgb(0x202632);
const INK_4: Color = rgb(0x2A3140);
const INK_5: Color = rgb(0x353D4E);
const INK_6: Color = rgb(0x424B5E);
const WHITE: Color = rgb(0xF3F5F9);
const GRAY_TEXT: Color = rgb(0xC6CCD8);
const GRAY_DIM: Color = rgb(0x8E97AA);
const GRAY_OFF: Color = rgb(0x5C6474);
const CYAN: Color = rgb(0x4CC9F0);
const CYAN_LIGHT: Color = rgb(0x7BD8F5);
const CYAN_DEEP: Color = rgb(0x2FB1DB);
const CYAN_SOFT: Color = rgb(0x143848);
const CYAN_SOFT_LIGHT: Color = rgb(0x1C4557);
const CYAN_LINE: Color = rgb(0x3493B4);
const CYAN_DIM: Color = rgb(0x1F6A85);
const CYAN_INK: Color = rgb(0x04161D);
const GREEN: Color = rgb(0x5ADB8E);
const RED: Color = rgb(0xFF6B70);
const AMBER: Color = rgb(0xF5A45C);
const YELLOW: Color = rgb(0xF8C74A);
const CORAL: Color = rgb(0xFF8A65);
const VIOLET: Color = rgb(0xB79CFF);
const MINT: Color = rgb(0x62D9C3);

/// Shadow under windows that float over the 3D scene.
pub fn window_shadow() -> BoxShadow {
    BoxShadow::new(rgba(0x000000, 0.55), px(0), px(18), px(0), px(48))
}

pub fn ui_theme() -> UiTheme {
    let mut props = create_dark_theme();

    // Feathers' own controls: same roles, this palette.
    props.semantic_base.extend([
        (semantic::SURFACE_WINDOW, INK_1),
        (semantic::SURFACE_PANE_BODY, INK_2),
        (semantic::SURFACE_PANE_HEADER, INK_1),
        (semantic::SURFACE_SUBPANE_BODY, INK_2),
        (semantic::SURFACE_SUBPANE_HEADER, INK_3),
        (semantic::SURFACE_GROUP, INK_3),
        (semantic::SURFACE_DIALOG, INK_2),
        (semantic::SURFACE_MENU, INK_2),
        (semantic::BORDER_DEFAULT, INK_4),
        (semantic::TEXT_DEFAULT, GRAY_TEXT),
        (semantic::TEXT_DIM, GRAY_DIM),
        (semantic::TEXT_DISABLED, GRAY_OFF),
        (semantic::TEXT_ON_ACCENT, WHITE),
        // Number inputs print white digits over their bar, so it is the dimmed accent.
        (semantic::FILL_ACCENT_DEFAULT, CYAN_DIM),
        (semantic::FILL_ACCENT_HOVER, CYAN_LINE),
        (semantic::FILL_ACCENT_PRESSED, CYAN),
        (semantic::FILL_ACCENT_DISABLED, INK_4),
        (semantic::FILL_SOLID_DEFAULT, INK_4),
        (semantic::FILL_SOLID_HOVER, INK_5),
        (semantic::FILL_SOLID_PRESSED, INK_6),
        (semantic::FILL_SOLID_DISABLED, INK_3),
        (semantic::FILL_ITEM_HOVER, INK_4),
        (semantic::FILL_ITEM_PRESSED, INK_5),
        (semantic::FILL_FIELD_DEFAULT, INK_4),
        (semantic::FILL_FIELD_HOVER, INK_5),
        (semantic::FILL_FIELD_PRESSED, INK_5),
        (semantic::FOCUS_RING, CYAN_LIGHT),
    ]);
    // One look on every surface: the palette above is already tuned per surface.
    props.semantic_overrides.clear();
    let scroll_thumb = SemanticToken::new_static("aim.scroll.thumb");
    let scroll_thumb_hover = SemanticToken::new_static("aim.scroll.thumb.hover");
    props.semantic_base.extend([(scroll_thumb.clone(), INK_5), (scroll_thumb_hover.clone(), INK_6)]);
    props.token_assignments.extend([
        (tokens::SCROLLBAR_THUMB, scroll_thumb),
        (tokens::SCROLLBAR_THUMB_HOVER, scroll_thumb_hover),
        (tokens::SCROLLBAR_BG, semantic::FILL_NONE),
    ]);

    for (token, color) in [
        (TEXT_STRONG, WHITE),
        (TEXT_ACCENT, CYAN),
        (TEXT_GOOD, GREEN),
        (TEXT_BAD, RED),
        (TEXT_WARN, AMBER),
        (TEXT_ON_ACCENT, CYAN_INK),
        (TEXT_DISABLED, GRAY_OFF),
        (GOLD, YELLOW),
        (SURFACE_SUNKEN, INK_0),
        (SURFACE_CARD, INK_2),
        (SURFACE_TILE, INK_3),
        (SURFACE_TILE_HOVER, INK_4),
        (BORDER, INK_4),
        (BORDER_STRONG, INK_5),
        (DIVIDER, rgba(0xFFFFFF, 0.10)),
        (CLEAR, Color::NONE),
        (ACCENT, CYAN),
        (ACCENT_HOVER, CYAN_LIGHT),
        (ACCENT_PRESSED, CYAN_DEEP),
        (ACCENT_SOFT, CYAN_SOFT),
        (ACCENT_SOFT_HOVER, CYAN_SOFT_LIGHT),
        (ACCENT_LINE, CYAN_LINE),
        (FILL, INK_4),
        (FILL_HOVER, INK_5),
        (FILL_PRESSED, INK_6),
        (FILL_DISABLED, INK_3),
        (GHOST_HOVER, rgba(0xFFFFFF, 0.06)),
        (GHOST_PRESSED, rgba(0xFFFFFF, 0.11)),
        (BAD_SOFT, rgba(0xFF6B70, 0.16)),
        (CHART_BAR, rgba(0x4CC9F0, 0.70)),
        (CHART_GRID, rgba(0xFFFFFF, 0.06)),
        (CHART_BAND, rgba(0x4CC9F0, 0.20)),
        (CHART_RANGE, rgba(0x5ADB8E, 0.12)),
        (CHART_RANGE_EDGE, rgba(0x5ADB8E, 0.55)),
        (CHART_BG, rgba(0x000000, 0.28)),
        (HUD_BG, rgba(0x080A0E, 0.80)),
        (PREVIEW_BG, rgb(0x2C2F38)),
        (SCRIM, rgba(0x05070A, 0.62)),
        (HEAT_NEUTRAL, rgb(0x343B4A)),
        (HEAT_WEAK, rgb(0xC2444B)),
        (HEAT_STRONG, rgb(0x2E9A5C)),
        (HEAT_EMPTY, INK_3),
        (GROUP_AIM, CYAN),
        (GROUP_TACTICAL, CORAL),
        (GROUP_EYES, VIOLET),
        (GROUP_TESTS, MINT),
        (GROUP_ROUTINE, YELLOW),
    ] {
        let semantic = SemanticToken::new(token.to_string().into());
        props.semantic_base.insert(semantic.clone(), color);
        props.token_assignments.insert(token, semantic);
    }
    UiTheme(props)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A token without a color renders as Feathers' fuchsia error color.
    #[test]
    fn every_token_used_by_the_ui_has_a_color() {
        let theme = ui_theme();
        for token in [
            TEXT_STRONG,
            TEXT_ON_ACCENT,
            SURFACE_SUNKEN,
            ACCENT_SOFT,
            GHOST_HOVER,
            HEAT_WEAK,
            GROUP_ROUTINE,
            tokens::WINDOW_BG,
            tokens::TEXT_MAIN,
            tokens::TEXT_DIM,
            tokens::SCROLLBAR_THUMB,
        ] {
            let semantic = theme.0.token_assignments.get(&token).unwrap_or_else(|| panic!("{token} is unassigned"));
            assert!(theme.0.semantic_base.contains_key(semantic), "{token} has no color");
        }
    }
}
