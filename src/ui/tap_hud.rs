//! What the bot drills add to the in-game view: the spread bar and, where the player moves, the
//! movement keys before the start.
//!
//! The spread bar is a thin line a little below the crosshair, exactly as wide on screen as the
//! next shot may land off it (the rifle not settled yet, or the player still moving). No bar:
//! the shot goes where the crosshair points. The crosshair itself stays as it is.

use bevy::feathers::constants::fonts;
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::prelude::*;

use crate::AppState;
use crate::camera::PlayerCamera;
use crate::drill::movement::PlayerMove;
use crate::drill::rifle::TapRun;
use crate::drill::{Phase, Run, range};
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// The bar's distance below the crosshair's centre and its thickness, px: clear of the
/// crosshair's lines, near enough to be seen without looking away.
const BAR_BELOW: f32 = 30.0;
const BAR_HEIGHT: f32 = 3.0;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct SpreadBar;

/// The movement keys' hint, shown until the run is live.
#[derive(Component, Clone, Copy, Default)]
pub(super) struct MoveKeys;

pub(super) fn spawn_tap_hud(mut commands: Commands, run: Res<Run>) {
    if !range::played_on(run.kind()) {
        return;
    }
    commands
        .spawn_scene(bsn! {
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
            }
            Children [
                // Centred like the crosshair, then moved down; hidden until there is spread.
                Node {
                    top: px(BAR_BELOW),
                    width: px(0),
                    height: px(BAR_HEIGHT),
                    display: Display::None,
                    border_radius: BorderRadius::MAX,
                }
                ThemeBackgroundColor(TEXT_WARN)
                SpreadBar
            ]
        })
        .insert(DespawnOnExit(AppState::Playing));
    if range::player_moves(run.kind()) {
        commands
            .spawn_scene(bsn! {
                @hud_anchor(percent(62))
                Children [
                    @hud_bar()
                    MoveKeys
                    Children [
                        @key_hint("A", "strafe left")
                        --
                        @key_hint("D", "strafe right")
                        --
                        @vertical_divider()
                        --
                        @styled("Stop before you shoot", fonts::BOLD, FONT_BODY, TEXT_ACCENT)
                    ]
                ]
            })
            .insert(DespawnOnExit(AppState::Playing));
    }
}

/// Width (px) of a bar spanning `error` degrees to each side of the crosshair, in a view
/// `viewport_height` px high with a vertical field of view of `vfov` radians.
fn spread_width(error: f32, vfov: f32, viewport_height: f32) -> f32 {
    error.to_radians().tan() / (vfov * 0.5).tan() * viewport_height
}

/// Sizes the spread bar to the current spread and hides the movement keys once the run is live.
#[allow(clippy::type_complexity)]
pub(super) fn update_tap_hud(
    run: Res<Run>,
    taps: Res<TapRun>,
    player: Option<Res<PlayerMove>>,
    ui_scale: Res<UiScale>,
    camera: Single<(&Camera, &Projection), With<PlayerCamera>>,
    mut bar: Single<&mut Node, (With<SpreadBar>, Without<MoveKeys>)>,
    mut keys: Query<&mut Node, (With<MoveKeys>, Without<SpreadBar>)>,
) {
    let error = taps.error(run.elapsed(), player.as_deref());
    let (camera, projection) = *camera;
    let (display, width) = match (projection, camera.logical_viewport_size()) {
        (Projection::Perspective(perspective), Some(viewport)) if error > 0.0 => {
            // Whole pixels, so the layout isn't redone for changes nobody can see.
            let width = spread_width(error, perspective.fov, viewport.y / ui_scale.0).round().max(BAR_HEIGHT);
            (Display::Flex, px(width))
        },
        _ => (Display::None, px(0)),
    };
    if bar.display != display {
        bar.display = display;
    }
    if bar.width != width {
        bar.width = width;
    }
    let display = if run.phase() == Phase::Live { Display::None } else { Display::Flex };
    for mut keys in &mut keys {
        if keys.display != display {
            keys.display = display;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar is to scale: an error of half the field of view spans the whole height, and a
    /// small one its share of the screen's tangent.
    #[test]
    fn the_spread_bar_is_as_wide_as_the_spread() {
        let vfov = 70f32.to_radians();
        assert!((spread_width(35.0, vfov, 720.0) - 720.0).abs() < 0.01);
        // 1.5° to each side at 720p and 70° is 27 px.
        let width = spread_width(1.5, vfov, 720.0);
        assert!((width - 2.0 * 1.5f32.to_radians().tan() * 360.0 / 35f32.to_radians().tan()).abs() < 1e-3);
        assert!((26.0..28.0).contains(&width), "{width}");
        assert_eq!(spread_width(0.0, vfov, 720.0), 0.0);
    }
}
