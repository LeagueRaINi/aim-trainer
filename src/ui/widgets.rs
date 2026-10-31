//! Shared UI building blocks: surfaces, text, buttons, setting rows and the arm delay.
//!
//! Layout uses the spacing scale in [`super::theme`]; text always uses the Feathers fonts and
//! theme tokens, never the default font or hard-coded colors.

use bevy::{
    app::Propagate,
    feathers::{
        constants::fonts,
        controls::{ButtonVariant, FeathersButton},
        display::{caption, label},
        theme::{SurfaceLevel, ThemeBackgroundColor, ThemeBorderColor, ThemeContext, ThemeTextColor, ThemeToken},
        tokens,
    },
    input_focus::tab_navigation::TabGroup,
    prelude::*,
    text::{FontSourceTemplate, FontWeight},
};

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

#[cfg(test)]
mod tests {
    use super::*;

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
