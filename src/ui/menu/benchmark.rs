//! Benchmark page: the index and each kind of aim over the benchmarks so far, when the next one
//! is due, and the fixed scenarios with the button that starts them. Built by
//! [`rebuild_benchmark`] when the page is opened.

use bevy::feathers::constants::fonts;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{MAIN_COLUMN_W, MenuTab, page};
use crate::AppState;
use crate::routine::start_routine;
use crate::training::benchmark::{self, BenchmarkLog, INTERVAL_DAYS, SCENARIO_SECS, SCENARIOS};
use crate::training::{day_label, local_day, today, utc_offset_secs};
use crate::ui::charts::*;
use crate::ui::screens::benchmark::{INDEX_EXPLAINED, move_color, trend_color};
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// The container [`rebuild_benchmark`] fills.
#[derive(Component, Clone, Copy, Default)]
pub struct BenchmarkBody;

/// Benchmarks shown in the charts: the latest this many.
const SHOWN: usize = 12;
const CHART_W: f32 = MAIN_COLUMN_W - 2.0 * SPACE_L;
const CHART_H: f32 = 64.0;
/// Width of the index chart's axis labels.
const AXIS_W: f32 = 28.0;
const KIND_W: f32 = 150.0;
const SPARK_W: f32 = 150.0;
const SPARK_H: f32 = 20.0;
const VALUE_W: f32 = 64.0;

pub(super) fn benchmark_page() -> impl Scene {
    bsn! {
        @page(MenuTab::Benchmark, bsn_list! {
            @scroll_column(bsn_list! {
                Node { flex_direction: FlexDirection::Row, column_gap: px(SPACE_L), align_items: AlignItems::Start }
                BenchmarkBody
            })
            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
        })
    }
}

pub fn rebuild_benchmark(mut commands: Commands, bodies: Query<Entity, Added<BenchmarkBody>>, log: Res<BenchmarkLog>) {
    for body in &bodies {
        let commands = &mut commands;
        let node = Node { width: px(MAIN_COLUMN_W), flex_shrink: 0.0, ..default() };
        let left = column_in(commands, body, node);
        let node = Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), ..default() };
        let right = column_in(commands, body, node);
        if log.entries().is_empty() {
            let text = format!(
                "{} short scenarios with fixed settings, about ten minutes. Your first benchmark sets the baseline (index 100); every later one shows how each kind of aim moved since.",
                SCENARIOS.len()
            );
            spawn_in(commands, left, empty_state("No benchmark yet", text));
            let steps = spawn_in(commands, left, bsn! { @surface() Node { row_gap: px(SPACE_M) } });
            spawn_in(
                commands,
                steps,
                numbered_step(
                    "1",
                    "Play the eight scenarios",
                    "Fixed settings, 60 s each, in one go with short breaks.",
                ),
            );
            spawn_in(
                commands,
                steps,
                numbered_step(
                    "2",
                    "Again each week",
                    "Same day, time of day and warm-up: only then do the numbers compare.",
                ),
            );
            spawn_in(
                commands,
                steps,
                numbered_step(
                    "3",
                    "See what moved",
                    "Your index and each kind of aim against your first benchmark; the weekly plan trains what moves least.",
                ),
            );
        } else {
            progress_card(commands, left, &log);
            kinds_card(commands, left, &log);
        }
        next_card(commands, right, &log);
        scenarios_card(commands, right);
    }
}

fn column_in(commands: &mut Commands, parent: Entity, node: Node) -> Entity {
    let node = Node { flex_direction: FlexDirection::Column, row_gap: px(SPACE_L), ..node };
    commands.spawn((ChildOf(parent), node)).id()
}

/// The latest index with its takeaway, and the index over the benchmarks so far.
fn progress_card(commands: &mut Commands, parent: Entity, log: &BenchmarkLog) {
    let entries = log.entries();
    let (first, latest) = (&entries[0], &entries[entries.len() - 1]);
    let card = spawn_in(commands, parent, surface());
    let top =
        spawn_in(commands, card, bsn! { @row() Node { column_gap: px(SPACE_XL), align_items: AlignItems::Center } });
    spawn_in(commands, top, hero("Index", format!("{:.0}", benchmark::index(first, latest))));
    let side = spawn_in(commands, top, column());
    let count = match entries.len() {
        1 => "Your first benchmark: the baseline".to_string(),
        n => format!("{n} benchmarks since {}", day_label(local_day(first.timestamp, utc_offset_secs()))),
    };
    spawn_in(commands, side, label(count));
    spawn_in(commands, side, small("Your first benchmark is 100; 110 is about 10% better across the board."));
    let (trend, title, takeaway) = benchmark::takeaway(entries, entries.len() - 1);
    spawn_in(commands, card, callout(trend_color(trend), title, takeaway));
    if entries.len() > 1 {
        let shown = &entries[entries.len().saturating_sub(SHOWN)..];
        let values: Vec<f32> = shown.iter().map(|b| benchmark::index(first, b)).collect();
        spawn_in(commands, card, overline("Index per benchmark, oldest to newest"));
        // The bars start above zero to show small changes: the axis says where.
        let (lo, hi) = score_range(&values);
        let chart = commands.spawn((ChildOf(card), Node { column_gap: px(SPACE_S), ..default() })).id();
        let axis = Node {
            width: px(AXIS_W),
            height: px(CHART_H),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::End,
            ..default()
        };
        let labels = commands.spawn((ChildOf(chart), axis)).id();
        for value in [hi, lo] {
            text(commands, labels, format!("{value:.0}"), FONT_MICRO, tokens::TEXT_DIM, Node::default());
        }
        let width = CHART_W - AXIS_W - SPACE_S;
        let bars = plot(commands, chart, width, CHART_H);
        score_bars(commands, bars, &values, width, CHART_H);
    }
    spawn_in(commands, card, details("How the index works", INDEX_EXPLAINED));
}

/// Each kind of aim: its index over the benchmarks, the latest, and how it moved last time.
fn kinds_card(commands: &mut Commands, parent: Entity, log: &BenchmarkLog) {
    let entries = log.entries();
    let first = &entries[0];
    let shown = &entries[entries.len().saturating_sub(SHOWN)..];
    let card = spawn_in(commands, parent, bsn! { @surface() Node { row_gap: px(2) } });
    spawn_in(commands, card, bsn! { @overline("Kinds of aim") Node { margin: UiRect::bottom(px(SPACE_XS)) } });
    let header = vec![
        header_cell("Kind", KIND_W),
        header_cell("Trend", SPARK_W + SPACE_M),
        header_cell("Index", VALUE_W),
        header_cell("Last time", VALUE_W),
    ];
    spawn_in(commands, card, table_header(header));
    let moves = benchmark::kind_moves(entries, entries.len() - 1);
    for (i, aim) in benchmark::kinds().into_iter().enumerate() {
        let values: Vec<f32> = shown.iter().filter_map(|b| benchmark::kind_index(first, b, aim)).collect();
        let row =
            spawn_in(commands, card, table_row(vec![cell(aim.name().to_string(), KIND_W, TEXT_STRONG)], i % 2 == 0));
        let node =
            Node { width: px(SPARK_W + SPACE_M), flex_shrink: 0.0, align_items: AlignItems::Center, ..default() };
        let spark = commands.spawn((ChildOf(row), node)).id();
        if values.len() > 1 {
            let bars = plot(commands, spark, SPARK_W, SPARK_H);
            score_bars(commands, bars, &values, SPARK_W, SPARK_H);
        }
        let latest = values.last().copied().unwrap_or(100.0);
        spawn_in(commands, row, cell(format!("{latest:.0}"), VALUE_W, TEXT_STRONG));
        let moved = moves.iter().find(|m| m.0 == aim).map(|m| m.1);
        spawn_in(
            commands,
            row,
            match moved {
                Some(pct) => cell(format!("{pct:+.0}%"), VALUE_W, move_color(pct)),
                None => cell("-".to_string(), VALUE_W, tokens::TEXT_DIM),
            },
        );
    }
    let note = "Each kind's index is the geometric mean of its scenarios' ratios to your first benchmark. Changes within ±5% show in white: single runs commonly differ by that much.";
    spawn_in(commands, card, bsn! { @small(note) Node { margin: UiRect::top(px(SPACE_XS)) } });
}

/// When the next benchmark is due, and the button that starts it.
fn next_card(commands: &mut Commands, parent: Entity, log: &BenchmarkLog) {
    let card = spawn_in(commands, parent, surface());
    spawn_in(commands, card, overline("Next benchmark"));
    let today = today();
    match log.latest().map(|b| local_day(b.timestamp, utc_offset_secs())) {
        None => {
            spawn_in(commands, card, label_strong("Today: your baseline"));
        },
        Some(last) => {
            let due = last + INTERVAL_DAYS;
            let (text, color) = if due > today {
                (format!("{}, in {} days", day_label(due), due - today), TEXT_STRONG)
            } else if due == today {
                ("Due today".to_string(), TEXT_ACCENT)
            } else {
                (format!("Due since {}", day_label(due)), TEXT_WARN)
            };
            spawn_in(commands, card, styled(text, fonts::BOLD, FONT_H2, color));
            spawn_in(commands, card, small(format!("A week after your last one ({}).", day_label(last))));
        },
    }
    let length = format!(
        "{} scenarios of {SCENARIO_SECS:.0} s, about ten minutes with breaks. Same time of day and the same warm-up each time, so the numbers compare.",
        SCENARIOS.len()
    );
    spawn_in(commands, card, label_dim(length));
    spawn_in(commands, card, bsn! { @cta("Start benchmark") on(start_benchmark) });
}

/// The scenarios in the order they are played, and what is fixed about them.
fn scenarios_card(commands: &mut Commands, parent: Entity) {
    let card = spawn_in(commands, parent, bsn! { @surface() Node { row_gap: px(2) } });
    spawn_in(commands, card, bsn! { @overline("The scenarios") Node { margin: UiRect::bottom(px(SPACE_XS)) } });
    for (i, scenario) in SCENARIOS.iter().enumerate() {
        let cells = vec![
            cell(format!("{}", i + 1), 24.0, tokens::TEXT_DIM),
            cell(scenario.drill.name().to_string(), 140.0, TEXT_STRONG),
            cell(scenario.aim.name().to_string(), 150.0, tokens::TEXT_MAIN),
        ];
        spawn_in(commands, card, table_row(cells, i % 2 == 0));
    }
    let fixed = "Every setting is pinned: 60 s each, target size 1x, no decay, and each drill's own parameters as they were when the benchmark was made. Your run settings don't apply, so benchmarks stay comparable. Only your sensitivity is your own; it is logged with each benchmark. Benchmark runs are kept apart from your practice scores.";
    spawn_in(commands, card, bsn! { @details("What is fixed", fixed) Node { margin: UiRect::top(px(SPACE_XS)) } });
}

pub(super) fn start_benchmark(_: On<Activate>, mut commands: Commands, mut next: ResMut<NextState<AppState>>) {
    start_routine(&mut commands, &mut next, benchmark::routine());
}
