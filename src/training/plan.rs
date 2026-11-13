//! The weekly plan: which day is a training, benchmark or rest day, and the blocks of each
//! training session with the reason each is there. [`generate`] is a pure function of
//! [`PlanInputs`]; [`PlanStore`] keeps this week's plan, its progress and last week's plan in
//! `aim_trainer_plan.ron`. The rules are in the [module docs of `training`](super).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::benchmark::{self, BenchmarkLog};
use super::{AimKind, day_label, local_day, monday, weekday, weekday_name};
use crate::analysis::{Cell, SECTORS, TRAINING_SECS, band_label};
use crate::calm_log::CalmLog;
use crate::custom::CustomContent;
use crate::drill::{DrillConfig, DrillKind, DrillSpec, VARIANT_SEPARATOR};
use crate::history::History;
use crate::metrics::calm::Part;
use crate::metrics::quest::QuestMode;
use crate::persist::Persisted;
use crate::routine::{Routine, RoutineOrigin, RoutineStep};
use crate::settings::Settings;

pub(super) const PLAN_PATH: &str = "aim_trainer_plan.ron";
/// Blocks in a training session.
pub const SESSION_BLOCKS: usize = 10;
/// Seconds each block lasts: 15 minutes of play a session, about 18 with breaks.
pub const BLOCK_SECS: f32 = 90.0;
/// Blocks of a session that go to the focus's kinds: 60%.
pub const FOCUS_BLOCKS: usize = 6;
/// A drill not played for this many days or more is due again.
pub const STALE_DAYS: i64 = 7;
/// Training days a week the player can choose, the benchmark day included: at least one session
/// besides the benchmark, at least one rest day.
pub const DAYS_RANGE: (u8, u8) = (2, 6);
const DEFAULT_DAYS: u8 = 4;

/// What a player trains for. Persisted in the plan file: never rename.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Focus {
    /// Valorant, CS2: first-shot clicking.
    #[default]
    Valorant,
    /// Games with long time-to-kill: Apex, Overwatch.
    Tracking,
    /// Every kind alike.
    Balanced,
}

impl Focus {
    pub const ALL: [Focus; 3] = [Focus::Valorant, Focus::Tracking, Focus::Balanced];

    pub fn name(self) -> &'static str {
        match self {
            Focus::Valorant => "Valorant / CS2",
            Focus::Tracking => "Tracking games",
            Focus::Balanced => "All-round",
        }
    }

    /// The kinds that get [`FOCUS_BLOCKS`] of a session; none for an even spread.
    pub fn kinds(self) -> &'static [AimKind] {
        match self {
            Focus::Valorant => &[AimKind::StaticClicking, AimKind::DynamicClicking],
            Focus::Tracking => &[AimKind::SmoothTracking, AimKind::ReactiveTracking],
            Focus::Balanced => &[],
        }
    }
}

/// The Insights weak spot a plan trains: a flick direction, distance band or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeakSpot {
    pub sector: Option<usize>,
    pub band: Option<usize>,
}

impl WeakSpot {
    pub fn cell(self) -> Cell {
        Cell { sector: self.sector, band: self.band }
    }

    /// The flicks it is about, e.g. `down-left flicks of 15-25°`.
    pub fn flicks(self) -> String {
        let direction = self.sector.map(|s| format!("{} ", SECTORS[s].to_lowercase())).unwrap_or_default();
        let band = self.band.map(|b| format!(" of {}", band_label(b))).unwrap_or_default();
        format!("{direction}flicks{band}")
    }
}

/// The kind that improved least since the first benchmark.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Weakest {
    pub aim: AimKind,
    /// Its index and the overall one in the latest benchmark.
    pub index: f32,
    pub overall: f32,
}

/// The weakest part of the latest Calm aim result and the drill that trains it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalmFocus {
    pub part: String,
    pub score: f32,
    pub drill: DrillKind,
}

/// What a week's plan was built from, so the page can say why and what changed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Basis {
    /// The latest benchmark's id, if there was one.
    pub benchmark: Option<u64>,
    pub benchmarks: usize,
    pub weakest: Option<Weakest>,
    pub weak_spot: Option<WeakSpot>,
    pub calm: Option<CalmFocus>,
}

/// What a block plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockDrill {
    Drill(DrillKind),
    /// The drill in adaptive Train mode.
    Adaptive(DrillKind),
    /// Constrained adaptive Microflick on a weak spot, as the Insights page trains it.
    WeakSpot(WeakSpot),
}

impl BlockDrill {
    /// The drill as the session plays it (and the history names it).
    pub fn name(self) -> String {
        match self {
            BlockDrill::Drill(kind) => kind.name().to_string(),
            BlockDrill::Adaptive(kind) => format!("{}{VARIANT_SEPARATOR}adaptive", kind.name()),
            BlockDrill::WeakSpot(spot) => spot.cell().scenario_name(),
        }
    }

    fn kind(self) -> DrillKind {
        match self {
            BlockDrill::Drill(kind) | BlockDrill::Adaptive(kind) => kind,
            BlockDrill::WeakSpot(_) => DrillKind::Microflick,
        }
    }
}

/// Why a block is in a session: the most specific reason that applies.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Why {
    /// The focus weighting.
    Focus,
    /// The kind that improved least since the first benchmark.
    Weakest,
    /// The Insights weak spot.
    WeakSpot,
    /// The weakest part of the latest Calm aim result.
    Calm,
    /// A drill the player has never played: blocked, not mixed.
    New,
    /// Not played since this local day.
    Stale(i64),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub drill: BlockDrill,
    pub aim: AimKind,
    pub why: Why,
}

impl Block {
    fn of(drill: DrillKind, aim: AimKind, why: Why) -> Self {
        let drill = if drill.def().adaptive { BlockDrill::Adaptive(drill) } else { BlockDrill::Drill(drill) };
        Self { drill, aim, why }
    }

    /// The reason in one line.
    pub fn reason(&self, plan: &WeekPlan) -> String {
        let basis = &plan.basis;
        match self.why {
            Why::Focus if plan.focus.kinds().contains(&self.aim) => {
                format!("{}: your {} focus, 60% of the blocks", self.aim.name(), plan.focus.name())
            },
            Why::Focus if plan.focus.kinds().is_empty() => format!("{}: every kind gets its share", self.aim.name()),
            Why::Focus => format!("{}: kept up so it doesn't fade, 40% of the blocks", self.aim.name()),
            Why::Weakest => match basis.weakest {
                Some(w) => format!(
                    "{}: least improved since your first benchmark ({:.0} vs {:.0} overall)",
                    self.aim.name(),
                    w.index,
                    w.overall
                ),
                None => self.aim.name().to_string(),
            },
            Why::WeakSpot => match basis.weak_spot {
                Some(spot) => format!("Insights: your {} are your weakest. Adaptive, only those flicks", spot.flicks()),
                None => "Insights weak spot".to_string(),
            },
            Why::Calm => match &basis.calm {
                Some(calm) => format!("Calm aim: {} was your weakest part ({:.0} / 100)", calm.part, calm.score),
                None => "Calm aim".to_string(),
            },
            Why::New => "New to you: played in a block to learn it before it is mixed in".to_string(),
            Why::Stale(since) => format!("{}: not played since {}", self.aim.name(), day_label(since)),
        }
    }

    /// What this block plays, with the player's settings for drills that use them. A weak-spot
    /// block uses (and keeps up to date) the same custom scenario as the Insights page's Train
    /// button, so its history continues there.
    fn spec(&self, settings: &Settings, content: &mut CustomContent) -> Option<DrillSpec> {
        Some(match self.drill {
            BlockDrill::Drill(kind) => DrillSpec::builtin(kind, settings),
            BlockDrill::Adaptive(kind) => DrillSpec::adaptive(kind, QuestMode::Train, settings),
            BlockDrill::WeakSpot(spot) => {
                let cell = spot.cell();
                let mut config = cell.training_config(DrillConfig::builtin(DrillKind::Microflick, settings));
                config.adaptive = Some(QuestMode::Train);
                let name = format!("{}{VARIANT_SEPARATOR}adaptive", cell.scenario_name());
                let id = content.upsert_scenario(&name, TRAINING_SECS, config);
                let scenario = content.scenario(id)?;
                DrillSpec { name: scenario.name.clone(), config: scenario.config.clone() }
            },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DayKind {
    Rest,
    Benchmark,
    Train,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanDay {
    /// Local day.
    pub day: i64,
    pub kind: DayKind,
    /// The session's blocks in the order they are played; empty unless a training day.
    pub blocks: Vec<Block>,
    /// A training session played (see [`finish_session`]).
    pub done: bool,
}

/// One week's plan, Monday to Sunday.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeekPlan {
    /// Local day of its Monday.
    pub week: i64,
    /// The day the week's first plan was made: days before it were never planned.
    pub made: i64,
    pub days: u8,
    pub focus: Focus,
    pub basis: Basis,
    /// Seven days, Monday first.
    pub schedule: Vec<PlanDay>,
}

impl WeekPlan {
    pub fn day(&self, day: i64) -> Option<&PlanDay> {
        self.schedule.iter().find(|d| d.day == day)
    }

    /// The benchmark day; after a mid-week change of plan, the later one (the earlier stays as it
    /// was, in the past).
    pub fn benchmark_day(&self) -> Option<i64> {
        self.schedule.iter().rev().find(|d| d.kind == DayKind::Benchmark).map(|d| d.day)
    }

    /// Training days, in order.
    pub fn sessions(&self) -> impl Iterator<Item = &PlanDay> {
        self.schedule.iter().filter(|d| d.kind == DayKind::Train)
    }
}

/// Everything [`generate`] builds a plan from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanInputs {
    /// Local day.
    pub today: i64,
    /// Training days a week, the benchmark day included.
    pub days: u8,
    pub focus: Focus,
    /// The local day each drill was last played (any practice run of it), for drills played.
    pub last_played: Vec<(DrillKind, i64)>,
    /// The latest benchmark: its id and local day.
    pub benchmark: Option<(u64, i64)>,
    pub benchmarks: usize,
    pub weakest: Option<Weakest>,
    pub weak_spot: Option<WeakSpot>,
    /// The latest Calm aim result's weakest part and its score.
    pub calm: Option<(Part, f32)>,
    /// The weekday (0 = Monday) of the benchmark in the last plan, if there was one.
    pub last_benchmark_weekday: Option<i64>,
}

/// The drill that trains a part of Calm aim.
pub fn calm_drill(part: Part) -> DrillKind {
    match part {
        Part::Smoothness => DrillKind::SmoothTracking,
        Part::Stillness => DrillKind::Peek,
        Part::Flick => DrillKind::Microflick,
        Part::Settle => DrillKind::WallFlicks,
    }
}

/// Blocks per session for each kind, in [`AimKind::ALL`] order: [`FOCUS_BLOCKS`] spread over the
/// focus's kinds and the rest over the others (all evenly without a focus; a remainder goes to
/// the earlier kinds), then one more for the least improved kind, taken from the kind with the
/// most (the earliest of equals).
pub fn block_counts(focus: Focus, weakest: Option<AimKind>) -> [usize; 6] {
    let spread = |total: usize, kinds: &[AimKind], counts: &mut [usize; 6]| {
        for (i, aim) in kinds.iter().enumerate() {
            let index = AimKind::ALL.iter().position(|a| a == aim).expect("a kind");
            counts[index] = total / kinds.len() + usize::from(i < total % kinds.len());
        }
    };
    let mut counts = [0; 6];
    let matching = focus.kinds();
    if matching.is_empty() {
        spread(SESSION_BLOCKS, &AimKind::ALL, &mut counts);
    } else {
        let others: Vec<AimKind> = AimKind::ALL.into_iter().filter(|aim| !matching.contains(aim)).collect();
        spread(FOCUS_BLOCKS, matching, &mut counts);
        spread(SESSION_BLOCKS - FOCUS_BLOCKS, &others, &mut counts);
    }
    if let Some(weakest) = weakest {
        let weak = AimKind::ALL.iter().position(|a| *a == weakest).expect("a kind");
        let donor = (0..6).filter(|&i| i != weak).max_by_key(|&i| (counts[i], std::cmp::Reverse(i)));
        if let Some(donor) = donor.filter(|&i| counts[i] > 0) {
            counts[donor] -= 1;
            counts[weak] += 1;
        }
    }
    counts
}

/// The week's plan (see the module docs of `training` for the rules).
pub fn generate(inputs: &PlanInputs) -> WeekPlan {
    let week = monday(inputs.today);
    let days = inputs.days.clamp(DAYS_RANGE.0, DAYS_RANGE.1);
    let bench = inputs
        .benchmark
        .map(|(_, day)| weekday(day))
        .or(inputs.last_benchmark_weekday)
        .unwrap_or_else(|| weekday(inputs.today));
    let sessions = i64::from(days) - 1;
    let offsets: Vec<i64> = (1..=sessions).map(|k| k * 7 / (sessions + 1)).collect();
    let basis = Basis {
        benchmark: inputs.benchmark.map(|(id, _)| id),
        benchmarks: inputs.benchmarks,
        weakest: inputs.weakest,
        weak_spot: inputs.weak_spot,
        calm: inputs.calm.map(|(part, score)| CalmFocus {
            part: part.title().to_string(),
            score,
            drill: calm_drill(part),
        }),
    };
    let counts = block_counts(inputs.focus, basis.weakest.map(|w| w.aim));
    let mut played: HashMap<DrillKind, i64> = HashMap::new();
    for &(kind, day) in &inputs.last_played {
        let latest = played.entry(kind).or_insert(day);
        *latest = (*latest).max(day);
    }
    let schedule = (0..7)
        .map(|offset| {
            let day = week + offset;
            let from_benchmark = (offset - bench).rem_euclid(7);
            let kind = if from_benchmark == 0 {
                DayKind::Benchmark
            } else if offsets.contains(&from_benchmark) {
                DayKind::Train
            } else {
                DayKind::Rest
            };
            let blocks = if kind == DayKind::Train { session(&counts, &basis, &mut played, day) } else { Vec::new() };
            PlanDay { day, kind, blocks, done: false }
        })
        .collect();
    WeekPlan { week, made: inputs.today, days, focus: inputs.focus, basis, schedule }
}

/// One session's blocks for `day`; notes its drills as played that day, so the next session
/// picks others where it can.
fn session(counts: &[usize; 6], basis: &Basis, played: &mut HashMap<DrillKind, i64>, day: i64) -> Vec<Block> {
    let mut units: Vec<Vec<Block>> = Vec::new();
    for (aim, &count) in AimKind::ALL.iter().copied().zip(counts) {
        let mut left = count;
        let mut used: Vec<DrillKind> = Vec::new();
        if let Some(spot) = basis.weak_spot.filter(|_| aim == AimKind::StaticClicking && left > 0) {
            units.push(vec![Block { drill: BlockDrill::WeakSpot(spot), aim, why: Why::WeakSpot }]);
            left -= 1;
        }
        if let Some(calm) = basis.calm.as_ref().filter(|calm| AimKind::of(calm.drill) == Some(aim) && left > 0) {
            units.push(vec![Block::of(calm.drill, aim, Why::Calm)]);
            used.push(calm.drill);
            left -= 1;
        }
        while left > 0 {
            let pool = aim.drills();
            let fresh: Vec<DrillKind> = pool.iter().copied().filter(|d| !used.contains(d)).collect();
            let candidates = if fresh.is_empty() { pool.to_vec() } else { fresh };
            // Never played first, then the longest ago; the pool's order breaks ties.
            let pick = candidates
                .iter()
                .copied()
                .min_by_key(|d| (played.get(d).copied().unwrap_or(i64::MIN), pool.iter().position(|p| p == d)))
                .expect("every kind has drills");
            used.push(pick);
            let (blocks, why) = match played.get(&pick) {
                None => (left.min(2), Why::New),
                Some(_) if basis.weakest.is_some_and(|w| w.aim == aim) => (1, Why::Weakest),
                Some(&last) if day - last >= STALE_DAYS => (1, Why::Stale(last)),
                Some(_) => (1, Why::Focus),
            };
            units.push(vec![Block::of(pick, aim, why); blocks]);
            left -= blocks;
        }
    }
    let blocks = interleave(units);
    for block in &blocks {
        played.insert(block.drill.kind(), day);
    }
    blocks
}

/// Orders the units (one block, or a new drill's blocks in a row) so that consecutive units are
/// of different kinds where possible: each time the kind with the most blocks left that isn't
/// the previous one, its earliest unit first.
fn interleave(mut units: Vec<Vec<Block>>) -> Vec<Block> {
    let mut order = Vec::new();
    let mut last: Option<AimKind> = None;
    while !units.is_empty() {
        let left =
            |aim: AimKind, units: &[Vec<Block>]| units.iter().filter(|u| u[0].aim == aim).map(Vec::len).sum::<usize>();
        let pick = (0..units.len())
            .filter(|&i| Some(units[i][0].aim) != last)
            .max_by_key(|&i| (left(units[i][0].aim, &units), std::cmp::Reverse(i)))
            .unwrap_or(0);
        let unit = units.remove(pick);
        last = Some(unit[0].aim);
        order.extend(unit);
    }
    order
}

/// What changed from last week's plan to this one, and why, one line each.
pub fn changes(previous: &WeekPlan, current: &WeekPlan) -> Vec<String> {
    let mut lines = Vec::new();
    let (before, now) = (&previous.basis, &current.basis);
    if previous.days != current.days {
        lines.push(format!("Training days: {} → {} a week, as you chose.", previous.days, current.days));
    }
    if previous.focus != current.focus {
        lines.push(format!("Focus: {} → {}, as you chose.", previous.focus.name(), current.focus.name()));
    }
    if now.benchmarks > before.benchmarks
        && let Some(id) = now.benchmark
    {
        let day = local_day(id, super::utc_offset_secs());
        lines.push(format!("Built on your benchmark of {}.", day_label(day)));
    }
    let counts_before = block_counts(previous.focus, before.weakest.map(|w| w.aim));
    let counts_now = block_counts(current.focus, now.weakest.map(|w| w.aim));
    for (i, aim) in AimKind::ALL.into_iter().enumerate() {
        if counts_before[i] == counts_now[i] {
            continue;
        }
        let why = if now.weakest.is_some_and(|w| w.aim == aim) {
            "least improved since your first benchmark"
        } else if before.weakest.is_some_and(|w| w.aim == aim) {
            "no longer your least improved kind"
        } else if previous.focus != current.focus {
            "the new focus"
        } else {
            "a block went to your least improved kind"
        };
        let blocks = if counts_now[i] == 1 { "block" } else { "blocks" };
        lines.push(format!("{}: {} → {} {blocks} a session: {why}.", aim.name(), counts_before[i], counts_now[i]));
    }
    match (before.weak_spot, now.weak_spot) {
        (None, Some(spot)) => lines.push(format!("New weak-spot block from Insights: {}.", spot.flicks())),
        (Some(_), None) => lines.push("No weak-spot block: Insights finds no clear weak spot now.".to_string()),
        (Some(a), Some(b)) if a != b => {
            lines.push(format!("The weak-spot block moved to {}, Insights' weakest now.", b.flicks()));
        },
        _ => {},
    }
    match (&before.calm, &now.calm) {
        (None, Some(calm)) => lines.push(format!("New Calm aim block: {} trains {}.", calm.drill.name(), calm.part)),
        (Some(a), Some(b)) if a.part != b.part => {
            lines.push(format!("Calm aim block: now {} for {}, your weakest part.", b.drill.name(), b.part));
        },
        _ => {},
    }
    if lines.is_empty() {
        lines.push("Same plan as last week: nothing new to adapt to.".to_string());
    }
    lines
}

/// The plan's file: the player's choices, this week's plan and its progress, last week's plan.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanStore {
    /// Training days a week, the benchmark day included.
    pub days: u8,
    pub focus: Focus,
    pub current: Option<WeekPlan>,
    pub previous: Option<WeekPlan>,
}

impl Default for PlanStore {
    fn default() -> Self {
        Self { days: DEFAULT_DAYS, focus: Focus::default(), current: None, previous: None }
    }
}

impl Persisted for PlanStore {
    const PATH: &'static str = PLAN_PATH;
}

impl PlanStore {
    /// Keeps a hand-edited file's choices in range.
    pub(super) fn repair(&mut self) {
        self.days = self.days.clamp(DAYS_RANGE.0, DAYS_RANGE.1);
    }

    /// Whether this week's plan has to be made (again): there is none for this week, the
    /// choices changed, or there is a newer benchmark than it was built on.
    pub fn stale(&self, today: i64, latest_benchmark: Option<u64>) -> bool {
        self.current.as_ref().is_none_or(|plan| {
            plan.week != monday(today)
                || plan.days != self.days
                || plan.focus != self.focus
                || plan.basis.benchmark != latest_benchmark
        })
    }

    /// Takes a freshly generated plan. Within the same week, days already past and sessions
    /// already done keep what they were; a new week moves this week's plan to `previous`.
    pub fn adopt(&mut self, plan: WeekPlan, today: i64) {
        match self.current.take() {
            Some(old) if old.week == plan.week => {
                let mut plan = plan;
                plan.made = old.made;
                for (slot, old_day) in plan.schedule.iter_mut().zip(old.schedule) {
                    if old_day.day < today || old_day.done {
                        *slot = old_day;
                    }
                }
                self.current = Some(plan);
            },
            old => {
                if old.is_some() {
                    self.previous = old;
                }
                self.current = Some(plan);
            },
        }
    }

    /// Marks the training session of `day` done.
    pub fn mark_done(&mut self, day: i64) {
        let session = self
            .current
            .as_mut()
            .and_then(|plan| plan.schedule.iter_mut().find(|d| d.day == day && d.kind == DayKind::Train && !d.done));
        if let Some(session) = session {
            session.done = true;
        }
    }

    /// The inputs for this week's plan from what the trainer knows. `weak_spot` is the Insights
    /// analysis' weakest finding (the caller runs it: it decodes replays).
    pub fn inputs(
        &self,
        history: &History,
        log: &BenchmarkLog,
        calm: &CalmLog,
        weak_spot: Option<WeakSpot>,
        today: i64,
        offset: i64,
    ) -> PlanInputs {
        let mut last_played: Vec<(DrillKind, i64)> = Vec::new();
        for name in history.drills() {
            if let Some(entry) = history.practice_runs(&name).next_back() {
                last_played.push((entry.kind, local_day(entry.timestamp, offset)));
            }
        }
        let calm_aim = DrillKind::CalmAim.name();
        let latest_calm = history.practice_runs(calm_aim).next_back().and_then(|run| calm.entry(run.timestamp));
        let last_plan = self.current.as_ref().or(self.previous.as_ref());
        PlanInputs {
            today,
            days: self.days,
            focus: self.focus,
            last_played,
            benchmark: log.latest().map(|b| (b.timestamp, local_day(b.timestamp, offset))),
            benchmarks: log.entries().len(),
            weakest: benchmark::least_improved(log.entries()).map(|(aim, index, overall)| Weakest {
                aim,
                index,
                overall,
            }),
            weak_spot,
            calm: latest_calm.map(|entry| {
                let part = entry.result.weakest();
                (part, entry.result.score(part))
            }),
            last_benchmark_weekday: last_plan.and_then(WeekPlan::benchmark_day).map(weekday),
        }
    }
}

/// The routine of the training session planned for `day`.
pub fn session_routine(plan: &WeekPlan, day: i64, settings: &Settings, content: &mut CustomContent) -> Option<Routine> {
    let session = plan.sessions().find(|d| d.day == day)?;
    let steps: Vec<RoutineStep> = session
        .blocks
        .iter()
        .filter_map(|block| Some(RoutineStep { drill: block.spec(settings, content)?, secs: BLOCK_SECS }))
        .collect();
    let number = plan.sessions().position(|d| d.day == day)? + 1;
    let blurb = format!(
        "Session {number} of {} this week: {} blocks of {BLOCK_SECS:.0} s, kinds mixed. Breaks as long as you like.",
        plan.sessions().count(),
        steps.len()
    );
    let name = format!("Plan · {}", weekday_name(day));
    Some(Routine::new(name, blurb, steps).with_origin(RoutineOrigin::Plan(day)))
}

/// A plan session counts as done once its routine reaches the end with at least half its blocks
/// played (the rest skipped).
pub fn session_done(routine: &Routine) -> bool {
    matches!(routine.origin, RoutineOrigin::Plan(_))
        && routine.finished()
        && routine.results.len() * 2 >= routine.steps.len()
}

/// Marks the plan's session done when its routine ends (its summary screen comes up).
pub(crate) fn finish_session(routine: Option<Res<Routine>>, mut store: ResMut<PlanStore>) {
    let Some(routine) = routine else { return };
    if let RoutineOrigin::Plan(day) = routine.origin
        && session_done(&routine)
    {
        store.mark_done(day);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Comparison;
    use crate::routine::StepResult;

    /// Friday 13 November 2026.
    const FRIDAY: i64 = 20_770;

    fn fresh(days: u8, focus: Focus) -> PlanInputs {
        PlanInputs { today: FRIDAY, days, focus, ..PlanInputs::default() }
    }

    /// Every drill of every kind played `ago` days before `today`.
    fn played_all(today: i64, ago: i64) -> Vec<(DrillKind, i64)> {
        AimKind::ALL.iter().flat_map(|aim| aim.drills()).map(|&d| (d, today - ago)).collect()
    }

    /// Inputs with everything the trainer can know.
    fn rich(weakest: Option<AimKind>) -> PlanInputs {
        PlanInputs {
            last_played: played_all(FRIDAY, 2),
            benchmark: Some((1_794_400_000, FRIDAY - 2)),
            benchmarks: 3,
            weakest: weakest.map(|aim| Weakest { aim, index: 97.0, overall: 104.0 }),
            weak_spot: Some(WeakSpot { sector: Some(5), band: Some(2) }),
            calm: Some((Part::Stillness, 61.0)),
            ..fresh(4, Focus::Valorant)
        }
    }

    fn check_week(plan: &WeekPlan, days: u8) {
        assert_eq!(plan.schedule.len(), 7);
        assert_eq!(plan.week, monday(FRIDAY));
        for (offset, day) in plan.schedule.iter().enumerate() {
            assert_eq!(day.day, plan.week + offset as i64);
            // A day is one thing: a benchmark day holds no session.
            match day.kind {
                DayKind::Train => assert_eq!(day.blocks.len(), SESSION_BLOCKS, "{day:?}"),
                DayKind::Benchmark | DayKind::Rest => assert!(day.blocks.is_empty(), "{day:?}"),
            }
        }
        let count = |kind| plan.schedule.iter().filter(|d| d.kind == kind).count();
        assert_eq!(count(DayKind::Benchmark), 1);
        assert_eq!(count(DayKind::Train), usize::from(days) - 1);
        assert!(count(DayKind::Rest) >= 1);
        // The day before the benchmark is a rest day.
        let bench = plan.benchmark_day().unwrap();
        let before = plan.week + (bench - 1 - plan.week).rem_euclid(7);
        assert_eq!(plan.day(before).unwrap().kind, DayKind::Rest);
        // Kinds alternate, except a new drill's blocks in a row.
        for session in plan.sessions() {
            for pair in session.blocks.windows(2) {
                if pair[0].aim == pair[1].aim {
                    assert!(pair[0].drill == pair[1].drill && pair[0].why == Why::New, "{:?}", session.blocks);
                }
            }
        }
    }

    #[test]
    fn same_inputs_same_plan() {
        for inputs in [fresh(4, Focus::Valorant), rich(Some(AimKind::SmoothTracking))] {
            assert_eq!(generate(&inputs), generate(&inputs.clone()));
        }
    }

    #[test]
    fn respects_the_days_a_week_and_never_doubles_a_day() {
        for days in DAYS_RANGE.0..=DAYS_RANGE.1 {
            for focus in Focus::ALL {
                check_week(&generate(&fresh(days, focus)), days);
                check_week(&generate(&PlanInputs { days, focus, ..rich(Some(AimKind::DynamicClicking)) }), days);
            }
        }
        // Out-of-range choices are clamped.
        check_week(&generate(&fresh(0, Focus::Valorant)), DAYS_RANGE.0);
        check_week(&generate(&fresh(7, Focus::Valorant)), DAYS_RANGE.1);
    }

    #[test]
    fn benchmark_keeps_its_weekday() {
        // Fresh: today. With a benchmark: its weekday. Else last plan's.
        assert_eq!(generate(&fresh(4, Focus::Valorant)).benchmark_day(), Some(FRIDAY));
        let tuesday = monday(FRIDAY) - 6;
        let inputs = PlanInputs { benchmark: Some((1, tuesday)), ..fresh(4, Focus::Valorant) };
        assert_eq!(generate(&inputs).benchmark_day(), Some(monday(FRIDAY) + 1));
        let inputs = PlanInputs { last_benchmark_weekday: Some(6), ..fresh(4, Focus::Valorant) };
        assert_eq!(generate(&inputs).benchmark_day(), Some(monday(FRIDAY) + 6));
    }

    /// Tolerance: the focus's kinds get 60% of every session's blocks, give or take one block
    /// (10 points), whatever else the plan adapts to.
    #[test]
    fn focus_weighting_holds_within_one_block() {
        for focus in [Focus::Valorant, Focus::Tracking] {
            let weakest_options = std::iter::once(None).chain(AimKind::ALL.into_iter().map(Some));
            for weakest in weakest_options {
                for inputs in [PlanInputs { focus, ..rich(weakest) }, PlanInputs { focus, ..fresh(5, focus) }] {
                    let plan = generate(&inputs);
                    for session in plan.sessions() {
                        let share = session.blocks.iter().filter(|b| focus.kinds().contains(&b.aim)).count();
                        assert!(
                            (FOCUS_BLOCKS - 1..=FOCUS_BLOCKS + 1).contains(&share),
                            "{focus:?} {weakest:?}: {share}"
                        );
                        if weakest.is_none() {
                            assert_eq!(share, FOCUS_BLOCKS);
                        }
                    }
                }
            }
        }
        // All-round: every kind in every session.
        for session in generate(&fresh(4, Focus::Balanced)).sessions() {
            assert!(AimKind::ALL.iter().all(|aim| session.blocks.iter().any(|b| b.aim == *aim)));
        }
    }

    #[test]
    fn least_improved_kind_gets_a_block_more() {
        let base = block_counts(Focus::Valorant, None);
        for (i, aim) in AimKind::ALL.into_iter().enumerate() {
            let counts = block_counts(Focus::Valorant, Some(aim));
            assert_eq!(counts[i], base[i] + 1, "{aim:?}");
            assert_eq!(counts.iter().sum::<usize>(), SESSION_BLOCKS);
        }
    }

    #[test]
    fn weak_spot_and_calm_part_are_trained() {
        let inputs = rich(None);
        for session in generate(&inputs).sessions() {
            let spots: Vec<&Block> =
                session.blocks.iter().filter(|b| matches!(b.drill, BlockDrill::WeakSpot(_))).collect();
            assert_eq!(spots.len(), 1);
            assert_eq!((spots[0].aim, spots[0].why), (AimKind::StaticClicking, Why::WeakSpot));
            assert!(session.blocks.iter().any(|b| b.drill == BlockDrill::Drill(DrillKind::Peek) && b.why == Why::Calm));
        }
        let without = PlanInputs { weak_spot: None, calm: None, ..rich(None) };
        for session in generate(&without).sessions() {
            assert!(session.blocks.iter().all(|b| !matches!(b.drill, BlockDrill::WeakSpot(_)) && b.why != Why::Calm));
        }
    }

    #[test]
    fn default_week_without_any_data() {
        let plan = generate(&fresh(4, Focus::Valorant));
        check_week(&plan, 4);
        assert_eq!(plan.basis, Basis::default());
        let first = plan.sessions().next().unwrap();
        // Everything is new: drills come in pairs, adaptive where the drill offers it.
        assert!(first.blocks.iter().all(|b| b.why == Why::New));
        assert!(first.blocks.contains(&Block::of(DrillKind::HeadLevel, AimKind::StaticClicking, Why::New)));
        assert_eq!(
            Block::of(DrillKind::HeadLevel, AimKind::StaticClicking, Why::New).drill,
            BlockDrill::Adaptive(DrillKind::HeadLevel)
        );
        // Later sessions rotate to drills the earlier ones didn't play.
        let second = plan.sessions().nth(1).unwrap();
        let statics = |day: &PlanDay| -> Vec<DrillKind> {
            day.blocks.iter().filter(|b| b.aim == AimKind::StaticClicking).map(|b| b.drill.kind()).collect()
        };
        assert!(
            statics(second).iter().all(|d| !statics(first).contains(d)),
            "{:?} {:?}",
            statics(first),
            statics(second)
        );
    }

    #[test]
    fn stale_drills_say_since_when() {
        let inputs = PlanInputs { last_played: played_all(FRIDAY, 30), ..fresh(2, Focus::Valorant) };
        let plan = generate(&inputs);
        let session = plan.sessions().next().unwrap();
        assert!(session.blocks.iter().all(|b| b.why == Why::Stale(FRIDAY - 30)), "{:?}", session.blocks);
    }

    #[test]
    fn regenerating_keeps_the_past_and_missed_days_do_not_pile_up() {
        let mut store = PlanStore { days: 5, ..PlanStore::default() };
        let monday = monday(FRIDAY);
        let plan = generate(&PlanInputs { today: monday, ..fresh(5, Focus::Valorant) });
        store.adopt(plan, monday);
        let sessions: Vec<i64> = store.current.as_ref().unwrap().sessions().map(|d| d.day).collect();
        store.mark_done(sessions[0]);
        store.mark_done(sessions[0] + 100);
        // Later in the week, with sessions missed, a new benchmark arrives and the plan is made
        // again: the done session and the past days stay, nothing is moved onto later days.
        let today = monday + 5;
        let inputs = PlanInputs { today, benchmark: Some((9, monday + 5)), benchmarks: 1, ..fresh(5, Focus::Valorant) };
        assert!(store.stale(today, Some(9)));
        store.adopt(generate(&inputs), today);
        let plan = store.current.as_ref().unwrap();
        assert!(!store.stale(today, Some(9)));
        assert_eq!(plan.made, monday);
        assert!(plan.day(sessions[0]).unwrap().done);
        for day in &plan.schedule {
            assert!(day.blocks.len() == SESSION_BLOCKS || day.blocks.is_empty());
        }
        // The benchmark moved to today; the missed one on Monday stays in the past as it was.
        assert_eq!(plan.benchmark_day(), Some(today));
        let benchmarks: Vec<i64> =
            plan.schedule.iter().filter(|d| d.kind == DayKind::Benchmark).map(|d| d.day).collect();
        assert_eq!(benchmarks, [monday, today]);
        assert_eq!(plan.day(today + 1).unwrap().kind, DayKind::Train);
        assert!(store.previous.is_none());
        // Next week: a fresh plan, this one kept as last week's.
        let next = monday + 7;
        assert!(store.stale(next, Some(9)));
        store.adopt(generate(&PlanInputs { today: next, ..inputs }), next);
        assert_eq!(store.previous.as_ref().map(|p| p.week), Some(monday));
        assert!(store.current.as_ref().unwrap().schedule.iter().all(|d| !d.done));
        // Changing a choice makes the plan again.
        store.days = 3;
        assert!(store.stale(next, Some(9)));
    }

    #[test]
    fn a_session_is_done_with_half_its_blocks_played() {
        let plan = generate(&fresh(4, Focus::Valorant));
        let day = plan.sessions().next().unwrap().day;
        let mut content = CustomContent::default();
        let mut routine = session_routine(&plan, day, &Settings::default(), &mut content).unwrap();
        assert_eq!(routine.steps.len(), SESSION_BLOCKS);
        assert_eq!(routine.origin, RoutineOrigin::Plan(day));
        assert!(session_routine(&plan, plan.benchmark_day().unwrap(), &Settings::default(), &mut content).is_none());
        let result = |routine: &Routine| StepResult {
            stats: test_stats(&routine.steps[routine.next].drill),
            previous: Comparison::default(),
        };
        for step in 0..SESSION_BLOCKS {
            if step < SESSION_BLOCKS / 2 - 1 {
                let result = result(&routine);
                routine.record(result.stats, result.previous);
            } else {
                routine.skip();
            }
        }
        assert!(routine.finished() && !session_done(&routine));
        let mut again = routine.restarted();
        for step in 0..SESSION_BLOCKS {
            if step < SESSION_BLOCKS / 2 {
                let result = result(&again);
                again.record(result.stats, result.previous);
            } else {
                again.skip();
            }
        }
        assert!(session_done(&again));
    }

    fn test_stats(drill: &DrillSpec) -> crate::drill::RunStats {
        crate::drill::RunStats {
            kind: drill.config.kind,
            drill: drill.name.clone(),
            sens: 0.4,
            cm_per_360: 40.0,
            duration: BLOCK_SECS,
            hits: 10,
            shots: 12,
            expired: 0,
            ttk_total: 5.0,
            best_ttk: 0.3,
            on_target: 0.0,
            landings: Vec::new(),
            clicks: Vec::new(),
            rating: None,
            rounds_hit: 0,
        }
    }

    #[test]
    fn plans_survive_the_file() {
        let mut store = PlanStore::default();
        store.adopt(generate(&rich(Some(AimKind::SpeedSwitching))), FRIDAY);
        let text = ron::ser::to_string_pretty(&store, ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(ron::from_str::<PlanStore>(&text).unwrap(), store);
        // A file from before any plan loads to the defaults.
        assert_eq!(ron::from_str::<PlanStore>("()").unwrap(), PlanStore::default());
    }
}
