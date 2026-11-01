//! The in-game HUD: title, timer, the drill's live stats (and an adaptive run's live θ75), the
//! "click to start" card and the countdown.

use std::borrow::Cow;

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::feathers::constants::fonts;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::text::{FontFeatureTag, FontFeatures};

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

/// Minimum widths of the HUD bar's changing segments: room for "600.0s", "Kills 123   Lives 10 /
/// 10" and "1234 fps   Esc to quit".
const TIME_W: f32 = 100.0;
const DETAIL_W: f32 = 230.0;
const INFO_W: f32 = 140.0;

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
                    // Each changing segment gets room for its usual widest value, so the bar
                    // keeps its size as numbers grow (9 → 10 hits, 99 → 100 fps).
                    Node { min_width: px(TIME_W), justify_content: JustifyContent::Center }
                    Children [ @styled("", fonts::BOLD, 26.0, TEXT_STRONG) TextFont { font_features: {tabular()} } HudField::Time ]
                    --
                    @vertical_divider()
                    --
                    Node { min_width: px(DETAIL_W), justify_content: JustifyContent::Center }
                    Children [
                        @styled("", fonts::REGULAR, 16.0, TEXT_STRONG)
                        TextFont { font_features: {tabular()} }
                        HudField::Detail
                    ]
                    --
                    @vertical_divider()
                    --
                    Node { min_width: px(INFO_W), justify_content: JustifyContent::Center }
                    Children [
                        @styled("", fonts::REGULAR, 12.0, tokens::TEXT_DIM)
                        TextFont { font_features: {tabular()} }
                        HudField::Info
                    ]
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
            Children [ @styled("", fonts::BOLD, 96.0, TEXT_STRONG) TextFont { font_features: {tabular()} } HudField::Countdown ]
        })
        .insert(DespawnOnExit(AppState::Playing));
}

/// Equal-width digits for text that changes every frame: Fira Sans' default digits differ in
/// width, so a changing number (the FPS, the timer) resizes its line, and the centred HUD bar
/// shifts with it.
fn tabular() -> FontFeatures {
    [FontFeatureTag::TABULAR_FIGURES].into()
}

pub(super) fn update_hud(
    run: Res<Run>,
    adaptive: Res<AdaptiveRun>,
    settings: Res<Settings>,
    diagnostics: Res<DiagnosticsStore>,
    mut fields: Query<(&HudField, &mut Text)>,
    mut ready_card: Single<&mut Node, With<ReadyCard>>,
) {
    let adaptive_mode = run.drill.config.adaptive();
    let fps = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|fps| fps.smoothed()).unwrap_or(0.0);
    for (field, mut text) in &mut fields {
        let wanted: Cow<str> = match field {
            HudField::Title | HudField::ReadyTitle => Cow::Borrowed(&run.title),
            // Endless drills (Survival) count up: the time survived.
            HudField::Time if run.kind().def().endless => Cow::Owned(format!("{:.1}s", run.elapsed())),
            HudField::Time => Cow::Owned(format!("{:.1}s", (run.duration - run.elapsed()).max(0.0))),
            HudField::Detail if !run.counting() => {
                Cow::Owned(format!("Warm-in {:.1}s (not scored)", run.warm_in_left()))
            },
            HudField::Detail => {
                let detail = run.kind().def().score.hud_detail(&run.live_stats(&settings), &run.drill.config);
                match adaptive.estimate().filter(|_| adaptive_mode.is_some()) {
                    Some(e) if e.trials > 0 => Cow::Owned(format!(
                        "{detail}   ·   θ75 {:.2}° ({:.2}–{:.2})   ·   {}/{} in time",
                        e.theta75, e.low, e.high, e.hits, e.trials
                    )),
                    _ => Cow::Owned(detail),
                }
            },
            HudField::Info => Cow::Owned(format!("{fps:.0} fps   Esc to quit")),
            // Only while the ready card shows it.
            HudField::ReadyBlurb if run.phase() != Phase::Ready => continue,
            HudField::ReadyBlurb => match adaptive_mode {
                Some(mode) => Cow::Owned(format!(
                    "Adaptive {}: {} Hit each target within {:.1} s.",
                    mode.name().to_lowercase(),
                    mode.blurb(),
                    run.drill.config.time_limit()
                )),
                None => run.drill.config.blurb(),
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
