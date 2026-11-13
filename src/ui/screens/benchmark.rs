//! The benchmark's result, shown instead of the routine summary when a benchmark routine ends:
//! the index with its takeaway, every scenario against the previous benchmark and the player's
//! best, the comparable physical measures, and how each kind of aim moved.

use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::AppState;
use crate::routine::{Routine, RoutineOrigin};
use crate::training::AimKind;
use crate::training::benchmark::{
    self, Benchmark, BenchmarkLog, Measure, SCENARIOS, STEADY_PCT, ScenarioResult, Trend, bests_before, kind_moves,
};
use crate::ui::menu::MenuTab;
use crate::ui::screens::score::screen_title;
use crate::ui::theme::*;
use crate::ui::widgets::*;

const PANEL_W: f32 = 820.0;
const NAME_W: f32 = 180.0;
const KIND_W: f32 = 160.0;
const VALUE_W: f32 = 96.0;
const MEASURE_W: f32 = 150.0;

/// The tone of a takeaway.
pub fn trend_color(trend: Trend) -> ThemeToken {
    match trend {
        Trend::Baseline | Trend::Steady => TEXT_ACCENT,
        Trend::Up => TEXT_GOOD,
        Trend::Down => TEXT_BAD,
    }
}

/// A change in percent, colored only when it is beyond what one run to the next varies by (as
/// shown, in whole percent).
pub fn move_color(percent: f32) -> ThemeToken {
    if percent.round().abs() < STEADY_PCT { TEXT_STRONG } else { delta_color(percent) }
}

/// A scenario's physical measure, e.g. `4.3 bits/s` or `61% on target`.
pub fn measure_text(measure: Measure, result: &ScenarioResult) -> String {
    match (measure, result.throughput, result.on_target) {
        (Measure::Throughput, Some(bits), _) => format!("{bits:.1} bits/s"),
        (Measure::OnTarget, _, Some(share)) => format!("{:.0}% on target", share * 100.0),
        _ => "-".to_string(),
    }
}

/// What the index means, for the `details` under it.
pub const INDEX_EXPLAINED: &str = "Each scenario's score is divided by its score in your first benchmark. The index is the geometric mean of those ratios after leaving out the highest and the lowest, times 100, so one fluke scenario can't move it on its own. 100 is your first benchmark; 110 means about 10% better across the board. Scores of different scenarios can't be compared with each other; bits/s (how much flick information you handle per second) and time on target can.";

pub fn spawn_benchmark_result(mut commands: Commands, routine: Res<Routine>, log: Res<BenchmarkLog>) {
    if !routine.finished() || routine.origin != RoutineOrigin::Benchmark {
        return;
    }
    let entries = log.entries();
    let latest = entries.last().filter(|_| routine.results.len() == SCENARIOS.len());
    let mut entity = match latest {
        Some(latest) => commands.spawn_scene(result_panel(entries, entries.len() - 1, latest)),
        None => commands.spawn_scene(bsn! {
            @overlay()
            Children [
                @panel()
                Node { width: px(560) }
                Children [
                    @screen_title("Benchmark", "Not recorded")
                    --
                    @label("A scenario was skipped, so this was not recorded as a benchmark: the index only means something when every scenario is played. The runs you played are in your history.")
                    --
                    @actions(bsn_list! { @button("Done", Btn::Primary) on(done) })
                ]
            ]
        }),
    };
    entity.insert(DespawnOnExit(AppState::Routine));
}

fn result_panel(log: &[Benchmark], at: usize, latest: &Benchmark) -> impl Scene {
    let first = &log[0];
    let index = benchmark::index(first, latest);
    let (trend, title, text) = benchmark::takeaway(log, at);
    let context = format!("Benchmark {}", at + 1);
    let headline = format!("Index {index:.0}");
    let tone = trend_color(trend);
    let previous = at.checked_sub(1).map(|i| &log[i]);
    let bests = bests_before(log, at);

    let header = vec![
        header_cell("Scenario", NAME_W),
        header_cell("Kind", KIND_W),
        header_cell("Score", VALUE_W),
        header_cell("vs last", VALUE_W),
        header_cell("vs your best", VALUE_W),
        header_cell("Comparable measure", MEASURE_W),
    ];
    let change = |score: f32, reference: Option<f32>| match reference.filter(|r| *r > 0.0) {
        Some(r) => {
            let percent = (score / r - 1.0) * 100.0;
            cell(format!("{percent:+.0}%"), VALUE_W, move_color(percent))
        },
        None => cell("-".to_string(), VALUE_W, tokens::TEXT_DIM),
    };
    let rows: Vec<_> = SCENARIOS
        .iter()
        .zip(&latest.results)
        .enumerate()
        .map(|(i, (scenario, result))| {
            let best = bests[i];
            let score_color = if best.is_some_and(|b| result.score > b) { GOLD } else { TEXT_STRONG };
            table_row(
                vec![
                    cell(scenario.drill.name().to_string(), NAME_W, TEXT_STRONG),
                    cell(scenario.aim.name().to_string(), KIND_W, tokens::TEXT_MAIN),
                    cell(score_text(result.score), VALUE_W, score_color),
                    change(result.score, previous.map(|p| p.results[i].score)),
                    change(result.score, best),
                    cell(measure_text(scenario.measure, result), MEASURE_W, tokens::TEXT_MAIN),
                ],
                i % 2 == 0,
            )
        })
        .collect();
    let moves = kind_moves(log, at);
    let kinds: Vec<_> = moves.iter().map(|&(aim, pct)| kind_tile(aim, pct)).collect();
    let kinds_title: Vec<_> =
        (!moves.is_empty()).then(|| overline("Kinds of aim since last time, most gained first")).into_iter().collect();
    let sens_note: Vec<_> =
        previous.and_then(|p| sens_change(p.cm_per_360, latest.cm_per_360)).map(small).into_iter().collect();
    bsn! {
        @overlay()
        Children [
            @panel()
            Node { width: px(PANEL_W) }
            Children [
                @screen_title(context, headline)
                --
                @callout(tone, title, text)
                --
                @column()
                Node { row_gap: px(2) }
                Children [ @table_header(header) -- {rows} ]
                --
                @column()
                Children [
                    {kinds_title}
                    --
                    @row()
                    Node { align_items: AlignItems::Stretch, column_gap: px(SPACE_S) }
                    Children [ {kinds} ]
                ]
                --
                {sens_note}
                --
                @details("How the index works", INDEX_EXPLAINED)
                --
                @actions(bsn_list! { @button("Done", Btn::Primary) on(done) })
            ]
        ]
    }
}

/// A kind of aim and how it moved.
fn kind_tile(aim: AimKind, percent: f32) -> impl Scene {
    stat_colored(aim.name(), format!("{percent:+.0}%"), move_color(percent))
}

/// A note when the sensitivity changed between two benchmarks by more than 2%.
pub fn sens_change(before: f32, now: f32) -> Option<String> {
    ((now / before - 1.0).abs() > 0.02).then(|| {
        format!(
            "Your sensitivity changed since the last benchmark ({before:.1} → {now:.1} cm/360): part of any change may come from that."
        )
    })
}

/// Back to the menu's Benchmark page.
fn done(_: On<Activate>, mut commands: Commands, mut tab: ResMut<MenuTab>, mut next: ResMut<NextState<AppState>>) {
    commands.remove_resource::<Routine>();
    *tab = MenuTab::Benchmark;
    next.set(AppState::Menu);
}
