//! The tabbed main menu: the window, tab strip and page layout. Each page lives in its own
//! module; the Custom, History and Insights pages are filled by systems.

mod audio;
mod crosshair;
pub mod custom;
mod finder;
pub mod history;
pub mod insights;
mod online;
mod play;
mod sensitivity;
mod video;

use bevy::app::Propagate;
use bevy::feathers::controls::ButtonVariant;
use bevy::feathers::display::label_dim;
use bevy::feathers::theme::{SurfaceLevel, ThemeBackgroundColor, ThemeContext};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
pub use online::{update_discord_status, update_update_status};
pub use sensitivity::update_sens_info;

use self::audio::audio_page;
use self::crosshair::crosshair_page;
use self::finder::finder_page;
use self::online::online_page;
use self::play::play_page;
use self::sensitivity::sensitivity_page;
use self::video::video_page;
use super::theme::*;
use super::widgets::*;
use crate::persist::LoadProblems;
use crate::settings::Settings;
use crate::update::current_version;
use crate::{AppState, LOG_PATH};

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
    Online,
}

impl MenuTab {
    /// Every page, in tab order.
    pub const ALL: [MenuTab; 10] = [
        MenuTab::Play,
        MenuTab::Finder,
        MenuTab::Custom,
        MenuTab::Sensitivity,
        MenuTab::Crosshair,
        MenuTab::Video,
        MenuTab::Audio,
        MenuTab::History,
        MenuTab::Insights,
        MenuTab::Online,
    ];

    pub(in crate::ui) fn name(self) -> &'static str {
        match self {
            MenuTab::Play => "Play",
            MenuTab::Finder => "Finder",
            MenuTab::Custom => "Custom",
            MenuTab::Sensitivity => "Sensitivity",
            MenuTab::Crosshair => "Crosshair",
            MenuTab::Video => "Video",
            MenuTab::Online => "Online",
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

pub fn spawn_menu(mut commands: Commands, settings: Res<Settings>, load_problems: Res<LoadProblems>) {
    let version = format!("v{}", current_version());
    let note = load_note(&load_problems.0);
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
                        @label_dim(version)
                        --
                        @label_dim(note)
                        --
                        @spacer()
                        --
                        @online::restart_button()
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
                    @online_page()
                    --
                    @page(MenuTab::History)
                    Children [ @history::history_body() ]
                    --
                    @page(MenuTab::Insights)
                    Children [
                        @scroll_column(bsn_list! {
                            Node { flex_direction: FlexDirection::Row, column_gap: px(SPACE_L), align_items: AlignItems::Start }
                            insights::InsightsBody
                        })
                        Node { flex_grow: 1.0 }
                    ]
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Menu));
}

/// Points at the log when data files couldn't be loaded, so falling back to defaults isn't silent.
fn load_note(files: &[&str]) -> String {
    if files.is_empty() {
        return String::new();
    }
    format!("Couldn't load {}: using defaults. Details in {LOG_PATH}.", files.join(" and "))
}

/// Segmented tab strip; the active tab is filled with the accent color.
fn tab_bar() -> impl Scene {
    let tabs: Vec<_> = MenuTab::ALL.into_iter().map(tab_button).collect();
    bsn! {
        @row()
        Node { padding: px(SPACE_XS), column_gap: px(SPACE_XS), border_radius: BorderRadius::all(px(8)) }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        Propagate::<ThemeContext>(ThemeContext(SurfaceLevel::Higher))
        Children [ {tabs} ]
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
