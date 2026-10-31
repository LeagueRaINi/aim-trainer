//! Finder page: sensitivity finder setup.

use bevy::{
    feathers::{
        controls::ButtonVariant,
        display::label_dim,
    },
    prelude::*,
};

use super::{MenuTab, main_column, page, side_column};
use crate::{
    scenario::Scenario,
    settings::{FinderFocus, FinderMethod, SettingField, Settings},
    ui::{bindings::*, finder::start_finder, theme::*, widgets::*},
};

pub(super) fn finder_page(settings: &Settings) -> impl Scene {
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

