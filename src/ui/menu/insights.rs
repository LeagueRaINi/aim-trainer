//! Insights tab: flick weak spots found in your recent replays (see [`crate::analysis`]), with
//! one-click training for each.

use bevy::{
    feathers::{constants::fonts, controls::ButtonVariant, theme::ThemeBackgroundColor, tokens},
    prelude::*,
    ui_widgets::Activate,
};

use super::MenuTab;
use crate::ui::{charts::*, theme::*, widgets::*};
use crate::{
    AppState,
    analysis::{
        self, Analysis, Cell, MIN_FLICKS, MIN_SAMPLES, RECENT_REPLAYS, SECTORS, TRAINING_SECS,
        band_label,
    },
    custom::CustomContent,
    replay::Replay,
    drill::{DrillConfig, DrillSpec, Run, RunPurpose, DrillKind, begin_run},
    settings::Settings,
};

/// Width of the left column (direction wheel and tendencies).
const LEFT_W: f32 = 312.0;
const TILE_W: f32 = 88.0;
const TILE_H: f32 = 56.0;
const TILE_GAP: f32 = 6.0;
/// Sectors laid out as on screen, row by row; `None` is the middle.
const WHEEL: [[Option<usize>; 3]; 3] = [[Some(3), Some(2), Some(1)], [Some(4), None, Some(0)], [Some(5), Some(6), Some(7)]];
/// Weakness score shown with full color.
const FULL_COLOR_SCORE: f32 = 0.8;

/// Container whose children are rebuilt, with a fresh analysis, whenever the tab is opened.
#[derive(Component, Clone, Copy, Default)]
pub struct InsightsBody;

#[derive(Component, Clone, Copy, Default)]
struct TrainButton(Cell);

/// Re-analyses your newest flick-drill replays when the Insights tab is opened (also after a run,
/// as the menu is rebuilt). Runs inline: at most `RECENT_REPLAYS` replays are decoded.
pub fn rebuild_insights(mut commands: Commands, tab: Res<MenuTab>, bodies: Query<(Entity, Ref<InsightsBody>)>) {
    if *tab != MenuTab::Insights {
        return;
    }
    for (body, marker) in &bodies {
        if !tab.is_changed() && !marker.is_added() {
            continue;
        }
        commands.entity(body).despawn_related::<Children>();
        let analysis = analysis::analyze(&Replay::recent(&analysis::flick_kinds(), RECENT_REPLAYS));
        if analysis.enough_data() {
            spawn_insights(&mut commands, body, &analysis);
        } else {
            spawn_empty(&mut commands, body, &analysis);
        }
    }
}

fn spawn_empty(commands: &mut Commands, body: Entity, analysis: &Analysis) {
    let card = commands.spawn_scene(bsn! { @surface() Node { flex_grow: 1.0 } }).insert(ChildOf(body)).id();
    commands.spawn_scene(subheading("Not enough flicks yet")).insert(ChildOf(card));
    let found = format!(
        "Play a few Microflick or Gridshot runs (Head level and Peripheral count too). Insights look at the flicks in your newest {RECENT_REPLAYS} replays and need at least {MIN_FLICKS}; found {} in {} runs so far.",
        analysis.flicks, analysis.replays
    );
    text(commands, card, found, 14.0, tokens::TEXT_DIM, Node::default());
}

fn stack(commands: &mut Commands, parent: Entity, node: Node) -> Entity {
    let node = Node { flex_direction: FlexDirection::Column, row_gap: px(SPACE_L), ..node };
    commands.spawn((ChildOf(parent), node)).id()
}

fn section(commands: &mut Commands, parent: Entity, title: &str) -> Entity {
    let card = commands.spawn_scene(surface()).insert(ChildOf(parent)).id();
    commands.spawn_scene(overline(title)).insert(ChildOf(card));
    card
}

fn spawn_insights(commands: &mut Commands, body: Entity, analysis: &Analysis) {
    let left = stack(commands, body, Node { width: px(LEFT_W), flex_shrink: 0.0, ..default() });
    let wheel = section(commands, left, "Kill time by direction");
    spawn_wheel(commands, wheel, analysis);
    let caption = format!(
        "vs your average for the distance. {} flicks from {} runs. Red = weaker, green = stronger, grey = under {MIN_SAMPLES} flicks.",
        analysis.flicks, analysis.replays
    );
    text(commands, wheel, caption, 11.0, tokens::TEXT_DIM, Node::default());
    let habits = section(commands, left, "Tendencies");
    for line in &analysis.tendencies {
        text(commands, habits, line.clone(), 13.0, tokens::TEXT_MAIN, Node::default());
    }

    let right = stack(commands, body, Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), ..default() });
    spawn_findings(commands, right, analysis);
    spawn_band_table(commands, right, analysis);
}

/// Background of a sector tile: red for weaker than your average, green for stronger.
fn weakness_color(score: f32) -> Color {
    let neutral = Color::srgb(0.24, 0.26, 0.3);
    let t = (score / FULL_COLOR_SCORE).clamp(-1.0, 1.0);
    if t >= 0.0 { neutral.mix(&Color::srgb(0.78, 0.24, 0.24), t) } else { neutral.mix(&Color::srgb(0.2, 0.58, 0.32), -t) }
}

/// The 8 directions around the crosshair, each colored by its weakness score and showing its kill
/// time versus your average.
fn spawn_wheel(commands: &mut Commands, parent: Entity, analysis: &Analysis) {
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
                border_radius: BorderRadius::all(px(6)),
                ..default()
            };
            let Some(sector) = slot else {
                let middle = commands.spawn((ChildOf(line), tile, ThemeBackgroundColor(tokens::GROUP_BG))).id();
                text(commands, middle, "AVERAGE".into(), 10.0, tokens::TEXT_DIM, Node::default());
                let typical = analysis.overall.ttk.map_or("-".into(), |t| format!("{:.0} ms", t * 1000.0));
                commands.spawn_scene(styled(typical, fonts::BOLD, 16.0, TEXT_STRONG)).insert(ChildOf(middle));
                text(commands, middle, "median kill".into(), 11.0, tokens::TEXT_DIM, Node::default());
                continue;
            };
            let stats = &analysis.sectors[sector];
            let (background, value, note) = match stats.score {
                Some(score) => (
                    weakness_color(score),
                    stats.ttk_vs(&analysis.overall).map_or("-".into(), |ms| format!("{ms:+.0} ms")),
                    format!("{} flicks", stats.n),
                ),
                None => (Color::srgb(0.17, 0.18, 0.2), "-".into(), format!("{} flicks", stats.n)),
            };
            let tile = commands.spawn((ChildOf(line), tile, BackgroundColor(background))).id();
            text(commands, tile, SECTORS[sector].to_uppercase(), 10.0, TEXT_STRONG, Node::default());
            commands.spawn_scene(styled(value, fonts::BOLD, 16.0, TEXT_STRONG)).insert(ChildOf(tile));
            text(commands, tile, note, 11.0, TEXT_STRONG, Node::default());
        }
    }
}

/// The top weak spots, each with a button that trains it.
fn spawn_findings(commands: &mut Commands, parent: Entity, analysis: &Analysis) {
    let list = section(commands, parent, "Weak spots");
    if analysis.findings.is_empty() {
        let even = "No clear weak spot: no direction or distance stands out from your average. Keep playing; this updates with every run.";
        text(commands, list, even.into(), 14.0, tokens::TEXT_DIM, Node::default());
        return;
    }
    for finding in &analysis.findings {
        let row = commands
            .spawn((ChildOf(list), Node { column_gap: px(SPACE_M), align_items: AlignItems::Center, ..default() }))
            .id();
        let words = commands
            .spawn((
                ChildOf(row),
                Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, flex_basis: px(0), row_gap: px(2), ..default() },
            ))
            .id();
        let title = format!("{}  ·  {} flicks", finding.title, finding.n);
        commands.spawn_scene(styled(title, fonts::BOLD, 14.0, TEXT_STRONG)).insert(ChildOf(words));
        text(commands, words, finding.detail.clone(), 13.0, tokens::TEXT_DIM, Node::default());
        let spot = finding.cell;
        commands
            .spawn_scene(bsn! {
                @compact_button("Train this", ButtonVariant::Primary)
                Node { flex_shrink: 0.0 }
                TrainButton(spot)
                on(train)
            })
            .insert(ChildOf(row));
    }
    let note = format!(
        "Train this plays a {TRAINING_SECS:.0} s Microflick of just those flicks. It's saved in the Custom tab, and its History shows your progress."
    );
    text(commands, list, note, 11.0, tokens::TEXT_DIM, Node::default());
}

/// One row per distance band.
fn spawn_band_table(commands: &mut Commands, parent: Entity, analysis: &Analysis) {
    const LABEL_W: f32 = 80.0;
    const VALUE_W: f32 = 66.0;
    let table = section(commands, parent, "By distance");
    let rows = commands
        .spawn((ChildOf(table), Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() }))
        .id();
    let headers = ["Flicks", "Landing", "Off-line", "Bow", "Corr.", "Kill", "vs avg", "Miss"];
    let mut header = vec![cell("Distance", LABEL_W, tokens::TEXT_DIM)];
    header.extend(headers.iter().map(|h| cell(*h, VALUE_W, tokens::TEXT_DIM)));
    commands.spawn_scene(table_row(header, false)).insert(ChildOf(rows));

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
            cell(percent(stats.error_rate), VALUE_W, main),
        ];
        commands.spawn_scene(table_row(cells, band % 2 == 0)).insert(ChildOf(rows));
    }
    let legend = "Landing: how far your first flick travels (100% = on target). Off-line: its sideways error. Bow: path curve. Corr.: corrective moves per kill. Kill: median time to kill.";
    text(commands, table, legend.into(), 11.0, tokens::TEXT_DIM, Node::default());
}

/// Creates (or updates) the weak spot's training scenario and starts it, like playing a custom
/// scenario from the Custom tab.
fn train(
    activate: On<Activate>,
    buttons: Query<&TrainButton>,
    settings: Res<Settings>,
    mut content: ResMut<CustomContent>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Ok(&TrainButton(spot)) = buttons.get(activate.event_target()) else { return };
    let config = spot.training_config(DrillConfig::builtin(DrillKind::Microflick, &settings));
    let id = content.upsert_scenario(&spot.scenario_name(), TRAINING_SECS, config);
    let Some(scenario) = content.scenario(id) else { return };
    let drill = DrillSpec { name: scenario.name.clone(), config: scenario.config.clone() };
    let run = Run::new(drill, settings.sens, scenario.duration, RunPurpose::Practice, scenario.name.clone());
    begin_run(&mut commands, &mut next_state, run);
}
