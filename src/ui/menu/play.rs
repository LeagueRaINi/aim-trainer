//! Play page: every warmup routine and built-in drill in a browser on the left, and the picked
//! one on the right with what it trains, your progress in it and its Play button.

use bevy::feathers::constants::fonts;
use bevy::feathers::theme::ThemeTextColor;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{CONTENT_W, MenuTab, page};
use crate::AppState;
use crate::drill::adaptive::MEASURE_SECS;
use crate::drill::def::{Category, DecayRule};
use crate::drill::stats::ScoreModel;
use crate::drill::{
    ADAPTIVE_TIME_LIMIT, DrillConfig, DrillKind, DrillSpec, Run, RunPurpose, VARIANT_SEPARATOR, begin_run,
};
use crate::history::{History, RECENT_RUNS, recent_change};
use crate::metrics::quest::{QuestMode, TARGET_HIT};
use crate::routine::{ROUTINES, Routine, start_routine};
use crate::settings::{SettingField, Settings};
use crate::ui::bindings::*;
use crate::ui::charts::*;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// What the Play page shows on the right. Kept across visits, so the drill you just played is
/// still the picked one.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaySelection {
    Drill(DrillKind),
    /// Index in [`ROUTINES`].
    Routine(usize),
}

impl Default for PlaySelection {
    fn default() -> Self {
        PlaySelection::Drill(DrillKind::default())
    }
}

/// A drill or routine in the browser.
#[derive(Component, Clone, Copy, Default)]
pub struct PlayTile(PlaySelection);

/// The note under a tile's name. A drill's (`Some`) is your best with the run settings as they
/// are, so it changes with them.
#[derive(Component, Clone, Copy, Default)]
pub struct TileNote(Option<DrillKind>);

/// Container whose children are rebuilt whenever another tile is picked.
#[derive(Component, Clone, Copy, Default)]
pub struct PlayDetail;

/// Container of the run settings, or of the note that a test ignores them; `Some(true)` while
/// it shows the note.
#[derive(Component, Clone, Copy, Default)]
pub struct RunSettingsHost(Option<bool>);

#[derive(Component, Clone, Copy, Default)]
struct AdaptiveButton(DrillKind, QuestMode);

/// Width of the picked item's column.
const DETAIL_W: f32 = 320.0;
/// Three tiles and the gaps between them fill the browser, also while its scrollbar shows: with
/// every group's tiles three to a row, all of them fit a 1280x720 window without scrolling.
const TILE_W: f32 = (CONTENT_W - SPACE_L - DETAIL_W - SCROLLBAR_GUTTER - 2.0 * SPACE_S) / 3.0 - 1.0;
/// A tile: its name over its note.
const TILE_H: f32 = 42.0;
/// Width of the charts and rows inside the detail card, also while its scrollbar shows.
const DETAIL_INNER_W: f32 = DETAIL_W - 2.0 * SPACE_L - SCROLLBAR_GUTTER;
/// Most recent runs drawn in the detail card's chart.
const CHART_RUNS: usize = 24;
const CHART_H: f32 = 34.0;

/// A built-in drill played on its own, with the global drill length.
fn practice_run(kind: DrillKind, settings: &Settings) -> Run {
    Run::new(
        DrillSpec::builtin(kind, settings),
        settings.sens,
        settings.run_secs,
        RunPurpose::Practice,
        kind.name().to_string(),
    )
}

/// A built-in drill in adaptive mode: Measure runs long enough for a solid estimate, Train for
/// the global drill length.
fn adaptive_run(kind: DrillKind, mode: QuestMode, settings: &Settings) -> Run {
    let drill = DrillSpec::adaptive(kind, mode, settings);
    let secs = match mode {
        QuestMode::Measure => MEASURE_SECS,
        QuestMode::Train => settings.run_secs,
    };
    let title = drill.name.clone();
    Run::new(drill, settings.sens, secs, RunPurpose::Practice, title)
}

/// The color that marks a group of drills.
fn group_color(category: Category) -> ThemeToken {
    match category {
        Category::Aim => GROUP_AIM,
        Category::Tactical => GROUP_TACTICAL,
        Category::Eyes => GROUP_EYES,
        Category::Tests => GROUP_TESTS,
    }
}

/// What a score counts, in a few words.
fn scoring(model: ScoreModel) -> &'static str {
    match model {
        ScoreModel::Flick => "kills per minute x accuracy",
        ScoreModel::Tracking => "the share of the run you are hitting the target, fire held",
        ScoreModel::Reaction => "reaction speed x accuracy",
        ScoreModel::Survival => "kills x accuracy, until your lives are gone",
        ScoreModel::Switch => "kills per minute, fire held on each target",
        ScoreModel::HeldFire => "kills per minute x share of your rounds that hit",
        ScoreModel::Taps => "kills per minute x kills per shot",
        ScoreModel::ReactionTest => "100 000 / your median reaction in ms (200 ms = 500)",
        ScoreModel::GoNoGo => "reaction speed x enemies hit x friendlies held",
        ScoreModel::Calm => "the mean of its four parts, each 0 to 100",
    }
}

/// A drill tile's note. A run is recorded, and compared, under the name its settings give it
/// (`Gridshot · size 0.75x`), so that is the history a tile shows: what pressing Play now would
/// be up against.
fn tile_note(kind: DrillKind, history: &History, settings: &Settings) -> (String, ThemeToken) {
    match history.compare(&DrillSpec::builtin(kind, settings).name).best {
        Some(best) => (format!("PB {}", score_text(best)), GOLD),
        None => ("Not played".to_string(), tokens::TEXT_DIM),
    }
}

pub(super) fn play_page(history: &History, settings: &Settings, selection: PlaySelection) -> impl Scene {
    let routines: Vec<_> = ROUTINES
        .iter()
        .enumerate()
        .map(|(index, plan)| {
            let item = PlaySelection::Routine(index);
            let note = format!("{} drills", plan.steps.len());
            play_tile(item, plan.name, GROUP_ROUTINE, note, tokens::TEXT_DIM, item == selection)
        })
        .collect();
    let mut groups = vec![group("Warmup routines", routines)];
    for category in Category::ALL {
        let tiles: Vec<_> = DrillKind::ALL
            .into_iter()
            .filter(|kind| kind.def().category == category)
            .map(|kind| {
                let item = PlaySelection::Drill(kind);
                let (note, color) = tile_note(kind, history, settings);
                play_tile(item, kind.name(), group_color(category), note, color, item == selection)
            })
            .collect();
        if !tiles.is_empty() {
            groups.push(group(category.title(), tiles));
        }
    }
    bsn! {
        @page(MenuTab::Play, bsn_list! {
            // More drills only add rows, and the browser scrolls once they outgrow the menu; the
            // picked drill and its Play button stay where they are.
            @scroll_column(bsn_list! { @column() Node { row_gap: px(SPACE_M) } Children [ {groups} ] })
            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
            --
            @scroll_column(bsn_list! {
                @surface()
                PlayDetail
                --
                @column()
                RunSettingsHost
            })
            Node { width: px(DETAIL_W), flex_shrink: 0.0 }
        })
    }
}

/// A titled group of the browser, its tiles three to a row.
fn group<T: Scene>(title: &'static str, tiles: Vec<T>) -> impl Scene {
    bsn! {
        @column()
        Node { row_gap: px(SPACE_XS) }
        Children [
            @overline(title)
            --
            @wrap_row()
            Node { column_gap: px(SPACE_S), row_gap: px(SPACE_S) }
            Children [ {tiles} ]
        ]
    }
}

/// A drill or routine in the browser: its group's color, its name and under it a note (your
/// best).
fn play_tile(
    item: PlaySelection,
    name: &'static str,
    color: ThemeToken,
    note: String,
    note_color: ThemeToken,
    picked: bool,
) -> impl Scene {
    let kind = tile_kind(picked);
    let drill = match item {
        PlaySelection::Drill(kind) => Some(kind),
        PlaySelection::Routine(_) => None,
    };
    bsn! {
        @button_box(kind, TILE_H)
        Node {
            width: px(TILE_W),
            justify_content: JustifyContent::FlexStart,
            padding: UiRect::new(px(SPACE_S), px(SPACE_S), px(SPACE_XS), px(SPACE_XS)),
            column_gap: px(SPACE_S),
        }
        Children [
            @marker(color)
            --
            @column()
            Node { row_gap: px(0), min_width: px(0) }
            Children [
                @label_strong(name)
                --
                @styled(note, fonts::BOLD, FONT_SMALL, note_color)
                TextFont { font_features: {tabular()} }
                TileNote(drill)
            ]
        ]
        PlayTile(item)
        on(pick_tile)
    }
}

fn tile_kind(picked: bool) -> Btn {
    if picked { Btn::Selected } else { Btn::Tile }
}

/// The settings every built-in drill is played with, as three compact fields.
fn run_settings(settings: &Settings) -> impl Scene {
    bsn! {
        @card("Run settings", bsn_list! {
            @row()
            Node { column_gap: px(SPACE_S), align_items: AlignItems::FlexStart }
            Children [
                @run_field("Length (s)", SettingField::RunSecs, settings)
                --
                @run_field("Target size (x)", SettingField::TargetScale, settings)
                --
                @run_field("Decay (s)", SettingField::TargetLifetime, settings)
            ]
            --
            @small("Decay 0: targets stay until they are hit.")
        })
    }
}

fn run_field(text: &'static str, field: SettingField, settings: &Settings) -> impl Scene {
    const FIELD_W: f32 = (DETAIL_INNER_W - 2.0 * SPACE_S) / 3.0;
    bsn! {
        @column()
        Node { row_gap: px(SPACE_XS), width: px(FIELD_W) }
        Children [ @small(text) -- @number_input(field, settings, FIELD_W) ]
    }
}

/// The note that takes the run settings' place while a test is picked.
fn test_note() -> impl Scene {
    let text = "Length, target size and decay don't apply: a test has a fixed procedure, so every result compares with the last.";
    bsn! { @card("Run settings", bsn_list! { @label_dim(text) }) }
}

/// Fills the detail card for the picked tile and marks that tile, when the page is built,
/// another tile is picked, or the run settings change which history the picked drill is
/// compared with (then the tiles' notes follow too). A test (which ignores the run settings)
/// shows a note in their place.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_play_detail(
    mut commands: Commands,
    selection: Res<PlaySelection>,
    history: Res<History>,
    settings: Res<Settings>,
    details: Query<(Entity, Ref<PlayDetail>)>,
    mut hosts: Query<(Entity, &mut RunSettingsHost)>,
    mut tiles: Query<(&PlayTile, &mut Btn)>,
    notes: Query<(Entity, &TileNote, &Text)>,
    mut shown_as: Local<String>,
) {
    if settings.is_changed() {
        for (note, &TileNote(kind), text) in &notes {
            let Some(kind) = kind else { continue };
            let (wanted, color) = tile_note(kind, &history, &settings);
            if text.0 != wanted {
                commands.entity(note).insert((Text(wanted), ThemeTextColor(color)));
            }
        }
    }
    let played_as = match *selection {
        PlaySelection::Drill(kind) => DrillSpec::builtin(kind, &settings).name,
        PlaySelection::Routine(_) => String::new(),
    };
    let renamed = *shown_as != played_as;
    *shown_as = played_as;
    let test = matches!(*selection, PlaySelection::Drill(kind) if kind.def().category == Category::Tests);
    for (host, mut shows) in &mut hosts {
        if shows.0 != Some(test) {
            shows.0 = Some(test);
            commands.entity(host).despawn_related::<Children>();
            if test {
                spawn_in(&mut commands, host, test_note());
            } else {
                spawn_in(&mut commands, host, run_settings(&settings));
            }
        }
    }
    for (detail, marker) in &details {
        if !selection.is_changed() && !marker.is_added() && !renamed {
            continue;
        }
        commands.entity(detail).despawn_related::<Children>();
        match *selection {
            PlaySelection::Drill(kind) => spawn_drill_detail(&mut commands, detail, kind, &history, &shown_as),
            PlaySelection::Routine(index) => spawn_routine_detail(&mut commands, detail, index),
        }
        for (&PlayTile(item), mut kind) in &mut tiles {
            kind.set_if_neq(tile_kind(item == *selection));
        }
    }
}

/// The picked drill's card. `played_as` is the name a run of it is recorded under with the run
/// settings as they are: its history is the one shown.
fn spawn_drill_detail(commands: &mut Commands, card: Entity, kind: DrillKind, history: &History, played_as: &str) {
    let def = kind.def();
    let name = kind.name();
    spawn_in(commands, card, heading(name));
    spawn_in(commands, card, label(DrillConfig::defaults(kind).blurb()));
    let decay = if def.decay == DecayRule::Global { " Uses target decay, if set." } else { "" };
    spawn_in(commands, card, small(format!("Score: {}.{decay}", scoring(def.score))));
    // A size or decay other than the drill's own makes it a different thing to be good at.
    let variant = played_as.strip_prefix(name).and_then(|rest| rest.strip_prefix(VARIANT_SEPARATOR));
    if let Some(variant) = variant {
        let text = format!("With {variant}: scores at other run settings are kept apart, under History.");
        spawn_in(commands, card, small(text));
    }

    let scores = history.scores(played_as);
    if let Some(&last) = scores.last() {
        let compared = history.compare(played_as);
        let tiles = spawn_in(commands, card, bsn! { @row() Node { align_items: AlignItems::Stretch } });
        spawn_in(commands, tiles, stat_colored("Best", score_text(compared.best.unwrap_or(last)), GOLD));
        spawn_in(commands, tiles, stat("Last", score_text(last)));
        let average = score_text(compared.recent_avg.unwrap_or(last));
        spawn_in(commands, tiles, stat(format!("Avg of {}", compared.recent_count), average));
        let shown = &scores[scores.len().saturating_sub(CHART_RUNS)..];
        let bars = plot(commands, card, DETAIL_INNER_W, CHART_H);
        score_bars(commands, bars, shown, DETAIL_INNER_W, CHART_H);
        let runs = if scores.len() == 1 { "1 run".to_string() } else { format!("{} runs", scores.len()) };
        let trend = spawn_in(commands, card, row());
        spawn_in(commands, trend, small(runs));
        if let Some(change) = recent_change(&scores) {
            let text = format!("{change:+.1}% vs the {RECENT_RUNS} runs before");
            spawn_in(commands, trend, styled(text, fonts::BOLD, FONT_SMALL, delta_color(change)));
        }
    } else {
        let title = if variant.is_some() { "Not played with these settings yet" } else { "Not played yet" };
        spawn_in(commands, card, callout(TEXT_ACCENT, title, "Your first run sets the baseline to beat."));
    }

    spawn_in(commands, card, bsn! { @cta("Play") on(start_picked) });

    if def.adaptive {
        spawn_in(commands, card, rule());
        spawn_in(commands, card, overline("Adaptive mode: targets sized to your limit"));
        for mode in QuestMode::ALL {
            let length = match mode {
                QuestMode::Measure => format!(" {MEASURE_SECS:.0} s."),
                QuestMode::Train => " Runs the drill length.".to_string(),
            };
            let note = format!("{}{length}", mode.blurb());
            spawn_in(
                commands,
                card,
                bsn! {
                    @row()
                    Node { column_gap: px(SPACE_M) }
                    Children [
                        @compact_button(mode.name(), Btn::Secondary)
                        Node { width: px(84), flex_shrink: 0.0 }
                        AdaptiveButton(kind, mode)
                        on(play_adaptive)
                        --
                        @small(note)
                        Node { flex_grow: 1.0, flex_basis: px(0) }
                    ]
                },
            );
        }
        let explanation = format!(
            "Each target must be hit within the decay time ({ADAPTIVE_TIME_LIMIT} s with decay off), and its size follows how you do. The result is θ75: the target radius you hit {:.0}% of the time. Lower is better.",
            TARGET_HIT * 100.0
        );
        spawn_in(commands, card, details("How adaptive mode works", explanation));
    }
}

fn spawn_routine_detail(commands: &mut Commands, card: Entity, index: usize) {
    let plan = &ROUTINES[index];
    spawn_in(commands, card, heading(plan.name));
    spawn_in(commands, card, label(plan.blurb));
    let steps = spawn_in(commands, card, bsn! { @column() Node { row_gap: px(2) } });
    for (step, (kind, secs)) in plan.steps.iter().enumerate() {
        let cells = vec![
            cell(format!("{}", step + 1), 28.0, tokens::TEXT_DIM),
            cell(kind.name().to_string(), DETAIL_INNER_W - 100.0, TEXT_STRONG),
            cell(format!("{secs:.0} s"), 50.0, tokens::TEXT_MAIN),
        ];
        spawn_in(commands, steps, table_row(cells, step % 2 == 0));
    }
    spawn_in(
        commands,
        card,
        small("Each drill is scored like a run of its own. Breaks in between are as long as you like."),
    );
    spawn_in(commands, card, bsn! { @cta("Start routine") on(start_picked) });
}

/// Starts what the Play page has picked.
fn start(item: PlaySelection, settings: &Settings, commands: &mut Commands, next_state: &mut NextState<AppState>) {
    match item {
        PlaySelection::Drill(kind) => begin_run(commands, next_state, practice_run(kind, settings)),
        PlaySelection::Routine(index) => {
            start_routine(commands, next_state, Routine::from_plan(&ROUTINES[index], settings));
        },
    }
}

/// Picks a tile; a click on the picked one starts it, so a double click plays.
fn pick_tile(
    activate: On<Activate>,
    tiles: Query<&PlayTile>,
    mut selection: ResMut<PlaySelection>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Ok(&PlayTile(item)) = tiles.get(activate.event_target()) else { return };
    if *selection == item {
        start(item, &settings, &mut commands, &mut next_state);
    } else {
        *selection = item;
    }
}

fn start_picked(
    _: On<Activate>,
    selection: Res<PlaySelection>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    start(*selection, &settings, &mut commands, &mut next_state);
}

fn play_adaptive(
    activate: On<Activate>,
    buttons: Query<&AdaptiveButton>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if let Ok(&AdaptiveButton(kind, mode)) = buttons.get(activate.event_target()) {
        begin_run(&mut commands, &mut next_state, adaptive_run(kind, mode, &settings));
    }
}
