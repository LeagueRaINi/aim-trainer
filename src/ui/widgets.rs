//! Shared UI building blocks: surfaces, text, buttons, setting rows and the arm delay.
//!
//! Layout uses the spacing scale in [`super::theme`], text its type scale, the Feathers fonts
//! and theme tokens: pages never use the default font, a font size of their own or a
//! hard-coded color.

use bevy::app::PropagateOver;
use bevy::feathers::constants::fonts;
use bevy::feathers::controls::{FeathersScrollbar, ScrollbarGutter};
use bevy::feathers::focus::FocusIndicator;
use bevy::feathers::font_styles::InheritableFont;
use bevy::feathers::theme::{
    InheritableThemeTextColor, ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemeToken, ThemedText,
};
use bevy::feathers::tokens;
use bevy::input_focus::tab_navigation::{TabGroup, TabIndex};
use bevy::picking::cursor::EntityCursor;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{FontFeatureTag, FontFeatures, FontSourceTemplate, FontWeight};
use bevy::ui::{InteractionDisabled, Pressed};
use bevy::ui_widgets::{Activate, Button, ControlOrientation, ScrollArea};
use bevy::window::SystemCursorIcon;

use super::theme::*;

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
        ThemeBackgroundColor(SCRIM)
    }
}

/// A window floating over the 3D scene, without padding or layout: the base of [`panel`] and
/// of the main menu.
pub fn window() -> impl Scene {
    let shadow = window_shadow();
    bsn! {
        Node {
            display: Display::Flex,
            border: px(1),
            border_radius: BorderRadius::all(px(RADIUS_L)),
        }
        TabGroup
        ThemeBackgroundColor(tokens::WINDOW_BG)
        ThemeBorderColor(BORDER)
        shadow
    }
}

/// The window of a post-run screen or dialog: a padded column.
pub fn panel() -> impl Scene {
    bsn! {
        @window()
        Node {
            flex_direction: FlexDirection::Column,
            padding: px(SPACE_XL),
            row_gap: px(SPACE_L),
            min_width: px(460),
        }
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
            border_radius: BorderRadius::all(px(RADIUS_M)),
        }
        ThemeBackgroundColor(SURFACE_CARD)
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

/// A small inset box (stat tiles, list rows), one level above its card.
pub fn tile() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(SPACE_M), px(SPACE_S)),
            row_gap: px(2),
            border_radius: BorderRadius::all(px(RADIUS_S)),
        }
        ThemeBackgroundColor(SURFACE_TILE)
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

/// Room on the right of a [`scroll_column`] for its scrollbar, while it shows.
pub const SCROLLBAR_GUTTER: f32 = 14.0;

/// `content` in a column that scrolls with the mouse wheel once it is taller than the space its
/// parent gives it, so long lists stay inside the fixed-size menu. A scrollbar shows (and can
/// be dragged) while it overflows, so it is clear there is more. Size the column by patching
/// this node; `content` sits in an inner column `SPACE_L` apart.
pub fn scroll_column(content: impl SceneList) -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            min_height: px(0),
            padding: UiRect::right(px(SCROLLBAR_GUTTER)),
        }
        ScrollbarGutter(px(SCROLLBAR_GUTTER))
        Children [
            #scroller
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                min_height: px(0),
                row_gap: px(SPACE_L),
                overflow: Overflow::scroll_y(),
            }
            ScrollArea
            Children [ {content} ]
            --
            @FeathersScrollbar {
                @target: #scroller,
                @orientation: {ControlOrientation::Vertical}
            }
            Node {
                position_type: PositionType::Absolute,
                right: px(2),
                top: px(0),
                bottom: px(0),
                width: px(6),
            }
        ]
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

/// Thin horizontal separator between the parts of a column.
pub fn rule() -> impl Scene {
    bsn! {
        Node { height: px(1), align_self: AlignSelf::Stretch, flex_shrink: 0.0 }
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
            border: px(1),
            border_radius: BorderRadius::all(px(RADIUS_M)),
        }
        ThemeBackgroundColor(HUD_BG)
        ThemeBorderColor(DIVIDER)
    }
}

/// A colored bar on the left edge of a row or tile: the group of a drill, the tone of a
/// [`callout`].
pub fn marker(color: ThemeToken) -> impl Scene {
    bsn! {
        Node { width: px(3), align_self: AlignSelf::Stretch, flex_shrink: 0.0, border_radius: BorderRadius::all(px(2)) }
        ThemeBackgroundColor(color)
    }
}

/// A small square of a chart color, for legends.
pub fn swatch(color: ThemeToken) -> impl Scene {
    bsn! {
        Node { width: px(10), height: px(10), flex_shrink: 0.0, border_radius: BorderRadius::all(px(3)) }
        ThemeBackgroundColor(color)
    }
}

/// A chart color and what it stands for; items of a legend wrap as a whole.
pub fn legend_item(color: ThemeToken, meaning: &'static str) -> impl Scene {
    bsn! {
        @row()
        Node { column_gap: px(6), margin: UiRect::right(px(SPACE_S)) }
        Children [ @swatch(color) -- @small(meaning) ]
    }
}

/// The takeaway of a section in plain words: a `tone`-colored title over one or two sentences.
pub fn callout(tone: ThemeToken, title: impl Into<String>, text: impl Into<String>) -> impl Scene {
    let bar = tone.clone();
    bsn! {
        @row()
        Node {
            column_gap: px(SPACE_M),
            padding: UiRect::axes(px(SPACE_M), px(SPACE_S)),
            border_radius: BorderRadius::all(px(RADIUS_S)),
        }
        ThemeBackgroundColor(SURFACE_TILE)
        Children [
            @marker(bar)
            --
            @column()
            Node { flex_grow: 1.0, flex_basis: px(0), row_gap: px(2) }
            Children [
                @styled(title, fonts::BOLD, FONT_BODY, tone)
                --
                @styled(text, fonts::REGULAR, FONT_BODY, tokens::TEXT_MAIN)
            ]
        ]
    }
}

/// What to do when a list or page has nothing to show yet.
pub fn empty_state(title: impl Into<String>, text: impl Into<String>) -> impl Scene {
    bsn! {
        @surface()
        Node { padding: px(SPACE_XL), row_gap: px(SPACE_XS) }
        Children [ @subheading(title) -- @label_dim(text) ]
    }
}

/// One numbered step of a short how-to: a title and a sentence or two.
pub fn numbered_step(number: &'static str, title: &'static str, text: &'static str) -> impl Scene {
    bsn! {
        @row()
        Node { column_gap: px(SPACE_M), align_items: AlignItems::FlexStart }
        Children [
            Node {
                width: px(26),
                height: px(26),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::MAX,
            }
            ThemeBackgroundColor(ACCENT_SOFT)
            Children [ @styled(number, fonts::BOLD, FONT_BODY, TEXT_ACCENT) ]
            --
            @column()
            Node { flex_grow: 1.0, flex_basis: px(0), row_gap: px(2) }
            Children [ @label_strong(title) -- @label_dim(text) ]
        ]
    }
}

// ---------------------------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------------------------

/// Text in a Feathers font, size and theme color. Every other text helper builds on this. It
/// keeps its look inside buttons, whose own font and color only reach [`caption`]s.
pub fn styled(text: impl Into<String>, font: &'static str, size: f32, color: ThemeToken) -> impl Scene {
    bsn! {
        Text(text)
        TextFont {
            font: FontSourceTemplate::Handle(font),
            font_size: FontSize::Px(size),
            weight: {if font == fonts::BOLD { FontWeight::BOLD } else { FontWeight::NORMAL }},
        }
        PropagateOver<TextFont>
        ThemeTextColor(color)
    }
}

/// Equal-width digits for numbers that change in place or line up in columns: Fira Sans'
/// default digits differ in width.
pub fn tabular() -> FontFeatures {
    [FontFeatureTag::TABULAR_FIGURES].into()
}

/// The title of a page or screen.
pub fn heading(text: impl Into<String>) -> impl Scene {
    styled(text, fonts::BOLD, FONT_H1, TEXT_STRONG)
}

/// The title of a card or of the selected item.
pub fn subheading(text: impl Into<String>) -> impl Scene {
    styled(text, fonts::BOLD, FONT_H2, TEXT_STRONG)
}

/// Body text.
pub fn label(text: impl Into<String>) -> impl Scene {
    styled(text, fonts::REGULAR, FONT_BODY, tokens::TEXT_MAIN)
}

/// Secondary body text.
pub fn label_dim(text: impl Into<String>) -> impl Scene {
    styled(text, fonts::REGULAR, FONT_BODY, tokens::TEXT_DIM)
}

/// Body text that stands out.
pub fn label_strong(text: impl Into<String>) -> impl Scene {
    styled(text, fonts::BOLD, FONT_BODY, TEXT_STRONG)
}

/// A hint or explanation under the thing it explains.
pub fn small(text: impl Into<String>) -> impl Scene {
    styled(text, fonts::REGULAR, FONT_SMALL, tokens::TEXT_DIM)
}

/// Small dim capitals: section titles and stat captions.
pub fn overline(text: impl Into<String>) -> impl Scene {
    overline_colored(text, tokens::TEXT_DIM)
}

pub fn overline_colored(text: impl Into<String>, color: ThemeToken) -> impl Scene {
    styled(text.into().to_uppercase(), fonts::BOLD, FONT_MICRO, color)
}

/// The caption of a button: takes the button's font and color, which follow its state.
pub fn caption(text: impl Into<String>) -> impl Scene {
    bsn! {
        Text(text)
        ThemedText
    }
}

/// A key of the keyboard (or "Click"), as drawn on it.
pub fn keycap(key: &'static str) -> impl Scene {
    bsn! {
        Node {
            min_width: px(22),
            height: px(20),
            padding: UiRect::horizontal(px(6)),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::new(px(1), px(1), px(1), px(2)),
            border_radius: BorderRadius::all(px(5)),
        }
        ThemeBackgroundColor(SURFACE_TILE)
        ThemeBorderColor(BORDER_STRONG)
        Children [ @styled(key, fonts::BOLD, FONT_MICRO, TEXT_STRONG) ]
    }
}

/// A key and what it does.
pub fn key_hint(key: &'static str, action: &'static str) -> impl Scene {
    bsn! {
        @row()
        Node { column_gap: px(6) }
        Children [ @keycap(key) -- @styled(action, fonts::REGULAR, FONT_SMALL, tokens::TEXT_MAIN) ]
    }
}

/// A short outlined tag ("Adaptive", "New best").
pub fn badge(text: impl Into<String>, color: ThemeToken) -> impl Scene {
    let outline = color.clone();
    bsn! {
        Node {
            height: px(22),
            padding: UiRect::horizontal(px(SPACE_S)),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            border: px(1),
            border_radius: BorderRadius::all(px(11)),
        }
        ThemeBorderColor(outline)
        Children [ @styled(text, fonts::BOLD, FONT_MICRO, color) ]
    }
}

/// A table cell of fixed width.
pub fn cell(text: impl Into<String>, width: f32, color: ThemeToken) -> impl Scene {
    bsn! {
        Node { width: px(width), flex_shrink: 0.0 }
        Children [ @styled(text, fonts::REGULAR, FONT_BODY, color) TextFont { font_features: {tabular()} } ]
    }
}

/// A column title of a table: a [`cell`] in small dim text.
pub fn header_cell(text: &'static str, width: f32) -> impl Scene {
    bsn! {
        Node { width: px(width), flex_shrink: 0.0 }
        Children [ @styled(text, fonts::REGULAR, FONT_SMALL, tokens::TEXT_DIM) ]
    }
}

/// A row of [`header_cell`]s, lined up with the [`table_row`]s under it.
pub fn table_header(cells: impl SceneList) -> impl Scene {
    bsn! {
        @row()
        Node { padding: UiRect::new(px(SPACE_S), px(3), px(0), px(SPACE_XS)), column_gap: px(0) }
        Children [ {cells} ]
    }
}

/// A score as text. Most scores are whole numbers in the hundreds or thousands and show without
/// decimals (also their averages); scores on a 0-100 scale (the Calm aim test, a mean of four
/// parts) keep up to two, so a card and the list next to it never disagree about the same run.
pub fn score_text(score: f32) -> String {
    if score.abs() >= 100.0 {
        return format!("{score:.0}");
    }
    let text = format!("{score:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
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
    stat_colored(caption, value, TEXT_STRONG)
}

pub fn stat_colored(caption: impl Into<String>, value: impl Into<String>, color: ThemeToken) -> impl Scene {
    bsn! {
        @tile()
        Node { flex_grow: 1.0, flex_basis: px(0) }
        Children [
            @overline(caption)
            --
            @styled(value, fonts::BOLD, FONT_STAT, color)
        ]
    }
}

/// A [`stat`] with a line under the value saying what it measures.
pub fn stat_hint(caption: impl Into<String>, value: impl Into<String>, hint: impl Into<String>) -> impl Scene {
    bsn! {
        @tile()
        Node { flex_grow: 1.0, flex_basis: px(0) }
        Children [
            @overline(caption)
            --
            @styled(value, fonts::BOLD, FONT_STAT, TEXT_STRONG)
            --
            @small(hint)
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
            @styled(value, fonts::BOLD, FONT_DISPLAY, TEXT_STRONG)
        ]
    }
}

/// A table row of [`cell`]s; `striped` rows get a subtle background.
pub fn table_row(cells: impl SceneList, striped: bool) -> impl Scene {
    let background = if striped { SURFACE_TILE } else { CLEAR };
    bsn! {
        @row()
        Node {
            min_height: px(30),
            padding: UiRect::new(px(SPACE_S), px(3), px(3), px(3)),
            column_gap: px(0),
            border_radius: BorderRadius::all(px(RADIUS_S)),
        }
        ThemeBackgroundColor(background)
        Children [ {cells} ]
    }
}

// ---------------------------------------------------------------------------------------------
// Buttons and rows
// ---------------------------------------------------------------------------------------------

/// What a button is for, which decides its look. A screen has at most one `Primary`.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Btn {
    /// The main action of a screen: filled with the accent color.
    Primary,
    /// Any other action.
    #[default]
    Secondary,
    /// A minor action that only shows a fill when pointed at.
    Ghost,
    /// The chosen one of a set of choices, tabs or list items: tinted with the accent color.
    Selected,
    /// A minor action that destroys something.
    Danger,
    /// A text-like action that reveals more.
    Link,
    /// An item of a list of tiles that is not the chosen one.
    Tile,
}

/// The theme colors of a button in one state.
struct Look {
    fill: ThemeToken,
    outline: ThemeToken,
    text: ThemeToken,
}

impl Btn {
    /// `Selected` or, for the other choices of the set, `Secondary`.
    pub fn selected(selected: bool) -> Self {
        if selected { Btn::Selected } else { Btn::Secondary }
    }

    fn look(self, disabled: bool, pressed: bool, hovered: bool) -> Look {
        // A fill per state: resting, hovered, pressed.
        let state = |rest: ThemeToken, hover: ThemeToken, press: ThemeToken| match (pressed, hovered) {
            (true, _) => press,
            (false, true) => hover,
            (false, false) => rest,
        };
        let (fill, outline, text) = match self {
            Btn::Primary if disabled => (FILL_DISABLED, FILL_DISABLED, TEXT_DISABLED),
            Btn::Secondary | Btn::Selected | Btn::Tile if disabled => (FILL_DISABLED, BORDER, TEXT_DISABLED),
            Btn::Ghost | Btn::Danger | Btn::Link if disabled => (CLEAR, CLEAR, TEXT_DISABLED),
            Btn::Primary => {
                let fill = state(ACCENT, ACCENT_HOVER, ACCENT_PRESSED);
                (fill.clone(), fill, TEXT_ON_ACCENT)
            },
            Btn::Secondary => (state(FILL, FILL_HOVER, FILL_PRESSED), BORDER_STRONG, TEXT_STRONG),
            Btn::Ghost => (state(CLEAR, GHOST_HOVER, GHOST_PRESSED), CLEAR, tokens::TEXT_MAIN),
            Btn::Selected => (state(ACCENT_SOFT, ACCENT_SOFT_HOVER, ACCENT_SOFT_HOVER), ACCENT_LINE, TEXT_ACCENT),
            Btn::Danger => (state(CLEAR, BAD_SOFT, BAD_SOFT), CLEAR, TEXT_BAD),
            Btn::Link => (state(CLEAR, GHOST_HOVER, GHOST_PRESSED), CLEAR, TEXT_ACCENT),
            Btn::Tile => (
                state(SURFACE_TILE, SURFACE_TILE_HOVER, FILL_HOVER),
                state(SURFACE_TILE, BORDER_STRONG, BORDER_STRONG),
                TEXT_STRONG,
            ),
        };
        Look { fill, outline, text }
    }
}

/// Keeps every button's colors in step with its [`Btn`] kind and its hover, pressed and disabled
/// state. Only visits buttons whose state changed.
#[allow(clippy::type_complexity)]
pub fn style_buttons(
    changed: Query<
        Entity,
        (With<Btn>, Or<(Changed<Hovered>, Changed<Btn>, Added<Pressed>, Added<InteractionDisabled>)>),
    >,
    mut released: RemovedComponents<Pressed>,
    mut enabled: RemovedComponents<InteractionDisabled>,
    buttons: Query<(
        &Btn,
        Has<InteractionDisabled>,
        Has<Pressed>,
        &Hovered,
        &ThemeBackgroundColor,
        &ThemeBorderColor,
        &InheritableThemeTextColor,
    )>,
    mut commands: Commands,
) {
    for entity in changed.iter().chain(released.read()).chain(enabled.read()) {
        let Ok((kind, disabled, pressed, hovered, fill, outline, text)) = buttons.get(entity) else { continue };
        let look = kind.look(disabled, pressed, hovered.get());
        let mut button = commands.entity(entity);
        if fill.0 != look.fill {
            button.insert(ThemeBackgroundColor(look.fill));
        }
        if outline.0 != look.outline {
            button.insert(ThemeBorderColor(look.outline));
        }
        if text.0 != look.text {
            button.insert(InheritableThemeTextColor(look.text));
            let cursor = if disabled { SystemCursorIcon::NotAllowed } else { SystemCursorIcon::Pointer };
            button.insert(EntityCursor::System(cursor));
        }
    }
}

/// A clickable, focusable box in the look of `kind`, `height` px tall; give it `Children`.
/// Emits `Activate` on a click, or on Enter / Space while it has keyboard focus.
pub fn button_box(kind: Btn, height: f32) -> impl Scene {
    let Look { fill, outline, text } = kind.look(false, false, false);
    let font = if kind == Btn::Primary { fonts::BOLD } else { fonts::REGULAR };
    bsn! {
        Node {
            height: px(height),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: px(SPACE_S),
            padding: UiRect::horizontal(px(SPACE_L)),
            border: px(1),
            border_radius: BorderRadius::all(px(RADIUS_S)),
        }
        Button
        kind
        Hovered
        EntityCursor::System(SystemCursorIcon::Pointer)
        TabIndex(0)
        FocusIndicator
        ThemeBackgroundColor(fill)
        ThemeBorderColor(outline)
        InheritableThemeTextColor(text)
        InheritableFont {
            font: font,
            font_size: FontSize::Px(FONT_BODY),
            weight: {if font == fonts::BOLD { FontWeight::BOLD } else { FontWeight::NORMAL }},
        }
    }
}

/// A main action button.
pub fn button(text: &'static str, kind: Btn) -> impl Scene {
    bsn! {
        @button_box(kind, 34.0)
        Node { min_width: px(120) }
        Children [ @caption(text) ]
    }
}

/// The primary action of a page, larger than everything around it.
pub fn cta(text: impl Into<String>) -> impl Scene {
    bsn! {
        @button_box(Btn::Primary, 42.0)
        InheritableFont { font_size: FontSize::Px(FONT_H2) }
        Children [ @caption(text) ]
    }
}

/// A small button for list rows and choices; the caption can be built at runtime.
pub fn compact_button(text: impl Into<String>, kind: Btn) -> impl Scene {
    bsn! {
        @button_box(kind, 28.0)
        Node { min_width: px(56), padding: UiRect::horizontal(px(SPACE_M)) }
        Children [ @caption(text) ]
    }
}

/// A labelled setting: the label on the left, its controls aligned right.
pub fn setting_row(text: impl Into<String>, controls: impl SceneList) -> impl Scene {
    bsn! {
        @row()
        Node { min_height: px(32) }
        Children [
            @label(text)
            --
            @spacer()
            --
            {controls}
        ]
    }
}

/// A [`setting_row`] with a line under the label saying what the setting does.
pub fn setting_row_hint(text: impl Into<String>, hint: impl Into<String>, controls: impl SceneList) -> impl Scene {
    bsn! {
        @row()
        Node { min_height: px(40), column_gap: px(SPACE_M) }
        Children [
            @column()
            Node { flex_grow: 1.0, flex_basis: px(0), row_gap: px(1) }
            Children [ @label(text) -- @small(hint) ]
            --
            {controls}
        ]
    }
}

// ---------------------------------------------------------------------------------------------
// Details: an explanation that stays folded away until asked for.
// ---------------------------------------------------------------------------------------------

#[derive(Component, Clone, Copy, Default)]
struct DetailsBody;

/// A link-like button that unfolds `text` under it: for the full explanation of numbers whose
/// takeaway is already on screen.
pub fn details(summary: &'static str, text: impl Into<String>) -> impl Scene {
    bsn! {
        @column()
        Node { row_gap: px(SPACE_XS), align_items: AlignItems::Start }
        Children [
            @button_box(Btn::Link, 24.0)
            Node { padding: UiRect::horizontal(px(SPACE_S)), margin: UiRect::left(px(-SPACE_S)) }
            InheritableFont { font_size: FontSize::Px(FONT_SMALL) }
            Children [ @caption(summary) ]
            on(toggle_details)
            --
            @small(text)
            Node { display: Display::None }
            DetailsBody
        ]
    }
}

fn toggle_details(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    mut bodies: Query<&mut Node, With<DetailsBody>>,
) {
    let Ok(parent) = parents.get(activate.event_target()) else { return };
    let Ok(siblings) = children.get(parent.parent()) else { return };
    for &sibling in siblings {
        if let Ok(mut node) = bodies.get_mut(sibling) {
            node.display = if node.display == Display::None { Display::Flex } else { Display::None };
        }
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
            commands.entity(entity).remove::<(ArmDelay, InteractionDisabled)>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armed_buttons_stay_disabled_until_the_delay_passes() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let mut app = App::new();
        app.insert_resource(Time::<Real>::new(start)).add_systems(Update, arm_buttons);
        let button = app.world_mut().spawn((ArmDelay::default(), InteractionDisabled)).id();
        let advance_to = |app: &mut App, ms: u64| {
            app.world_mut().resource_mut::<Time<Real>>().update_with_instant(start + Duration::from_millis(ms));
            app.update();
        };
        // The clock's first update only records the start; deltas begin after it.
        advance_to(&mut app, 0);
        // A stray click right after the screen appears must not count.
        advance_to(&mut app, 500);
        assert!(app.world().get::<InteractionDisabled>(button).is_some());
        advance_to(&mut app, 1100);
        assert!(app.world().get::<InteractionDisabled>(button).is_none());
        assert!(app.world().get::<ArmDelay>(button).is_none());
    }

    #[test]
    fn every_button_kind_shows_hover_press_and_disabled() {
        for kind in [Btn::Primary, Btn::Secondary, Btn::Ghost, Btn::Selected, Btn::Danger, Btn::Link, Btn::Tile] {
            let rest = kind.look(false, false, false);
            let hover = kind.look(false, false, true);
            let press = kind.look(false, true, true);
            let disabled = kind.look(true, false, false);
            assert!(rest.fill != hover.fill, "{kind:?} has no hover state");
            assert!(rest.fill != press.fill, "{kind:?} has no pressed state");
            assert!(disabled.text == TEXT_DISABLED && rest.text != TEXT_DISABLED, "{kind:?} has no disabled state");
        }
    }
}
