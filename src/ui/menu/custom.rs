//! Custom tab: create, edit and play your own scenarios and playlists.
//!
//! The lists and the editor are rebuilt only on structural changes (select, create, delete,
//! reorder), never while a value is being dragged or typed, so inputs keep focus.

use bevy::clipboard::{Clipboard, ClipboardError};
use bevy::feathers::constants::fonts;
use bevy::feathers::controls::{
    FeathersNumberInput, FeathersTextInput, FeathersTextInputContainer, HardLimit, NumberInputPrecision,
    NumberInputStep, NumberInputValue, SoftLimit,
};
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::prelude::*;
use bevy::text::{EditableText, TextEdit, TextEditChange};
use bevy::ui_widgets::{Activate, ValueChange};

use super::{MAIN_COLUMN_W, MenuTab, page};
use crate::AppState;
use crate::custom::{CustomContent, DrillRef, MAX_NAME_CHARS, PlaylistStep};
use crate::drill::params::{ParamId, ParamSpec};
use crate::drill::{ADAPTIVE_TIME_LIMIT, DrillConfig, DrillKind, DrillSpec, Run, RunPurpose, SECS_RANGE, begin_run};
use crate::routine::start_routine;
use crate::settings::Settings;
use crate::ui::bindings::*;
use crate::ui::charts::spawn_in;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// What the editor shows, plus a revision counter bumped on structural changes.
#[derive(Resource, Default)]
pub struct CustomSelection {
    editing: Editing,
    revision: u32,
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum Editing {
    #[default]
    Nothing,
    Scenario(u32),
    Playlist(u32),
}

impl CustomSelection {
    fn select(&mut self, editing: Editing) {
        self.editing = editing;
        self.revision += 1;
    }

    fn refresh(&mut self) {
        self.revision += 1;
    }

    /// Opens this scenario in the editor.
    pub(in crate::ui) fn edit_scenario(&mut self, id: u32) {
        self.select(Editing::Scenario(id));
    }
}

/// Step length used when adding drills to a playlist.
#[derive(Resource)]
pub struct PlaylistDraft {
    secs: f32,
}

impl Default for PlaylistDraft {
    fn default() -> Self {
        Self { secs: 45.0 }
    }
}

/// Feedback line for share/import.
#[derive(Resource, Default)]
pub struct CustomStatus(String);

#[derive(Component, Clone, Copy, Default)]
pub struct StatusText;

/// Containers rebuilt by [`rebuild_custom`].
#[derive(Component, Clone, Copy, Default)]
pub struct ScenarioList;
#[derive(Component, Clone, Copy, Default)]
pub struct PlaylistList;
#[derive(Component, Clone, Copy, Default)]
pub struct CustomEditor;

#[derive(Component, Clone, Copy, Default)]
struct NewScenarioButton(DrillKind);

/// Buttons acting on one item; the payload is the item id (or step index).
#[derive(Component, Clone, Copy, Default)]
enum ItemAction {
    #[default]
    None,
    PlayScenario(u32),
    EditScenario(u32),
    CopyScenario(u32),
    DeleteScenario(u32),
    PlayPlaylist(u32),
    EditPlaylist(u32),
    DeletePlaylist(u32),
    SharePlaylist(u32),
    StepUp(u32, usize),
    StepRemove(u32, usize),
    AddStep(u32, DrillRef),
    Close,
}

/// Text input bound to a scenario's or playlist's name.
#[derive(Component, Clone, Copy, Default)]
pub struct NameInput(Editing);

/// A numeric field of the editor: the scenario's length, one of its parameters, or the
/// playlist draft's step length.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
enum EditorField {
    #[default]
    Duration,
    /// Not a scenario field: the playlist draft step length.
    StepSecs,
    Param(ParamId),
}

impl EditorField {
    /// (label, precision, step, soft range, hard range)
    fn spec(self) -> (&'static str, i32, f64, (f32, f32), (f32, f32)) {
        match self {
            EditorField::Duration => ("Length (s)", 0, 5.0, (10.0, 180.0), SECS_RANGE),
            EditorField::StepSecs => ("Step length (s)", 0, 5.0, (10.0, 180.0), SECS_RANGE),
            EditorField::Param(param) => {
                let ParamSpec { label, precision, step, soft, hard } = param.spec();
                (label, precision, step, soft, hard)
            },
        }
    }

    fn get(self, content: &CustomContent, editing: Editing, draft: &PlaylistDraft) -> f32 {
        if self == EditorField::StepSecs {
            return draft.secs;
        }
        let Editing::Scenario(id) = editing else { return 0.0 };
        let Some(scenario) = content.scenario(id) else { return 0.0 };
        match self {
            EditorField::Param(param) => param.get(&scenario.config),
            _ => scenario.duration,
        }
    }

    fn set(self, content: &mut CustomContent, editing: Editing, draft: &mut PlaylistDraft, value: f32) {
        let secs = || value.round().clamp(SECS_RANGE.0, SECS_RANGE.1);
        if self == EditorField::StepSecs {
            draft.secs = secs();
            return;
        }
        let Editing::Scenario(id) = editing else { return };
        let Some(scenario) = content.scenario_mut(id) else { return };
        match self {
            EditorField::Param(param) => param.set(&mut scenario.config, value),
            _ => scenario.duration = secs(),
        }
        scenario.config.sanitize();
    }
}

// ---------------------------------------------------------------------------------------------
// Page layout (static part; lists and editor are filled in by `rebuild_custom`)
// ---------------------------------------------------------------------------------------------

pub(super) fn custom_page() -> impl Scene {
    bsn! { @page(MenuTab::Custom, bsn_list! { @custom_lists_column() -- @custom_editor_column() }) }
}

fn custom_lists_column() -> impl Scene {
    let new_buttons: Vec<_> = DrillKind::ALL.into_iter().map(new_scenario_button).collect();
    bsn! {
        @scroll_column(bsn_list! {
            @card("My scenarios", bsn_list! {
                @column() Node { row_gap: px(SPACE_XS) } ScenarioList
                --
                @small("New scenario, starting from a drill:")
                Node { margin: UiRect::top(px(SPACE_XS)) }
                --
                @wrap_row()
                Node { column_gap: px(SPACE_XS), row_gap: px(SPACE_XS) }
                Children [ {new_buttons} ]
            })
            --
            @card("My playlists", bsn_list! {
                @column() Node { row_gap: px(SPACE_XS) } PlaylistList
                --
                @actions(bsn_list! {
                    @button("New playlist", Btn::Secondary)
                    on(|_: On<Activate>, mut content: ResMut<CustomContent>, mut selection: ResMut<CustomSelection>| {
                        let id = content.new_playlist();
                        selection.select(Editing::Playlist(id));
                    })
                    --
                    @button("Import share code", Btn::Secondary)
                    on(import_playlist)
                })
                --
                @small("Import reads a playlist share code from the clipboard. Share copies one.")
                --
                @styled("", fonts::REGULAR, FONT_BODY, TEXT_ACCENT)
                StatusText
            })
        })
        // As wide as the other pages' main column, plus the scrollbar gutter.
        Node { width: px(MAIN_COLUMN_W + SCROLLBAR_GUTTER), flex_shrink: 0.0 }
    }
}

/// The editor scrolls on its own, so long scenarios and playlists stay reachable.
fn custom_editor_column() -> impl Scene {
    bsn! {
        @scroll_column(bsn_list! { @surface() CustomEditor })
        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
    }
}

fn new_scenario_button(kind: DrillKind) -> impl Scene {
    bsn! {
        @compact_button(kind.name(), Btn::Secondary)
        NewScenarioButton(kind)
        on(|activate: On<Activate>, buttons: Query<&NewScenarioButton>, mut content: ResMut<CustomContent>, mut selection: ResMut<CustomSelection>| {
            if let Ok(&NewScenarioButton(kind)) = buttons.get(activate.event_target()) {
                let id = content.new_scenario(kind);
                selection.select(Editing::Scenario(id));
            }
        })
    }
}

fn small_button(text: &'static str, kind: Btn, action: ItemAction) -> impl Scene {
    bsn! {
        @compact_button(text, kind)
        Node { min_width: px(0) }
        action
        on(item_action)
    }
}

/// A list entry; the one open in the editor gets an accent bar on its left edge and the tint
/// of a selected item.
fn list_row(selected: bool) -> impl Scene {
    let (edge, fill) = if selected { (ACCENT, ACCENT_SOFT) } else { (CLEAR, SURFACE_TILE) };
    bsn! {
        @row()
        Node {
            min_height: px(38),
            padding: UiRect::all(px(SPACE_XS)),
            column_gap: px(SPACE_XS),
            border_radius: BorderRadius::all(px(RADIUS_S)),
        }
        ThemeBackgroundColor(fill)
        Children [ @marker(edge) Node { margin: UiRect::right(px(SPACE_XS)) } ]
    }
}

/// A scenario or playlist in the lists; its name follows renames via [`ItemName`].
fn item_name(name: String, item: Editing) -> impl Scene {
    bsn! { @label_strong(name) Node { flex_grow: 1.0, flex_basis: px(0) } ItemName(item) }
}

/// A step of the playlist being edited.
fn step_name(text: String) -> impl Scene {
    bsn! { @label(text) Node { flex_grow: 1.0, flex_basis: px(0) } }
}

/// Play + Done at the bottom of the editor.
fn editor_actions(play: ItemAction) -> impl Scene {
    actions(bsn_list! {
        @button("Play", Btn::Primary)
        play
        on(item_action)
        --
        @button("Done", Btn::Secondary)
        ItemAction::Close
        on(item_action)
    })
}

// ---------------------------------------------------------------------------------------------
// Rebuilding
// ---------------------------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
pub fn rebuild_custom(
    mut commands: Commands,
    selection: Res<CustomSelection>,
    content: Res<CustomContent>,
    draft: Res<PlaylistDraft>,
    scenario_lists: Query<(Entity, Ref<ScenarioList>)>,
    playlist_lists: Query<(Entity, Ref<PlaylistList>)>,
    editors: Query<(Entity, Ref<CustomEditor>)>,
) {
    let added = scenario_lists.iter().any(|(_, m)| m.is_added())
        || playlist_lists.iter().any(|(_, m)| m.is_added())
        || editors.iter().any(|(_, m)| m.is_added());
    if !selection.is_changed() && !added {
        return;
    }

    for (list, _) in &scenario_lists {
        commands.entity(list).despawn_related::<Children>();
        if content.scenarios.is_empty() {
            commands.spawn_scene(label_dim("None yet. Pick a drill below to start one from.")).insert(ChildOf(list));
        }
        for scenario in &content.scenarios {
            let id = scenario.id;
            let selected = selection.editing == Editing::Scenario(id);
            let row = spawn_in(&mut commands, list, list_row(selected));
            spawn_in(&mut commands, row, item_name(scenario.name.clone(), Editing::Scenario(id)));
            for ((text, variant), action) in [
                (("Play", Btn::Secondary), ItemAction::PlayScenario(id)),
                (("Edit", Btn::Ghost), ItemAction::EditScenario(id)),
                (("Copy", Btn::Ghost), ItemAction::CopyScenario(id)),
                (("Delete", Btn::Danger), ItemAction::DeleteScenario(id)),
            ] {
                commands.spawn_scene(small_button(text, variant, action)).insert(ChildOf(row));
            }
        }
    }

    for (list, _) in &playlist_lists {
        commands.entity(list).despawn_related::<Children>();
        if content.playlists.is_empty() {
            let none = "None yet. A playlist plays drills and scenarios back to back, like a warmup.";
            commands.spawn_scene(label_dim(none)).insert(ChildOf(list));
        }
        for playlist in &content.playlists {
            let id = playlist.id;
            let selected = selection.editing == Editing::Playlist(id);
            let row = spawn_in(&mut commands, list, list_row(selected));
            spawn_in(&mut commands, row, item_name(playlist.name.clone(), Editing::Playlist(id)));
            for ((text, variant), action) in [
                (("Play", Btn::Secondary), ItemAction::PlayPlaylist(id)),
                (("Edit", Btn::Ghost), ItemAction::EditPlaylist(id)),
                (("Share", Btn::Ghost), ItemAction::SharePlaylist(id)),
                (("Delete", Btn::Danger), ItemAction::DeletePlaylist(id)),
            ] {
                commands.spawn_scene(small_button(text, variant, action)).insert(ChildOf(row));
            }
        }
    }

    for (editor, _) in &editors {
        commands.entity(editor).despawn_related::<Children>();
        match selection.editing {
            Editing::Nothing => {
                spawn_in(&mut commands, editor, overline("How it works"));
                spawn_in(&mut commands, editor, subheading("Make the drills your own"));
                for (number, title, text) in [
                    (
                        "1",
                        "Start a scenario from a drill",
                        "Pick one under My scenarios. It opens here with that drill's settings.",
                    ),
                    (
                        "2",
                        "Tune it",
                        "Grid size, spacing and targets at once, flick ranges, strafe speeds, peek timing and more. \
                         Its scores get their own history.",
                    ),
                    (
                        "3",
                        "Chain drills into a playlist",
                        "Playlists play drills and scenarios back to back, with a break in between. Share one as a \
                         code, or import a friend's.",
                    ),
                ] {
                    spawn_in(&mut commands, editor, numbered_step(number, title, text));
                }
            },
            Editing::Scenario(id) => {
                let Some(scenario) = content.scenario(id) else { continue };
                let kind = scenario.config.kind;
                commands.spawn_scene(overline("Edit scenario")).insert(ChildOf(editor));
                spawn_name_input(&mut commands, editor, Editing::Scenario(id));
                commands
                    .spawn_scene(label_dim(based_on(&scenario.config)))
                    .insert((ChildOf(editor), ScenarioBlurb(id)));
                if let Some(mode) = scenario.config.adaptive() {
                    let note = format!(
                        "Adaptive ({}): each target's size adapts to you, so Size is ignored, and a target counts if hit within Lifetime ({:.1} s when 0).",
                        mode.name().to_lowercase(),
                        ADAPTIVE_TIME_LIMIT
                    );
                    commands.spawn_scene(label_dim(note)).insert(ChildOf(editor));
                }
                let mut fields = vec![EditorField::Duration, EditorField::Param(ParamId::Size)];
                if scenario.config.supports_decay() {
                    fields.push(EditorField::Param(ParamId::Lifetime));
                }
                fields.extend(kind.def().params.iter().map(|&param| EditorField::Param(param)));
                for field in fields {
                    let value = field.get(&content, selection.editing, &draft);
                    commands.spawn_scene(custom_field_row(field, value)).insert(ChildOf(editor));
                }
                commands.spawn_scene(editor_actions(ItemAction::PlayScenario(id))).insert(ChildOf(editor));
            },
            Editing::Playlist(id) => {
                let Some(playlist) = content.playlist(id) else { continue };
                commands.spawn_scene(overline("Edit playlist")).insert(ChildOf(editor));
                spawn_name_input(&mut commands, editor, Editing::Playlist(id));
                if playlist.steps.is_empty() {
                    commands.spawn_scene(label_dim("No steps yet. Add drills below.")).insert(ChildOf(editor));
                }
                for (index, step) in playlist.steps.iter().enumerate() {
                    let name = content.drill_name(step.drill).unwrap_or_else(|| "(deleted)".into());
                    let row = spawn_in(&mut commands, editor, list_row(false));
                    let text = format!("{}. {name}  ({:.0} s)", index + 1, step.secs);
                    spawn_in(&mut commands, row, step_name(text));
                    if index > 0 {
                        commands
                            .spawn_scene(small_button("Up", Btn::Ghost, ItemAction::StepUp(id, index)))
                            .insert(ChildOf(row));
                    }
                    commands
                        .spawn_scene(small_button("Remove", Btn::Danger, ItemAction::StepRemove(id, index)))
                        .insert(ChildOf(row));
                }
                commands.spawn_scene(custom_field_row(EditorField::StepSecs, draft.secs)).insert(ChildOf(editor));
                commands.spawn_scene(small("Add a step of that length:")).insert(ChildOf(editor));
                let mut drills: Vec<DrillRef> = DrillKind::ALL.iter().map(|&s| DrillRef::Builtin(s)).collect();
                drills.extend(content.scenarios.iter().map(|s| DrillRef::Custom(s.id)));
                let chips = commands
                    .spawn_scene(bsn! { @wrap_row() Node { column_gap: px(SPACE_XS), row_gap: px(SPACE_XS) } })
                    .insert(ChildOf(editor))
                    .id();
                for drill in drills {
                    let name = content.drill_name(drill).unwrap_or_default();
                    commands
                        .spawn_scene(bsn! {
                            @compact_button(name, Btn::Secondary)
                            ItemAction::AddStep(id, drill)
                            on(item_action)
                        })
                        .insert(ChildOf(chips));
                }
                commands.spawn_scene(editor_actions(ItemAction::PlayPlaylist(id))).insert(ChildOf(editor));
            },
        }
    }
}

/// Names shown in the lists; kept in sync while typing without rebuilding.
#[derive(Component, Clone, Copy, Default)]
pub struct ItemName(Editing);

/// The editor's description of the scenario being edited; follows its parameters without
/// rebuilding (Gridshot's names its grid).
#[derive(Component, Clone, Copy)]
pub struct ScenarioBlurb(u32);

fn based_on(config: &DrillConfig) -> String {
    format!("Based on {}: {}", config.kind.name(), config.blurb())
}

fn spawn_name_input(commands: &mut Commands, parent: Entity, editing: Editing) {
    commands
        .spawn_scene(setting_row(
            "Name",
            bsn_list! {
                @FeathersTextInputContainer
                Node { width: px(240), flex_grow: 0.0 }
                Children [
                    @FeathersTextInput { @max_characters: MAX_NAME_CHARS }
                    NameInput(editing)
                    on(rename)
                ]
            },
        ))
        .insert(ChildOf(parent));
}

fn custom_field_row(field: EditorField, value: f32) -> impl Scene {
    let (text, precision, step, soft, hard) = field.spec();
    let soft = SoftLimit::f32(soft.0..=soft.1);
    let hard = HardLimit::f32(hard.0..=hard.1);
    setting_row(
        text,
        bsn_list! {
            Node { display: Display::Flex, width: px(FIELD_W), flex_shrink: 0.0 }
            Children [
                @FeathersNumberInput
                NumberInputValue::F32(value)
                NumberInputPrecision(precision)
                NumberInputStep(step)
                soft
                hard
                Node { flex_grow: 1.0 }
                field
                on(edit_custom_field)
            ]
        },
    )
}

// ---------------------------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------------------------

fn edit_custom_field(
    change: On<ValueChange<f32>>,
    fields: Query<(Entity, &EditorField, &NumberInputValue)>,
    mut content: ResMut<CustomContent>,
    mut draft: ResMut<PlaylistDraft>,
    selection: Res<CustomSelection>,
    mut commands: Commands,
) {
    let input = change.event_target();
    let Ok((_, &field, _)) = fields.get(input) else { return };
    field.set(&mut content, selection.editing, &mut draft, change.value);
    // Sanitizing can change other fields too ("targets at once" capped by the grid size,
    // max raised to min), so echo every field whose stored value now differs from its display.
    // The edited input is always echoed: it shows the raw typed value until told otherwise.
    for (entity, field, shown) in &fields {
        let wanted = field.get(&content, selection.editing, &draft);
        if entity == input || !matches!(shown, NumberInputValue::F32(v) if *v == wanted) {
            commands.entity(entity).insert(NumberInputValue::F32(wanted));
        }
    }
}

/// Fills name inputs with the current name once they exist.
pub fn init_name_inputs(
    content: Res<CustomContent>,
    mut inputs: Query<(&NameInput, &mut EditableText), Added<NameInput>>,
) {
    for (NameInput(editing), mut text) in &mut inputs {
        let name = match *editing {
            Editing::Scenario(id) => content.scenario(id).map(|s| s.name.clone()),
            Editing::Playlist(id) => content.playlist(id).map(|p| p.name.clone()),
            Editing::Nothing => None,
        };
        if let Some(name) = name {
            text.queue_edit(TextEdit::SelectAll);
            text.queue_edit(TextEdit::Insert(name.into()));
        }
    }
}

fn rename(change: On<TextEditChange>, inputs: Query<(&NameInput, &EditableText)>, mut content: ResMut<CustomContent>) {
    let Ok((NameInput(editing), text)) = inputs.get(change.event_target()) else { return };
    let name = text.value().to_string();
    match *editing {
        // Invalid names (empty, duplicate, built-in) are simply not applied.
        Editing::Scenario(id) => {
            if content.scenario(id).is_some_and(|s| s.name != name.trim()) {
                content.rename_scenario(id, &name);
            }
        },
        Editing::Playlist(id) => {
            let trimmed = name.trim();
            if let Some(playlist) = content.playlist_mut(id)
                && !trimmed.is_empty()
                && playlist.name != trimmed
            {
                playlist.name = trimmed.to_string();
            }
        },
        Editing::Nothing => {},
    }
}

fn import_playlist(
    _: On<Activate>,
    mut clipboard: ResMut<Clipboard>,
    mut content: ResMut<CustomContent>,
    mut selection: ResMut<CustomSelection>,
    mut status: ResMut<CustomStatus>,
) {
    let text = match clipboard.fetch_text().poll_result() {
        Some(Ok(text)) => text,
        Some(Err(ClipboardError::ContentNotAvailable)) => {
            return status.0 = "The clipboard has no text. Copy a share code first, then click Import.".into();
        },
        Some(Err(ClipboardError::ClipboardOccupied)) => {
            return status.0 = "Another program is using the clipboard, try again.".into();
        },
        Some(Err(err)) => return status.0 = format!("Couldn't read the clipboard: {err}"),
        None => return status.0 = "The clipboard isn't ready yet, try again.".into(),
    };
    match content.import_playlist(&text) {
        Ok(id) => {
            status.0 = "Playlist imported.".into();
            selection.select(Editing::Playlist(id));
        },
        Err(err) => status.0 = format!("{err} Copy a share code first, then click Import."),
    }
}

pub fn sync_status(status: Res<CustomStatus>, mut labels: Query<&mut Text, With<StatusText>>) {
    for mut text in &mut labels {
        if text.0 != status.0 {
            text.0.clone_from(&status.0);
        }
    }
}

/// Status messages describe the last action; don't carry them over to the next menu visit.
pub fn clear_status(mut status: ResMut<CustomStatus>) {
    status.0.clear();
}

/// Keeps list labels in sync with renames.
pub fn sync_item_names(content: Res<CustomContent>, mut labels: Query<(&ItemName, &mut Text)>) {
    if !content.is_changed() {
        return;
    }
    for (ItemName(editing), mut text) in &mut labels {
        let name = match *editing {
            Editing::Scenario(id) => content.scenario(id).map(|s| s.name.as_str()),
            Editing::Playlist(id) => content.playlist(id).map(|p| p.name.as_str()),
            Editing::Nothing => None,
        };
        if let Some(name) = name
            && text.0 != name
        {
            text.0 = name.to_string();
        }
    }
}

/// Keeps the editor's scenario description in sync with parameter edits.
pub fn sync_scenario_blurbs(content: Res<CustomContent>, mut labels: Query<(&ScenarioBlurb, &mut Text)>) {
    if !content.is_changed() {
        return;
    }
    for (ScenarioBlurb(id), mut text) in &mut labels {
        if let Some(scenario) = content.scenario(*id) {
            let blurb = based_on(&scenario.config);
            if text.0 != blurb {
                text.0 = blurb;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn item_action(
    activate: On<Activate>,
    actions: Query<&ItemAction>,
    mut content: ResMut<CustomContent>,
    mut selection: ResMut<CustomSelection>,
    draft: Res<PlaylistDraft>,
    settings: Res<Settings>,
    mut clipboard: ResMut<Clipboard>,
    mut status: ResMut<CustomStatus>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Ok(&action) = actions.get(activate.event_target()) else { return };
    match action {
        ItemAction::None => {},
        ItemAction::PlayScenario(id) => {
            let Some(scenario) = content.scenario(id) else { return };
            let drill = DrillSpec { name: scenario.name.clone(), config: scenario.config.clone() };
            let run = Run::new(drill, settings.sens, scenario.duration, RunPurpose::Practice, scenario.name.clone());
            begin_run(&mut commands, &mut next_state, run);
        },
        ItemAction::EditScenario(id) => selection.select(Editing::Scenario(id)),
        ItemAction::CopyScenario(id) => {
            if let Some(copy) = content.duplicate_scenario(id) {
                selection.select(Editing::Scenario(copy));
            }
        },
        ItemAction::DeleteScenario(id) => {
            content.delete_scenario(id);
            let editing = if selection.editing == Editing::Scenario(id) { Editing::Nothing } else { selection.editing };
            selection.select(editing);
        },
        ItemAction::PlayPlaylist(id) => {
            if let Some(routine) = content.playlist_routine(id, &settings) {
                start_routine(&mut commands, &mut next_state, routine);
            }
        },
        ItemAction::EditPlaylist(id) => selection.select(Editing::Playlist(id)),
        ItemAction::SharePlaylist(id) => {
            let Some(code) = content.export_playlist(id) else { return };
            status.0 = match clipboard.set_text(code) {
                Ok(()) => "Share code copied to the clipboard. Paste it to a friend; they use \"Import\".".into(),
                Err(err) => format!("Couldn't copy to the clipboard: {err}"),
            };
        },
        ItemAction::DeletePlaylist(id) => {
            content.delete_playlist(id);
            let editing = if selection.editing == Editing::Playlist(id) { Editing::Nothing } else { selection.editing };
            selection.select(editing);
        },
        ItemAction::StepUp(id, index) => {
            if let Some(playlist) = content.playlist_mut(id)
                && index > 0
                && index < playlist.steps.len()
            {
                playlist.steps.swap(index - 1, index);
                selection.refresh();
            }
        },
        ItemAction::StepRemove(id, index) => {
            if let Some(playlist) = content.playlist_mut(id)
                && index < playlist.steps.len()
            {
                playlist.steps.remove(index);
                selection.refresh();
            }
        },
        ItemAction::AddStep(id, drill) => {
            if let Some(playlist) = content.playlist_mut(id) {
                playlist.steps.push(PlaylistStep { drill, secs: draft.secs });
                selection.refresh();
            }
        },
        ItemAction::Close => selection.select(Editing::Nothing),
    }
}
