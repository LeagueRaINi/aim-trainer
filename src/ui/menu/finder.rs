//! Finder page, in the order a player should go through it: the usable range (from a
//! calibration), the monitor's status, the checks (tired check, long comparison), then the
//! quick finders as a first look. Built by [`rebuild_finder`] whenever the page is opened or
//! [`FinderView`] changes; what it shows comes from resources.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{MenuTab, main_column, note_card, page, side_column};
use crate::AppState;
use crate::drill::DrillKind;
use crate::finder::checkup::{Checkup, CheckupKind};
use crate::finder::compare::{self, CompareSession, CompareStore, DAY_BLOCKS, LongComparison};
use crate::finder::limits::{CalibrationStore, sens_for};
use crate::finder::monitor::{
    self, Advice, Finding, HeldBy, MIN_DAYS, MIN_FLICKS, MIN_RUNS, MonitorLog, Outcome, Pattern, Review, STEP, Status,
};
use crate::finder::tired::{Freshness, MIN_CHECKS, TiredLog, TiredVerdict};
use crate::finder::unix_now;
use crate::settings::{FinderFocus, FinderMethod, SettingField, Settings};
use crate::ui::bindings::*;
use crate::ui::charts::spawn_in;
use crate::ui::range::{RangeView, assumptions, calibrated_note, range_chart, reasons, round_sens, verdict_text};
use crate::ui::screens::finder::start_finder;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Which part of the finder page is open.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FinderView {
    /// Range, monitor, checks and the way to the quick finder.
    #[default]
    Overview,
    /// The quick finders' settings and start button.
    Quick,
}

/// The container [`rebuild_finder`] fills.
#[derive(Component, Clone, Copy, Default)]
pub struct FinderBody;

/// Width of the range card's column, and of the chart inside the card.
const RANGE_W: f32 = 488.0;
const CHART_W: f32 = RANGE_W - 2.0 * SPACE_L;

pub(super) fn finder_page() -> impl Scene {
    bsn! {
        @page(MenuTab::Finder, bsn_list! {
            @scroll_column(bsn_list! {
                Node { flex_direction: FlexDirection::Row, column_gap: px(SPACE_L), align_items: AlignItems::Start }
                FinderBody
            })
            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
        })
    }
}

/// A button that sets the sensitivity to this value.
#[derive(Component, Clone, Copy)]
struct ApplySens(f32);

/// A button that starts a tired check marked this way.
#[derive(Component, Clone, Copy)]
struct TiredButton(Freshness);

/// (Re)builds the page: when it is opened, and when a button on it changed what it shows.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_finder(
    mut commands: Commands,
    bodies: Query<(Entity, Ref<FinderBody>)>,
    view: Res<FinderView>,
    settings: Res<Settings>,
    calibration: Res<CalibrationStore>,
    monitor: Res<MonitorLog>,
    tired: Res<TiredLog>,
    comparison: Res<CompareStore>,
) {
    for (body, marker) in &bodies {
        if !marker.is_added() && !view.is_changed() {
            continue;
        }
        commands.entity(body).despawn_related::<Children>();
        if *view == FinderView::Quick {
            spawn_in(&mut commands, body, quick_view(&settings));
            continue;
        }
        let current = settings.cm_per_360(settings.sens);
        let range =
            RangeView { settings: &settings, calibration: calibration.latest.as_ref(), tired: tired.verdict(current) };
        let limits = range.limits();
        let status = monitor::status(monitor.entries(), current, limits.as_ref());
        let commands = &mut commands;
        let left = column_in(commands, body, Node { width: px(RANGE_W), flex_shrink: 0.0, ..default() });
        range_card(commands, left, &range);
        let right =
            column_in(commands, body, Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), ..default() });
        monitor_card(commands, right, &status, &settings, range.calibration.is_some());
        let fast = limits.map(|limits| (limits.fresh_fast, limits.fast));
        checks_card(commands, right, range.tired, fast, comparison.active.as_ref());
        quick_card(commands, right);
    }
}

fn column_in(commands: &mut Commands, parent: Entity, node: Node) -> Entity {
    let node = Node { flex_direction: FlexDirection::Column, row_gap: px(SPACE_L), ..node };
    commands.spawn((ChildOf(parent), node)).id()
}

/// A card with a numbered title: the page's steps in order.
fn section(commands: &mut Commands, parent: Entity, title: &str) -> Entity {
    let card = spawn_in(commands, parent, surface());
    spawn_in(commands, card, bsn! { @overline(title) Node { margin: UiRect::bottom(px(SPACE_XS)) } });
    card
}

// ---------------------------------------------------------------------------------------------
// 1: the range
// ---------------------------------------------------------------------------------------------

fn range_card(commands: &mut Commands, parent: Entity, view: &RangeView) {
    let card = section(commands, parent, "1 · Your range");
    range_chart(commands, card, CHART_W, view);
    let Some(calibration) = view.calibration else {
        let text = "A guided 2-minute calibration measures how far you sweep without lifting, how far your wrist reaches and how steady your hand is. From that: the sensitivities that fit you, each limit with its reason.";
        spawn_in(commands, card, callout(TEXT_ACCENT, "Not calibrated yet", text));
        spawn_in(
            commands,
            card,
            numbered_step(
                "1",
                "Two sweeps",
                "Across the pad without lifting, then from the wrist with the forearm planted.",
            ),
        );
        spawn_in(
            commands,
            card,
            numbered_step("2", "Hold and follow", "Keep the crosshair on a head-size dot: still, then moving slowly."),
        );
        spawn_in(
            commands,
            card,
            bsn! {
                @cta("Calibrate (2 min)")
                Node { align_self: AlignSelf::FlexStart, min_width: px(200), margin: UiRect::top(px(SPACE_XS)) }
                on(start_calibration)
            },
        );
        return;
    };
    if let Some((tone, title, text)) = verdict_text(view) {
        spawn_in(commands, card, callout(tone, title, text));
    }
    reasons(commands, card, view);
    let footer = spawn_in(commands, card, bsn! { @row() Node { margin: UiRect::top(px(SPACE_XS)) } });
    spawn_in(
        commands,
        footer,
        bsn! {
            @small(calibrated_note(calibration, view.settings))
            Node { flex_grow: 1.0, flex_basis: px(0) }
        },
    );
    if let Some(pick) = view.pick_sens() {
        let use_pick = spawn_in(
            commands,
            footer,
            bsn! { @compact_button(format!("Use {pick:.3}"), Btn::Secondary) on(apply_sens) },
        );
        commands.entity(use_pick).insert(ApplySens(pick));
    }
    spawn_in(commands, footer, bsn! { @compact_button("Redo calibration", Btn::Secondary) on(start_calibration) });
    spawn_in(commands, card, details("What these numbers rest on", assumptions()));
}

fn start_calibration(_: On<Activate>, mut commands: Commands, mut next_state: ResMut<NextState<AppState>>) {
    commands.insert_resource(Checkup::new(CheckupKind::Calibration));
    next_state.set(AppState::Finder);
}

fn apply_sens(
    activate: On<Activate>,
    buttons: Query<&ApplySens>,
    mut settings: ResMut<Settings>,
    mut view: ResMut<FinderView>,
) {
    let Ok(&ApplySens(sens)) = buttons.get(activate.event_target()) else { return };
    settings.sens = sens;
    // Everything on the page depends on the sensitivity: build it again.
    view.set_changed();
}

// ---------------------------------------------------------------------------------------------
// 2: the monitor
// ---------------------------------------------------------------------------------------------

/// The pattern in a sentence, with how much data it rests on.
fn finding_text(finding: &Finding) -> String {
    let what = match finding.pattern {
        Pattern::Overshoot => format!("Your first flicks travel {:.0}% past the target", finding.value * 100.0),
        Pattern::PastCentre => {
            format!("Your first clicks land {:.0}% of a target radius past the centre", finding.value * 100.0)
        },
        Pattern::Undershoot => format!("Your first flicks stop {:.0}% short of the target", -finding.value * 100.0),
        Pattern::Shaky => format!("Your flicks arrive, then need {:.1} corrections per kill", finding.value),
        Pattern::Fade => {
            format!("Late in a session your clicks land {:.0}% further from the centre", (finding.value - 1.0) * 100.0)
        },
    };
    let unit = if finding.pattern == Pattern::Fade { "sessions" } else { "runs" };
    format!("{what}: {} {unit} on {} days.", finding.samples, finding.days)
}

/// How the latest change of sensitivity turned out, in a sentence.
fn review_text(review: &Review) -> String {
    let change = |ratio: f32, more: &str, less: &str| {
        let percent = (ratio - 1.0) * 100.0;
        format!("{:.0}% {}", percent.abs(), if percent >= 0.0 { more } else { less })
    };
    let outcome = match review.outcome {
        Outcome::Helped => "It helped: keep it.",
        Outcome::Hurt => "It did not help.",
        Outcome::Mixed => "More precise but lower scores: your call.",
        Outcome::Unclear => "No clear difference: keep it, going back would cost adapting again.",
    };
    format!(
        "Since the change from {:.1} cm/360, clicks land {} the centre and scores are {}. {outcome}",
        review.from_cm,
        change(review.click_error, "further from", "closer to"),
        change(review.score, "higher", "lower"),
    )
}

fn monitor_card(commands: &mut Commands, parent: Entity, status: &Status, settings: &Settings, calibrated: bool) {
    let card = section(commands, parent, "2 · Monitor");
    let progress = status.progress;
    // The one primary action of the page is calibrating, until that is done.
    let kind = if calibrated { Btn::Primary } else { Btn::Secondary };
    let apply = |commands: &mut Commands, cm: f32, text: String| {
        let sens = round_sens(sens_for(settings, cm));
        let button = spawn_in(
            commands,
            card,
            bsn! { @compact_button(text, kind) Node { align_self: AlignSelf::FlexStart } on(apply_sens) },
        );
        commands.entity(button).insert(ApplySens(sens));
    };
    match status.advice {
        Advice::Collecting => {
            let missing = progress.missing();
            let mut parts = Vec::new();
            for (count, unit) in
                [(missing.runs as u32, "runs"), (missing.flicks, "flicks"), (missing.days as u32, "days")]
            {
                if count > 0 {
                    parts.push(format!("{count} {unit}"));
                }
            }
            let text = format!(
                "Still missing at this sensitivity: {}. It watches every flick drill you play.",
                parts.join(", ")
            );
            spawn_in(commands, card, callout(TEXT_ACCENT, "Not enough data yet", text));
            let tiles = spawn_in(commands, card, bsn! { @row() Node { align_items: AlignItems::Stretch } });
            spawn_in(commands, tiles, stat("Runs", format!("{}/{MIN_RUNS}", progress.runs)));
            spawn_in(commands, tiles, stat("Flicks", format!("{}/{MIN_FLICKS}", progress.flicks)));
            spawn_in(commands, tiles, stat("Days", format!("{}/{MIN_DAYS}", progress.days)));
            if let Some(previous) = status.previous_cm {
                let text = format!("Then the change from {previous:.1} cm/360 is checked against the runs before it.");
                spawn_in(commands, card, small(text));
            }
        },
        Advice::Quiet => {
            let text = format!(
                "{} runs on {} days at this sensitivity show no stable pattern. No suggestion without one.",
                progress.runs, progress.days
            );
            spawn_in(commands, card, callout(TEXT_GOOD, "Nothing to change", text));
        },
        Advice::Step { finding, to_cm } => {
            let sens = round_sens(sens_for(settings, to_cm));
            let way = if finding.pattern.slower() { "slower" } else { "faster" };
            let text = format!(
                "{} One step of {:.0}% ({to_cm:.1} cm/360), checked against these runs once it has its own.",
                finding_text(&finding),
                STEP * 100.0
            );
            spawn_in(commands, card, callout(TEXT_WARN, format!("Try one step {way}: {sens:.3}"), text));
            apply(commands, to_cm, format!("Apply {sens:.3}"));
        },
        Advice::Held { finding, by } => {
            let why = match by {
                HeldBy::NoRange => "A step is only suggested inside your measured range: calibrate first.",
                HeldBy::EmptyRange => "Your limits leave no range to step in.",
                HeldBy::Outside => "Your sensitivity is outside your range: the range's pick comes first.",
                HeldBy::Limit => "You are at the edge of your range on that side, so no step is suggested.",
            };
            let text = format!("{} {why}", finding_text(&finding));
            spawn_in(commands, card, callout(TEXT_WARN, "A pattern, but no step", text));
        },
        Advice::Revert { to_cm } => {
            let sens = round_sens(sens_for(settings, to_cm));
            let text = status.review.as_ref().map(review_text).unwrap_or_default();
            spawn_in(commands, card, callout(TEXT_BAD, format!("The change did not help: back to {sens:.3}"), text));
            apply(commands, to_cm, format!("Back to {sens:.3}"));
        },
    }
    if let Some(review) = status.review.as_ref().filter(|_| !matches!(status.advice, Advice::Revert { .. })) {
        spawn_in(commands, card, small(review_text(review)));
    }
    let note = format!(
        "After every practice run of a flick drill the monitor keeps a summary (first-flick landing, where clicks land, corrections, misses). It speaks only when a pattern holds over at least {MIN_RUNS} runs, {MIN_FLICKS} flicks and {MIN_DAYS} days at one sensitivity, stands clear of your own run-to-run scatter and showed on each of the latest days. Runs at one sensitivity cannot tell how another would have gone: a step is something to try, never an optimum, and it is checked afterwards."
    );
    spawn_in(commands, card, details("When it speaks", note));
}

// ---------------------------------------------------------------------------------------------
// 3: the checks
// ---------------------------------------------------------------------------------------------

/// A check: its name and status on the left, its buttons on the right.
fn check_row(commands: &mut Commands, parent: Entity, title: &'static str, status: String) -> Entity {
    let line =
        spawn_in(commands, parent, bsn! { @row() Node { column_gap: px(SPACE_M), align_items: AlignItems::Start } });
    spawn_in(
        commands,
        line,
        bsn! {
            @column()
            Node { flex_grow: 1.0, flex_basis: px(0), row_gap: px(1) }
            Children [ @label_strong(title) -- @small(status) ]
        },
    );
    spawn_in(commands, line, bsn! { @row() Node { flex_shrink: 0.0 } })
}

fn checks_card(
    commands: &mut Commands,
    parent: Entity,
    tired: TiredVerdict,
    fast: Option<(f32, f32)>,
    comparison: Option<&LongComparison>,
) {
    let card = section(commands, parent, "3 · Checks");
    let status = match tired {
        TiredVerdict::Collecting { fresh, tired } => format!(
            "A minute of fine control, rested and again after a match. {fresh} fresh, {tired} tired so far; {MIN_CHECKS} of each are needed."
        ),
        TiredVerdict::Steady { fresh, tired, change } => format!(
            "No clear drop when tired: control error {:+.0}%, inside the scatter of your {fresh} fresh and {tired} tired checks.",
            (change - 1.0) * 100.0
        ),
        TiredVerdict::Drops { loss, .. } => match fast {
            Some((fresh, now)) => format!(
                "Control drops when tired: {:.0}% more error. Your fast limit moved from {fresh:.0} to {now:.0} cm/360.",
                (loss - 1.0) * 100.0
            ),
            None => format!(
                "Control drops when tired: {:.0}% more error. Calibrate to see what that means for your range.",
                (loss - 1.0) * 100.0
            ),
        },
    };
    let buttons = check_row(commands, card, "Tired check", status);
    for (text, state) in [("Fresh", Freshness::Fresh), ("Tired", Freshness::Tired)] {
        let button = spawn_in(commands, buttons, bsn! { @compact_button(text, Btn::Secondary) on(start_tired_check) });
        commands.entity(button).insert(TiredButton(state));
    }
    spawn_in(commands, card, rule());
    let (status, action) = match comparison {
        None => (
            format!(
                "Two sensitivities, 8 minutes a day for {} to {} days, judged on precision.",
                compare::MIN_DAYS,
                compare::MAX_DAYS
            ),
            "Set up",
        ),
        Some(comparison) => {
            let (days, today) = (comparison.days().len(), comparison.played_today(unix_now()));
            let pair = format!("{:.1} against {:.1} cm/360", comparison.a_cm, comparison.b_cm);
            match comparison.verdict() {
                compare::Verdict::Running if today >= DAY_BLOCKS => {
                    (format!("{pair}: {days} days done, today's blocks are played. Next ones tomorrow."), "Open")
                },
                compare::Verdict::Running => {
                    (format!("{pair}: {days} days done, {today} of {DAY_BLOCKS} blocks today."), "Continue")
                },
                _ => (format!("{pair}: the verdict is in after {days} days."), "See verdict"),
            }
        },
    };
    let buttons = check_row(commands, card, "Long comparison", status);
    spawn_in(commands, buttons, bsn! { @compact_button(action, Btn::Secondary) on(open_comparison) });
}

fn start_tired_check(
    activate: On<Activate>,
    buttons: Query<&TiredButton>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Ok(&TiredButton(state)) = buttons.get(activate.event_target()) else { return };
    commands.insert_resource(Checkup::new(CheckupKind::Tired(state)));
    next_state.set(AppState::Finder);
}

fn open_comparison(
    _: On<Activate>,
    store: Res<CompareStore>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let session = if store.active.is_some() { CompareSession::Running } else { CompareSession::Setup };
    commands.insert_resource(session);
    next_state.set(AppState::Finder);
}

// ---------------------------------------------------------------------------------------------
// 4: the quick finders
// ---------------------------------------------------------------------------------------------

fn quick_card(commands: &mut Commands, parent: Entity) {
    let card = section(commands, parent, "4 · Quick finder: a first look");
    let buttons = check_row(
        commands,
        card,
        "Blind 20 s trials",
        "A hint in ten minutes: short trials mostly measure how fast you adapt.".to_string(),
    );
    spawn_in(
        commands,
        buttons,
        bsn! {
            @compact_button("Open", Btn::Secondary)
            on(|_: On<Activate>, mut view: ResMut<FinderView>| *view = FinderView::Quick)
        },
    );
}

/// The quick finders' settings: method, trial length, the smart finder's focus and the classic
/// finder's drill and rounds.
fn quick_view(settings: &Settings) -> impl Scene {
    let classic_drills: Vec<_> = DrillKind::ALL
        .into_iter()
        .filter(|kind| kind.def().classic_finder)
        .map(|kind| choice(kind.name(), Choice::FinderDrill(kind)))
        .collect();
    bsn! {
        @column()
        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), row_gap: px(SPACE_M) }
        Children [
            @row()
            Children [
                @compact_button("Back to your range", Btn::Ghost)
                on(|_: On<Activate>, mut view: ResMut<FinderView>| *view = FinderView::Overview)
                --
                @label_dim("Quick finder: a first look, not a verdict")
            ]
            --
            Node { flex_direction: FlexDirection::Row, column_gap: px(SPACE_L), flex_grow: 1.0, min_height: px(0) }
            Children [
                @main_column()
                Children [
                    @card("Method", bsn_list! {
                        @setting_row("Method", bsn_list! {
                            @choice("Smart (score model)", Choice::FinderMethod(FinderMethod::Smart))
                            --
                            @choice("Classic (A/B)", Choice::FinderMethod(FinderMethod::Classic))
                        })
                        --
                        @number_field_hint(SettingField::FinderTrialSecs, "How long each drill of a trial lasts.", settings)
                    })
                    --
                    @card("Smart", bsn_list! {
                        @setting_row_hint("Focus", "The drills every trial plays.", bsn_list! {
                            @choice("Balanced", Choice::FinderFocus(FinderFocus::Balanced))
                            --
                            @choice("Flicking", Choice::FinderFocus(FinderFocus::Flicking))
                            --
                            @choice("Tracking", Choice::FinderFocus(FinderFocus::Tracking))
                        })
                        --
                        @small("Balanced: Microflick + Tracking. Flicking: Gridshot + Microflick. Tracking: Tracking.")
                    })
                    --
                    @card("Classic", bsn_list! {
                        @setting_row("Drill", classic_drills)
                        --
                        @number_field_hint(SettingField::FinderRounds, "The range around your sens halves every round.", settings)
                    })
                    --
                    @cta("Start quick finder")
                    Node { align_self: AlignSelf::FlexStart, min_width: px(200) }
                    on(start_finder)
                ]
                --
                @side_column()
                Children [
                    @card("What a first look can tell", bsn_list! {
                        @callout(TEXT_ACCENT, "A hint, not a verdict", "Aim is near-equal over a wide range of sensitivities, and 20 s trials mostly measure how fast you adapt. Check a change it proposes with the long comparison.")
                        --
                        @numbered_step("1", "6 to 12 blind trials", "Between 0.6x and 1.6x your sens: first both ends and your own, then wherever a model of your scores expects to learn most.")
                        --
                        @numbered_step("2", "Play, then rate the feel", "Every trial plays the focus drills at one hidden sens, the first 3 s of each unscored. Then you rate how it felt.")
                        --
                        @numbered_step("3", "Kept inside your range", "It keeps your current sens if that is in its best range, and never leaves your calibrated range.")
                        --
                        @details("More on the model", "The model is a Gaussian process that also factors out warm-up. Your best range is every sens likely within 3% of your best; only a clear trend in where your flicks and clicks land can move the pick inside it.")
                    })
                    --
                    @note_card("How Classic works", bsn_list! {
                        @small("Blind A/B rounds: play the two ends of a range around your sens and pick the one that felt better. Uses the Classic drill and rounds.")
                    })
                ]
            ]
        ]
    }
}
