//! History page: the drills on the left, and the picked one's trend numbers, progress chart
//! and recent runs with their replays on the right.

use std::path::Path;

use bevy::feathers::constants::fonts;
use bevy::feathers::font_styles::InheritableFont;
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::play::PlaySelection;
use super::{CONTENT_W, MenuTab, page};
use crate::AppState;
use crate::drill::DrillKind;
use crate::drill::stats::HistoryStat;
use crate::history::{History, HistoryEntry, RECENT_RUNS, format_date, overall_change, recent_change};
use crate::replay::{Replay, ReplayDir, watch};
use crate::ui::charts::*;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Most recent runs drawn in the chart.
const CHART_RUNS: usize = 40;
/// Most recent runs listed under the chart.
const LISTED_RUNS: usize = 6;
/// Width of the drill list.
const LIST_W: f32 = 250.0;
/// Width of the cards right of it, also while the page's scrollbar shows.
const BODY_W: f32 = CONTENT_W - LIST_W - SPACE_L - SCROLLBAR_GUTTER;
/// Width of the y-axis labels left of the plot.
const AXIS_W: f32 = 40.0;
const CHART_W: f32 = BODY_W - 2.0 * SPACE_L - AXIS_W - SPACE_S;
const CHART_H: f32 = 92.0;

/// The drill (by name) shown on the History page; `None` until one is picked, which shows the
/// drill played most recently.
#[derive(Resource, Clone, Default, PartialEq, Eq)]
pub struct HistoryView(Option<String>);

impl HistoryView {
    /// Shows this drill's history.
    pub(in crate::ui) fn show(&mut self, drill: &str) {
        self.0 = Some(drill.to_string());
    }

    /// The drill shown: the one picked, else the one played most recently (before any run, the
    /// first built-in drill).
    fn drill(&self, history: &History) -> String {
        self.0
            .clone()
            .or_else(|| played(history).into_iter().next().map(|(name, _)| name))
            .unwrap_or_else(|| DrillKind::default().name().to_string())
    }
}

/// The drills with practice runs and how many, the most recently played first.
fn played(history: &History) -> Vec<(String, usize)> {
    let mut played: Vec<(u64, String, usize)> = history
        .drills()
        .into_iter()
        .filter_map(|name| {
            let last = history.practice_runs(&name).next_back()?.timestamp;
            let runs = history.practice_runs(&name).count();
            Some((last, name, runs))
        })
        .collect();
    played.sort_by_key(|played| std::cmp::Reverse(played.0));
    played.into_iter().map(|(_, name, runs)| (name, runs)).collect()
}

/// Containers whose children are rebuilt whenever the picked drill changes.
#[derive(Component, Clone, Copy, Default)]
pub struct HistoryList;
#[derive(Component, Clone, Copy, Default)]
pub struct HistoryBody;

/// The History page; both columns scroll once many drills or runs outgrow the menu.
pub(super) fn history_page() -> impl Scene {
    bsn! {
        @page(MenuTab::History, bsn_list! {
            @scroll_column(bsn_list! {
                @surface()
                Node { padding: px(SPACE_S), row_gap: px(2) }
                HistoryList
            })
            Node { width: px(LIST_W), flex_shrink: 0.0 }
            --
            @scroll_column(bsn_list! { @column() Node { row_gap: px(SPACE_M) } HistoryBody })
            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
        })
    }
}

#[derive(Component, Clone, Default)]
struct HistoryDrillButton(String);

/// The folded list of built-in drills without runs.
#[derive(Component, Clone, Copy, Default)]
struct UnplayedList;

#[derive(Component, Clone, Copy, Default)]
struct ReplayButton(u64);

/// One list entry per drill with history (built-ins always, custom ones once played), with its
/// number of runs; long custom names wrap.
fn drill_button(name: String, runs: usize, selected: bool) -> impl Scene {
    let kind = if selected { Btn::Selected } else { Btn::Ghost };
    let marker = HistoryDrillButton(name.clone());
    let (count, color) =
        if runs == 0 { ("-".to_string(), TEXT_DISABLED) } else { (runs.to_string(), tokens::TEXT_DIM) };
    bsn! {
        @button_box(kind, 30.0)
        Node {
            height: Val::Auto,
            min_height: px(30),
            justify_content: JustifyContent::FlexStart,
            padding: UiRect::axes(px(SPACE_M), px(5)),
        }
        Children [
            @caption(name)
            Node { flex_grow: 1.0, flex_basis: px(0) }
            --
            @styled(count, fonts::REGULAR, FONT_SMALL, color)
            TextFont { font_features: {tabular()} }
        ]
        marker
        on(|activate: On<Activate>, buttons: Query<&HistoryDrillButton>, mut view: ResMut<HistoryView>| {
            if let Ok(HistoryDrillButton(name)) = buttons.get(activate.event_target())
                && view.0.as_ref() != Some(name)
            {
                view.0 = Some(name.clone());
            }
        })
    }
}

pub fn rebuild_history(
    mut commands: Commands,
    view: Res<HistoryView>,
    history: Res<History>,
    replays: Res<ReplayDir>,
    lists: Query<Entity, With<HistoryList>>,
    bodies: Query<(Entity, Ref<HistoryBody>)>,
) {
    for (body, marker) in &bodies {
        if !view.is_changed() && !marker.is_added() {
            continue;
        }
        let drill = view.drill(&history);
        for list in &lists {
            commands.entity(list).despawn_related::<Children>();
            let header = spawn_in(
                &mut commands,
                list,
                bsn! { @row() Node { padding: UiRect::new(px(SPACE_M), px(SPACE_M), px(SPACE_XS), px(SPACE_XS)) } },
            );
            spawn_in(&mut commands, header, overline("Played, latest first"));
            spawn_in(&mut commands, header, spacer());
            spawn_in(&mut commands, header, overline("Runs"));
            let played = played(&history);
            if played.is_empty() {
                let note = "Nothing played yet.";
                spawn_in(
                    &mut commands,
                    list,
                    bsn! { @small(note) Node { margin: UiRect::axes(px(SPACE_M), px(SPACE_XS)) } },
                );
            }
            for (name, runs) in &played {
                spawn_in(&mut commands, list, drill_button(name.clone(), *runs, *name == drill));
            }
            // Drills without runs stay out of the way, folded, unless one of them is shown.
            let unplayed: Vec<String> =
                history.drills().into_iter().filter(|name| history.practice_runs(name).next().is_none()).collect();
            if unplayed.is_empty() {
                continue;
            }
            // Open only when one of them was picked.
            let open = view.0.as_ref().is_some_and(|picked| unplayed.contains(picked));
            let fold = format!("Not played yet ({})", unplayed.len());
            spawn_in(
                &mut commands,
                list,
                bsn! {
                    @button_box(Btn::Link, 28.0)
                    Node { justify_content: JustifyContent::FlexStart, padding: UiRect::horizontal(px(SPACE_M)), margin: UiRect::top(px(SPACE_XS)) }
                    InheritableFont { font_size: FontSize::Px(FONT_SMALL) }
                    Children [ @caption(fold) ]
                    on(toggle_unplayed)
                },
            );
            let display = if open { Display::Flex } else { Display::None };
            let folded = spawn_in(
                &mut commands,
                list,
                bsn! { @column() Node { row_gap: px(2), display: display } UnplayedList },
            );
            for name in unplayed {
                let selected = open && name == drill;
                spawn_in(&mut commands, folded, drill_button(name, 0, selected));
            }
        }

        commands.entity(body).despawn_related::<Children>();
        let runs: Vec<&HistoryEntry> = history.practice_runs(&drill).collect();
        if runs.is_empty() {
            let (title, text) = match &view.0 {
                Some(_) => (
                    format!("No runs of {drill} yet"),
                    "Play it once: its scores, trend and replays then show up here.",
                ),
                None => (
                    "No runs yet".to_string(),
                    "Play a drill or a routine: your scores, trend and replays show up here, the drills you played most recently first.",
                ),
            };
            let empty = spawn_in(&mut commands, body, empty_state(title, text));
            spawn_in(
                &mut commands,
                empty,
                bsn! {
                    @button("Go to Play", Btn::Primary)
                    Node { align_self: AlignSelf::FlexStart, margin: UiRect::top(px(SPACE_M)) }
                    on(open_play)
                },
            );
            continue;
        }

        spawn_in(&mut commands, body, subheading(drill.clone()));
        spawn_summary(&mut commands, body, &history, &drill);
        let chart = spawn_in(&mut commands, body, surface());
        spawn_in(&mut commands, chart, overline("Progress"));
        spawn_chart(&mut commands, chart, &runs[runs.len().saturating_sub(CHART_RUNS)..]);
        let list = spawn_in(&mut commands, body, bsn! { @surface() Node { row_gap: px(2) } });
        spawn_run_list(&mut commands, list, &runs, &replays.0);
    }
}

/// Folds the list of drills without runs in or out.
fn toggle_unplayed(_: On<Activate>, mut lists: Query<&mut Node, With<UnplayedList>>) {
    for mut node in &mut lists {
        node.display = if node.display == Display::None { Display::Flex } else { Display::None };
    }
}

/// Opens the Play page on the drill shown here, if it is a built-in one.
fn open_play(
    _: On<Activate>,
    view: Res<HistoryView>,
    history: Res<History>,
    mut tab: ResMut<MenuTab>,
    mut selection: ResMut<PlaySelection>,
) {
    if let Some(kind) = DrillKind::from_name(&view.drill(&history)) {
        *selection = PlaySelection::Drill(kind);
    }
    *tab = MenuTab::Play;
}

/// Run count, personal best, recent average and trends as a row of tiles.
fn spawn_summary(commands: &mut Commands, parent: Entity, history: &History, drill: &str) {
    let scores = history.scores(drill);
    let runs: Vec<_> = history.practice_runs(drill).collect();
    let (best_index, best) = scores.iter().copied().enumerate().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or_default();
    let recent = &scores[scores.len().saturating_sub(RECENT_RUNS)..];
    let average = recent.iter().sum::<f32>() / recent.len() as f32;

    let tiles = spawn_in(commands, parent, bsn! { @row() Node { align_items: AlignItems::Stretch } });
    let set_on = format!("Set on {}", format_date(runs[best_index].timestamp));
    spawn_in(commands, tiles, stat_note("Personal best", score_text(best), GOLD, set_on));
    let (last, count) = match scores.len() {
        1 => ("Your only run".to_string(), "1 run".to_string()),
        runs => (format!("Avg of last {}", recent.len()), format!("{runs} runs")),
    };
    spawn_in(commands, tiles, stat_note(last, score_text(average), TEXT_STRONG, count));
    if let Some(change) = recent_change(&scores) {
        let note = format!("vs the {RECENT_RUNS} runs before");
        spawn_in(commands, tiles, stat_note("Lately", format!("{change:+.1}%"), delta_color(change), note));
    }
    if let Some(change) = overall_change(&scores) {
        let note = "first runs to latest".to_string();
        spawn_in(commands, tiles, stat_note("Since you started", format!("{change:+.0}%"), delta_color(change), note));
    }
}

/// A stat tile with a colored value and a note under it.
fn stat_note(caption: impl Into<String>, value: String, color: ThemeToken, note: String) -> impl Scene {
    bsn! {
        @tile()
        Node { flex_grow: 1.0, flex_basis: px(0) }
        ThemeBackgroundColor(SURFACE_CARD)
        Children [
            @overline(caption)
            --
            @styled(value, fonts::BOLD, FONT_STAT, color)
            --
            @small(note)
        ]
    }
}

/// Bar per run (PB highlighted) between its lowest and highest score.
fn spawn_chart(commands: &mut Commands, parent: Entity, runs: &[&HistoryEntry]) {
    let scores: Vec<f32> = runs.iter().map(|r| r.score).collect();
    let (lo, hi) = score_range(&scores);

    let chart = commands.spawn((ChildOf(parent), Node { column_gap: px(SPACE_S), ..default() })).id();
    let labels = commands
        .spawn((
            ChildOf(chart),
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
        text(commands, labels, format!("{value:.decimals$}"), FONT_MICRO, tokens::TEXT_DIM, Node::default());
    }
    let plot = plot(commands, chart, CHART_W, CHART_H);
    for fraction in [0.25, 0.5, 0.75] {
        commands.spawn((ChildOf(plot), absolute(0.0, CHART_H * fraction, CHART_W, 1.0), ThemeBackgroundColor(DIVIDER)));
    }
    score_bars(commands, plot, &scores, CHART_W, CHART_H);

    let legend = spawn_in(
        commands,
        parent,
        bsn! { @row() Node { margin: UiRect::left(px(AXIS_W + SPACE_S)), column_gap: px(SPACE_S) } },
    );
    let caption = if scores.len() == 1 {
        "Your first run.".to_string()
    } else {
        format!("Last {} runs, oldest to newest.", scores.len())
    };
    spawn_in(commands, legend, small(caption));
    spawn_in(commands, legend, legend_item(GOLD, "personal best"));
}

fn spawn_run_list(commands: &mut Commands, parent: Entity, runs: &[&HistoryEntry], replays: &Path) {
    let kind = runs.last().map_or(DrillKind::default(), |run| run.kind);
    let columns = kind.def().score.history_columns();
    const NUMBER_W: f32 = 44.0;
    const DATE_W: f32 = 100.0;
    const VALUE_W: f32 = 72.0;

    let total = runs.len();
    let title = if total > LISTED_RUNS {
        format!("Recent runs  ·  latest {LISTED_RUNS} of {total}")
    } else {
        "Recent runs".to_string()
    };
    spawn_in(commands, parent, bsn! { @overline(title) Node { margin: UiRect::bottom(px(SPACE_XS)) } });
    let mut header = vec![header_cell("#", NUMBER_W), header_cell("Date", DATE_W)];
    let headers = columns.iter().map(|(header, _)| header);
    header.extend(["Score"].iter().chain(headers).chain(&["Sens", "cm/360"]).map(|h| header_cell(h, VALUE_W)));
    spawn_in(commands, parent, table_header(header));

    let best = runs.iter().map(|r| r.score).fold(f32::MIN, f32::max);
    for (age, run) in runs.iter().rev().take(LISTED_RUNS).enumerate() {
        let mut cells = vec![
            cell(format!("#{}", total - age), NUMBER_W, tokens::TEXT_DIM),
            cell(format_date(run.timestamp), DATE_W, tokens::TEXT_MAIN),
            cell(score_text(run.score), VALUE_W, if run.score >= best { GOLD } else { TEXT_STRONG }),
        ];
        for &(_, stat) in columns {
            let value = match stat {
                HistoryStat::OnTarget => format!("{:.1}%", run.on_target_pct),
                // A tracking run from before fire had to be held has no rounds to be accurate with.
                HistoryStat::Accuracy if run.shots == 0 && run.kind.def().score.held_fire() => "-".to_string(),
                HistoryStat::Accuracy => format!("{:.1}%", run.accuracy * 100.0),
                HistoryStat::KillMs => format!("{:.0}", run.avg_kill_ms),
            };
            cells.push(cell(value, VALUE_W, tokens::TEXT_MAIN));
        }
        cells.push(cell(format!("{:.3}", run.sens), VALUE_W, tokens::TEXT_MAIN));
        cells.push(cell(format!("{:.1}", run.cm_per_360), VALUE_W, tokens::TEXT_MAIN));
        let entry = commands.spawn_scene(table_row(cells, age % 2 == 0)).insert(ChildOf(parent)).id();

        let timestamp = run.timestamp;
        if Replay::exists(replays, timestamp) {
            commands
                .spawn_scene(bsn! {
                    @compact_button("Replay", Btn::Secondary)
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
    replays: Res<ReplayDir>,
) {
    let Ok(&ReplayButton(timestamp)) = buttons.get(activate.event_target()) else { return };
    match Replay::load(&replays.0, timestamp) {
        Some(replay) => watch(&mut commands, &mut next_state, replay, AppState::Menu),
        None => warn!("Replay {timestamp} could not be loaded"),
    }
}
