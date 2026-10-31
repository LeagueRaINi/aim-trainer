//! Custom tab: create, edit and play your own scenarios and playlists.
//!
//! The lists and the editor are rebuilt only on structural changes (select, create, delete,
//! reorder), never while a value is being dragged or typed, so inputs keep focus.

use bevy::{
    app::Propagate,
    clipboard::{Clipboard, ClipboardError},
    feathers::{
        controls::{
            ButtonVariant, FeathersNumberInput, FeathersTextInput, FeathersTextInputContainer, HardLimit,
            NumberInputPrecision, NumberInputStep, NumberInputValue, SoftLimit,
        },
        display::{label, label_dim},
        theme::{SurfaceLevel, ThemeBackgroundColor, ThemeBorderColor, ThemeContext},
        tokens,
    },
    prelude::*,
    text::{EditableText, TextEdit, TextEditChange},
    ui_widgets::{Activate, ValueChange},
};

use crate::ui::{bindings::*, theme::*, widgets::*};
use crate::{
    AppState,
    custom::{CustomContent, DrillRef, MAX_NAME_CHARS, PlaylistStep},
    drill::{
        ADAPTIVE_TIME_LIMIT, DrillKind, DrillSpec, Run, RunPurpose, SECS_RANGE, begin_run,
        params::{ParamId, ParamSpec},
    },
    routine::start_routine,
    settings::Settings,
};

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
            }
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

pub fn custom_lists_column() -> impl Scene {
    let new_buttons: Vec<_> = DrillKind::ALL.into_iter().map(new_scenario_button).collect();
    bsn! {
        @column()
        Node { width: px(470), row_gap: px(SPACE_L), flex_shrink: 0.0 }
        Children [
            @card("My scenarios", bsn_list! {
                @column() Node { row_gap: px(SPACE_XS) } ScenarioList
                --
                @label_dim("New scenario based on:")
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
                    @button("New playlist", ButtonVariant::Normal)
                    on(|_: On<Activate>, mut content: ResMut<CustomContent>, mut selection: ResMut<CustomSelection>| {
                        let id = content.new_playlist();
                        selection.select(Editing::Playlist(id));
                    })
                    --
                    @button("Import from clipboard", ButtonVariant::Normal)
                    on(import_playlist)
                })
                --
                @label_dim("")
                StatusText
            })
        ]
    }
}

pub fn custom_editor_column() -> impl Scene {
    bsn! {
        @surface()
        Node { flex_grow: 1.0, flex_basis: px(0), align_self: AlignSelf::FlexStart }
        CustomEditor
    }
}

fn new_scenario_button(kind: DrillKind) -> impl Scene {
    bsn! {
        @compact_button(kind.name(), ButtonVariant::Normal)
        NewScenarioButton(kind)
        on(|activate: On<Activate>, buttons: Query<&NewScenarioButton>, mut content: ResMut<CustomContent>, mut selection: ResMut<CustomSelection>| {
            if let Ok(&NewScenarioButton(kind)) = buttons.get(activate.event_target()) {
                let id = content.new_scenario(kind);
                selection.select(Editing::Scenario(id));
            }
        })
    }
}

fn small_button(text: &'static str, variant: ButtonVariant, action: ItemAction) -> impl Scene {
    bsn! {
        @compact_button(text, variant)
        action
        on(item_action)
    }
}

/// A list entry; the one open in the editor gets an accent bar on its left edge.
fn list_row(selected: bool) -> impl Scene {
    let edge = if selected { tokens::BUTTON_PRIMARY_BG } else { tokens::GROUP_BG };
    bsn! {
        @row()
        Node {
            min_height: px(34),
            padding: UiRect::new(px(SPACE_M), px(SPACE_XS), px(SPACE_XS), px(SPACE_XS)),
            column_gap: px(SPACE_XS),
            border: UiRect::left(px(3)),
            border_radius: BorderRadius::all(px(6)),
        }
        ThemeBackgroundColor(tokens::GROUP_BG)
        ThemeBorderColor(edge)
        Propagate::<ThemeContext>(ThemeContext(SurfaceLevel::Highest))
    }
}

/// A scenario or playlist in the lists; its name follows renames via [`ItemName`].
fn item_row(name: String, item: Editing, selected: bool) -> impl Scene {
    bsn! {
        @list_row(selected)
        Children [ @label(name) Node { flex_grow: 1.0 } ItemName(item) ]
    }
}

/// A step of the playlist being edited.
fn step_row(text: String) -> impl Scene {
    bsn! {
        @list_row(false)
        Children [ @label(text) Node { flex_grow: 1.0 } ]
    }
}

/// Play + Done at the bottom of the editor.
fn editor_actions(play: ItemAction) -> impl Scene {
    actions(bsn_list! {
        @button("Play", ButtonVariant::Primary)
        play
        on(item_action)
        --
        @button("Done", ButtonVariant::Normal)
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
            commands.spawn_scene(label_dim("None yet. Pick a base drill below.")).insert(ChildOf(list));
        }
        for scenario in &content.scenarios {
            let id = scenario.id;
            let selected = selection.editing == Editing::Scenario(id);
            let row = commands
                .spawn_scene(item_row(scenario.name.clone(), Editing::Scenario(id), selected))
                .insert(ChildOf(list))
                .id();
            for ((text, variant), action) in [
                (("Play", ButtonVariant::Primary), ItemAction::PlayScenario(id)),
                (("Edit", ButtonVariant::Normal), ItemAction::EditScenario(id)),
                (("Copy", ButtonVariant::Plain), ItemAction::CopyScenario(id)),
                (("Delete", ButtonVariant::Plain), ItemAction::DeleteScenario(id)),
            ] {
                commands.spawn_scene(small_button(text, variant, action)).insert(ChildOf(row));
            }
        }
    }

    for (list, _) in &playlist_lists {
        commands.entity(list).despawn_related::<Children>();
        if content.playlists.is_empty() {
            commands.spawn_scene(label_dim("None yet.")).insert(ChildOf(list));
        }
        for playlist in &content.playlists {
            let id = playlist.id;
            let selected = selection.editing == Editing::Playlist(id);
            let row = commands
                .spawn_scene(item_row(playlist.name.clone(), Editing::Playlist(id), selected))
                .insert(ChildOf(list))
                .id();
            for ((text, variant), action) in [
                (("Play", ButtonVariant::Primary), ItemAction::PlayPlaylist(id)),
                (("Edit", ButtonVariant::Normal), ItemAction::EditPlaylist(id)),
                (("Share", ButtonVariant::Plain), ItemAction::SharePlaylist(id)),
                (("Delete", ButtonVariant::Plain), ItemAction::DeletePlaylist(id)),
            ] {
                commands.spawn_scene(small_button(text, variant, action)).insert(ChildOf(row));
            }
        }
    }

    for (editor, _) in &editors {
        commands.entity(editor).despawn_related::<Children>();
        match selection.editing {
            Editing::Nothing => {
                commands.spawn_scene(overline("Editor")).insert(ChildOf(editor));
                commands
                    .spawn_scene(label_dim(
                        "Create a scenario from one of the built-in drills and tune it: grid size, spacing and \
                         targets at once, flick ranges, strafe speeds, peek timing and more. Then combine \
                         scenarios into playlists that play back-to-back with breaks in between.",
                    ))
                    .insert(ChildOf(editor));
            }
            Editing::Scenario(id) => {
                let Some(scenario) = content.scenario(id) else { continue };
                let kind = scenario.config.kind;
                commands.spawn_scene(overline("Edit scenario")).insert(ChildOf(editor));
                spawn_name_input(&mut commands, editor, Editing::Scenario(id));
                commands
                    .spawn_scene(label_dim(format!("Based on {}: {}", kind.name(), kind.blurb())))
                    .insert(ChildOf(editor));
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
            }
            Editing::Playlist(id) => {
                let Some(playlist) = content.playlist(id) else { continue };
                commands.spawn_scene(overline("Edit playlist")).insert(ChildOf(editor));
                spawn_name_input(&mut commands, editor, Editing::Playlist(id));
                if playlist.steps.is_empty() {
                    commands.spawn_scene(label_dim("No steps yet. Add drills below.")).insert(ChildOf(editor));
                }
                for (index, step) in playlist.steps.iter().enumerate() {
                    let name = content.drill_name(step.drill).unwrap_or_else(|| "(deleted)".into());
                    let row = commands
                        .spawn_scene(step_row(format!("{}. {name}  ({:.0} s)", index + 1, step.secs)))
                        .insert(ChildOf(editor))
                        .id();
                    if index > 0 {
                        commands.spawn_scene(small_button("Up", ButtonVariant::Normal, ItemAction::StepUp(id, index))).insert(ChildOf(row));
                    }
                    commands
                        .spawn_scene(small_button("Remove", ButtonVariant::Plain, ItemAction::StepRemove(id, index)))
                        .insert(ChildOf(row));
                }
                commands.spawn_scene(custom_field_row(EditorField::StepSecs, draft.secs)).insert(ChildOf(editor));
                commands.spawn_scene(label_dim("Add a step:")).insert(ChildOf(editor));
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
                            @compact_button(name, ButtonVariant::Normal)
                            ItemAction::AddStep(id, drill)
                            on(item_action)
                        })
                        .insert(ChildOf(chips));
                }
                commands.spawn_scene(editor_actions(ItemAction::PlayPlaylist(id))).insert(ChildOf(editor));
            }
        }
    }
}

/// Names shown in the lists; kept in sync while typing without rebuilding.
#[derive(Component, Clone, Copy, Default)]
pub struct ItemName(Editing);

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
pub fn init_name_inputs(content: Res<CustomContent>, mut inputs: Query<(&NameInput, &mut EditableText), Added<NameInput>>) {
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
        }
        Editing::Playlist(id) => {
            let trimmed = name.trim();
            if let Some(playlist) = content.playlist_mut(id)
                && !trimmed.is_empty()
                && playlist.name != trimmed
            {
                playlist.name = trimmed.to_string();
            }
        }
        Editing::Nothing => {}
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
        }
        Some(Err(ClipboardError::ClipboardOccupied)) => {
            return status.0 = "Another program is using the clipboard, try again.".into();
        }
        Some(Err(err)) => return status.0 = format!("Couldn't read the clipboard: {err}"),
        None => return status.0 = "The clipboard isn't ready yet, try again.".into(),
    };
    match content.import_playlist(&text) {
        Ok(id) => {
            status.0 = "Playlist imported.".into();
            selection.select(Editing::Playlist(id));
        }
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
        ItemAction::None => {}
        ItemAction::PlayScenario(id) => {
            let Some(scenario) = content.scenario(id) else { return };
            let drill = DrillSpec { name: scenario.name.clone(), config: scenario.config.clone() };
            let run = Run::new(drill, settings.sens, scenario.duration, RunPurpose::Practice, scenario.name.clone());
            begin_run(&mut commands, &mut next_state, run);
        }
        ItemAction::EditScenario(id) => selection.select(Editing::Scenario(id)),
        ItemAction::CopyScenario(id) => {
            if let Some(copy) = content.duplicate_scenario(id) {
                selection.select(Editing::Scenario(copy));
            }
        }
        ItemAction::DeleteScenario(id) => {
            content.delete_scenario(id);
            let editing = if selection.editing == Editing::Scenario(id) { Editing::Nothing } else { selection.editing };
            selection.select(editing);
        }
        ItemAction::PlayPlaylist(id) => {
            if let Some(routine) = content.playlist_routine(id, &settings) {
                start_routine(&mut commands, &mut next_state, routine);
            }
        }
        ItemAction::EditPlaylist(id) => selection.select(Editing::Playlist(id)),
        ItemAction::SharePlaylist(id) => {
            let Some(code) = content.export_playlist(id) else { return };
            status.0 = match clipboard.set_text(code) {
                Ok(()) => "Share code copied to the clipboard. Paste it to a friend; they use \"Import\".".into(),
                Err(err) => format!("Couldn't copy to the clipboard: {err}"),
            };
        }
        ItemAction::DeletePlaylist(id) => {
            content.delete_playlist(id);
            let editing = if selection.editing == Editing::Playlist(id) { Editing::Nothing } else { selection.editing };
            selection.select(editing);
        }
        ItemAction::StepUp(id, index) => {
            if let Some(playlist) = content.playlist_mut(id)
                && index > 0
                && index < playlist.steps.len()
            {
                playlist.steps.swap(index - 1, index);
                selection.refresh();
            }
        }
        ItemAction::StepRemove(id, index) => {
            if let Some(playlist) = content.playlist_mut(id)
                && index < playlist.steps.len()
            {
                playlist.steps.remove(index);
                selection.refresh();
            }
        }
        ItemAction::AddStep(id, drill) => {
            if let Some(playlist) = content.playlist_mut(id) {
                playlist.steps.push(PlaylistStep { drill, secs: draft.secs });
                selection.refresh();
            }
        }
        ItemAction::Close => selection.select(Editing::Nothing),
    }
}
