//! Shared UI building blocks and the generic setting widgets (number fields and choice buttons).
//!
//! Layout uses the spacing scale in [`super::theme`]; text always uses the Feathers fonts and
//! theme tokens, never the default font or hard-coded colors.

use bevy::{
    app::Propagate,
    feathers::{
        constants::fonts,
        controls::{
            ButtonVariant, FeathersButton, FeathersNumberInput, HardLimit, NumberInputPrecision,
            NumberInputStep, NumberInputValue, SoftLimit,
        },
        display::{caption, label},
        theme::{SurfaceLevel, ThemeBackgroundColor, ThemeBorderColor, ThemeContext, ThemeTextColor, ThemeToken},
        tokens,
    },
    input_focus::tab_navigation::TabGroup,
    prelude::*,
    text::{FontSourceTemplate, FontWeight},
    ui_widgets::{Activate, ValueChange},
};

use super::theme::*;
use crate::{
    scenario::Scenario,
    settings::{
        AspectMode, CROSSHAIR_COLORS, DisplayMode, FinderFocus, FinderMethod, HitSound, PRESETS, SettingField, Settings,
        TARGET_COLORS, fields::FieldSpec,
    },
    sfx::HitSounds,
};

/// Width of every number input in a setting row.
pub const FIELD_W: f32 = 120.0;

// ---------------------------------------------------------------------------------------------
// Surfaces
// ---------------------------------------------------------------------------------------------

/// Full-screen dimmed backdrop that centers its child.
pub fn overlay() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6))
    }
}

/// The main window of a screen, with a thin outline.
pub fn panel() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: px(SPACE_XL),
            row_gap: px(SPACE_L),
            min_width: px(460),
            border: px(1),
            border_radius: BorderRadius::all(px(12)),
        }
        TabGroup
        ThemeBackgroundColor(tokens::WINDOW_BG)
        ThemeBorderColor(tokens::GROUP_BG)
    }
}

/// A raised section without a title; for containers whose content is built by systems.
pub fn surface() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: px(SPACE_L),
            row_gap: px(SPACE_S),
            border_radius: BorderRadius::all(px(8)),
        }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        Propagate::<ThemeContext>(ThemeContext(SurfaceLevel::Higher))
    }
}

/// A raised section with a small caps title.
pub fn card(title: impl Into<String>, body: impl SceneList) -> impl Scene {
    bsn! {
        @surface()
        Children [
            @overline(title)
            Node { margin: UiRect::bottom(px(SPACE_XS)) }
            --
            {body}
        ]
    }
}

/// A small inset box (stat tiles, list rows), one level above its surroundings.
pub fn tile() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(SPACE_M), px(SPACE_S)),
            row_gap: px(2),
            border_radius: BorderRadius::all(px(6)),
        }
        ThemeBackgroundColor(tokens::GROUP_BG)
        Propagate::<ThemeContext>(ThemeContext(SurfaceLevel::Highest))
    }
}

pub fn column() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(SPACE_S),
        }
    }
}

pub fn row() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(SPACE_S),
        }
    }
}

/// A row whose items wrap onto further lines when they run out of width.
pub fn wrap_row() -> impl Scene {
    bsn! {
        @row()
        Node { flex_wrap: FlexWrap::Wrap, row_gap: px(SPACE_S) }
    }
}

pub fn spacer() -> impl Scene {
    bsn! { Node { flex_grow: 1.0 } }
}

/// Thin vertical separator for rows (HUD segments).
pub fn vertical_divider() -> impl Scene {
    bsn! {
        Node { width: px(1), align_self: AlignSelf::Stretch }
        ThemeBackgroundColor(DIVIDER)
    }
}

/// Row of the main actions at the bottom of a screen.
pub fn actions(buttons: impl SceneList) -> impl Scene {
    bsn! {
        @row()
        Node { margin: UiRect::top(px(SPACE_XS)) }
        Children [ {buttons} ]
    }
}

/// Full-width strip pinned `top` from the top of the screen, centering its child.
pub fn hud_anchor(top: Val) -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: top,
            width: percent(100),
            justify_content: JustifyContent::Center,
        }
    }
}

/// Translucent rounded backing for in-game text, readable over the 3D scene.
pub fn hud_bar() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(SPACE_L),
            padding: UiRect::axes(px(SPACE_L + 2.0), px(SPACE_S)),
            border_radius: BorderRadius::all(px(10)),
        }
        ThemeBackgroundColor(HUD_BG)
    }
}

// ---------------------------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------------------------

/// Text in a Feathers font, size and theme color. Every other text helper builds on this.
pub fn styled(text: impl Into<String>, font: &'static str, size: f32, color: ThemeToken) -> impl Scene {
    bsn! {
        Text(text)
        TextFont {
            font: FontSourceTemplate::Handle(font),
            font_size: FontSize::Px(size),
            weight: {if font == fonts::BOLD { FontWeight::BOLD } else { FontWeight::NORMAL }},
        }
        ThemeTextColor(color)
    }
}

pub fn heading(text: impl Into<String>) -> impl Scene {
    styled(text, fonts::BOLD, 26.0, TEXT_STRONG)
}

pub fn subheading(text: impl Into<String>) -> impl Scene {
    styled(text, fonts::BOLD, 16.0, TEXT_STRONG)
}

/// Small dim capitals: section titles and stat captions.
pub fn overline(text: impl Into<String>) -> impl Scene {
    styled(text.into().to_uppercase(), fonts::BOLD, 11.0, tokens::TEXT_DIM)
}

/// A table cell of fixed width.
pub fn cell(text: impl Into<String>, width: f32, color: ThemeToken) -> impl Scene {
    bsn! {
        Node { width: px(width), flex_shrink: 0.0 }
        Children [ @styled(text, fonts::REGULAR, 14.0, color) ]
    }
}

/// Theme token for a percentage change: green when better, red when worse.
pub fn delta_color(percent: f32) -> ThemeToken {
    if percent > 0.05 {
        TEXT_GOOD
    } else if percent < -0.05 {
        TEXT_BAD
    } else {
        tokens::TEXT_DIM
    }
}

/// A captioned value in a tile; tiles in a row share the width equally.
pub fn stat(caption: impl Into<String>, value: impl Into<String>) -> impl Scene {
    bsn! {
        @tile()
        Node { flex_grow: 1.0, flex_basis: px(0) }
        Children [
            @overline(caption)
            --
            @styled(value, fonts::BOLD, 20.0, TEXT_STRONG)
        ]
    }
}

/// The headline number of a screen (score, recommended sensitivity).
pub fn hero(caption: impl Into<String>, value: impl Into<String>) -> impl Scene {
    bsn! {
        @column()
        Node { row_gap: px(0) }
        Children [
            @overline(caption)
            --
            @styled(value, fonts::BOLD, 52.0, TEXT_STRONG)
        ]
    }
}

/// A dim label on the left and a bold value on the right.
pub fn key_value(key: String, value: String, color: ThemeToken) -> impl Scene {
    bsn! {
        @row()
        Node { min_height: px(22) }
        Children [
            @styled(key, fonts::REGULAR, 14.0, tokens::TEXT_DIM)
            --
            @spacer()
            --
            @styled(value, fonts::BOLD, 15.0, color)
        ]
    }
}

/// A table row of [`cell`]s; `striped` rows get a subtle background.
pub fn table_row(cells: impl SceneList, striped: bool) -> impl Scene {
    let background = if striped { tokens::GROUP_BG } else { tokens::LISTROW_BG };
    bsn! {
        @row()
        Node {
            min_height: px(30),
            padding: UiRect::new(px(SPACE_S), px(3), px(3), px(3)),
            column_gap: px(0),
            border_radius: BorderRadius::all(px(4)),
        }
        ThemeBackgroundColor(background)
        Children [ {cells} ]
    }
}

// ---------------------------------------------------------------------------------------------
// Buttons and rows
// ---------------------------------------------------------------------------------------------

/// A main action button.
pub fn button(text: &'static str, variant: ButtonVariant) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { @caption(text) },
            @variant: variant,
        }
        Node { min_width: px(120), height: px(30), padding: UiRect::horizontal(px(SPACE_L)) }
    }
}

/// A small button for list rows; the caption can be built at runtime.
pub fn compact_button(text: impl Into<String>, variant: ButtonVariant) -> impl Scene {
    let text = text.into();
    bsn! {
        @FeathersButton {
            @caption: bsn! { @caption(text) },
            @variant: variant,
        }
        Node { min_width: px(56), height: px(26), padding: UiRect::horizontal(px(SPACE_M)) }
    }
}

/// A labelled setting: the label on the left, its controls aligned right.
pub fn setting_row(text: impl Into<String>, controls: impl SceneList) -> impl Scene {
    bsn! {
        @row()
        Node { min_height: px(30) }
        Children [
            @label(text)
            --
            @spacer()
            --
            {controls}
        ]
    }
}

// ---------------------------------------------------------------------------------------------
// Arm delay: buttons on screens that appear right after a run start disabled, so clicks still
// in flight from shooting can't land on them.
// ---------------------------------------------------------------------------------------------

/// Spawn together with `InteractionDisabled`; the button unlocks once the delay has passed.
#[derive(Component, Clone, Copy, Debug)]
pub struct ArmDelay(f32);

impl Default for ArmDelay {
    fn default() -> Self {
        Self(1.0)
    }
}

pub fn arm_buttons(time: Res<Time<Real>>, mut buttons: Query<(Entity, &mut ArmDelay)>, mut commands: Commands) {
    for (entity, mut delay) in &mut buttons {
        delay.0 -= time.delta_secs();
        if delay.0 <= 0.0 {
            commands.entity(entity).remove::<(ArmDelay, bevy::ui::InteractionDisabled)>();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Choice buttons: a button that sets one discrete setting and highlights while it is active.
// ---------------------------------------------------------------------------------------------

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum Choice {
    Preset(usize),
    FinderDrill(Scenario),
    Display(DisplayMode),
    Aspect(AspectMode),
    Vsync(bool),
    Msaa(u32),
    HitSound(HitSound),
    CrosshairColor(usize),
    TargetColor(usize),
    FinderMethod(FinderMethod),
    FinderFocus(FinderFocus),
    CenterDot(bool),
    InnerLines(bool),
    Outlines(bool),
    Discord(bool),
}

impl Default for Choice {
    fn default() -> Self {
        Choice::Preset(0)
    }
}

impl Choice {
    fn apply(self, settings: &mut Settings) {
        match self {
            Choice::Preset(index) => settings.apply_preset(&PRESETS[index]),
            Choice::FinderDrill(scenario) => settings.finder_drill = scenario,
            Choice::Display(mode) => settings.video.display = mode,
            Choice::Aspect(mode) => settings.video.aspect = mode,
            Choice::Vsync(on) => settings.video.vsync = on,
            Choice::Msaa(samples) => settings.video.msaa = samples,
            Choice::HitSound(sound) => settings.audio.hit_sound = sound,
            Choice::CrosshairColor(index) => settings.crosshair.color = CROSSHAIR_COLORS[index].1,
            Choice::TargetColor(index) => settings.targets.color = TARGET_COLORS[index].1,
            Choice::FinderMethod(method) => settings.finder_method = method,
            Choice::FinderFocus(focus) => settings.finder_focus = focus,
            Choice::CenterDot(on) => settings.crosshair.center_dot = on,
            Choice::InnerLines(on) => settings.crosshair.inner_lines = on,
            Choice::Outlines(on) => settings.crosshair.outlines = on,
            Choice::Discord(on) => settings.discord.enabled = on,
        }
    }

    fn is_selected(self, settings: &Settings) -> bool {
        match self {
            Choice::Preset(index) => settings.matching_preset() == Some(&PRESETS[index]),
            Choice::FinderDrill(scenario) => settings.finder_drill == scenario,
            Choice::Display(mode) => settings.video.display == mode,
            Choice::Aspect(mode) => settings.video.aspect == mode,
            Choice::Vsync(on) => settings.video.vsync == on,
            Choice::Msaa(samples) => settings.video.msaa == samples,
            Choice::HitSound(sound) => settings.audio.hit_sound == sound,
            Choice::CrosshairColor(index) => settings.crosshair.color == CROSSHAIR_COLORS[index].1,
            Choice::TargetColor(index) => settings.targets.color == TARGET_COLORS[index].1,
            Choice::FinderMethod(method) => settings.finder_method == method,
            Choice::FinderFocus(focus) => settings.finder_focus == focus,
            Choice::CenterDot(on) => settings.crosshair.center_dot == on,
            Choice::InnerLines(on) => settings.crosshair.inner_lines == on,
            Choice::Outlines(on) => settings.crosshair.outlines == on,
            Choice::Discord(on) => settings.discord.enabled == on,
        }
    }
}

pub fn choice(text: &'static str, choice: Choice) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { @caption(text) },
        }
        Node { min_width: px(64), height: px(28), padding: UiRect::horizontal(px(SPACE_M)) }
        choice
        on(pick_choice)
    }
}

/// A choice button with a color swatch in front of its caption.
pub fn swatch_choice(text: &'static str, rgb: [u8; 3], choice: Choice) -> impl Scene {
    let swatch = BackgroundColor(Color::srgb_u8(rgb[0], rgb[1], rgb[2]));
    bsn! {
        @FeathersButton {
            @caption: bsn_list! {
                Node { width: px(10), height: px(10), border_radius: BorderRadius::all(px(3)) }
                swatch
                --
                @caption(text)
            },
        }
        Node { min_width: px(64), height: px(28), padding: UiRect::horizontal(px(SPACE_M)), column_gap: px(6) }
        choice
        on(pick_choice)
    }
}

/// Swatch choices for the target colors.
pub fn target_color_choices() -> impl Scene {
    bsn! {
        @wrap_row()
        Children [
            @swatch_choice(TARGET_COLORS[0].0, TARGET_COLORS[0].1, Choice::TargetColor(0))
            --
            @swatch_choice(TARGET_COLORS[1].0, TARGET_COLORS[1].1, Choice::TargetColor(1))
            --
            @swatch_choice(TARGET_COLORS[2].0, TARGET_COLORS[2].1, Choice::TargetColor(2))
            --
            @swatch_choice(TARGET_COLORS[3].0, TARGET_COLORS[3].1, Choice::TargetColor(3))
            --
            @swatch_choice(TARGET_COLORS[4].0, TARGET_COLORS[4].1, Choice::TargetColor(4))
            --
            @swatch_choice(TARGET_COLORS[5].0, TARGET_COLORS[5].1, Choice::TargetColor(5))
        ]
    }
}

/// Swatch choices for Valorant's crosshair colors.
pub fn crosshair_color_choices() -> impl Scene {
    bsn! {
        @wrap_row()
        Children [
            @swatch_choice(CROSSHAIR_COLORS[0].0, CROSSHAIR_COLORS[0].1, Choice::CrosshairColor(0))
            --
            @swatch_choice(CROSSHAIR_COLORS[1].0, CROSSHAIR_COLORS[1].1, Choice::CrosshairColor(1))
            --
            @swatch_choice(CROSSHAIR_COLORS[2].0, CROSSHAIR_COLORS[2].1, Choice::CrosshairColor(2))
            --
            @swatch_choice(CROSSHAIR_COLORS[3].0, CROSSHAIR_COLORS[3].1, Choice::CrosshairColor(3))
            --
            @swatch_choice(CROSSHAIR_COLORS[4].0, CROSSHAIR_COLORS[4].1, Choice::CrosshairColor(4))
            --
            @swatch_choice(CROSSHAIR_COLORS[5].0, CROSSHAIR_COLORS[5].1, Choice::CrosshairColor(5))
            --
            @swatch_choice(CROSSHAIR_COLORS[6].0, CROSSHAIR_COLORS[6].1, Choice::CrosshairColor(6))
            --
            @swatch_choice(CROSSHAIR_COLORS[7].0, CROSSHAIR_COLORS[7].1, Choice::CrosshairColor(7))
        ]
    }
}

fn pick_choice(
    activate: On<Activate>,
    choices: Query<&Choice>,
    mut settings: ResMut<Settings>,
    sounds: Res<HitSounds>,
    mut commands: Commands,
) {
    let Ok(&choice) = choices.get(activate.event_target()) else { return };
    choice.apply(&mut settings);
    if let Choice::HitSound(_) = choice {
        sounds.play(&mut commands, &settings.audio);
    }
}

pub fn highlight_choices(settings: Res<Settings>, mut buttons: Query<(&Choice, &mut ButtonVariant)>) {
    for (&choice, mut variant) in &mut buttons {
        variant.set_if_neq(ButtonVariant::selected(choice.is_selected(&settings)));
    }
}

// ---------------------------------------------------------------------------------------------
// Number fields
// ---------------------------------------------------------------------------------------------

/// A labelled number input row.
pub fn number_field(field: SettingField, settings: &Settings) -> impl Scene {
    let text = field.spec().label;
    setting_row(text, bsn_list! { @number_input(field, settings, FIELD_W) })
}

/// A bare number input of fixed width.
pub fn number_input(field: SettingField, settings: &Settings, width: f32) -> impl Scene {
    let FieldSpec { precision, step, soft, hard, .. } = field.spec();
    let value = field.get(settings);
    let soft = SoftLimit::f32(soft.0..=soft.1);
    let hard = HardLimit::f32(hard.0..=hard.1);
    bsn! {
        Node { display: Display::Flex, width: px(width), flex_shrink: 0.0 }
        Children [
            @FeathersNumberInput
            NumberInputValue::F32(value)
            NumberInputPrecision(precision)
            NumberInputStep(step)
            soft
            hard
            Node { flex_grow: 1.0 }
            field
            on(edit_field)
        ]
    }
}

fn edit_field(
    change: On<ValueChange<f32>>,
    fields: Query<&SettingField>,
    mut settings: ResMut<Settings>,
    sounds: Res<HitSounds>,
    mut commands: Commands,
) {
    let input = change.event_target();
    let Ok(&field) = fields.get(input) else { return };
    // Leaving a field always reports its value; only a real change should mark the settings
    // for saving or replay the volume preview.
    let before = field.get(&settings);
    field.set(settings.bypass_change_detection(), change.value);
    let after = field.get(&settings);
    if after != before {
        settings.set_changed();
    }
    // The widget is controlled: echo the (clamped/rounded) value back so it displays it.
    commands.entity(input).insert(NumberInputValue::F32(after));
    if field == SettingField::Volume && change.is_final && after != before {
        sounds.play(&mut commands, &settings.audio);
    }
}

/// Pushes setting changes made elsewhere (presets, finder results) into the number inputs.
pub fn sync_fields(settings: Res<Settings>, fields: Query<(Entity, &SettingField, &NumberInputValue)>, mut commands: Commands) {
    for (entity, field, value) in &fields {
        let wanted = field.get(&settings);
        if !matches!(value, NumberInputValue::F32(shown) if *shown == wanted) {
            commands.entity(entity).insert(NumberInputValue::F32(wanted));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Chart primitives (plain entities; used inside containers built by systems)
// ---------------------------------------------------------------------------------------------

/// Chart labels: small text in the regular Feathers font.
pub fn text(commands: &mut Commands, parent: Entity, value: String, size: f32, color: ThemeToken, node: Node) {
    commands.spawn_scene(bsn! { @styled(value, fonts::REGULAR, size, color) node }).insert(ChildOf(parent));
}

pub fn absolute(left: f32, top: f32, width: f32, height: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(left),
        top: px(top),
        width: px(width),
        height: px(height),
        ..default()
    }
}

/// Connected 2 px line through `points` (in the parent's pixel space), as rotated thin nodes.
pub fn polyline(commands: &mut Commands, parent: Entity, points: &[Vec2], color: ThemeToken) {
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let delta = b - a;
        let length = delta.length();
        let center = (a + b) * 0.5;
        commands.spawn((
            ChildOf(parent),
            absolute(center.x - length * 0.5, center.y - 1.0, length, 2.0),
            UiTransform { rotation: Rot2::radians(delta.y.atan2(delta.x)), ..default() },
            ThemeBackgroundColor(color.clone()),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_a_preset_selects_it_and_keeps_cm_per_360() {
        let mut settings = Settings::default();
        let before = settings.cm_per_360(settings.sens);
        Choice::Preset(1).apply(&mut settings);
        assert!(Choice::Preset(1).is_selected(&settings));
        assert!(!Choice::Preset(0).is_selected(&settings));
        assert!(Choice::Aspect(AspectMode::Native).is_selected(&settings));
        assert!((settings.cm_per_360(settings.sens) - before).abs() < 1e-3);
    }

    #[test]
    fn armed_buttons_stay_disabled_until_the_delay_passes() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let mut app = App::new();
        app.insert_resource(Time::<Real>::new(start)).add_systems(Update, arm_buttons);
        let button = app.world_mut().spawn((ArmDelay::default(), bevy::ui::InteractionDisabled)).id();
        let advance_to = |app: &mut App, ms: u64| {
            app.world_mut().resource_mut::<Time<Real>>().update_with_instant(start + Duration::from_millis(ms));
            app.update();
        };
        // The clock's first update only records the start; deltas begin after it.
        advance_to(&mut app, 0);
        // A stray click right after the screen appears must not count.
        advance_to(&mut app, 500);
        assert!(app.world().get::<bevy::ui::InteractionDisabled>(button).is_some());
        advance_to(&mut app, 1100);
        assert!(app.world().get::<bevy::ui::InteractionDisabled>(button).is_none());
        assert!(app.world().get::<ArmDelay>(button).is_none());
    }
}
