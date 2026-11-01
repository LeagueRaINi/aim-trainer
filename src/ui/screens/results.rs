//! The results screen after a practice run.

use bevy::feathers::controls::ButtonVariant;
use bevy::feathers::display::label_dim;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::score::*;
use crate::AppState;
use crate::adaptive_log::{AdaptiveEntry, AdaptiveLog};
use crate::drill::{Run, RunPurpose, RunStats, begin_run};
use crate::history::{LastRun, RunRecorded};
use crate::metrics::{quest, throughput};
use crate::replay::{Replay, ReplayDir, watch};
use crate::settings::Settings;
use crate::ui::charts::*;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Side of the hit scatter plot, px.
const SCATTER: f32 = 120.0;
/// The scatter spans this many target radii from the centre each way.
const SCATTER_RANGE: f32 = 2.0;
const DOT: f32 = 5.0;

/// Container the hit scatter is drawn into (see [`draw_hit_scatter`]).
#[derive(Component, Clone, Copy, Default)]
pub struct HitScatter;

/// A finished practice run goes to the results screen.
pub(crate) fn show_results(
    mut commands: Commands,
    mut recorded: MessageReader<RunRecorded>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for recorded in recorded.read().filter(|r| r.run.purpose == RunPurpose::Practice) {
        let run = &recorded.run;
        commands.insert_resource(LastRun {
            stats: run.stats.clone(),
            previous: recorded.previous.clone(),
            timestamp: recorded.timestamp,
            drill: run.drill.clone(),
            duration: run.duration,
        });
        next_state.set(AppState::Results);
    }
}

pub fn spawn_results(
    mut commands: Commands,
    last: Res<LastRun>,
    settings: Res<Settings>,
    adaptive_log: Res<AdaptiveLog>,
) {
    let stats = &last.stats;
    let kind = stats.kind.name();
    let context = if stats.drill == kind { "Results".to_string() } else { format!("Results  ·  {kind}") };
    let sens = format!("Sensitivity {:.3}  |  {}", stats.sens, settings.describe_sens(stats.sens));
    // Flick drills: where the clicks landed, once there are any measured flicks.
    let clicked = throughput::measured(&stats.clicks).next().is_some();
    let precision_display = if clicked { Display::Flex } else { Display::None };
    // Adaptive runs: the measured θ75 and how it compares with the last one.
    let adaptive = adaptive_log.entry(last.timestamp);
    let previous = adaptive.and_then(|entry| adaptive_log.previous(entry));
    let adaptive_display = if adaptive.is_some() { Display::Flex } else { Display::None };
    commands
        .spawn_scene(bsn! {
            @overlay()
            Children [
                @panel()
                Node { width: px(660) }
                Children [
                    @screen_title(context, stats.drill.clone())
                    --
                    @score_summary(stats, &last.previous)
                    --
                    @adaptive_result(adaptive, previous)
                    Node { display: adaptive_display }
                    --
                    @precision(stats)
                    Node { display: precision_display }
                    --
                    @label_dim(sens)
                    --
                    @actions(bsn_list! {
                        @button("Retry", ButtonVariant::Primary)
                        on(retry)
                        --
                        @button("Watch replay", ButtonVariant::Normal)
                        on(watch_last_replay)
                        --
                        @button("Menu", ButtonVariant::Normal)
                        on(go_to_menu)
                    })
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Results));
}

fn retry(
    _: On<Activate>,
    last: Res<LastRun>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let run = Run::new(last.drill.clone(), settings.sens, last.duration, RunPurpose::Practice, last.drill.name.clone());
    begin_run(&mut commands, &mut next_state, run);
}

fn watch_last_replay(
    _: On<Activate>,
    last: Res<LastRun>,
    replays: Res<ReplayDir>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    match Replay::load(&replays.0, last.timestamp) {
        Some(replay) => watch(&mut commands, &mut next_state, replay, AppState::Results),
        None => warn!("Replay for this run could not be loaded"),
    }
}

/// An adaptive run's θ75 with its credible interval, the share of targets hit in time, and the
/// change from the task's previous result (smaller is better).
fn adaptive_result(entry: Option<&AdaptiveEntry>, previous: Option<&AdaptiveEntry>) -> impl Scene {
    let (theta, interval, in_time, change, note) = match entry {
        Some(entry) => {
            let e = &entry.estimate;
            let change = match previous {
                Some(before) => {
                    let percent = (e.theta75 / before.estimate.theta75 - 1.0) * 100.0;
                    let verdict = match percent.round() {
                        p if p < 0.0 => "smaller, better",
                        p if p > 0.0 => "larger",
                        _ => "same",
                    };
                    format!("{percent:+.0}% ({verdict})")
                },
                None => "first result".to_string(),
            };
            let note = format!(
                "θ75: the target radius you hit {:.0}% of the time within {:.1} s, estimated by QUEST+ from this run's {} targets ({} mode); smaller is better. The interval holds it with {:.0}% probability. Change: versus your previous adaptive result on this drill.",
                quest::TARGET_HIT * 100.0,
                entry.time_limit,
                e.trials,
                entry.mode.name().to_lowercase(),
                quest::CREDIBLE * 100.0
            );
            (
                format!("{:.2}°", e.theta75),
                format!("{:.2}–{:.2}°", e.low, e.high),
                format!("{} / {}", e.hits, e.trials),
                change,
                note,
            )
        },
        None => Default::default(),
    };
    let interval_caption = format!("{:.0}% interval", quest::CREDIBLE * 100.0);
    bsn! {
        @column()
        Children [
            @row()
            Node { align_items: AlignItems::Stretch }
            Children [
                @stat("θ75 (target radius)", theta)
                --
                @stat(interval_caption, interval)
                --
                @stat("Hit in time", in_time)
                --
                @stat("Change", change)
            ]
            --
            @styled(note, bevy::feathers::constants::fonts::REGULAR, 11.0, tokens::TEXT_DIM)
        ]
    }
}

/// Throughput, centre hits and click bias next to the hit scatter.
fn precision(stats: &RunStats) -> impl Scene {
    let throughput = stats.throughput().map_or("-".to_string(), |tp| format!("{tp:.2} bit/s"));
    let centre = stats.centre_hits().map_or("-".to_string(), |c| format!("{:.0}%", c * 100.0));
    let bias = stats.click_bias().map_or("-".to_string(), |b| match (b * 100.0).round() {
        p if p > 0.0 => format!("{p:.0}% past"),
        p if p < 0.0 => format!("{:.0}% short", -p),
        _ => "centred".to_string(),
    });
    let sideways = stats.click_off_axis().map_or("-".to_string(), |s| format!("{:.0}%", s * 100.0));
    let note = format!(
        "Throughput (ISO 9241-411 effective): aiming difficulty resolved per second, from the spread and timing of your first clicks; unlike the score it barely moves when you play faster or more carefully (needs {} flicks of one distance and size). Centre hits: first clicks within half a radius. Bias / sideways: mean offset along / across the flick, in target radii. Plot: flick going right, ring = target edge.",
        throughput::MIN_GROUP
    );
    bsn! {
        @row()
        Node { column_gap: px(SPACE_L), align_items: AlignItems::Start }
        Children [
            Node { width: px(SCATTER), height: px(SCATTER), flex_shrink: 0.0, border_radius: BorderRadius::all(px(4)) }
            ThemeBackgroundColor(CHART_BG)
            HitScatter
            --
            @column()
            Node { flex_grow: 1.0, flex_basis: px(0) }
            Children [
                @row()
                Node { align_items: AlignItems::Stretch }
                Children [
                    @stat("Throughput", throughput)
                    --
                    @stat("Centre hits", centre)
                    --
                    @stat("Click bias", bias)
                    --
                    @stat("Sideways", sideways)
                ]
                --
                @styled(note, bevy::feathers::constants::fonts::REGULAR, 11.0, tokens::TEXT_DIM)
            ]
        ]
    }
}

/// Draws the last run's first clicks into a new [`HitScatter`]: target-radius units, the flick
/// pointing right, hits green and misses red (clamped to the plot's edge).
pub fn draw_hit_scatter(
    mut commands: Commands,
    last: Option<Res<LastRun>>,
    scatters: Query<Entity, Added<HitScatter>>,
) {
    let Some(last) = last else { return };
    let scale = SCATTER * 0.5 / SCATTER_RANGE;
    let centre = SCATTER * 0.5;
    for plot in &scatters {
        commands.spawn((ChildOf(plot), absolute(0.0, centre, SCATTER, 1.0), ThemeBackgroundColor(DIVIDER)));
        for (radii, color) in [(1.0, tokens::TEXT_DIM), (throughput::CENTRE, DIVIDER)] {
            let size = 2.0 * radii * scale;
            commands.spawn((
                ChildOf(plot),
                Node {
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::MAX,
                    ..absolute(centre - size * 0.5, centre - size * 0.5, size, size)
                },
                ThemeBorderColor(color),
            ));
        }
        for (click, offset) in throughput::measured(&last.stats.clicks) {
            // Screen y grows downward; counter-clockwise (positive y) is up.
            let at = (Vec2::new(centre + offset.x * scale, centre - offset.y * scale))
                .clamp(Vec2::ZERO, Vec2::splat(SCATTER));
            let color = if click.hit { TEXT_GOOD } else { TEXT_BAD };
            commands.spawn((
                ChildOf(plot),
                Node { border_radius: BorderRadius::MAX, ..absolute(at.x - DOT * 0.5, at.y - DOT * 0.5, DOT, DOT) },
                ThemeBackgroundColor(color),
            ));
        }
        text(&mut commands, plot, "flick →".into(), 10.0, tokens::TEXT_DIM, absolute(SCATTER - 44.0, 2.0, 42.0, 14.0));
    }
}
