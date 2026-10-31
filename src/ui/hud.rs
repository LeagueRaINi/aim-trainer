//! The in-game HUD: title, timer, the drill's live stats (and an adaptive run's live θ75), the
//! "click to start" card and the countdown.

use std::borrow::Cow;

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::feathers::constants::fonts;
use bevy::feathers::tokens;
use bevy::prelude::*;

use crate::AppState;
use crate::drill::adaptive::AdaptiveRun;
use crate::drill::{Phase, Run};
use crate::settings::Settings;
use crate::ui::theme::{TEXT_ACCENT, TEXT_STRONG};
use crate::ui::widgets::{hud_anchor, hud_bar, styled, vertical_divider};

/// Text slots of the in-game HUD, filled by `update_hud`.
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum HudField {
    #[default]
    Title,
    Time,
    Detail,
    Info,
    ReadyTitle,
    ReadyBlurb,
    Countdown,
}

/// The "click to start" card, shown only while waiting in the Ready phase.
#[derive(Component, Clone, Copy, Default)]
pub(super) struct ReadyCard;

pub(super) fn spawn_hud(mut commands: Commands) {
    commands
        .spawn_scene(bsn! {
            @hud_anchor(px(14))
            Children [
                @hud_bar()
                Children [
                    @styled("", fonts::BOLD, 15.0, tokens::TEXT_MAIN)
                    HudField::Title
                    --
                    @vertical_divider()
                    --
                    Node { min_width: px(72), justify_content: JustifyContent::Center }
                    Children [ @styled("", fonts::BOLD, 26.0, TEXT_STRONG) HudField::Time ]
                    --
                    @vertical_divider()
                    --
                    @styled("", fonts::REGULAR, 16.0, TEXT_STRONG)
                    HudField::Detail
                    --
                    @vertical_divider()
                    --
                    @styled("", fonts::REGULAR, 12.0, tokens::TEXT_DIM)
                    HudField::Info
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Playing));
    commands
        .spawn_scene(bsn! {
            @hud_anchor(percent(24))
            Children [
                @hud_bar()
                ReadyCard
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    padding: UiRect::axes(px(36), px(22)),
                    border_radius: BorderRadius::all(px(14)),
                }
                Children [
                    @styled("", fonts::BOLD, 30.0, TEXT_STRONG)
                    HudField::ReadyTitle
                    --
                    @styled("", fonts::REGULAR, 16.0, tokens::TEXT_MAIN)
                    HudField::ReadyBlurb
                    --
                    @styled("CLICK OR PRESS SPACE TO START", fonts::BOLD, 14.0, TEXT_ACCENT)
                    Node { margin: UiRect::top(px(10)) }
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Playing));
    commands
        .spawn_scene(bsn! {
            @hud_anchor(percent(28))
            Children [ @styled("", fonts::BOLD, 96.0, TEXT_STRONG) HudField::Countdown ]
        })
        .insert(DespawnOnExit(AppState::Playing));
}

pub(super) fn update_hud(
    run: Res<Run>,
    adaptive: Res<AdaptiveRun>,
    settings: Res<Settings>,
    diagnostics: Res<DiagnosticsStore>,
    mut fields: Query<(&HudField, &mut Text)>,
    mut ready_card: Single<&mut Node, With<ReadyCard>>,
) {
    let stats = run.stats(&settings);
    let adaptive_mode = run.drill.config.adaptive();
    let fps = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|fps| fps.smoothed()).unwrap_or(0.0);
    for (field, mut text) in &mut fields {
        let wanted: Cow<str> = match field {
            HudField::Title | HudField::ReadyTitle => Cow::Borrowed(&run.title),
            HudField::Time => Cow::Owned(format!("{:.1}s", (run.duration - run.elapsed()).max(0.0))),
            HudField::Detail if !run.counting() => {
                Cow::Owned(format!("Warm-in {:.1}s (not scored)", run.warm_in_left()))
            },
            HudField::Detail => {
                let detail = run.kind().def().score.hud_detail(&stats);
                match adaptive.estimate().filter(|_| adaptive_mode.is_some()) {
                    Some(e) if e.trials > 0 => Cow::Owned(format!(
                        "{detail}   ·   θ75 {:.2}° ({:.2}–{:.2})   ·   {}/{} in time",
                        e.theta75, e.low, e.high, e.hits, e.trials
                    )),
                    _ => Cow::Owned(detail),
                }
            },
            HudField::Info => Cow::Owned(format!("{fps:.0} fps   Esc to quit")),
            HudField::ReadyBlurb => match adaptive_mode {
                Some(mode) => Cow::Owned(format!(
                    "Adaptive {}: {} Hit each target within {:.1} s.",
                    mode.name().to_lowercase(),
                    mode.blurb(),
                    run.drill.config.time_limit()
                )),
                None => Cow::Borrowed(run.kind().blurb()),
            },
            HudField::Countdown => match run.phase() {
                Phase::Countdown => Cow::Owned(format!("{}", run.countdown().ceil() as u32)),
                Phase::Ready | Phase::Live => Cow::Borrowed(""),
            },
        };
        if text.0 != wanted {
            text.0 = wanted.into_owned();
        }
    }
    let display = if run.phase() == Phase::Ready { Display::Flex } else { Display::None };
    if ready_card.display != display {
        ready_card.display = display;
    }
}
