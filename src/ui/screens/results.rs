//! The results screen after a practice run.

use bevy::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::reaction_result::reaction_result;
use super::score::*;
use super::taps::tap_breakdown;
use super::{calm, follow};
use crate::AppState;
use crate::adaptive_log::{AdaptiveEntry, AdaptiveLog};
use crate::calm_log::CalmLog;
use crate::drill::rifle::TapRun;
use crate::drill::{Run, RunPurpose, RunStats, begin_run};
use crate::history::{LastRun, RunRecorded};
use crate::metrics::{quest, steadiness, throughput};
use crate::reaction_log::ReactionLog;
use crate::replay::{Replay, ReplayDir, watch};
use crate::settings::Settings;
use crate::ui::charts::*;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Width of the results window.
const RESULTS_W: f32 = 720.0;
/// Side of the hit scatter plot, px.
const SCATTER: f32 = 120.0;
/// The scatter spans this many target radii from the centre each way.
const SCATTER_RANGE: f32 = 2.0;
const DOT: f32 = 5.0;
/// Clicks whose mean lands within this many target radii of the centre count as centred.
const CENTRED: f32 = 0.15;

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

/// The score with its comparison, then one card per breakdown the run has: which ones depends
/// on the drill (flick drills: where the clicks landed) and its mode (adaptive: the measured
/// θ75). They scroll if a window is too short for them; the actions stay in view.
#[allow(clippy::too_many_arguments)]
pub fn spawn_results(
    mut commands: Commands,
    last: Res<LastRun>,
    settings: Res<Settings>,
    adaptive_log: Res<AdaptiveLog>,
    taps: Option<Res<TapRun>>,
    reaction_log: Res<ReactionLog>,
    calm_log: Res<CalmLog>,
    replays: Res<ReplayDir>,
) {
    let stats = &last.stats;
    let kind = stats.kind.name();
    let context = if stats.drill == kind { "Results".to_string() } else { format!("Results  ·  {kind}") };
    let sens = format!("Sensitivity {:.3}  |  {}", stats.sens, settings.describe_sens(stats.sens));
    // Adaptive runs: the measured θ75 and how it compares with the last one.
    let adaptive: Vec<_> = adaptive_log
        .entry(last.timestamp)
        .map(|entry| adaptive_result(entry, adaptive_log.previous(entry)))
        .into_iter()
        .collect();
    // Flick drills: where the clicks landed, once there are any measured flicks.
    let clicked = throughput::measured(&stats.clicks).next().is_some();
    let precision: Vec<_> = clicked.then(|| precision(stats)).into_iter().collect();
    // Bot drills: what the shots were made of.
    let taps = taps.as_deref().filter(|taps| taps.kind == stats.kind && taps.stats.shots > 0);
    let taps: Vec<_> = taps.map(|taps| tap_breakdown(&taps.stats)).into_iter().collect();
    // Reaction tests: the median with its spread and every trial, compared with earlier tests.
    let reaction: Vec<_> = reaction_log
        .entry(last.timestamp)
        .map(|entry| reaction_result(entry, reaction_log.previous(entry), reaction_log.best_before(entry)))
        .into_iter()
        .collect();
    // Tracking runs: how the crosshair followed the target's turns, from the replay just saved.
    let followed = follow::applies(stats.kind)
        .then(|| Replay::load(&replays.0, last.timestamp))
        .flatten()
        .and_then(|replay| follow::measure(&replay));
    let followed: Vec<_> = followed.as_ref().map(follow::direction_changes).into_iter().collect();
    // Calm aim: its four parts, and how they compare with the last test.
    let mm_per_degree = steadiness::mouse_mm(1.0, settings.deg_per_count(stats.sens), settings.dpi);
    let calm: Vec<_> = calm_log
        .entry(last.timestamp)
        .map(|entry| calm::calm_result(entry, calm_log.previous(entry), mm_per_degree))
        .into_iter()
        .collect();
    commands
        .spawn_scene(bsn! {
            @overlay()
            Children [
                @panel()
                Node { width: px(RESULTS_W), max_height: percent(96) }
                Children [
                    @screen_title(context, stats.drill.clone())
                    --
                    @scroll_column(bsn_list! {
                        @score_summary(stats, &last.previous)
                        --
                        {adaptive}
                        --
                        {precision}
                        --
                        {taps}
                        --
                        {reaction}
                        --
                        {followed}
                        --
                        {calm}
                    })
                    --
                    @small(sens)
                    --
                    @actions(bsn_list! {
                        @button("Retry", Btn::Primary)
                        on(retry)
                        --
                        @button("Watch replay", Btn::Secondary)
                        on(watch_last_replay)
                        --
                        @button("Menu", Btn::Secondary)
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
fn adaptive_result(entry: &AdaptiveEntry, previous: Option<&AdaptiveEntry>) -> impl Scene {
    let e = &entry.estimate;
    let theta = format!("{:.2}°", e.theta75);
    let percent = previous.map(|before| (e.theta75 / before.estimate.theta75 - 1.0) * 100.0);
    let (tone, headline, change) = match percent.map(f32::round) {
        Some(p) if p < 0.0 => (TEXT_GOOD, "You hit smaller targets than last time", format!("{p:+.0}%")),
        Some(p) if p > 0.0 => (TEXT_WARN, "Your targets were larger than last time", format!("{p:+.0}%")),
        Some(_) => (TEXT_ACCENT, "Same target size as last time", "same".to_string()),
        None => (TEXT_ACCENT, "Your first adaptive result on this drill", "first result".to_string()),
    };
    let takeaway = format!(
        "You hit a {theta} target {:.0}% of the time within {:.1} s. Smaller is better.",
        quest::TARGET_HIT * 100.0,
        entry.time_limit
    );
    let note = format!(
        "θ75 is estimated by QUEST+ from this run's {} targets ({} mode). The interval holds it with {:.0}% probability. Change: versus your previous adaptive result on this drill.",
        e.trials,
        entry.mode.name().to_lowercase(),
        quest::CREDIBLE * 100.0
    );
    let interval_caption = format!("{:.0}% interval", quest::CREDIBLE * 100.0);
    let interval = format!("{:.2}–{:.2}°", e.low, e.high);
    let in_time = format!("{} / {}", e.hits, e.trials);
    bsn! {
        @card("Adaptive result", bsn_list! {
            @callout(tone, headline, takeaway)
            --
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
            @details("How θ75 is measured", note)
        })
    }
}

/// What the first clicks say in plain words: where they land along the flick.
fn precision_takeaway(stats: &RunStats) -> (ThemeToken, &'static str, String) {
    let centre = stats.centre_hits().map_or(String::new(), |c| {
        format!(" {:.0}% of your first clicks landed in the inner half of the target.", c * 100.0)
    });
    match stats.click_bias() {
        Some(bias) if bias > CENTRED => (
            TEXT_WARN,
            "You click past the centre",
            format!(
                "Your first clicks land {:.0}% of a target radius beyond it on average: a slight overshoot.{centre}",
                bias * 100.0
            ),
        ),
        Some(bias) if bias < -CENTRED => (
            TEXT_WARN,
            "You click short of the centre",
            format!(
                "Your first clicks land {:.0}% of a target radius before it on average: a slight undershoot.{centre}",
                -bias * 100.0
            ),
        ),
        Some(_) => (TEXT_GOOD, "Your clicks are centred", format!("No lean past or short of the centre.{centre}")),
        None => (TEXT_ACCENT, "First clicks", centre.trim_start().to_string()),
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
    let (tone, headline, takeaway) = precision_takeaway(stats);
    let note = format!(
        "Throughput (ISO 9241-411 effective): aiming difficulty resolved per second, from the spread and timing of your first clicks; unlike the score it barely moves when you play faster or more carefully (needs {} flicks of one distance and size). Centre hits: first clicks within half a radius. Bias / sideways: mean offset along / across the flick, in target radii. Plot: every flick turned to point right; the ring is the target's edge, green dots hit, red missed.",
        throughput::MIN_GROUP
    );
    bsn! {
        @card("Where your first clicks landed", bsn_list! {
            @callout(tone, headline, takeaway)
            --
            @row()
            Node { column_gap: px(SPACE_M), align_items: AlignItems::Start }
            Children [
                Node {
                    width: px(SCATTER),
                    height: px(SCATTER),
                    flex_shrink: 0.0,
                    border_radius: BorderRadius::all(px(RADIUS_S)),
                }
                ThemeBackgroundColor(CHART_BG)
                HitScatter
                --
                @column()
                Node { flex_grow: 1.0, flex_basis: px(0) }
                Children [
                    @row()
                    Node { align_items: AlignItems::Stretch }
                    Children [
                        @stat_hint("Throughput", throughput, "higher is better")
                        --
                        @stat_hint("Centre hits", centre, "within half a radius")
                        --
                        @stat_hint("Click bias", bias, "along the flick")
                        --
                        @stat_hint("Sideways", sideways, "across the flick")
                    ]
                    --
                    @details("How to read this", note)
                ]
            ]
        })
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
        let label = absolute(SCATTER - 46.0, 3.0, 44.0, 14.0);
        text(&mut commands, plot, "flick →".into(), FONT_MICRO, tokens::TEXT_DIM, label);
    }
}
