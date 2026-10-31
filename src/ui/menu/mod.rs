//! The tabbed main menu: the window, tab strip and page layout. Each page lives in its own
//! module; the Custom, History and Insights pages are filled by systems.

mod audio;
mod crosshair;
mod discord;
mod finder;
mod play;
mod sensitivity;
mod video;

use bevy::{
    app::Propagate,
    feathers::{
        controls::ButtonVariant,
        theme::{SurfaceLevel, ThemeBackgroundColor, ThemeContext},
        tokens,
    },
    prelude::*,
    ui_widgets::Activate,
};

pub use discord::update_discord_status;
pub use sensitivity::update_sens_info;

use self::{
    audio::audio_page, crosshair::crosshair_page, discord::discord_page, finder::finder_page, play::play_page,
    sensitivity::sensitivity_page, video::video_page,
};
use super::{custom, insights, theme::*, widgets::*};
use crate::{AppState, settings::Settings, ui::history};

/// Which menu page is visible. Kept across visits so you return to where you were.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuTab {
    #[default]
    Play,
    Finder,
    Custom,
    Sensitivity,
    Crosshair,
    Video,
    Audio,
    History,
    Insights,
    Discord,
}

impl MenuTab {
    fn name(self) -> &'static str {
        match self {
            MenuTab::Play => "Play",
            MenuTab::Finder => "Finder",
            MenuTab::Custom => "Custom",
            MenuTab::Sensitivity => "Sensitivity",
            MenuTab::Crosshair => "Crosshair",
            MenuTab::Video => "Video",
            MenuTab::Discord => "Discord",
            MenuTab::Audio => "Audio",
            MenuTab::History => "History",
            MenuTab::Insights => "Insights",
        }
    }
}

#[derive(Component, Clone, Copy, Default)]
pub struct TabButton(MenuTab);

#[derive(Component, Clone, Copy, Default)]
pub struct TabPage(MenuTab);

/// Outer size of the menu window, chosen to fit 1280x720 logical with a margin.
const MENU_W: f32 = 1040.0;
const MENU_H: f32 = 660.0;
/// Width of the left column on two-column pages.
const MAIN_COLUMN_W: f32 = 540.0;

pub fn spawn_menu(mut commands: Commands, settings: Res<Settings>) {
    commands
        .spawn_scene(bsn! {
            @overlay()
            Children [
                @panel()
                Node { width: px(MENU_W), height: px(MENU_H) }
                Children [
                    @row()
                    Node { column_gap: px(SPACE_M) }
                    Children [
                        Node { width: px(4), height: px(26), border_radius: BorderRadius::all(px(2)) }
                        ThemeBackgroundColor(tokens::BUTTON_PRIMARY_BG)
                        --
                        @heading("Aim Trainer")
                        --
                        @spacer()
                        --
                        @button("Quit", ButtonVariant::Normal)
                        Node { min_width: px(90) }
                        on(|_: On<Activate>, mut exit: MessageWriter<AppExit>| {
                            exit.write(AppExit::Success);
                        })
                    ]
                    --
                    @tab_bar()
                    --
                    @play_page(&settings)
                    --
                    @finder_page(&settings)
                    --
                    @page(MenuTab::Custom)
                    Children [ @custom::custom_lists_column() -- @custom::custom_editor_column() ]
                    --
                    @sensitivity_page(&settings)
                    --
                    @crosshair_page(&settings)
                    --
                    @video_page(&settings)
                    --
                    @audio_page(&settings)
                    --
                    @discord_page()
                    --
                    @page(MenuTab::History)
                    Node { flex_direction: FlexDirection::Column, row_gap: px(SPACE_M) }
                    history::HistoryBody
                    --
                    @page(MenuTab::Insights)
                    insights::InsightsBody
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Menu));
}

/// Segmented tab strip; the active tab is filled with the accent color.
fn tab_bar() -> impl Scene {
    bsn! {
        @row()
        Node { padding: px(SPACE_XS), column_gap: px(SPACE_XS), border_radius: BorderRadius::all(px(8)) }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        Propagate::<ThemeContext>(ThemeContext(SurfaceLevel::Higher))
        Children [
            @tab_button(MenuTab::Play)
            --
            @tab_button(MenuTab::Finder)
            --
            @tab_button(MenuTab::Custom)
            --
            @tab_button(MenuTab::Sensitivity)
            --
            @tab_button(MenuTab::Crosshair)
            --
            @tab_button(MenuTab::Video)
            --
            @tab_button(MenuTab::Audio)
            --
            @tab_button(MenuTab::History)
            --
            @tab_button(MenuTab::Insights)
            --
            @tab_button(MenuTab::Discord)
        ]
    }
}

/// Tabs share the strip's width: each gets its caption's width plus an equal share of the rest,
/// so no caption is ever squeezed or wrapped.
pub(super) fn tab_button(tab: MenuTab) -> impl Scene {
    bsn! {
        @button(tab.name(), ButtonVariant::Plain)
        Node { min_width: px(0), flex_grow: 1.0, padding: UiRect::horizontal(px(SPACE_S)) }
        TabButton(tab)
        on(|activate: On<Activate>, tabs: Query<&TabButton>, mut current: ResMut<MenuTab>| {
            if let Ok(&TabButton(tab)) = tabs.get(activate.event_target()) {
                *current = tab;
            }
        })
    }
}

/// A page container; `show_tab` toggles its display.
pub(super) fn page(tab: MenuTab) -> impl Scene {
    bsn! {
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Row,
            column_gap: px(SPACE_L),
            flex_grow: 1.0,
            min_height: px(0),
        }
        TabPage(tab)
    }
}

/// Fixed-width left column of a page.
pub(super) fn main_column() -> impl Scene {
    bsn! {
        @column()
        Node { width: px(MAIN_COLUMN_W), row_gap: px(SPACE_L), flex_shrink: 0.0 }
    }
}

/// Right column of a page, taking the remaining width.
pub(super) fn side_column() -> impl Scene {
    bsn! {
        @column()
        Node { flex_grow: 1.0, flex_basis: px(0), row_gap: px(SPACE_L) }
    }
}

pub fn show_tab(
    current: Res<MenuTab>,
    mut pages: Query<(&TabPage, &mut Node)>,
    mut buttons: Query<(&TabButton, &mut ButtonVariant)>,
) {
    for (&TabPage(tab), mut node) in &mut pages {
        let display = if tab == *current { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for (&TabButton(tab), mut variant) in &mut buttons {
        variant.set_if_neq(if tab == *current { ButtonVariant::Primary } else { ButtonVariant::Plain });
    }
}

