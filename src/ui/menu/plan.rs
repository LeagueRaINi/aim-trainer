//! Weekly plan page: today's session with its blocks and why each is there (and its one Play
//! button), the week at a glance with what is done, the player's choices (days a week, focus),
//! and what changed since last week's plan. [`refresh_plan`] makes the week's plan when it is
//! out of date; [`rebuild_plan`] fills the page from it.

use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::benchmark::start_benchmark;
use super::{MAIN_COLUMN_W, MenuTab, page};
use crate::AppState;
use crate::analysis::{self, RECENT_REPLAYS};
use crate::calm_log::CalmLog;
use crate::custom::CustomContent;
use crate::history::History;
use crate::replay::{Replay, ReplayDir};
use crate::routine::start_routine;
use crate::settings::Settings;
use crate::training::benchmark::{BenchmarkLog, SCENARIOS};
use crate::training::plan::{
    BLOCK_SECS, DAYS_RANGE, DayKind, Focus, PlanDay, PlanStore, SESSION_BLOCKS, WeakSpot, WeekPlan, changes, generate,
    session_routine,
};
use crate::training::{day_label, local_day, today, utc_offset_secs};
use crate::ui::charts::*;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// The container [`rebuild_plan`] fills.
#[derive(Component, Clone, Copy, Default)]
pub struct PlanBody;

#[derive(Component, Clone, Copy, Default)]
struct DaysButton(u8);

#[derive(Component, Clone, Copy, Default)]
struct FocusButton(Focus);

const DAY_W: f32 = 96.0;
const WHAT_W: f32 = 150.0;
const STATUS_W: f32 = 70.0;

pub(super) fn plan_page() -> impl Scene {
    bsn! {
        @page(MenuTab::Plan, bsn_list! {
            @scroll_column(bsn_list! {
                Node { flex_direction: FlexDirection::Row, column_gap: px(SPACE_L), align_items: AlignItems::Start }
                PlanBody
            })
            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
        })
    }
}

/// Makes this week's plan when the page is opened, or a choice on it changed, and the stored
/// plan is out of date (see [`PlanStore::stale`]). Only then are the replays analysed for the
/// weak spot, inline as the Insights page does.
pub fn refresh_plan(
    opened: Query<(), Added<PlanBody>>,
    mut store: ResMut<PlanStore>,
    history: Res<History>,
    log: Res<BenchmarkLog>,
    calm: Res<CalmLog>,
    replays: Res<ReplayDir>,
) {
    if opened.is_empty() && !store.is_changed() {
        return;
    }
    let today = today();
    if !store.stale(today, log.latest().map(|b| b.timestamp)) {
        return;
    }
    let analysis = analysis::analyze(&Replay::recent(&replays.0, &analysis::flick_kinds(), RECENT_REPLAYS));
    let weak_spot = analysis
        .findings
        .first()
        .filter(|_| analysis.enough_data())
        .map(|finding| WeakSpot { sector: finding.cell.sector, band: finding.cell.band });
    let inputs = store.inputs(&history, &log, &calm, weak_spot, today, utc_offset_secs());
    store.adopt(generate(&inputs), today);
}

pub fn rebuild_plan(
    mut commands: Commands,
    bodies: Query<(Entity, Ref<PlanBody>)>,
    store: Res<PlanStore>,
    log: Res<BenchmarkLog>,
) {
    for (body, marker) in &bodies {
        if !marker.is_added() && !store.is_changed() {
            continue;
        }
        commands.entity(body).despawn_related::<Children>();
        let Some(plan) = store.current.as_ref() else { continue };
        let today = today();
        let offset = utc_offset_secs();
        let benchmarked = |day: i64| log.entries().iter().any(|b| local_day(b.timestamp, offset) == day);
        let commands = &mut commands;
        let node = Node { width: px(MAIN_COLUMN_W), flex_shrink: 0.0, ..default() };
        let left = column_in(commands, body, node);
        let node = Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), ..default() };
        let right = column_in(commands, body, node);
        today_card(commands, left, plan, today, benchmarked(today));
        choices_card(commands, left, &store);
        week_card(commands, right, plan, today, &benchmarked);
        changes_card(commands, right, store.previous.as_ref(), plan);
    }
}

fn column_in(commands: &mut Commands, parent: Entity, node: Node) -> Entity {
    let node = Node { flex_direction: FlexDirection::Column, row_gap: px(SPACE_L), ..node };
    commands.spawn((ChildOf(parent), node)).id()
}

/// The next planned day after `today` that isn't a rest day.
fn next_day(plan: &WeekPlan, today: i64) -> Option<&PlanDay> {
    plan.schedule.iter().find(|d| d.day > today && d.kind != DayKind::Rest)
}

fn day_kind(kind: DayKind) -> &'static str {
    match kind {
        DayKind::Train => "Training session",
        DayKind::Benchmark => "Benchmark",
        DayKind::Rest => "Rest",
    }
}

/// Today: the session with its blocks and Play, the benchmark, or a rest day.
fn today_card(commands: &mut Commands, parent: Entity, plan: &WeekPlan, today: i64, benchmarked: bool) {
    let card = spawn_in(commands, parent, surface());
    spawn_in(commands, card, overline(format!("Today  ·  {}", day_label(today))));
    let next = match next_day(plan, today) {
        Some(day) => format!("Next: {}, {}.", day_label(day.day), day_kind(day.kind).to_lowercase()),
        None => "Next week's plan comes on Monday.".to_string(),
    };
    let Some(day) = plan.day(today) else { return };
    match day.kind {
        DayKind::Train if !day.done => {
            spawn_in(commands, card, subheading("Training session"));
            let length = format!(
                "{SESSION_BLOCKS} blocks of {BLOCK_SECS:.0} s with the kinds of aim mixed: about 18 minutes with breaks."
            );
            spawn_in(commands, card, label_dim(length));
            spawn_in(commands, card, bsn! { @cta("Start today's session") on(start_session) });
        },
        DayKind::Train => {
            let text = format!("Rest counts too: spreading practice over days beats one long session. {next}");
            spawn_in(commands, card, callout(TEXT_GOOD, "Session done", text));
            spawn_in(
                commands,
                card,
                bsn! {
                    @compact_button("Play it again", Btn::Secondary)
                    Node { align_self: AlignSelf::FlexStart }
                    on(start_session)
                },
            );
        },
        DayKind::Benchmark if !benchmarked => {
            spawn_in(commands, card, subheading("Benchmark day"));
            let text = format!(
                "{} fixed scenarios, about ten minutes. Same time of day and warm-up as last time, so it compares.",
                SCENARIOS.len()
            );
            spawn_in(commands, card, label_dim(text));
            spawn_in(commands, card, bsn! { @cta("Start benchmark") on(start_benchmark) });
        },
        DayKind::Benchmark => {
            let text = format!("Your result is on the Benchmark page, and next week's plan builds on it. {next}");
            spawn_in(commands, card, callout(TEXT_GOOD, "Benchmark done", text));
        },
        DayKind::Rest => {
            let text =
                format!("Short sessions on several days beat one long one, and the plan counts on the rest. {next}");
            spawn_in(commands, card, callout(TEXT_ACCENT, "Rest day", text));
        },
    }
    if !day.blocks.is_empty() {
        let list =
            spawn_in(commands, card, bsn! { @column() Node { row_gap: px(2), margin: UiRect::top(px(SPACE_XS)) } });
        for (i, block) in day.blocks.iter().enumerate() {
            block_row(commands, list, i, &block.drill.name(), block.reason(plan));
        }
    }
    if plan.basis.benchmark.is_none() {
        let text = "Without a benchmark this is a default week for your focus. Once you have one, the kind of aim that improves least gets more blocks.";
        spawn_in(commands, parent, callout(TEXT_ACCENT, "A default week for now", text));
    }
}

/// One block of a session: its number, what it plays and why.
fn block_row(commands: &mut Commands, parent: Entity, index: usize, name: &str, reason: String) {
    let row = spawn_in(
        commands,
        parent,
        bsn! {
            @row()
            Node { column_gap: px(SPACE_M), align_items: AlignItems::FlexStart, padding: UiRect::axes(px(SPACE_S), px(3)) }
        },
    );
    spawn_in(commands, row, bsn! { @label_dim(format!("{}", index + 1)) Node { width: px(18), flex_shrink: 0.0 } });
    let text = spawn_in(commands, row, bsn! { @column() Node { row_gap: px(0), flex_grow: 1.0, flex_basis: px(0) } });
    spawn_in(commands, text, label_strong(name.to_string()));
    spawn_in(commands, text, small(reason));
}

/// The week, Monday to Sunday, with what each day is and whether it is done.
fn week_card(commands: &mut Commands, parent: Entity, plan: &WeekPlan, today: i64, benchmarked: &dyn Fn(i64) -> bool) {
    let card = spawn_in(commands, parent, bsn! { @surface() Node { row_gap: px(2) } });
    let done = plan.sessions().filter(|d| d.done).count();
    let title = format!("This week  ·  {done} of {} sessions done", plan.sessions().count());
    spawn_in(commands, card, bsn! { @overline(title) Node { margin: UiRect::bottom(px(SPACE_XS)) } });
    for day in &plan.schedule {
        let finished = match day.kind {
            DayKind::Train => day.done,
            DayKind::Benchmark => benchmarked(day.day),
            DayKind::Rest => false,
        };
        let (status, color) = if day.kind == DayKind::Rest {
            ("", tokens::TEXT_DIM)
        } else if finished {
            ("Done", TEXT_GOOD)
        } else if day.day == today {
            ("Today", TEXT_ACCENT)
        } else if day.day < plan.made {
            ("", tokens::TEXT_DIM)
        } else if day.day < today {
            ("Missed", tokens::TEXT_DIM)
        } else {
            ("Planned", tokens::TEXT_MAIN)
        };
        let what = match day.kind {
            DayKind::Train => format!("Training · {} blocks", day.blocks.len()),
            kind => day_kind(kind).to_string(),
        };
        // Days before the plan was made were never part of it.
        let what_color = if day.kind == DayKind::Rest || day.day < plan.made { tokens::TEXT_DIM } else { TEXT_STRONG };
        let day_color = if day.day == today { TEXT_ACCENT } else { tokens::TEXT_MAIN };
        let cells = vec![
            cell(day_label(day.day), DAY_W, day_color),
            cell(what, WHAT_W, what_color),
            cell(status.to_string(), STATUS_W, color),
        ];
        spawn_in(commands, card, table_row(cells, day.day == today));
    }
}

/// Days a week and focus.
fn choices_card(commands: &mut Commands, parent: Entity, store: &PlanStore) {
    let card = spawn_in(commands, parent, surface());
    spawn_in(commands, card, overline("Your week"));
    spawn_in(commands, card, label("Days you train, the benchmark day included"));
    let days = spawn_in(commands, card, bsn! { @row() Node { column_gap: px(SPACE_XS) } });
    for n in DAYS_RANGE.0..=DAYS_RANGE.1 {
        let kind = if n == store.days { Btn::Selected } else { Btn::Secondary };
        spawn_in(
            commands,
            days,
            bsn! {
                @compact_button(format!("{n}"), kind)
                Node { min_width: px(40) }
                DaysButton(n)
                on(pick_days)
            },
        );
    }
    spawn_in(commands, card, bsn! { @label("Focus") Node { margin: UiRect::top(px(SPACE_XS)) } });
    let focus = spawn_in(commands, card, bsn! { @wrap_row() Node { column_gap: px(SPACE_XS), row_gap: px(SPACE_XS) } });
    for option in Focus::ALL {
        let kind = if option == store.focus { Btn::Selected } else { Btn::Secondary };
        spawn_in(commands, focus, bsn! { @compact_button(option.name(), kind) FocusButton(option) on(pick_focus) });
    }
    let hint = match store.focus {
        Focus::Valorant => {
            "60% of each session on clicking (still and moving targets), 40% on tracking and switching so they don't fade."
        },
        Focus::Tracking => "60% of each session on tracking (smooth and reactive), 40% on clicking and switching.",
        Focus::Balanced => "Every kind of aim gets about the same share.",
    };
    spawn_in(commands, card, small(hint));
}

/// What changed since last week's plan, and why.
fn changes_card(commands: &mut Commands, parent: Entity, previous: Option<&WeekPlan>, plan: &WeekPlan) {
    let card = spawn_in(commands, parent, surface());
    spawn_in(commands, card, overline("Since last week's plan"));
    match previous {
        Some(previous) => {
            for line in changes(previous, plan) {
                spawn_in(commands, card, label_dim(line));
            }
        },
        None => {
            spawn_in(commands, card, label_dim("Your first plan. From next week this says what changed and why."));
        },
    }
}

fn start_session(
    _: On<Activate>,
    store: Res<PlanStore>,
    settings: Res<Settings>,
    mut content: ResMut<CustomContent>,
    mut commands: Commands,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(plan) = store.current.as_ref() else { return };
    if let Some(routine) = session_routine(plan, today(), &settings, &mut content) {
        start_routine(&mut commands, &mut next, routine);
    }
}

fn pick_days(activate: On<Activate>, buttons: Query<&DaysButton>, mut store: ResMut<PlanStore>) {
    if let Ok(&DaysButton(days)) = buttons.get(activate.event_target())
        && store.days != days
    {
        store.days = days;
    }
}

fn pick_focus(activate: On<Activate>, buttons: Query<&FocusButton>, mut store: ResMut<PlanStore>) {
    if let Ok(&FocusButton(focus)) = buttons.get(activate.event_target())
        && store.focus != focus
    {
        store.focus = focus;
    }
}
