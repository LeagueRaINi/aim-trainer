//! Calm aim's guide through its four parts: a strip under the HUD bar that always says which
//! part is running and what to do in it, and a banner that introduces each part before it
//! starts. Both exist only in a Calm aim run and stay in the upper part of the screen.

use std::borrow::Cow;

use bevy::feathers::constants::fonts;
use bevy::feathers::tokens;
use bevy::prelude::*;

use crate::drill::kinds::calm_aim::{CalmRun, PLAN};
use crate::drill::{DrillKind, Run};
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::{AppState, PlayState};

/// Text slots of the strip and the banner, filled by [`update_calm_hud`].
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum CalmField {
    /// "Part 2 of 4".
    #[default]
    Step,
    Title,
    Task,
    /// Seconds left of the part.
    Left,
    /// Seconds until the part starts.
    StartsIn,
}

/// The strip under the HUD bar, shown while a part is running.
#[derive(Component, Clone, Copy, Default)]
pub(super) struct PartStrip;

/// The banner that introduces a part, shown until it starts.
#[derive(Component, Clone, Copy, Default)]
pub(super) struct PartBanner;

/// Room for "18 s left".
const LEFT_W: f32 = 64.0;

pub(super) fn spawn_calm_hud(mut commands: Commands, run: Res<Run>) {
    if run.kind() != DrillKind::CalmAim {
        return;
    }
    commands
        .spawn_scene(bsn! {
            // Right under the HUD bar.
            @hud_anchor(px(68))
            Children [
                @hud_bar()
                PartStrip
                Node { display: Display::None }
                Children [
                    @styled("", fonts::BOLD, FONT_MICRO, TEXT_ACCENT)
                    CalmField::Step
                    --
                    @styled("", fonts::BOLD, FONT_HUD, TEXT_STRONG)
                    CalmField::Title
                    --
                    @vertical_divider()
                    --
                    @styled("", fonts::REGULAR, FONT_HUD, tokens::TEXT_MAIN)
                    CalmField::Task
                    --
                    @vertical_divider()
                    --
                    Node { min_width: px(LEFT_W), justify_content: JustifyContent::Center }
                    Children [
                        @styled("", fonts::REGULAR, FONT_SMALL, tokens::TEXT_DIM)
                        TextFont { font_features: {tabular()} }
                        CalmField::Left
                    ]
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Playing));
    commands
        .spawn_scene(bsn! {
            // Under the strip and higher than the ready card was: clear of the crosshair and of
            // what the part puts around eye level (the dot's loop, the ring).
            @hud_anchor(percent(16))
            Children [
                @hud_bar()
                PartBanner
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    padding: UiRect::axes(px(36), px(20)),
                    border_radius: BorderRadius::all(px(RADIUS_L)),
                }
                Children [
                    @styled("", fonts::BOLD, FONT_MICRO, TEXT_ACCENT)
                    CalmField::Step
                    --
                    @styled("", fonts::BOLD, FONT_READY, TEXT_STRONG)
                    CalmField::Title
                    --
                    @styled("", fonts::REGULAR, FONT_HUD, tokens::TEXT_MAIN)
                    CalmField::Task
                    --
                    @styled("", fonts::BOLD, FONT_BODY, TEXT_ACCENT)
                    TextFont { font_features: {tabular()} }
                    Node { margin: UiRect::top(px(SPACE_S)) }
                    CalmField::StartsIn
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Playing));
}

/// Fills the strip and the banner from the part the run's clock is in. The banner is up until
/// its part starts, and never over the pause menu or the countdown back into the run; the strip
/// is there for as long as the test is live.
pub(super) fn update_calm_hud(
    run: Res<Run>,
    play: Res<State<PlayState>>,
    mut fields: Query<(&CalmField, &mut Text)>,
    mut strip: Single<&mut Node, (With<PartStrip>, Without<PartBanner>)>,
    mut banner: Single<&mut Node, (With<PartBanner>, Without<PartStrip>)>,
) {
    let view = CalmRun::view(run.elapsed()).filter(|_| run.live());
    let show = |shown: bool| if shown { Display::Flex } else { Display::None };
    let (strip_display, banner_display) = (
        show(view.is_some()),
        show(view.is_some_and(|view| view.starts_in.is_some()) && *play.get() == PlayState::Running),
    );
    if strip.display != strip_display {
        strip.display = strip_display;
    }
    if banner.display != banner_display {
        banner.display = banner_display;
    }
    let Some(view) = view else { return };
    for (field, mut text) in &mut fields {
        let wanted: Cow<str> = match field {
            CalmField::Step => Cow::Owned(format!("PART {} OF {}", view.number, PLAN.len())),
            CalmField::Title => Cow::Borrowed(view.part.title()),
            CalmField::Task => Cow::Borrowed(view.part.task()),
            CalmField::Left => Cow::Owned(format!("{:.0} s left", view.left.ceil())),
            CalmField::StartsIn => match view.starts_in {
                Some(secs) => Cow::Owned(format!("Starts in {}", secs.ceil() as u32)),
                None => Cow::Borrowed(""),
            },
        };
        if text.0 != wanted {
            text.0 = wanted.into_owned();
        }
    }
}
