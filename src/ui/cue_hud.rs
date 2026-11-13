//! The in-game banner of drills that are answered cue by cue (Reaction test, Go / no-go): where
//! the run is, how the last cue went and what that means, under the HUD bar and well above the
//! crosshair. It shows [`CueBanner`], which only changes when a cue was answered, so the banner
//! is rebuilt a few times a minute and never while a cue is awaited.

use bevy::feathers::constants::fonts;
use bevy::feathers::tokens;
use bevy::prelude::*;

use crate::AppState;
use crate::drill::cue::{CueBanner, Tone};
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// The banner's root, replaced whenever the banner changes.
#[derive(Component, Clone, Copy, Default)]
pub(super) struct CueBannerRoot;

/// Minimum width of the banner, so it doesn't jump between a reaction time and a longer line.
const BANNER_W: f32 = 320.0;

/// Rebuilds the banner; run when [`CueBanner`] changed. An empty banner (a drill without cues,
/// or a run that hasn't started) shows nothing.
pub(super) fn show_cue_banner(
    mut commands: Commands,
    banner: Res<CueBanner>,
    shown: Query<Entity, With<CueBannerRoot>>,
) {
    for root in &shown {
        commands.entity(root).despawn();
    }
    if *banner == CueBanner::default() {
        return;
    }
    let tone = match banner.tone {
        Tone::Neutral => TEXT_STRONG,
        Tone::Good => TEXT_GOOD,
        Tone::Warn => TEXT_WARN,
        Tone::Bad => TEXT_BAD,
    };
    // An empty line keeps its height, so the banner doesn't change size from trial to trial.
    let line = |text: &str| if text.is_empty() { " ".to_string() } else { text.to_string() };
    let (caption, result, hint) = (banner.caption.clone(), line(&banner.result), line(banner.hint));
    commands
        .spawn_scene(bsn! {
            @hud_anchor(percent(10))
            CueBannerRoot
            Children [
                @hud_bar()
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(0),
                    min_width: px(BANNER_W),
                    padding: UiRect::axes(px(SPACE_XL), px(SPACE_S)),
                }
                Children [
                    @overline(caption)
                    --
                    @styled(result, fonts::BOLD, FONT_HUD_TIME, tone)
                    TextFont { font_features: {tabular()} }
                    --
                    @styled(hint, fonts::REGULAR, FONT_SMALL, tokens::TEXT_MAIN)
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Playing));
}
