//! The tabbed main menu.

use bevy::{
    app::Propagate,
    feathers::{
        controls::ButtonVariant,
        display::{label, label_dim},
        theme::{SurfaceLevel, ThemeBackgroundColor, ThemeContext},
        tokens,
    },
    prelude::*,
    ui_widgets::Activate,
};

use super::{custom, finder::start_finder, insights, theme::*, widgets::*};
use crate::{
    AppState,
    crosshair::CrosshairPreview,
    presence::{Connection, PresenceLink},
    routine::{ROUTINES, Routine, start_routine},
    scenario::{DrillSpec, Run, RunPurpose, Scenario, begin_run},
    settings::{AspectMode, CrosshairSettings, DisplayMode, FinderFocus, FinderMethod, HitSound, SettingField, Settings},
    ui::history,
};

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

#[derive(Component, Clone, Copy, Default)]
struct PlayButton(Scenario);

#[derive(Component, Clone, Copy, Default)]
struct RoutineButton(usize);

#[derive(Component, Clone, Copy, Default)]
pub struct SensInfo;

/// A built-in drill played on its own, with the global drill length.
pub fn practice_run(scenario: Scenario, settings: &Settings) -> Run {
    Run::new(
        DrillSpec::builtin(scenario, settings),
        settings.sens,
        settings.run_secs,
        RunPurpose::Practice,
        scenario.name().to_string(),
    )
}

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

// ---------------------------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------------------------

fn play_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Play)
        Children [
            @main_column()
            Children [
                @card("Aim", bsn_list! {
                    @play_button(Scenario::Gridshot)
                    --
                    @play_button(Scenario::Microflick)
                    --
                    @play_button(Scenario::Tracking)
                })
                --
                @card("Valorant / CS2", bsn_list! {
                    @play_button(Scenario::HeadLevel)
                    --
                    @play_button(Scenario::Peek)
                })
                --
                @card("Eye training", bsn_list! {
                    @play_button(Scenario::Reaction)
                    --
                    @play_button(Scenario::Peripheral)
                })
            ]
            --
            @side_column()
            Children [
                @card("Warmup routines", bsn_list! {
                    @routine_button(0)
                    --
                    @routine_button(1)
                    --
                    @routine_button(2)
                })
                --
                @card("Drill settings", bsn_list! {
                    @number_field(SettingField::RunSecs, settings)
                    --
                    @number_field(SettingField::TargetScale, settings)
                    --
                    @number_field(SettingField::TargetLifetime, settings)
                    --
                    @label_dim("Decay: targets shrink and count as a miss if not hit in time (Gridshot, Microflick, Head level). 0 = off.")
                    --
                    @label("Target color")
                    --
                    @target_color_choices()
                })
            ]
        ]
    }
}

fn play_button(scenario: Scenario) -> impl Scene {
    bsn! {
        @row()
        Node { column_gap: px(SPACE_M) }
        Children [
            @button(scenario.name(), ButtonVariant::Normal)
            Node { width: px(140), flex_shrink: 0.0 }
            PlayButton(scenario)
            on(play)
            --
            @label_dim(scenario.blurb())
        ]
    }
}

fn routine_button(index: usize) -> impl Scene {
    let plan = &ROUTINES[index];
    bsn! {
        @row()
        Node { column_gap: px(SPACE_M) }
        Children [
            @button(plan.name, ButtonVariant::Primary)
            Node { width: px(140), flex_shrink: 0.0 }
            RoutineButton(index)
            on(|activate: On<Activate>, buttons: Query<&RoutineButton>, settings: Res<Settings>, mut commands: Commands, mut next: ResMut<NextState<AppState>>| {
                if let Ok(&RoutineButton(index)) = buttons.get(activate.event_target()) {
                    start_routine(&mut commands, &mut next, Routine::from_plan(&ROUTINES[index], &settings));
                }
            })
            --
            @label_dim(plan.blurb)
        ]
    }
}

fn finder_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Finder)
        Children [
            @main_column()
            Children [
                @card("Setup", bsn_list! {
                    @setting_row("Method", bsn_list! {
                        @choice("Smart (score model)", Choice::FinderMethod(FinderMethod::Smart))
                        --
                        @choice("Classic (A/B)", Choice::FinderMethod(FinderMethod::Classic))
                    })
                    --
                    @setting_row("Smart focus", bsn_list! {
                        @choice("Balanced", Choice::FinderFocus(FinderFocus::Balanced))
                        --
                        @choice("Flicking", Choice::FinderFocus(FinderFocus::Flicking))
                        --
                        @choice("Tracking", Choice::FinderFocus(FinderFocus::Tracking))
                    })
                    --
                    @setting_row("Classic drill", bsn_list! {
                        @choice("Gridshot", Choice::FinderDrill(Scenario::Gridshot))
                        --
                        @choice("Microflick", Choice::FinderDrill(Scenario::Microflick))
                        --
                        @choice("Tracking", Choice::FinderDrill(Scenario::Tracking))
                        --
                        @choice("Head level", Choice::FinderDrill(Scenario::HeadLevel))
                    })
                    --
                    @number_field(SettingField::FinderTrialSecs, settings)
                    --
                    @number_field(SettingField::FinderRounds, settings)
                    --
                    @actions(bsn_list! {
                        @button("Start finder", ButtonVariant::Primary)
                        on(start_finder)
                    })
                })
            ]
            --
            @side_column()
            Children [
                @card("How it works", bsn_list! {
                    @subheading("Smart")
                    --
                    @label_dim("9 blind trials: 5 sensitivities from 0.6x to 1.6x yours, then 4 more around the predicted best. Each trial plays the Smart focus drills at the same sens (Balanced: Microflick + Tracking, Flicking: Gridshot + Microflick, Tracking: Tracking), the first 3 s of each unscored. After each trial you rate how it felt. Every sens within the trial noise of the best counts as equally good; your current sens is kept if it's among them, and only a clear first-flick trend (landing short or past) can move the pick inside that range.")
                    --
                    @subheading("Classic")
                    Node { margin: UiRect::top(px(SPACE_S)) }
                    --
                    @label_dim("Blind A/B rounds: play the two ends of a range around your sens, pick the one that felt better; the range halves every round. Uses Classic drill and Rounds.")
                })
            ]
        ]
    }
}

fn sensitivity_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Sensitivity)
        Children [
            @main_column()
            Children [
                @card("Sensitivity", bsn_list! {
                    @setting_row("Game", bsn_list! {
                        @choice("Valorant", Choice::Preset(0))
                        --
                        @choice("CS2", Choice::Preset(1))
                    })
                    --
                    @number_field(SettingField::Sens, settings)
                    --
                    @number_field(SettingField::Dpi, settings)
                    --
                    @number_field(SettingField::Yaw, settings)
                    --
                    @number_field(SettingField::Hfov, settings)
                    --
                    @tile()
                    Node { margin: UiRect::top(px(SPACE_XS)), row_gap: px(SPACE_XS) }
                    Children [
                        @overline("Equivalent")
                        --
                        @label("")
                        SensInfo
                    ]
                })
            ]
            --
            @side_column()
            Children [
                @card("How it maps", bsn_list! {
                    @label_dim("Mouse input is raw (unaccelerated) counts, like both games. One count turns the view by sens x yaw degrees.")
                    --
                    @label_dim("Valorant: yaw 0.07, 103 deg horizontal at 16:9, black bars on other aspect ratios.")
                    --
                    @label_dim("CS2: yaw 0.022 (m_yaw), fov 90 at 4:3 = 106.26 deg at 16:9, wider screens see more.")
                    --
                    @label_dim("Switching game keeps your cm/360. No preset highlighted = custom yaw/FOV.")
                })
            ]
        ]
    }
}

fn crosshair_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Crosshair)
        Children [
            @main_column()
            Children [
                @card("Color", bsn_list! {
                    @crosshair_color_choices()
                    --
                    @setting_row("Custom RGB", bsn_list! {
                        @number_input(SettingField::ColorR, settings, 72.0)
                        --
                        @number_input(SettingField::ColorG, settings, 72.0)
                        --
                        @number_input(SettingField::ColorB, settings, 72.0)
                    })
                })
                --
                @row()
                Node { align_items: AlignItems::Start, column_gap: px(SPACE_L) }
                Children [
                    @card("Center dot & lines", bsn_list! {
                        @on_off_row("Center dot", Choice::CenterDot(true), Choice::CenterDot(false))
                        --
                        @number_field(SettingField::DotThickness, settings)
                        --
                        @on_off_row("Inner lines", Choice::InnerLines(true), Choice::InnerLines(false))
                        --
                        @number_field(SettingField::LineLength, settings)
                        --
                        @number_field(SettingField::LineThickness, settings)
                        --
                        @number_field(SettingField::LineOffset, settings)
                    })
                    Node { flex_grow: 1.0, flex_basis: px(0) }
                    --
                    @card("Outlines", bsn_list! {
                        @on_off_row("Outlines", Choice::Outlines(true), Choice::Outlines(false))
                        --
                        @number_field(SettingField::OutlineThickness, settings)
                        --
                        @number_field(SettingField::OutlineOpacity, settings)
                    })
                    Node { flex_grow: 1.0, flex_basis: px(0) }
                ]
            ]
            --
            @side_column()
            Children [
                @card("Preview", bsn_list! {
                    Node {
                        height: px(220),
                        border_radius: BorderRadius::all(px(6)),
                    }
                    BackgroundColor(Color::srgb_u8(44, 47, 56))
                    CrosshairPreview
                    --
                    @label_dim("Actual pixel size.")
                    --
                    @actions(bsn_list! {
                        @button("Reset to Valorant default", ButtonVariant::Normal)
                        on(|_: On<Activate>, mut settings: ResMut<Settings>| {
                            settings.crosshair = CrosshairSettings::default();
                        })
                    })
                })
            ]
        ]
    }
}

fn on_off_row(text: &'static str, on: Choice, off: Choice) -> impl Scene {
    setting_row(text, bsn_list! { @choice("On", on) -- @choice("Off", off) })
}

fn video_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Video)
        Children [
            @main_column()
            Children [
                @card("Display", bsn_list! {
                    @setting_row("Display mode", bsn_list! {
                        @choice("Windowed", Choice::Display(DisplayMode::Windowed))
                        --
                        @choice("Borderless", Choice::Display(DisplayMode::Borderless))
                        --
                        @choice("Fullscreen", Choice::Display(DisplayMode::Fullscreen))
                    })
                    --
                    @setting_row("Aspect ratio", bsn_list! {
                        @choice("16:9 + black bars", Choice::Aspect(AspectMode::Lock16x9))
                        --
                        @choice("Fill screen (Hor+)", Choice::Aspect(AspectMode::Native))
                    })
                })
                --
                @card("Performance", bsn_list! {
                    @on_off_row("VSync", Choice::Vsync(true), Choice::Vsync(false))
                    --
                    @setting_row("Anti-aliasing (MSAA)", bsn_list! {
                        @choice("Off", Choice::Msaa(1))
                        --
                        @choice("2x", Choice::Msaa(2))
                        --
                        @choice("4x", Choice::Msaa(4))
                        --
                        @choice("8x", Choice::Msaa(8))
                    })
                    --
                    @number_field(SettingField::FpsCap, settings)
                })
            ]
            --
            @side_column()
            Children [
                @card("Tips", bsn_list! {
                    @label_dim("16:9 + black bars renders exactly what Valorant shows on an ultrawide or 16:10 screen. Fill screen behaves like CS2.")
                    --
                    @label_dim("For the lowest input lag: VSync off, Fullscreen, FPS cap 0 or well above your refresh rate.")
                })
            ]
        ]
    }
}

fn audio_page(settings: &Settings) -> impl Scene {
    bsn! {
        @page(MenuTab::Audio)
        Children [
            @main_column()
            Children [
                @card("Sound", bsn_list! {
                    @setting_row("Hit sound", bsn_list! {
                        @choice(HitSound::ALL[0].name(), Choice::HitSound(HitSound::ALL[0]))
                        --
                        @choice(HitSound::ALL[1].name(), Choice::HitSound(HitSound::ALL[1]))
                        --
                        @choice(HitSound::ALL[2].name(), Choice::HitSound(HitSound::ALL[2]))
                        --
                        @choice(HitSound::ALL[3].name(), Choice::HitSound(HitSound::ALL[3]))
                    })
                    --
                    @number_field(SettingField::Volume, settings)
                    --
                    @label_dim("Picking a sound or changing the volume plays a preview.")
                })
            ]
        ]
    }
}

fn discord_page() -> impl Scene {
    bsn! {
        @page(MenuTab::Discord)
        Children [
            @main_column()
            Children [
                @card("Rich Presence", bsn_list! {
                    @setting_row("Show activity", bsn_list! {
                        @choice("On", Choice::Discord(true))
                        --
                        @choice("Off", Choice::Discord(false))
                    })
                    --
                    @label("")
                    DiscordStatus
                })
            ]
            --
            @side_column()
            Children [
                @card("What it shows", bsn_list! {
                    @label_dim("Your Discord profile shows \"Playing Aim Trainer\" with what you're doing: the drill and a countdown, your warmup routine step, the sens finder, or your last score. Your sensitivity is never shown.")
                    --
                    @label_dim("The Discord desktop app must be running on this PC.")
                })
            ]
        ]
    }
}

#[derive(Component, Clone, Copy, Default)]
pub struct DiscordStatus;

pub fn update_discord_status(settings: Res<Settings>, link: Res<PresenceLink>, mut labels: Query<&mut Text, With<DiscordStatus>>) {
    let wanted = match (settings.discord.enabled, link.connection()) {
        (false, _) => "Off.",
        (true, Connection::Connected) => "Connected: your profile shows what you're playing.",
        (true, Connection::Searching | Connection::Off) => "Not connected: is the Discord app running? Retrying every 10 s.",
    };
    for mut text in &mut labels {
        if text.0 != wanted {
            text.0 = wanted.to_string();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Behaviour
// ---------------------------------------------------------------------------------------------

fn play(
    activate: On<Activate>,
    buttons: Query<&PlayButton>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if let Ok(&PlayButton(scenario)) = buttons.get(activate.event_target()) {
        begin_run(&mut commands, &mut next_state, practice_run(scenario, &settings));
    }
}

pub fn update_sens_info(settings: Res<Settings>, mut info: Query<&mut Text, With<SensInfo>>) {
    let wanted = settings.describe_sens(settings.sens).replace("  |  ", "\n");
    for mut text in &mut info {
        if text.0 != wanted {
            text.0.clone_from(&wanted);
        }
    }
}
