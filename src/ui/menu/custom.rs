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
    custom::{CustomContent, DrillRef, PlaylistStep},
    routine::start_routine,
    drill::{DrillConfig, DrillSpec, Run, RunPurpose, DrillKind, begin_run},
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
    DrillKind(u32),
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

/// A numeric field of the scenario being edited.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CustomField {
    #[default]
    Duration,
    Size,
    Lifetime,
    GridCols,
    GridRows,
    GridSpacing,
    GridActive,
    YawRange,
    PitchRange,
    MinFlick,
    StrafeMin,
    StrafeMax,
    TurnMin,
    TurnMax,
    Lane,
    PeekSpeedMin,
    PeekSpeedMax,
    HoldMin,
    HoldMax,
    DelayMin,
    DelayMax,
    /// Not a scenario field: the playlist draft step length.
    StepSecs,
}

impl CustomField {
    /// (label, precision, step, soft range)
    fn spec(self) -> (&'static str, i32, f64, (f32, f32)) {
        match self {
            CustomField::Duration => ("Length (s)", 0, 5.0, (10.0, 180.0)),
            CustomField::Size => ("Target size (x)", 2, 0.05, (0.2, 3.0)),
            CustomField::Lifetime => ("Decay (s, 0 = off)", 2, 0.1, (0.0, 3.0)),
            CustomField::GridCols => ("Grid columns", 0, 1.0, (1.0, 9.0)),
            CustomField::GridRows => ("Grid rows", 0, 1.0, (1.0, 9.0)),
            CustomField::GridSpacing => ("Cell spacing (deg)", 1, 0.5, (2.0, 20.0)),
            CustomField::GridActive => ("Targets at once", 0, 1.0, (1.0, 8.0)),
            CustomField::YawRange => ("Horizontal range (+- deg)", 0, 1.0, (0.0, 70.0)),
            CustomField::PitchRange => ("Vertical range (+- deg)", 0, 1.0, (0.0, 30.0)),
            CustomField::MinFlick => ("Min flick distance (deg)", 0, 1.0, (0.0, 40.0)),
            CustomField::StrafeMin => ("Strafe speed min (m/s)", 1, 0.5, (0.5, 12.0)),
            CustomField::StrafeMax => ("Strafe speed max (m/s)", 1, 0.5, (0.5, 12.0)),
            CustomField::TurnMin => ("Direction change min (s)", 2, 0.05, (0.05, 3.0)),
            CustomField::TurnMax => ("Direction change max (s)", 2, 0.05, (0.05, 3.0)),
            CustomField::Lane => ("Lane half-width (m)", 1, 0.5, (1.0, 12.0)),
            CustomField::PeekSpeedMin => ("Peek speed min (m/s)", 1, 0.5, (1.0, 12.0)),
            CustomField::PeekSpeedMax => ("Peek speed max (m/s)", 1, 0.5, (1.0, 12.0)),
            CustomField::HoldMin => ("Hold min (s)", 2, 0.05, (0.0, 1.5)),
            CustomField::HoldMax => ("Hold max (s)", 2, 0.05, (0.0, 1.5)),
            CustomField::DelayMin => ("Cue delay min (s)", 2, 0.1, (0.2, 5.0)),
            CustomField::DelayMax => ("Cue delay max (s)", 2, 0.1, (0.2, 5.0)),
            CustomField::StepSecs => ("Step length (s)", 0, 5.0, (10.0, 180.0)),
        }
    }

    /// Fields shown for a drill kind, after the common ones.
    fn for_kind(kind: DrillKind) -> &'static [CustomField] {
        use CustomField::*;
        match kind {
            DrillKind::Gridshot => &[GridCols, GridRows, GridSpacing, GridActive],
            DrillKind::Microflick | DrillKind::Peripheral => &[YawRange, PitchRange, MinFlick],
            DrillKind::HeadLevel => &[YawRange, MinFlick],
            DrillKind::Tracking => &[StrafeMin, StrafeMax, TurnMin, TurnMax, Lane],
            DrillKind::Peek => &[PeekSpeedMin, PeekSpeedMax, HoldMin, HoldMax],
            DrillKind::Reaction => &[DelayMin, DelayMax],
        }
    }

    fn config_value(self, config: &mut DrillConfig) -> Option<&mut f32> {
        Some(match self {
            CustomField::Size => &mut config.size,
            CustomField::Lifetime => &mut config.lifetime,
            CustomField::GridSpacing => &mut config.grid_spacing,
            CustomField::YawRange => &mut config.yaw_range,
            CustomField::PitchRange => &mut config.pitch_range,
            CustomField::MinFlick => &mut config.min_flick,
            CustomField::StrafeMin => &mut config.strafe_min,
            CustomField::StrafeMax => &mut config.strafe_max,
            CustomField::TurnMin => &mut config.turn_min,
            CustomField::TurnMax => &mut config.turn_max,
            CustomField::Lane => &mut config.lane,
            CustomField::PeekSpeedMin => &mut config.peek_speed_min,
            CustomField::PeekSpeedMax => &mut config.peek_speed_max,
            CustomField::HoldMin => &mut config.hold_min,
            CustomField::HoldMax => &mut config.hold_max,
            CustomField::DelayMin => &mut config.delay_min,
            CustomField::DelayMax => &mut config.delay_max,
            _ => return None,
        })
    }

    fn get(self, content: &CustomContent, editing: Editing, draft: &PlaylistDraft) -> f32 {
        if self == CustomField::StepSecs {
            return draft.secs;
        }
        let Editing::DrillKind(id) = editing else { return 0.0 };
        let Some(scenario) = content.scenario(id) else { return 0.0 };
        let mut config = scenario.config.clone();
        match self {
            CustomField::Duration => scenario.duration,
            CustomField::GridCols => config.grid_cols as f32,
            CustomField::GridRows => config.grid_rows as f32,
            CustomField::GridActive => config.grid_active as f32,
            _ => self.config_value(&mut config).map_or(0.0, |v| *v),
        }
    }

    fn set(self, content: &mut CustomContent, editing: Editing, draft: &mut PlaylistDraft, value: f32) {
        if self == CustomField::StepSecs {
            draft.secs = value.round().clamp(5.0, 600.0);
            return;
        }
        let Editing::DrillKind(id) = editing else { return };
        let Some(scenario) = content.scenario_mut(id) else { return };
        match self {
            CustomField::Duration => scenario.duration = value.round().clamp(5.0, 600.0),
            CustomField::GridCols => scenario.config.grid_cols = value.round().max(1.0) as u32,
            CustomField::GridRows => scenario.config.grid_rows = value.round().max(1.0) as u32,
            CustomField::GridActive => scenario.config.grid_active = value.round().max(1.0) as u32,
            _ => {
                if let Some(slot) = self.config_value(&mut scenario.config) {
                    *slot = value;
                }
            }
        }
        scenario.config.sanitize();
    }
}

// ---------------------------------------------------------------------------------------------
// Page layout (static part; lists and editor are filled in by `rebuild_custom`)
// ---------------------------------------------------------------------------------------------

pub fn custom_lists_column() -> impl Scene {
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
                Children [
                    @new_scenario_button(DrillKind::Gridshot)
                    --
                    @new_scenario_button(DrillKind::Microflick)
                    --
                    @new_scenario_button(DrillKind::Tracking)
                    --
                    @new_scenario_button(DrillKind::HeadLevel)
                    --
                    @new_scenario_button(DrillKind::Peek)
                    --
                    @new_scenario_button(DrillKind::Reaction)
                    --
                    @new_scenario_button(DrillKind::Peripheral)
                ]
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
                selection.select(Editing::DrillKind(id));
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
            let selected = selection.editing == Editing::DrillKind(id);
            let row = commands
                .spawn_scene(item_row(scenario.name.clone(), Editing::DrillKind(id), selected))
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
            Editing::DrillKind(id) => {
                let Some(scenario) = content.scenario(id) else { continue };
                let kind = scenario.config.kind;
                commands.spawn_scene(overline("Edit scenario")).insert(ChildOf(editor));
                spawn_name_input(&mut commands, editor, Editing::DrillKind(id));
                commands
                    .spawn_scene(label_dim(format!("Based on {}: {}", kind.name(), kind.blurb())))
                    .insert(ChildOf(editor));
                let mut fields = vec![CustomField::Duration, CustomField::Size];
                if scenario.config.supports_decay() {
                    fields.push(CustomField::Lifetime);
                }
                fields.extend_from_slice(CustomField::for_kind(kind));
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
                commands.spawn_scene(custom_field_row(CustomField::StepSecs, draft.secs)).insert(ChildOf(editor));
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
                    @FeathersTextInput { @max_characters: 40usize }
                    NameInput(editing)
                    on(rename)
                ]
            },
        ))
        .insert(ChildOf(parent));
}

fn custom_field_row(field: CustomField, value: f32) -> impl Scene {
    let (text, precision, step, soft) = field.spec();
    let soft = SoftLimit::f32(soft.0..=soft.1);
    let hard = HardLimit::f32(0.0..=600.0);
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
    fields: Query<(Entity, &CustomField, &NumberInputValue)>,
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
            Editing::DrillKind(id) => content.scenario(id).map(|s| s.name.clone()),
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
        Editing::DrillKind(id) => {
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
            Editing::DrillKind(id) => content.scenario(id).map(|s| s.name.as_str()),
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
        ItemAction::EditScenario(id) => selection.select(Editing::DrillKind(id)),
        ItemAction::CopyScenario(id) => {
            if let Some(copy) = content.duplicate_scenario(id) {
                selection.select(Editing::DrillKind(copy));
            }
        }
        ItemAction::DeleteScenario(id) => {
            content.delete_scenario(id);
            let editing = if selection.editing == Editing::DrillKind(id) { Editing::Nothing } else { selection.editing };
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
