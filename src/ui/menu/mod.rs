//! The main menu: a window with the navigation rail on the left and the open page on the right.
//! Each page lives in its own module and is built when it is opened, so the menu only carries
//! the widgets it shows; the Play, Custom, History and Insights pages are (partly) filled by
//! systems.

mod audio;
mod crosshair;
pub mod custom;
pub mod finder;
pub mod history;
pub mod insights;
mod online;
pub mod play;
mod sensitivity;
mod video;

pub mod benchmark;
pub mod plan;

use bevy::feathers::constants::fonts;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
pub use online::{update_discord_status, update_update_status};
pub use sensitivity::update_sens_info;

use super::theme::*;
use super::widgets::*;
use crate::history::History;
use crate::persist::LoadProblems;
use crate::settings::Settings;
use crate::update::current_version;
use crate::{AppState, LOG_PATH};

/// Which menu page is open. Kept across visits so you return to where you were.
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
    Plan,
    Benchmark,
}

/// The navigation rail: the pages by what you come to do.
const NAV: [(&str, &[MenuTab]); 3] = [
    ("Train", &[MenuTab::Play, MenuTab::Plan, MenuTab::Custom, MenuTab::Finder]),
    ("Progress", &[MenuTab::History, MenuTab::Benchmark, MenuTab::Insights]),
    ("Settings", &[MenuTab::Sensitivity, MenuTab::Crosshair, MenuTab::Video, MenuTab::Audio, MenuTab::Online]),
];

impl MenuTab {
    /// Every page, in navigation order.
    pub const ALL: [MenuTab; 12] = [
        MenuTab::Play,
        MenuTab::Plan,
        MenuTab::Custom,
        MenuTab::Finder,
        MenuTab::History,
        MenuTab::Benchmark,
        MenuTab::Insights,
        MenuTab::Sensitivity,
        MenuTab::Crosshair,
        MenuTab::Video,
        MenuTab::Audio,
        MenuTab::Online,
    ];

    /// Short name: the navigation caption and the screenshot tour's file name.
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
            MenuTab::Plan => "Plan",
            MenuTab::Benchmark => "Benchmark",
        }
    }

    /// The page's caption in the navigation rail.
    fn nav_caption(self) -> &'static str {
        match self {
            MenuTab::Finder => "Sens finder",
            MenuTab::Online => "Online & updates",
            MenuTab::Plan => "Weekly plan",
            _ => self.name(),
        }
    }

    /// The page's title and one line on what it is for.
    fn header(self) -> (&'static str, &'static str) {
        match self {
            MenuTab::Play => ("Play", "Pick a drill or a warmup, then press Play."),
            MenuTab::Custom => ("Custom", "Your own scenarios and playlists, built from the drills."),
            MenuTab::Finder => {
                ("Sensitivity finder", "The sensitivities that fit your hand and pad, and evidence before any change.")
            },
            MenuTab::History => ("History", "Your scores per drill over time, and replays of recent runs."),
            MenuTab::Insights => {
                ("Insights", "Where your flicks are weak, from your recent replays, and what to train.")
            },
            MenuTab::Sensitivity => ("Sensitivity", "Match your game: the view turns exactly as it does there."),
            MenuTab::Crosshair => ("Crosshair", "A Valorant-style crosshair, previewed at its real size."),
            MenuTab::Video => ("Video", "Display, targets and the room you train in."),
            MenuTab::Audio => ("Audio", "Hit sound and volume."),
            MenuTab::Online => ("Online & updates", "Discord status and automatic updates."),
            MenuTab::Plan => {
                ("Weekly plan", "What to train today and why: short sessions on several days, a benchmark each week.")
            },
            MenuTab::Benchmark => {
                ("Benchmark", "The same scenarios with the same settings every week: your progress per kind of aim.")
            },
        }
    }
}

#[derive(Component, Clone, Copy, Default)]
pub struct TabButton(MenuTab);

/// The container the open page is built into by [`show_tab`].
#[derive(Component, Clone, Copy, Default)]
pub struct PageHost;

/// Outer size of the menu window, chosen to fit 1280x720 logical with a margin.
const MENU_W: f32 = 1136.0;
const MENU_H: f32 = 680.0;
/// Width of the navigation rail.
const NAV_W: f32 = 196.0;
/// Width a page has for its content.
const CONTENT_W: f32 = MENU_W - NAV_W - 2.0 * SPACE_XL - 2.0;
/// Width of the left column on two-column pages.
const MAIN_COLUMN_W: f32 = 480.0;

pub fn spawn_menu(mut commands: Commands, current: Res<MenuTab>, load_problems: Res<LoadProblems>) {
    let version = format!("v{}", current_version());
    let note = load_note(&load_problems.0);
    let groups: Vec<_> = NAV.into_iter().map(|(title, tabs)| nav_group(title, tabs, *current)).collect();
    commands
        .spawn_scene(bsn! {
            @overlay()
            Children [
                @window()
                Node { width: px(MENU_W), height: px(MENU_H) }
                Children [
                    @column()
                    Node {
                        width: px(NAV_W),
                        flex_shrink: 0.0,
                        padding: px(SPACE_L),
                        row_gap: px(SPACE_L),
                        border_radius: BorderRadius::left(px(RADIUS_L - 1.0)),
                    }
                    ThemeBackgroundColor(SURFACE_SUNKEN)
                    Children [
                        @row()
                        Node { column_gap: px(SPACE_M), padding: UiRect::new(px(SPACE_XS), px(0), px(SPACE_XS), px(SPACE_S)) }
                        Children [
                            @marker(ACCENT)
                            --
                            @column()
                            Node { row_gap: px(0) }
                            Children [ @subheading("Aim Trainer") -- @small(version) ]
                        ]
                        --
                        {groups}
                        --
                        @spacer()
                        --
                        @styled(note, fonts::REGULAR, FONT_SMALL, TEXT_WARN)
                        --
                        @online::restart_button()
                        --
                        @button("Quit", Btn::Secondary)
                        Node { min_width: px(0) }
                        on(|_: On<Activate>, mut exit: MessageWriter<AppExit>| {
                            exit.write(AppExit::Success);
                        })
                    ]
                    --
                    @column()
                    Node {
                        flex_grow: 1.0,
                        flex_basis: px(0),
                        min_width: px(0),
                        padding: UiRect::axes(px(SPACE_XL), px(SPACE_L + SPACE_XS)),
                    }
                    PageHost
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

/// A titled group of pages in the navigation rail.
fn nav_group(title: &'static str, tabs: &'static [MenuTab], current: MenuTab) -> impl Scene {
    let buttons: Vec<_> = tabs.iter().map(|&tab| tab_button(tab, tab == current)).collect();
    bsn! {
        @column()
        Node { row_gap: px(2) }
        Children [
            @overline(title)
            Node { margin: UiRect::new(px(SPACE_XS), px(0), px(0), px(SPACE_XS)) }
            --
            {buttons}
        ]
    }
}

/// A page's entry in the navigation rail; the open page's is tinted with the accent color.
fn tab_button(tab: MenuTab, open: bool) -> impl Scene {
    let text = tab.nav_caption();
    let kind = nav_kind(open);
    bsn! {
        @button_box(kind, 30.0)
        Node { justify_content: JustifyContent::FlexStart, padding: UiRect::horizontal(px(SPACE_M)) }
        Children [ @caption(text) ]
        TabButton(tab)
        on(|activate: On<Activate>, tabs: Query<&TabButton>, mut current: ResMut<MenuTab>| {
            if let Ok(&TabButton(tab)) = tabs.get(activate.event_target())
                && *current != tab
            {
                *current = tab;
            }
        })
    }
}

fn nav_kind(open: bool) -> Btn {
    if open { Btn::Selected } else { Btn::Ghost }
}

/// A page: its title and purpose over its content, which is laid out as a row of columns.
fn page(tab: MenuTab, body: impl SceneList) -> impl Scene {
    let (title, summary) = tab.header();
    bsn! {
        @column()
        Node { flex_grow: 1.0, min_height: px(0), row_gap: px(SPACE_L) }
        Children [
            @column()
            Node { row_gap: px(2) }
            Children [ @heading(title) -- @label_dim(summary) ]
            --
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(SPACE_L),
                flex_grow: 1.0,
                min_height: px(0),
            }
            Children [ {body} ]
        ]
    }
}

/// Fixed-width left column of a page.
fn main_column() -> impl Scene {
    bsn! {
        @column()
        Node { width: px(MAIN_COLUMN_W), row_gap: px(SPACE_L), flex_shrink: 0.0 }
    }
}

/// Right column of a page, taking the remaining width.
fn side_column() -> impl Scene {
    bsn! {
        @column()
        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), row_gap: px(SPACE_L) }
    }
}

/// A short note in a side column: what the settings next to it mean in practice.
fn note_card(title: &'static str, lines: impl SceneList) -> impl Scene {
    bsn! {
        @column()
        Node {
            padding: px(SPACE_L),
            row_gap: px(SPACE_S),
            border: px(1),
            border_radius: BorderRadius::all(px(RADIUS_M)),
        }
        ThemeBorderColor(BORDER)
        Children [ @overline(title) -- {lines} ]
    }
}

/// Builds the open page into the menu when it is opened or another page is picked, and marks
/// its entry in the navigation rail.
pub fn show_tab(
    mut commands: Commands,
    current: Res<MenuTab>,
    settings: Res<Settings>,
    history: Res<History>,
    selection: Res<play::PlaySelection>,
    hosts: Query<(Entity, Ref<PageHost>)>,
    mut buttons: Query<(&TabButton, &mut Btn)>,
) {
    for (host, marker) in &hosts {
        if !current.is_changed() && !marker.is_added() {
            continue;
        }
        commands.entity(host).despawn_related::<Children>();
        let page = match *current {
            MenuTab::Play => commands.spawn_scene(play::play_page(&history, &settings, *selection)).id(),
            MenuTab::Finder => commands.spawn_scene(finder::finder_page()).id(),
            MenuTab::Custom => commands.spawn_scene(custom::custom_page()).id(),
            MenuTab::Sensitivity => commands.spawn_scene(sensitivity::sensitivity_page(&settings)).id(),
            MenuTab::Crosshair => commands.spawn_scene(crosshair::crosshair_page(&settings)).id(),
            MenuTab::Video => commands.spawn_scene(video::video_page(&settings)).id(),
            MenuTab::Audio => commands.spawn_scene(audio::audio_page(&settings)).id(),
            MenuTab::History => commands.spawn_scene(history::history_page()).id(),
            MenuTab::Insights => commands.spawn_scene(insights::insights_page()).id(),
            MenuTab::Online => commands.spawn_scene(online::online_page()).id(),
            MenuTab::Plan => commands.spawn_scene(plan::plan_page()).id(),
            MenuTab::Benchmark => commands.spawn_scene(benchmark::benchmark_page()).id(),
        };
        commands.entity(page).insert(ChildOf(host));
        for (&TabButton(tab), mut kind) in &mut buttons {
            kind.set_if_neq(nav_kind(tab == *current));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_in_the_navigation_once() {
        let listed: Vec<MenuTab> = NAV.iter().flat_map(|(_, tabs)| tabs.iter().copied()).collect();
        assert_eq!(listed, MenuTab::ALL);
    }
}
