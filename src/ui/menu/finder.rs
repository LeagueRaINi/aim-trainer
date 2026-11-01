//! Finder page: sensitivity finder setup.

use bevy::feathers::controls::ButtonVariant;
use bevy::feathers::display::label_dim;
use bevy::prelude::*;

use super::{MenuTab, main_column, page, side_column};
use crate::drill::DrillKind;
use crate::settings::{FinderFocus, FinderMethod, SettingField, Settings};
use crate::ui::bindings::*;
use crate::ui::screens::finder::start_finder;
use crate::ui::theme::*;
use crate::ui::widgets::*;

pub(super) fn finder_page(settings: &Settings) -> impl Scene {
    let classic_drills: Vec<_> = DrillKind::ALL
        .into_iter()
        .filter(|kind| kind.def().classic_finder)
        .map(|kind| choice(kind.name(), Choice::FinderDrill(kind)))
        .collect();
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
                    @setting_row("Classic drill", classic_drills)
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
                    @label_dim("6 to 12 blind trials between 0.6x and 1.6x your sens: first both ends and your own in random order, then wherever a statistical model of your performance (a Gaussian process that also factors out warm-up) expects to learn most.")
                    --
                    @label_dim("Each trial plays the Smart focus drills at the same sens (Balanced: Microflick + Tracking, Flicking: Gridshot + Microflick, Tracking: Tracking), the first 3 s of each unscored. After each trial you rate how it felt.")
                    --
                    @label_dim("It stops once your best range (every sens likely within 3% of your best) settles; your current sens is kept if it's in that range, and only a clear trend in where your flicks and clicks land can move the pick inside it.")
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
