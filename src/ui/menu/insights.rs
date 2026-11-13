//! Insights tab: flick weak spots found in your recent replays (see [`crate::analysis`]), with
//! one-click adaptive training for each.

use bevy::feathers::constants::fonts;
use bevy::feathers::theme::{ThemeBackgroundColor, UiTheme};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::play::PlaySelection;
use super::{CONTENT_W, MenuTab, page};
use crate::AppState;
use crate::analysis::{
    self, Analysis, Cell, MIN_FLICKS, MIN_SAMPLES, RECENT_REPLAYS, SECTORS, TRAINING_SECS, band_label,
};
use crate::custom::CustomContent;
use crate::drill::{DrillConfig, DrillKind, DrillSpec, Run, RunPurpose, VARIANT_SEPARATOR, begin_run};
use crate::metrics::quest::{QuestMode, TARGET_HIT};
use crate::replay::{Replay, ReplayDir};
use crate::settings::Settings;
use crate::ui::charts::*;
use crate::ui::theme::*;
use crate::ui::widgets::*;

const TILE_W: f32 = 78.0;
const TILE_H: f32 = 54.0;
const TILE_GAP: f32 = 6.0;
/// Width of the left column: the direction wheel in a card.
const LEFT_W: f32 = 3.0 * TILE_W + 2.0 * TILE_GAP + 2.0 * SPACE_M;
/// Width of a value column of the distance table: nine of them and the labels fill the right
/// column, also while the page's scrollbar shows.
const VALUE_W: f32 = (CONTENT_W - SCROLLBAR_GUTTER - LEFT_W - SPACE_L - 2.0 * SPACE_L - LABEL_W - 12.0) / 9.0;
const LABEL_W: f32 = 66.0;
/// Sectors laid out as on screen, row by row; `None` is the middle.
const WHEEL: [[Option<usize>; 3]; 3] =
    [[Some(3), Some(2), Some(1)], [Some(4), None, Some(0)], [Some(5), Some(6), Some(7)]];
/// Weakness score shown with full color.
const FULL_COLOR_SCORE: f32 = 0.8;

/// Container whose children are rebuilt, with a fresh analysis, whenever the page is opened.
#[derive(Component, Clone, Copy, Default)]
pub struct InsightsBody;

#[derive(Component, Clone, Copy, Default)]
struct TrainButton(Cell);

pub(super) fn insights_page() -> impl Scene {
    bsn! {
        @page(MenuTab::Insights, bsn_list! {
            @scroll_column(bsn_list! {
                Node { flex_direction: FlexDirection::Row, column_gap: px(SPACE_L), align_items: AlignItems::Start }
                InsightsBody
            })
            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
        })
    }
}

/// Re-analyses your newest flick-drill replays when the Insights page is opened (also after a
/// run, as the menu is rebuilt). Runs inline: at most `RECENT_REPLAYS` replays are decoded.
pub fn rebuild_insights(
    mut commands: Commands,
    replays: Res<ReplayDir>,
    theme: Res<UiTheme>,
    bodies: Query<Entity, Added<InsightsBody>>,
) {
    for body in &bodies {
        let analysis = analysis::analyze(&Replay::recent(&replays.0, &analysis::flick_kinds(), RECENT_REPLAYS));
        if analysis.enough_data() {
            spawn_insights(&mut commands, body, &analysis, &theme);
        } else {
            spawn_empty(&mut commands, body, &analysis);
        }
    }
}

/// Not enough flicks yet: how many there are, how many it takes and where to get them.
fn spawn_empty(commands: &mut Commands, body: Entity, analysis: &Analysis) {
    let drills: Vec<&str> = analysis::flick_kinds().into_iter().map(DrillKind::name).collect();
    let text = format!(
        "Insights look at the flicks in your newest {RECENT_REPLAYS} replays of the flick drills ({}) and need at least {MIN_FLICKS} of them.",
        drills.join(", ")
    );
    let card = spawn_in(commands, body, bsn! { @empty_state("Not enough flicks yet", text) Node { flex_grow: 1.0 } });
    let found = format!("{} of {MIN_FLICKS} flicks so far, from {} runs", analysis.flicks, analysis.replays);
    spawn_in(commands, card, bsn! { @label_strong(found) Node { margin: UiRect::top(px(SPACE_S)) } });
    let track = commands
        .spawn((
            ChildOf(card),
            Node { width: px(320), height: px(6), border_radius: BorderRadius::all(px(3)), ..default() },
            ThemeBackgroundColor(DIVIDER),
        ))
        .id();
    let share = (analysis.flicks as f32 / MIN_FLICKS as f32).min(1.0);
    commands.spawn((
        ChildOf(track),
        Node {
            width: percent(share * 100.0),
            height: percent(100),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        ThemeBackgroundColor(ACCENT),
    ));
    spawn_in(
        commands,
        card,
        bsn! {
            @button("Play Microflick", Btn::Primary)
            Node { align_self: AlignSelf::FlexStart, margin: UiRect::top(px(SPACE_M)) }
            on(|_: On<Activate>, mut tab: ResMut<MenuTab>, mut selection: ResMut<PlaySelection>| {
                *selection = PlaySelection::Drill(DrillKind::Microflick);
                *tab = MenuTab::Play;
            })
        },
    );
}

fn stack(commands: &mut Commands, parent: Entity, node: Node) -> Entity {
    let node = Node { flex_direction: FlexDirection::Column, row_gap: px(SPACE_L), ..node };
    commands.spawn((ChildOf(parent), node)).id()
}

fn section(commands: &mut Commands, parent: Entity, title: &str) -> Entity {
    let card = spawn_in(commands, parent, surface());
    spawn_in(commands, card, overline(title));
    card
}

fn spawn_insights(commands: &mut Commands, body: Entity, analysis: &Analysis, theme: &UiTheme) {
    let left = stack(commands, body, Node { width: px(LEFT_W), flex_shrink: 0.0, ..default() });
    let wheel = spawn_in(commands, left, bsn! { @surface() Node { padding: px(SPACE_M) } });
    spawn_in(commands, wheel, overline("Kill time by direction"));
    spawn_wheel(commands, wheel, analysis, theme);
    let legend = spawn_in(commands, wheel, bsn! { @wrap_row() Node { column_gap: px(0), row_gap: px(2) } });
    for (color, meaning) in [(HEAT_WEAK, "slower"), (HEAT_STRONG, "faster"), (HEAT_EMPTY, "too few flicks")] {
        spawn_in(commands, legend, legend_item(color, meaning));
    }
    let caption = format!(
        "Than your average for that distance. Under it: flicks, and the throughput of your first clicks (b/s). Fewer than {MIN_SAMPLES} flicks aren't rated."
    );
    spawn_in(commands, wheel, details("How to read the wheel", caption));
    let habits = section(commands, left, "Tendencies");
    for line in &analysis.tendencies {
        spawn_in(commands, habits, label(line.clone()));
    }

    let right = stack(commands, body, Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), ..default() });
    spawn_findings(commands, right, analysis);
    spawn_band_table(commands, right, analysis);
}

/// Background of a sector tile: red for weaker than your average, green for stronger.
fn weakness_color(theme: &UiTheme, score: f32) -> Color {
    let neutral = theme.color(&HEAT_NEUTRAL);
    let t = (score / FULL_COLOR_SCORE).clamp(-1.0, 1.0);
    if t >= 0.0 { neutral.mix(&theme.color(&HEAT_WEAK), t) } else { neutral.mix(&theme.color(&HEAT_STRONG), -t) }
}

/// The 8 directions around the crosshair, each colored by its weakness score and showing its kill
/// time versus your average and its throughput.
fn spawn_wheel(commands: &mut Commands, parent: Entity, analysis: &Analysis, theme: &UiTheme) {
    let grid = commands
        .spawn((ChildOf(parent), Node { flex_direction: FlexDirection::Column, row_gap: px(TILE_GAP), ..default() }))
        .id();
    for row in WHEEL {
        let line = commands.spawn((ChildOf(grid), Node { column_gap: px(TILE_GAP), ..default() })).id();
        for slot in row {
            let tile = Node {
                width: px(TILE_W),
                height: px(TILE_H),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(RADIUS_S)),
                ..default()
            };
            let Some(sector) = slot else {
                let middle = commands.spawn((ChildOf(line), tile)).id();
                spawn_in(commands, middle, overline("Average"));
                let typical = analysis.overall.ttk.map_or("-".into(), |t| format!("{:.0} ms", t * 1000.0));
                spawn_in(commands, middle, styled(typical, fonts::BOLD, FONT_BODY, TEXT_STRONG));
                text(commands, middle, "median kill".into(), FONT_MICRO, tokens::TEXT_DIM, Node::default());
                continue;
            };
            let stats = &analysis.sectors[sector];
            let note = match stats.throughput {
                Some(tp) => format!("{} · {tp:.1} b/s", stats.n),
                None => format!("{} flicks", stats.n),
            };
            let (background, value) = match stats.score {
                Some(score) => (
                    weakness_color(theme, score),
                    stats.ttk_vs(&analysis.overall).map_or("-".into(), |ms| format!("{ms:+.0} ms")),
                ),
                None => (theme.color(&HEAT_EMPTY), "-".into()),
            };
            let tile = commands.spawn((ChildOf(line), tile, BackgroundColor(background))).id();
            spawn_in(commands, tile, overline_colored(SECTORS[sector], TEXT_STRONG));
            spawn_in(commands, tile, styled(value, fonts::BOLD, FONT_BODY, TEXT_STRONG));
            text(commands, tile, note, FONT_MICRO, TEXT_STRONG, Node::default());
        }
    }
}

/// The top weak spots, each with a button that trains it; the first is the one to start with.
fn spawn_findings(commands: &mut Commands, parent: Entity, analysis: &Analysis) {
    let title = format!("Weak spots  ·  {} flicks from {} runs", analysis.flicks, analysis.replays);
    let list = section(commands, parent, &title);
    if analysis.findings.is_empty() {
        let even = "No direction or distance stands out from your average. Keep playing: this updates with every run.";
        spawn_in(commands, list, callout(TEXT_GOOD, "No clear weak spot", even));
        return;
    }
    for (rank, finding) in analysis.findings.iter().enumerate() {
        let title = format!("{}  ·  {} flicks", finding.title, finding.n);
        let detail = finding.detail.clone();
        let spot = finding.cell;
        let (tone, kind) = if rank == 0 { (TEXT_WARN, Btn::Primary) } else { (BORDER_STRONG, Btn::Secondary) };
        spawn_in(
            commands,
            list,
            bsn! {
                @row()
                Node {
                    column_gap: px(SPACE_M),
                    padding: UiRect::axes(px(SPACE_M), px(SPACE_S)),
                    border_radius: BorderRadius::all(px(RADIUS_S)),
                }
                ThemeBackgroundColor(SURFACE_TILE)
                Children [
                    @marker(tone)
                    --
                    @column()
                    Node { flex_grow: 1.0, flex_basis: px(0), row_gap: px(2) }
                    Children [ @label_strong(title) -- @label_dim(detail) ]
                    --
                    @compact_button("Train this", kind)
                    Node { flex_shrink: 0.0 }
                    TrainButton(spot)
                    on(train)
                ]
            },
        );
    }
    let note = format!(
        "Train this plays a {TRAINING_SECS:.0} s adaptive Microflick of just those flicks: target sizes adapt so you hit about {:.0}% in time, and the result shows the size you reached (θ75). It's saved on the Custom page, and its History shows your progress.",
        TARGET_HIT * 100.0
    );
    spawn_in(commands, list, details("What Train this does", note));
}

/// One row per distance band.
fn spawn_band_table(commands: &mut Commands, parent: Entity, analysis: &Analysis) {
    let table = section(commands, parent, "By flick distance");
    let rows = commands
        .spawn((ChildOf(table), Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() }))
        .id();
    let headers = ["Flicks", "Landing", "Off-line", "Bow", "Corr.", "Kill", "vs avg", "Miss", "TPe"];
    let mut header = vec![header_cell("Distance", LABEL_W)];
    header.extend(headers.iter().map(|h| header_cell(h, VALUE_W)));
    spawn_in(commands, rows, table_header(header));

    let percent = |v: Option<f32>| v.map_or("-".into(), |v| format!("{:.0}%", v * 100.0));
    for (band, stats) in analysis.bands.iter().enumerate() {
        let scored = stats.n >= MIN_SAMPLES;
        let main = if scored { tokens::TEXT_MAIN } else { tokens::TEXT_DIM };
        let versus = stats.ttk_vs(&analysis.overall);
        let versus_color = match versus {
            Some(ms) if scored && ms >= 15.0 => TEXT_BAD,
            Some(ms) if scored && ms <= -15.0 => TEXT_GOOD,
            _ => main.clone(),
        };
        let cells = vec![
            cell(band_label(band), LABEL_W, TEXT_STRONG),
            cell(stats.n.to_string(), VALUE_W, main.clone()),
            cell(percent(stats.landing), VALUE_W, main.clone()),
            cell(percent(stats.off_axis), VALUE_W, main.clone()),
            cell(percent(stats.bow), VALUE_W, main.clone()),
            cell(stats.corrections.map_or("-".into(), |c| format!("{c:.1}")), VALUE_W, main.clone()),
            cell(stats.ttk.map_or("-".into(), |t| format!("{:.0} ms", t * 1000.0)), VALUE_W, main.clone()),
            cell(versus.map_or("-".into(), |ms| format!("{ms:+.0} ms")), VALUE_W, versus_color),
            cell(percent(stats.error_rate), VALUE_W, main.clone()),
            cell(stats.throughput.map_or("-".into(), |tp| format!("{tp:.1}")), VALUE_W, main),
        ];
        commands.spawn_scene(table_row(cells, band % 2 == 0)).insert(ChildOf(rows));
    }
    spawn_in(
        commands,
        table,
        small("vs avg: kill time against your average for that distance. Green is faster, red slower."),
    );
    let legend = "Landing: how far your first flick travels (100% = on target). Off-line: its sideways error. Bow: path curve. Corr.: corrective moves per kill. Kill: median time to kill. Miss: missed shots and expired targets per attempt. TPe: effective throughput of your first clicks, bits/s.";
    spawn_in(commands, table, details("What the columns mean", legend));
}

/// Creates (or updates) the weak spot's adaptive training scenario (Train mode, so targets stay
/// at the size you hit 75% of the time) and starts it, like playing a custom scenario from the
/// Custom tab. Its own name keeps its history apart from earlier fixed-size weak-spot runs.
fn train(
    activate: On<Activate>,
    buttons: Query<&TrainButton>,
    settings: Res<Settings>,
    mut content: ResMut<CustomContent>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Ok(&TrainButton(spot)) = buttons.get(activate.event_target()) else { return };
    let mut config = spot.training_config(DrillConfig::builtin(DrillKind::Microflick, &settings));
    config.adaptive = Some(QuestMode::Train);
    let name = format!("{}{VARIANT_SEPARATOR}adaptive", spot.scenario_name());
    let id = content.upsert_scenario(&name, TRAINING_SECS, config);
    let Some(scenario) = content.scenario(id) else { return };
    let drill = DrillSpec { name: scenario.name.clone(), config: scenario.config.clone() };
    let run = Run::new(drill, settings.sens, scenario.duration, RunPurpose::Practice, scenario.name.clone());
    begin_run(&mut commands, &mut next_state, run);
}
