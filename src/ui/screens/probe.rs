//! What is on screen while a [`Probe`] measures: its ready card, countdown, the live sweep bar
//! or the timer of a hold or follow, and its pause menu. Built once per stage of the probe;
//! within a stage only the texts and the sweep marker that changed are touched.

use std::borrow::Cow;

use bevy::feathers::constants::fonts;
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::checkup::{step_text, step_title};
use super::score::screen_title;
use crate::AppState;
use crate::crosshair::CrosshairPreview;
use crate::finder::checkup::Step;
use crate::finder::probe::{LEAD_SECS, Probe, ProbePause, ProbePhase, SWEEPS};
use crate::settings::Settings;
use crate::ui::charts::spawn_in;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// The part of a probe's life the HUD is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Ready,
    Countdown,
    Live,
    Paused,
}

impl Stage {
    fn of(probe: &Probe) -> Self {
        match (probe.pause_state(), probe.phase()) {
            (ProbePause::Menu, _) => Stage::Paused,
            (ProbePause::Resuming(_), _) | (_, ProbePhase::Countdown) => Stage::Countdown,
            (_, ProbePhase::Ready) => Stage::Ready,
            (_, ProbePhase::Live | ProbePhase::Done) => Stage::Live,
        }
    }
}

/// The root of the probe's HUD and the stage it was built for.
#[derive(Component)]
pub struct ProbeHud(Stage);

/// Text slots filled by [`update_probe_hud`].
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProbeField {
    /// The top bar's changing part: time left, or what is being waited for.
    #[default]
    Status,
    Countdown,
    /// How many sweeps are done and how long they were.
    Sweeps,
    /// How far apart the hand's furthest points are.
    Span,
}

/// The dot on the sweep bar that follows the hand.
#[derive(Component, Clone, Copy, Default)]
pub struct SweepMarker;

/// Width of the sweep bar and of its marker.
const BAR_W: f32 = 420.0;
const MARKER_W: f32 = 10.0;

/// Builds the HUD for the probe's current stage whenever that changes (or there is none yet),
/// then keeps its texts and the sweep marker up to date.
#[allow(clippy::type_complexity)]
pub fn update_probe_hud(
    mut commands: Commands,
    probe: Res<Probe>,
    roots: Query<(Entity, &ProbeHud)>,
    mut fields: Query<(&ProbeField, &mut Text)>,
    mut markers: Query<&mut Node, With<SweepMarker>>,
) {
    let stage = Stage::of(&probe);
    if roots.iter().next().is_none_or(|(_, hud)| hud.0 != stage) {
        for (root, _) in &roots {
            commands.entity(root).despawn();
        }
        spawn_hud(&mut commands, &probe, stage);
        return;
    }
    for (field, mut text) in &mut fields {
        let wanted: Cow<str> = match field {
            ProbeField::Status => status(&probe),
            ProbeField::Countdown => match probe.countdown() {
                Some(secs) => Cow::Owned(format!("{}", secs.ceil() as u32)),
                None => Cow::Borrowed(""),
            },
            ProbeField::Sweeps => {
                let done: Vec<String> = probe.sweeps_cm().map(|cm| format!("{cm:.1} cm")).collect();
                match done.len() {
                    0 => Cow::Owned(format!("Sweep 1 of {SWEEPS}: it counts from your first turn")),
                    n => Cow::Owned(format!("Sweep {} of {SWEEPS}  ·  {}", (n + 1).min(SWEEPS), done.join("  ·  "))),
                }
            },
            ProbeField::Span => Cow::Owned(format!("{:.1} cm between your furthest points", probe.sweep_span().1)),
        };
        if text.0 != wanted {
            text.0 = wanted.into_owned();
        }
    }
    let left = percent(probe.sweep_span().0 * 100.0);
    for mut marker in &mut markers {
        if marker.left != left {
            marker.left = left;
        }
    }
}

/// The top bar's changing text.
fn status(probe: &Probe) -> Cow<'static, str> {
    match (probe.phase(), probe.time_left()) {
        (ProbePhase::Ready, _) => Cow::Borrowed("Not started"),
        (ProbePhase::Live, Some(left)) if !probe.scoring() => {
            Cow::Owned(format!("Get on the dot  ·  {:.1}s", (left - probe.task.scored_secs().unwrap_or(0.0)).max(0.0)))
        },
        (_, Some(left)) => Cow::Owned(format!("Measuring  ·  {left:.1}s")),
        (_, None) => Cow::Borrowed("Measuring"),
    }
}

fn spawn_hud(commands: &mut Commands, probe: &Probe, stage: Stage) {
    let step = Step::Probe(probe.task);
    let title = step_title(step);
    let root = commands
        .spawn((
            ProbeHud(stage),
            DespawnOnExit(AppState::Finder),
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        ))
        .id();
    if probe.task.aims() {
        // The crosshair, centred on the screen as in a drill.
        commands.spawn((
            ChildOf(root),
            CrosshairPreview,
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        ));
    }
    spawn_in(
        commands,
        root,
        bsn! {
            @hud_anchor(px(14))
            Children [
                @hud_bar()
                Children [
                    @styled(title, fonts::BOLD, FONT_BODY, tokens::TEXT_MAIN)
                    --
                    @vertical_divider()
                    --
                    Node { min_width: px(210), justify_content: JustifyContent::Center }
                    Children [
                        @styled("", fonts::REGULAR, FONT_HUD, TEXT_STRONG)
                        TextFont { font_features: {tabular()} }
                        ProbeField::Status
                    ]
                    --
                    @vertical_divider()
                    --
                    @key_hint("Esc", "pause")
                ]
            ]
        },
    );
    match stage {
        Stage::Ready => {
            let blurb = step_text(step);
            spawn_in(
                commands,
                root,
                bsn! {
                    @hud_anchor(percent(20))
                    Children [
                        @hud_bar()
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(6),
                            max_width: px(640),
                            padding: UiRect::axes(px(36), px(20)),
                            border_radius: BorderRadius::all(px(RADIUS_L)),
                        }
                        Children [
                            @styled(title, fonts::BOLD, FONT_READY, TEXT_STRONG)
                            --
                            @styled(blurb, fonts::REGULAR, FONT_HUD, tokens::TEXT_MAIN)
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
                },
            );
        },
        Stage::Countdown => {
            let caption = if probe.phase() == ProbePhase::Live { "RESUMING IN" } else { "STARTING IN" };
            spawn_in(
                commands,
                root,
                bsn! {
                    @hud_anchor(percent(24))
                    Children [
                        @column()
                        Node { align_items: AlignItems::Center, row_gap: px(0) }
                        Children [
                            @styled(caption, fonts::BOLD, FONT_BODY, TEXT_ACCENT)
                            --
                            @styled("", fonts::BOLD, FONT_COUNTDOWN, TEXT_STRONG)
                            TextFont { font_features: {tabular()} }
                            ProbeField::Countdown
                        ]
                    ]
                },
            );
        },
        Stage::Live if !probe.task.aims() => {
            spawn_in(commands, root, sweep_card());
        },
        Stage::Live => {},
        Stage::Paused => {
            let progress = match probe.time_left() {
                Some(left) if probe.phase() == ProbePhase::Live => format!("{left:.1} s left"),
                None if probe.phase() == ProbePhase::Live => {
                    format!("{} of {SWEEPS} sweeps done", probe.sweeps_cm().count())
                },
                _ => "Not started yet".to_string(),
            };
            let resumes = if probe.task.aims() {
                format!(
                    "Resume continues where you stopped, after a short countdown (the first {LEAD_SECS:.0} s of a step never count)."
                )
            } else {
                "Resume keeps the sweeps already counted; the one you were in starts again.".to_string()
            };
            spawn_in(
                commands,
                root,
                bsn! {
                    @overlay()
                    GlobalZIndex(1)
                    Children [
                        @panel()
                        Node { width: px(400), min_width: px(0), row_gap: px(SPACE_S) }
                        Children [
                            @screen_title("Paused", title)
                            --
                            @column()
                            Node { row_gap: px(2), margin: UiRect::bottom(px(SPACE_S)) }
                            Children [ @label_strong(progress) -- @small(resumes) ]
                            --
                            @pause_action("Resume", "Esc", Btn::Primary)
                            on(|_: On<Activate>, mut probe: ResMut<Probe>| probe.resume())
                            --
                            @pause_action("Restart step", "R", Btn::Secondary)
                            on(|_: On<Activate>, mut probe: ResMut<Probe>, settings: Res<Settings>| {
                                let task = probe.task;
                                *probe = Probe::new(task, &settings);
                            })
                            --
                            @pause_action("Back to the steps", "Q", Btn::Secondary)
                            on(|_: On<Activate>, mut commands: Commands, mut next: ResMut<NextState<AppState>>| {
                                commands.remove_resource::<Probe>();
                                next.set(AppState::Finder);
                            })
                        ]
                    ]
                },
            );
        },
    }
}

/// The live sweep display: a bar between the hand's furthest points with a marker that follows
/// it, and the sweeps counted so far.
fn sweep_card() -> impl Scene {
    bsn! {
        @hud_anchor(percent(22))
        Children [
            @hud_bar()
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(SPACE_M),
                padding: UiRect::axes(px(36), px(20)),
                border_radius: BorderRadius::all(px(RADIUS_L)),
            }
            Children [
                @styled("Left and right, as far as is comfortable, without lifting", fonts::BOLD, FONT_HUD, TEXT_STRONG)
                --
                Node { width: px(BAR_W), height: px(MARKER_W), border_radius: BorderRadius::all(px(MARKER_W * 0.5)) }
                ThemeBackgroundColor(DIVIDER)
                Children [
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(50),
                        margin: UiRect::left(px(-MARKER_W * 0.5)),
                        width: px(MARKER_W),
                        height: px(MARKER_W),
                        border_radius: BorderRadius::MAX,
                    }
                    ThemeBackgroundColor(ACCENT)
                    SweepMarker
                ]
                --
                @styled("", fonts::REGULAR, FONT_HUD, tokens::TEXT_MAIN)
                TextFont { font_features: {tabular()} }
                ProbeField::Sweeps
                --
                @styled("", fonts::REGULAR, FONT_SMALL, tokens::TEXT_DIM)
                TextFont { font_features: {tabular()} }
                ProbeField::Span
            ]
        ]
    }
}

/// A full-width button of the pause menu with the key that does the same.
fn pause_action(text: &'static str, key: &'static str, kind: Btn) -> impl Scene {
    bsn! {
        @button_box(kind, 44.0)
        Node { justify_content: JustifyContent::SpaceBetween }
        Children [ @caption(text) -- @keycap(key) ]
    }
}
