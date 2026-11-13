//! The in-game HUD: title, timer, the drill's live stats (and an adaptive run's live θ75) and the
//! pause key in a bar at the top, the "click to start" card and the countdown. All of it stays
//! in the upper part of the screen, clear of the crosshair.

use std::borrow::Cow;

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::feathers::constants::fonts;
use bevy::feathers::tokens;
use bevy::prelude::*;

use crate::drill::adaptive::AdaptiveRun;
use crate::drill::pause::ResumeCountdown;
use crate::drill::{Phase, Run};
use crate::settings::Settings;
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::{AppState, PlayState};

/// Text slots of the in-game HUD, filled by `update_hud`.
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum HudField {
    #[default]
    Title,
    Time,
    Detail,
    Fps,
    ReadyTitle,
    ReadyBlurb,
    /// What the countdown counts down to.
    CountdownLabel,
    Countdown,
}

/// The "click to start" card, shown only while waiting in the Ready phase.
#[derive(Component, Clone, Copy, Default)]
pub(super) struct ReadyCard;

/// Minimum widths of the HUD bar's changing segments: room for "600.0s", "Kills 123   Lives 10 /
/// 10" and "1234 fps".
const TIME_W: f32 = 100.0;
const DETAIL_W: f32 = 230.0;
const FPS_W: f32 = 56.0;

pub(super) fn spawn_hud(mut commands: Commands) {
    commands
        .spawn_scene(bsn! {
            @hud_anchor(px(14))
            Children [
                @hud_bar()
                Children [
                    @styled("", fonts::BOLD, FONT_BODY, tokens::TEXT_MAIN)
                    HudField::Title
                    --
                    @vertical_divider()
                    --
                    // Each changing segment gets room for its usual widest value, so the bar
                    // keeps its size as numbers grow (9 → 10 hits, 99 → 100 fps).
                    Node { min_width: px(TIME_W), justify_content: JustifyContent::Center }
                    Children [ @styled("", fonts::BOLD, FONT_HUD_TIME, TEXT_STRONG) TextFont { font_features: {tabular()} } HudField::Time ]
                    --
                    @vertical_divider()
                    --
                    Node { min_width: px(DETAIL_W), justify_content: JustifyContent::Center }
                    Children [
                        @styled("", fonts::REGULAR, FONT_HUD, TEXT_STRONG)
                        TextFont { font_features: {tabular()} }
                        HudField::Detail
                    ]
                    --
                    @vertical_divider()
                    --
                    Node { min_width: px(FPS_W), justify_content: JustifyContent::Center }
                    Children [
                        @styled("", fonts::REGULAR, FONT_SMALL, tokens::TEXT_DIM)
                        TextFont { font_features: {tabular()} }
                        HudField::Fps
                    ]
                    --
                    @key_hint("Esc", "pause")
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Playing));
    commands
        .spawn_scene(bsn! {
            @hud_anchor(percent(22))
            Children [
                @hud_bar()
                ReadyCard
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    padding: UiRect::axes(px(36), px(20)),
                    border_radius: BorderRadius::all(px(RADIUS_L)),
                }
                Children [
                    @styled("", fonts::BOLD, FONT_READY, TEXT_STRONG)
                    HudField::ReadyTitle
                    --
                    @styled("", fonts::REGULAR, FONT_HUD, tokens::TEXT_MAIN)
                    HudField::ReadyBlurb
                    --
                    @row()
                    Node { margin: UiRect::top(px(10)), column_gap: px(SPACE_S) }
                    Children [
                        @keycap("Click")
                        --
                        @styled("or", fonts::REGULAR, FONT_SMALL, tokens::TEXT_DIM)
                        --
                        @keycap("Space")
                        --
                        @styled("to start", fonts::BOLD, FONT_BODY, TEXT_ACCENT)
                        --
                        @vertical_divider()
                        Node { margin: UiRect::horizontal(px(SPACE_S)) }
                        --
                        @key_hint("Esc", "pause or leave")
                    ]
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Playing));
    commands
        .spawn_scene(bsn! {
            @hud_anchor(percent(24))
            Children [
                @column()
                Node { align_items: AlignItems::Center, row_gap: px(0) }
                Children [
                    @styled("", fonts::BOLD, FONT_BODY, TEXT_ACCENT)
                    HudField::CountdownLabel
                    --
                    @styled("", fonts::BOLD, FONT_COUNTDOWN, TEXT_STRONG)
                    TextFont { font_features: {tabular()} }
                    HudField::Countdown
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Playing));
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update_hud(
    run: Res<Run>,
    play: Res<State<PlayState>>,
    resume: Res<ResumeCountdown>,
    adaptive: Res<AdaptiveRun>,
    settings: Res<Settings>,
    diagnostics: Res<DiagnosticsStore>,
    mut fields: Query<(&HudField, &mut Text)>,
    mut ready_card: Single<&mut Node, With<ReadyCard>>,
) {
    // The run's own countdown, or the one back into a resumed run.
    let countdown = match (play.get(), run.phase()) {
        (PlayState::Resuming, _) => Some(resume.left()),
        (PlayState::Running, Phase::Countdown) => Some(run.countdown()),
        _ => None,
    };
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
            // Nothing counts in a held-fire drill until the gun is fired: say so rather than
            // show a row of zeros.
            HudField::Detail if run.live() && run.fired() == 0 && run.kind().def().score.held_fire() => {
                Cow::Borrowed("Hold the fire button: only fire on the target counts")
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
            HudField::Fps => Cow::Owned(format!("{fps:.0} fps")),
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
            HudField::CountdownLabel => Cow::Borrowed(match (countdown, play.get()) {
                (None, _) => "",
                (Some(_), PlayState::Resuming) => "RESUMING IN",
                (Some(_), _) => "STARTING IN",
            }),
            HudField::Countdown => match countdown {
                Some(secs) => Cow::Owned(format!("{}", secs.ceil() as u32)),
                None => Cow::Borrowed(""),
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
