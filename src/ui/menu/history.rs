//! History tab: per-drill progress chart, trend numbers and recent runs with replays.

use bevy::feathers::controls::ButtonVariant;
use bevy::feathers::display::label_dim;
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::AppState;
use crate::drill::DrillKind;
use crate::drill::stats::HistoryStat;
use crate::history::{History, HistoryEntry, RECENT_RUNS, format_date, overall_change, recent_change};
use crate::replay::{Replay, watch};
use crate::ui::charts::*;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Most recent runs drawn in the chart.
const CHART_RUNS: usize = 40;
/// Most recent runs listed under the chart.
const LISTED_RUNS: usize = 6;
const CHART_W: f32 = 650.0;
const CHART_H: f32 = 90.0;
/// Width of the y-axis labels left of the plot.
const AXIS_W: f32 = 40.0;
/// Width of the summary card next to the chart.
const SUMMARY_W: f32 = 240.0;

/// The drill (by name) shown on the History tab.
#[derive(Resource, Clone, PartialEq, Eq)]
pub struct HistoryView(String);

impl Default for HistoryView {
    fn default() -> Self {
        Self(DrillKind::default().name().to_string())
    }
}

/// Container whose children are rebuilt whenever the selected drill changes.
#[derive(Component, Clone, Copy, Default)]
pub struct HistoryBody;

#[derive(Component, Clone, Default)]
struct HistoryDrillButton(String);

#[derive(Component, Clone, Copy, Default)]
struct ReplayButton(u64);

/// One selector button per drill with history (built-ins always, custom ones once played).
fn drill_button(name: String, selected: bool) -> impl Scene {
    let variant = ButtonVariant::selected(selected);
    let marker = HistoryDrillButton(name.clone());
    bsn! {
        @compact_button(name, variant)
        Node { height: px(28), min_width: px(84) }
        marker
        on(|activate: On<Activate>, buttons: Query<&HistoryDrillButton>, mut view: ResMut<HistoryView>| {
            if let Ok(HistoryDrillButton(name)) = buttons.get(activate.event_target()) {
                view.0.clone_from(name);
            }
        })
    }
}

pub fn rebuild_history(
    mut commands: Commands,
    view: Res<HistoryView>,
    history: Res<History>,
    bodies: Query<(Entity, Ref<HistoryBody>)>,
) {
    for (body, marker) in &bodies {
        if !view.is_changed() && !marker.is_added() {
            continue;
        }
        commands.entity(body).despawn_related::<Children>();

        let chips = commands.spawn_scene(wrap_row()).insert(ChildOf(body)).id();
        for name in history.drills() {
            let selected = name == view.0;
            commands.spawn_scene(drill_button(name, selected)).insert(ChildOf(chips));
        }

        let runs: Vec<&HistoryEntry> = history.practice_runs(&view.0).collect();
        if runs.is_empty() {
            commands
                .spawn_scene(bsn! {
                    @surface()
                    Children [ @label_dim("No practice runs of this drill yet. Play one from the Play tab.") ]
                })
                .insert(ChildOf(body));
            continue;
        }

        let top = commands
            .spawn((ChildOf(body), Node { column_gap: px(SPACE_L), align_items: AlignItems::Stretch, ..default() }))
            .id();
        let chart = commands.spawn_scene(bsn! { @surface() Node { flex_grow: 1.0 } }).insert(ChildOf(top)).id();
        commands.spawn_scene(overline("Progress")).insert(ChildOf(chart));
        spawn_chart(&mut commands, chart, &runs[runs.len().saturating_sub(CHART_RUNS)..]);
        let summary = commands
            .spawn_scene(bsn! { @surface() Node { width: px(SUMMARY_W), flex_shrink: 0.0 } })
            .insert(ChildOf(top))
            .id();
        spawn_summary(&mut commands, summary, &history, &view.0);

        let list = commands.spawn_scene(bsn! { @surface() Node { row_gap: px(2) } }).insert(ChildOf(body)).id();
        spawn_run_list(&mut commands, list, &runs);
    }
}

/// Run count, personal best, recent average and trends as label/value rows.
fn spawn_summary(commands: &mut Commands, parent: Entity, history: &History, drill: &str) {
    let scores = history.scores(drill);
    let runs: Vec<_> = history.practice_runs(drill).collect();
    let (best_index, best) = scores.iter().copied().enumerate().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or_default();
    let recent = &scores[scores.len().saturating_sub(RECENT_RUNS)..];
    let average = recent.iter().sum::<f32>() / recent.len() as f32;

    commands.spawn_scene(overline("Summary")).insert(ChildOf(parent));
    let mut spawn = |scene| {
        commands.spawn_scene(scene).insert(ChildOf(parent));
    };
    spawn(key_value("Runs".into(), scores.len().to_string(), TEXT_STRONG));
    spawn(key_value("Personal best".into(), format!("{best:.0}"), GOLD));
    spawn(key_value("Set on".into(), format_date(runs[best_index].timestamp), tokens::TEXT_MAIN));
    spawn(key_value(format!("Avg of last {}", recent.len()), format!("{average:.0}"), TEXT_STRONG));
    if let Some(change) = recent_change(&scores) {
        spawn(key_value(format!("vs the {RECENT_RUNS} before"), format!("{change:+.1}%"), delta_color(change)));
    }
    if let Some(change) = overall_change(&scores) {
        spawn(key_value("Since you started".into(), format!("{change:+.0}%"), delta_color(change)));
    }
}

/// Bar per run (PB highlighted) with a moving-average line on top.
fn spawn_chart(commands: &mut Commands, parent: Entity, runs: &[&HistoryEntry]) {
    let scores: Vec<f32> = runs.iter().map(|r| r.score).collect();
    let (min, max) = scores.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)));
    let pad = ((max - min) * 0.15).max(1.0);
    let (lo, hi) = ((min - pad).max(0.0), max + pad);
    let y = |value: f32| CHART_H - (value - lo) / (hi - lo) * CHART_H;
    let slot = CHART_W / scores.len() as f32;
    let bar_width = (slot * 0.6).clamp(2.0, 28.0);

    let row = commands.spawn((ChildOf(parent), Node { column_gap: px(SPACE_S), ..default() })).id();
    let labels = commands
        .spawn((
            ChildOf(row),
            Node {
                width: px(AXIS_W),
                height: px(CHART_H),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::End,
                ..default()
            },
        ))
        .id();
    let decimals = if hi - lo < 10.0 { 1 } else { 0 };
    for value in [hi, (hi + lo) * 0.5, lo] {
        text(commands, labels, format!("{value:.decimals$}"), 11.0, tokens::TEXT_DIM, Node::default());
    }
    let plot = commands
        .spawn((
            ChildOf(row),
            Node { width: px(CHART_W), height: px(CHART_H), border_radius: BorderRadius::all(px(4)), ..default() },
            ThemeBackgroundColor(CHART_BG),
        ))
        .id();

    for fraction in [0.25, 0.5, 0.75] {
        commands.spawn((ChildOf(plot), absolute(0.0, CHART_H * fraction, CHART_W, 1.0), ThemeBackgroundColor(DIVIDER)));
    }

    let best = scores.iter().copied().fold(f32::MIN, f32::max);
    for (i, &score) in scores.iter().enumerate() {
        let top = y(score);
        let color = if score >= best { GOLD } else { CHART_BAR };
        commands.spawn((
            ChildOf(plot),
            absolute(i as f32 * slot + (slot - bar_width) * 0.5, top, bar_width, CHART_H - top),
            ThemeBackgroundColor(color),
        ));
    }

    text(
        commands,
        parent,
        format!("Last {} runs, oldest to newest. Gold = personal best.", scores.len()),
        11.0,
        tokens::TEXT_DIM,
        Node { margin: UiRect::left(px(AXIS_W + SPACE_S)), ..default() },
    );
}

fn spawn_run_list(commands: &mut Commands, parent: Entity, runs: &[&HistoryEntry]) {
    let kind = runs.last().map_or(DrillKind::default(), |run| run.kind);
    let columns = kind.def().score.history_columns();
    const NUMBER_W: f32 = 56.0;
    const DATE_W: f32 = 120.0;
    const VALUE_W: f32 = 96.0;

    commands
        .spawn_scene(bsn! { @overline("Recent runs") Node { margin: UiRect::bottom(px(SPACE_XS)) } })
        .insert(ChildOf(parent));
    let mut header = vec![cell("#", NUMBER_W, tokens::TEXT_DIM), cell("Date", DATE_W, tokens::TEXT_DIM)];
    let headers = columns.iter().map(|(header, _)| header);
    header.extend(
        ["Score"].iter().chain(headers).chain(&["Sens", "cm/360"]).map(|h| cell(*h, VALUE_W, tokens::TEXT_DIM)),
    );
    commands.spawn_scene(table_row(header, false)).insert(ChildOf(parent));

    let best = runs.iter().map(|r| r.score).fold(f32::MIN, f32::max);
    let total = runs.len();
    for (age, run) in runs.iter().rev().take(LISTED_RUNS).enumerate() {
        let mut cells = vec![
            cell(format!("#{}", total - age), NUMBER_W, tokens::TEXT_DIM),
            cell(format_date(run.timestamp), DATE_W, tokens::TEXT_MAIN),
            cell(format!("{}", run.score), VALUE_W, if run.score >= best { GOLD } else { TEXT_STRONG }),
        ];
        for &(_, stat) in columns {
            let value = match stat {
                HistoryStat::OnTarget => format!("{:.1}%", run.on_target_pct),
                HistoryStat::Accuracy => format!("{:.1}%", run.accuracy * 100.0),
                HistoryStat::KillMs => format!("{:.0}", run.avg_kill_ms),
            };
            cells.push(cell(value, VALUE_W, tokens::TEXT_MAIN));
        }
        cells.push(cell(format!("{:.3}", run.sens), VALUE_W, tokens::TEXT_MAIN));
        cells.push(cell(format!("{:.1}", run.cm_per_360), VALUE_W, tokens::TEXT_MAIN));
        let entry = commands.spawn_scene(table_row(cells, age % 2 == 0)).insert(ChildOf(parent)).id();

        let timestamp = run.timestamp;
        if Replay::exists(timestamp) {
            commands
                .spawn_scene(bsn! {
                    @compact_button("Replay", ButtonVariant::Normal)
                    Node { margin: UiRect::left(Val::Auto), height: px(24) }
                    ReplayButton(timestamp)
                    on(open_replay)
                })
                .insert(ChildOf(entry));
        }
    }
}

fn open_replay(
    activate: On<Activate>,
    buttons: Query<&ReplayButton>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Ok(&ReplayButton(timestamp)) = buttons.get(activate.event_target()) else { return };
    match Replay::load(timestamp) {
        Some(replay) => watch(&mut commands, &mut next_state, replay, AppState::Menu),
        None => warn!("Replay {timestamp} could not be loaded"),
    }
}
