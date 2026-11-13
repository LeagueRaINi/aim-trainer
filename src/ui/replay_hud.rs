//! The replay player's HUD: title, time, speed, shot counts and the timeline.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::feathers::constants::fonts;
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::prelude::*;

use crate::AppState;
use crate::replay::Playback;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Text slots of the replay HUD, filled by `update_replay_hud`.
#[derive(Component, Clone, Copy, Default)]
pub(super) enum ReplayField {
    #[default]
    Title,
    Time,
    Speed,
    Hits,
    Misses,
    Fps,
}

#[derive(Component, Clone, Copy, Default)]
pub(super) struct TimelineFill;

pub(super) fn spawn_replay_hud(mut commands: Commands) {
    commands
        .spawn_scene(bsn! {
            @hud_anchor(px(14))
            Children [
                @hud_bar()
                Children [
                    Node { padding: UiRect::axes(px(8), px(2)), border_radius: BorderRadius::all(px(4)) }
                    ThemeBackgroundColor(ACCENT)
                    Children [ @styled("REPLAY", fonts::BOLD, FONT_MICRO, TEXT_ON_ACCENT) ]
                    --
                    @styled("", fonts::BOLD, FONT_BODY, tokens::TEXT_MAIN)
                    ReplayField::Title
                    --
                    @vertical_divider()
                    --
                    @styled("", fonts::BOLD, FONT_REPLAY_TIME, TEXT_STRONG)
                    ReplayField::Time
                    --
                    @styled("", fonts::BOLD, FONT_BODY, TEXT_ACCENT)
                    ReplayField::Speed
                    --
                    @vertical_divider()
                    --
                    @styled("", fonts::REGULAR, FONT_HUD, TEXT_GOOD)
                    ReplayField::Hits
                    --
                    @styled("", fonts::REGULAR, FONT_HUD, TEXT_BAD)
                    ReplayField::Misses
                    --
                    @vertical_divider()
                    --
                    @styled("", fonts::REGULAR, FONT_SMALL, tokens::TEXT_DIM)
                    ReplayField::Fps
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Replay));
    commands
        .spawn_scene(bsn! {
            Node {
                position_type: PositionType::Absolute,
                bottom: px(18),
                left: percent(10),
                width: percent(80),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(10),
            }
            Children [
                @hud_bar()
                Node { padding: UiRect::axes(px(SPACE_L), px(6)), column_gap: px(SPACE_L) }
                Children [
                    @key_hint("Space", "play / pause")
                    --
                    @keys_hint("Left", "Right", "-/+ 5 s")
                    --
                    @keys_hint(",", ".", "frame step")
                    --
                    @keys_hint("Up", "Down", "speed")
                    --
                    @key_hint("T", "tracer")
                    --
                    @key_hint("R", "restart")
                    --
                    @key_hint("Esc", "back")
                ]
                --
                Node { width: percent(100), height: px(6), border_radius: BorderRadius::all(px(3)) }
                ThemeBackgroundColor(DIVIDER)
                Children [
                    Node { width: percent(0), height: percent(100), border_radius: BorderRadius::all(px(3)) }
                    ThemeBackgroundColor(ACCENT)
                    TimelineFill
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Replay));
}

/// Two keys that do opposite things, and what they change.
fn keys_hint(first: &'static str, second: &'static str, action: &'static str) -> impl Scene {
    bsn! {
        @row()
        Node { column_gap: px(SPACE_XS) }
        Children [
            @keycap(first)
            --
            @keycap(second)
            --
            @styled(action, fonts::REGULAR, FONT_SMALL, tokens::TEXT_MAIN)
            Node { margin: UiRect::left(px(2)) }
        ]
    }
}

pub(super) fn update_replay_hud(
    playback: Res<Playback>,
    diagnostics: Res<DiagnosticsStore>,
    mut fields: Query<(&ReplayField, &mut Text)>,
    mut fill: Single<&mut Node, With<TimelineFill>>,
) {
    let end = playback.end().max(f32::EPSILON);
    let (hits, misses) = playback.shots_until(playback.time);
    let fps = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|fps| fps.smoothed()).unwrap_or(0.0);
    for (field, mut text) in &mut fields {
        let wanted = match field {
            ReplayField::Title => format!("{}  ·  sens {:.3}", playback.replay.name, playback.replay.sens),
            ReplayField::Time => format!("{:.1} / {:.1} s", playback.time, end),
            ReplayField::Speed => {
                format!("{}x{}", playback.speed(), if playback.paused { "  PAUSED" } else { "" })
            },
            ReplayField::Hits => format!("{hits} hits"),
            ReplayField::Misses => format!("{misses} misses"),
            ReplayField::Fps => format!("{fps:.0} fps"),
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    fill.width = percent(playback.time / end * 100.0);
}
