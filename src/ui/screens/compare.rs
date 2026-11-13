//! Screens of the long comparison ([`LongComparison`]): choosing the second candidate, the
//! day's blocks, and the verdict.

use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;

use super::checkup::leave_finder;
use super::score::screen_title;
use crate::AppState;
use crate::drill::begin_run;
use crate::finder::compare::{
    ADAPT_SECS, CompareSession, CompareStore, DAY_BLOCKS, LongComparison, MAX_DAYS, MIN_DAYS, SCORED_SECS, Side,
    Verdict,
};
use crate::finder::limits::{CalibrationStore, Limits, sens_for};
use crate::finder::monitor::{self, Advice, MonitorLog, STEP};
use crate::finder::tired::TiredLog;
use crate::finder::unix_now;
use crate::history::format_date;
use crate::settings::Settings;
use crate::ui::charts::spawn_in;
use crate::ui::range::round_sens;
use crate::ui::theme::*;
use crate::ui::widgets::*;

const COMPARE_W: f32 = 660.0;

/// The second candidate picked on the setup screen: an index into [`candidates`].
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ComparePick(pub usize);

/// A button that picks this candidate.
#[derive(Component, Clone, Copy)]
struct CandidateButton(usize);

/// A sensitivity offered as the second candidate, with why.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub cm: f32,
    pub why: &'static str,
}

/// What can be compared with the current sensitivity: the range's pick and the monitor's step
/// if there are any, the nearest end of the range if the current sensitivity is outside it,
/// then one and two steps slower and faster. Nothing outside the calibrated range is offered,
/// and candidates within 2% of the current sensitivity or of an earlier one are dropped.
pub fn candidates(current: f32, limits: Option<&Limits>, advice: &Advice) -> Vec<Candidate> {
    use crate::finder::limits::Verdict as Fit;
    let mut wanted = Vec::new();
    let range = limits.and_then(Limits::range);
    if let Some(Fit::TooFast { pick, .. } | Fit::TooSlow { pick, .. }) = limits.map(|limits| limits.verdict(current)) {
        wanted.push(Candidate { cm: pick, why: "your range's pick" });
    }
    if let Advice::Step { to_cm, .. } | Advice::Revert { to_cm } = *advice {
        wanted.push(Candidate { cm: to_cm, why: "the monitor's suggestion" });
    }
    if let Some((fast, slow)) = range {
        wanted.push(Candidate { cm: current.clamp(fast, slow), why: "the nearest end of your range" });
    }
    let (one, two) = (1.0 + STEP, (1.0 + STEP).powi(2));
    wanted.extend([
        Candidate { cm: current * one, why: "5% slower" },
        Candidate { cm: current / one, why: "5% faster" },
        Candidate { cm: current * two, why: "10% slower" },
        Candidate { cm: current / two, why: "10% faster" },
    ]);
    let mut list: Vec<Candidate> = Vec::new();
    for candidate in wanted {
        let inside = range.is_none_or(|(fast, slow)| (fast..=slow).contains(&candidate.cm));
        let apart = |other: f32| (candidate.cm / other).ln().abs() >= 0.02;
        if inside && apart(current) && list.iter().all(|listed| apart(listed.cm)) {
            list.push(candidate);
        }
    }
    list
}

fn panel_in(commands: &mut Commands) -> Entity {
    let root = commands.spawn_scene(overlay()).insert(DespawnOnExit(AppState::Finder)).id();
    spawn_in(commands, root, bsn! { @panel() Node { width: px(COMPARE_W), max_height: percent(96) } })
}

/// The comparison's screen: setup, the day's plan, or the verdict.
#[allow(clippy::too_many_arguments)]
pub fn spawn_compare_screen(
    mut commands: Commands,
    session: Option<Res<CompareSession>>,
    store: Res<CompareStore>,
    pick: Res<ComparePick>,
    settings: Res<Settings>,
    calibration: Res<CalibrationStore>,
    tired: Res<TiredLog>,
    monitor: Res<MonitorLog>,
) {
    let Some(session) = session else { return };
    let commands = &mut commands;
    match (*session, &store.active) {
        (CompareSession::Setup, _) | (_, None) => {
            let current = settings.cm_per_360(settings.sens);
            let limits = calibration.latest.as_ref().map(|c| Limits::new(c, tired.verdict(current).loss()));
            let advice = monitor::status(monitor.entries(), current, limits.as_ref()).advice;
            setup(commands, &settings, &candidates(current, limits.as_ref(), &advice), pick.0);
        },
        (_, Some(comparison)) => match comparison.verdict() {
            Verdict::Running => day(commands, comparison, &settings),
            verdict => verdict_screen(commands, comparison, verdict, &settings),
        },
    }
}

fn setup(commands: &mut Commands, settings: &Settings, candidates: &[Candidate], picked: usize) {
    let current = settings.cm_per_360(settings.sens);
    let panel = panel_in(commands);
    spawn_in(commands, panel, screen_title("Long comparison", "What do you want to compare?"));
    let a = format!("A is your sensitivity now: {:.3} ({current:.1} cm/360). Pick B:", settings.sens);
    spawn_in(commands, panel, label(a));
    if candidates.is_empty() {
        let text =
            "Your range leaves no other sensitivity to compare with: its two limits are closer than a step apart.";
        spawn_in(commands, panel, callout(TEXT_WARN, "Nothing to compare", text));
        spawn_in(commands, panel, bsn! { @actions(bsn_list! { @button("Back", Btn::Primary) on(leave_finder) }) });
        return;
    }
    let list = spawn_in(commands, panel, bsn! { @wrap_row() });
    for (index, candidate) in candidates.iter().enumerate() {
        let text = format!(
            "{:.3}  ·  {:.1} cm/360  ·  {}",
            round_sens(sens_for(settings, candidate.cm)),
            candidate.cm,
            candidate.why
        );
        let kind = if index == picked.min(candidates.len() - 1) { Btn::Selected } else { Btn::Tile };
        let button = spawn_in(commands, list, bsn! { @compact_button(text, kind) on(pick_candidate) });
        commands.entity(button).insert(CandidateButton(index));
    }
    let plan = format!(
        "Each day you play {DAY_BLOCKS} blocks of {:.0} minutes: A, B, B, A (the other way round the next day). The first {ADAPT_SECS:.0} s of a block are for getting used to its sensitivity and don't count; the other {SCORED_SECS:.0} s are judged on how close to the centre your first clicks land, with accuracy beside it. A verdict comes after {MIN_DAYS} to {MAX_DAYS} days, and only if the days agree.",
        (ADAPT_SECS + SCORED_SECS) / 60.0
    );
    spawn_in(commands, panel, callout(TEXT_ACCENT, "8 minutes a day, for at least 5 days", plan));
    spawn_in(
        commands,
        panel,
        bsn! {
            @actions(bsn_list! {
                @button("Start comparison", Btn::Primary)
                on(start_comparison)
                --
                @button("Cancel", Btn::Secondary)
                on(leave_finder)
            })
        },
    );
}

fn pick_candidate(
    activate: On<Activate>,
    buttons: Query<&CandidateButton>,
    mut pick: ResMut<ComparePick>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if let Ok(&CandidateButton(index)) = buttons.get(activate.event_target()) {
        pick.0 = index;
        // Re-enter to show the choice.
        next_state.set(AppState::Finder);
    }
}

#[allow(clippy::too_many_arguments)]
fn start_comparison(
    _: On<Activate>,
    settings: Res<Settings>,
    calibration: Res<CalibrationStore>,
    tired: Res<TiredLog>,
    monitor: Res<MonitorLog>,
    pick: Res<ComparePick>,
    mut store: ResMut<CompareStore>,
    mut session: ResMut<CompareSession>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let current = settings.cm_per_360(settings.sens);
    let limits = calibration.latest.as_ref().map(|c| Limits::new(c, tired.verdict(current).loss()));
    let advice = monitor::status(monitor.entries(), current, limits.as_ref()).advice;
    let candidates = candidates(current, limits.as_ref(), &advice);
    let Some(b) = candidates.get(pick.0.min(candidates.len().saturating_sub(1))) else { return };
    store.active = Some(LongComparison::new(unix_now(), current, b.cm));
    *session = CompareSession::Running;
    next_state.set(AppState::Finder);
}

/// "0.380 (43.0 cm/360)" for a candidate.
fn name(comparison: &LongComparison, side: Side, settings: &Settings) -> String {
    let cm = comparison.cm(side);
    format!("{:.3} ({cm:.1} cm/360)", round_sens(sens_for(settings, cm)))
}

/// The complete days so far, one row each.
fn day_table(commands: &mut Commands, parent: Entity, comparison: &LongComparison) {
    let days = comparison.days();
    if days.is_empty() {
        return;
    }
    const DAY_W: f32 = 120.0;
    const VALUE_W: f32 = 118.0;
    let table = spawn_in(commands, parent, bsn! { @surface() Node { row_gap: px(2) } });
    let header: Vec<_> = [
        ("Day", DAY_W),
        ("A: click error", VALUE_W),
        ("B: click error", VALUE_W),
        ("A: accuracy", VALUE_W),
        ("B: accuracy", VALUE_W),
    ]
    .into_iter()
    .map(|(text, width)| header_cell(text, width))
    .collect();
    spawn_in(commands, table, table_header(header));
    for (index, day) in days.iter().enumerate() {
        // The lower click error and the higher accuracy of the day stand out.
        let strong = |better: bool| if better { TEXT_STRONG } else { tokens::TEXT_DIM };
        let cells = vec![
            cell(format_date(day.day * crate::finder::stats::SECS_PER_DAY), DAY_W, tokens::TEXT_DIM),
            cell(format!("{:.2}°", day.a.0), VALUE_W, strong(day.a.0 <= day.b.0)),
            cell(format!("{:.2}°", day.b.0), VALUE_W, strong(day.b.0 <= day.a.0)),
            cell(format!("{:.0}%", day.a.1 * 100.0), VALUE_W, strong(day.a.1 >= day.b.1)),
            cell(format!("{:.0}%", day.b.1 * 100.0), VALUE_W, strong(day.b.1 >= day.a.1)),
        ];
        spawn_in(commands, table, table_row(cells, index % 2 == 0));
    }
}

/// The day's plan: which block is next, or that today is done.
fn day(commands: &mut Commands, comparison: &LongComparison, settings: &Settings) {
    let now = unix_now();
    let days = comparison.days();
    let today_done = comparison.played_today(now);
    let finished_today = today_done >= DAY_BLOCKS;
    let panel = panel_in(commands);
    let context = format!(
        "Long comparison  ·  A {}  against  B {}",
        name(comparison, Side::A, settings),
        name(comparison, Side::B, settings)
    );
    let day_number = days.len() + usize::from(!finished_today);
    spawn_in(commands, panel, screen_title(context, format!("Day {day_number}")));
    let pips: Vec<_> = (0..DAY_BLOCKS)
        .map(|index| {
            let color = match index.cmp(&today_done) {
                std::cmp::Ordering::Less => ACCENT,
                std::cmp::Ordering::Equal => TEXT_STRONG,
                std::cmp::Ordering::Greater => BORDER_STRONG,
            };
            bsn! {
                Node { flex_grow: 1.0, height: px(4), border_radius: BorderRadius::all(px(2)) }
                ThemeBackgroundColor(color)
            }
        })
        .collect();
    spawn_in(commands, panel, bsn! { @row() Node { column_gap: px(SPACE_XS) } Children [ {pips} ] });
    let more = MIN_DAYS.saturating_sub(days.len());
    let outlook = if more > 0 {
        format!("{} of {MIN_DAYS} days needed before a verdict.", days.len())
    } else {
        format!("{} days so far and no clear difference yet. It ends after day {MAX_DAYS} at the latest.", days.len())
    };
    match comparison.next_side(now) {
        Some(side) => {
            let text = format!(
                "Next: block {} of {DAY_BLOCKS}, sensitivity {} = {}. The first {ADAPT_SECS:.0} s are for adapting and don't count; then {SCORED_SECS:.0} s where precision counts, not kills.",
                today_done + 1,
                side.letter(),
                name(comparison, side, settings),
            );
            spawn_in(commands, panel, label(text));
            spawn_in(commands, panel, small(outlook));
        },
        None => {
            spawn_in(
                commands,
                panel,
                callout(TEXT_GOOD, "Today's blocks are done", format!("Come back tomorrow. {outlook}")),
            );
        },
    }
    day_table(commands, panel, comparison);
    let actions = spawn_in(commands, panel, actions(bsn_list! {}));
    // Clicks of the block just played may still be in flight.
    let armed = |commands: &mut Commands, button: Entity| {
        commands.entity(button).insert((ArmDelay::default(), InteractionDisabled));
    };
    if finished_today {
        let done = spawn_in(commands, actions, bsn! { @button("Done for today", Btn::Primary) on(leave_finder) });
        armed(commands, done);
    } else {
        let start = spawn_in(commands, actions, bsn! { @button("Start block", Btn::Primary) on(start_block) });
        let later = spawn_in(commands, actions, bsn! { @button("Later", Btn::Secondary) on(leave_finder) });
        armed(commands, start);
        armed(commands, later);
    }
    spawn_in(commands, actions, spacer());
    let end = spawn_in(commands, actions, bsn! { @button("End comparison", Btn::Danger) on(end_comparison) });
    armed(commands, end);
}

fn start_block(
    _: On<Activate>,
    store: Res<CompareStore>,
    settings: Res<Settings>,
    mut session: ResMut<CompareSession>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let now = unix_now();
    let Some(comparison) = &store.active else { return };
    let Some(side) = comparison.next_side(now) else { return };
    *session = CompareSession::Block(side);
    begin_run(&mut commands, &mut next_state, comparison.block_run(side, now, &settings));
}

fn end_comparison(
    _: On<Activate>,
    mut store: ResMut<CompareStore>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    store.active = None;
    commands.remove_resource::<CompareSession>();
    next_state.set(AppState::Menu);
}

/// A button that sets the sensitivity to this value and ends the comparison.
#[derive(Component, Clone, Copy)]
struct ApplyWinner(f32);

fn verdict_screen(commands: &mut Commands, comparison: &LongComparison, verdict: Verdict, settings: &Settings) {
    let days = comparison.days().len();
    let current = settings.cm_per_360(settings.sens);
    let is_current = |side: Side| (comparison.cm(side) / current).ln().abs() < 0.005;
    let panel = panel_in(commands);
    let context = format!(
        "Long comparison  ·  A {}  against  B {}",
        name(comparison, Side::A, settings),
        name(comparison, Side::B, settings)
    );
    spawn_in(commands, panel, screen_title(context, format!("Verdict after {days} days")));
    // The sensitivity to go on with, and whether that is a change.
    let (winner, tone, title, text) = match verdict {
        Verdict::Better { side, click_error, accuracy } => (
            Some(side),
            TEXT_GOOD,
            format!("{} is more precise: {}", side.letter(), name(comparison, side, settings)),
            format!(
                "Over {days} days its first clicks landed {:.0}% closer to the centre, with accuracy {:+.1} points, each day compared with the other sensitivity on the same day. The difference is clear of the day-to-day scatter.",
                (1.0 - click_error) * 100.0,
                accuracy * 100.0
            ),
        ),
        Verdict::TradeOff => (
            None,
            TEXT_WARN,
            "No winner: a trade-off".to_string(),
            "One sensitivity clearly clicks closer to the centre, the other clearly hits more of its shots. The table shows which; that choice is yours, and there is no reason in it to change.".to_string(),
        ),
        _ => (
            None,
            TEXT_ACCENT,
            "No measurable difference".to_string(),
            format!(
                "After {days} days neither sensitivity is clearly more precise. Both sit on the same plateau for you: keep the one you have, there is nothing to gain from changing."
            ),
        ),
    };
    let headline = match winner {
        Some(side) => round_sens(sens_for(settings, comparison.cm(side))),
        None => settings.sens,
    };
    let caption = if winner.is_some_and(|side| !is_current(side)) { "Change to" } else { "Keep" };
    spawn_in(commands, panel, hero(caption, format!("{headline:.3}")));
    spawn_in(commands, panel, callout(tone, title, text));
    day_table(commands, panel, comparison);
    let actions = spawn_in(commands, panel, actions(bsn_list! {}));
    match winner.filter(|&side| !is_current(side)) {
        Some(_) => {
            let apply = spawn_in(commands, actions, bsn! { @button("Apply", Btn::Primary) on(apply_winner) });
            commands.entity(apply).insert(ApplyWinner(headline));
            spawn_in(commands, actions, bsn! { @button("Keep mine", Btn::Secondary) on(end_comparison) });
        },
        None => {
            spawn_in(commands, actions, bsn! { @button("Close comparison", Btn::Primary) on(end_comparison) });
        },
    }
    spawn_in(commands, actions, bsn! { @button("Decide later", Btn::Ghost) on(leave_finder) });
}

fn apply_winner(
    activate: On<Activate>,
    buttons: Query<&ApplyWinner>,
    mut settings: ResMut<Settings>,
    mut store: ResMut<CompareStore>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Ok(&ApplyWinner(sens)) = buttons.get(activate.event_target()) else { return };
    settings.sens = sens;
    store.active = None;
    commands.remove_resource::<CompareSession>();
    next_state.set(AppState::Menu);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finder::limits::tests::calibration;
    use crate::finder::monitor::{Finding, Pattern};

    #[test]
    fn candidates_stay_in_the_range_and_apart() {
        // Range 26.9 to 58 cm/360.
        let limits = Limits::new(&calibration(29.0, 7.5, 0.12), None);
        let cms = |list: Vec<Candidate>| list.into_iter().map(|c| (c.cm, c.why)).collect::<Vec<_>>();
        // In the middle of the range: the four steps.
        let list = cms(candidates(40.0, Some(&limits), &Advice::Quiet));
        assert_eq!(
            list.iter().map(|c| c.1).collect::<Vec<_>>(),
            ["5% slower", "5% faster", "10% slower", "10% faster"]
        );
        assert!((list[0].0 - 42.0).abs() < 1e-3 && (list[3].0 - 40.0 / 1.1025).abs() < 1e-3);
        // At the pad's limit nothing slower is offered; the monitor's step comes first.
        let finding = Finding { pattern: Pattern::Undershoot, value: -0.2, samples: 30, days: 5 };
        let step = Advice::Step { finding, to_cm: 58.0 / 1.05 };
        let list = cms(candidates(58.0, Some(&limits), &step));
        assert_eq!(list.iter().map(|c| c.1).collect::<Vec<_>>(), ["the monitor's suggestion", "10% faster"]);
        // Outside the range: the pick first, and every candidate inside the range.
        let list = cms(candidates(20.0, Some(&limits), &Advice::Quiet));
        assert_eq!(
            list.iter().map(|c| c.1).collect::<Vec<_>>(),
            ["your range's pick", "the nearest end of your range"]
        );
        assert!(list.iter().all(|c| (limits.fast..=limits.slow).contains(&c.0)), "{list:?}");
        // Without a calibration: plain steps.
        assert_eq!(candidates(40.0, None, &Advice::Quiet).len(), 4);
    }
}
