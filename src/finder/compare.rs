//! The long comparison: two candidate sensitivities played on several days, a few minutes each
//! per day, interleaved, each block after an unscored adaptation stretch, and compared on
//! precision. It is the trainer's only way to say that one sensitivity is better than another
//! for a player; the blind 20 s trials of the quick finders mostly measure adaptation. Saved in
//! `aim_trainer_comparison.ron`, as it runs for days. The reasoning is in
//! [`limits`](super::limits).

use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::limits::sens_for;
use super::stats::{Summary, day_of, t_critical};
use crate::drill::{DrillConfig, DrillKind, DrillSpec, Run, RunPurpose, RunStats};
use crate::persist::{LoadReport, Persisted, load_ron};
use crate::settings::Settings;

/// Unscored seconds at the start of every block: getting used to the block's sensitivity.
pub const ADAPT_SECS: f32 = 45.0;
/// Scored seconds of a block.
pub const SCORED_SECS: f32 = 75.0;
/// Blocks per day: two per candidate, in ABBA order (BAAB every other day), so neither
/// candidate is always the one played warmer.
pub const DAY_BLOCKS: usize = 4;
/// Complete days before any verdict, and the day after which "no difference" is the verdict.
pub const MIN_DAYS: usize = 5;
pub const MAX_DAYS: usize = 10;
/// Measured first clicks a block needs to count.
const MIN_CLICKS: usize = 8;
/// The history name of the comparison's blocks (logged as finder trials, not practice).
const BLOCK_NAME: &str = "Long comparison";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    A,
    B,
}

impl Side {
    pub fn other(self) -> Self {
        match self {
            Side::A => Side::B,
            Side::B => Side::A,
        }
    }

    pub fn letter(self) -> &'static str {
        match self {
            Side::A => "A",
            Side::B => "B",
        }
    }
}

/// One scored block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    /// The block's history timestamp; its UTC day decides which day it belongs to.
    pub unix_time: u64,
    pub side: Side,
    /// Mean distance of the first clicks from the target centre, degrees.
    pub click_deg: f32,
    /// Hits per shot.
    pub accuracy: f32,
    /// First clicks measured.
    pub clicks: u32,
}

impl Block {
    /// `None` with fewer than [`MIN_CLICKS`] measured first clicks.
    pub fn from_stats(unix_time: u64, side: Side, stats: &RunStats) -> Option<Self> {
        let offsets: Vec<f32> =
            stats.clicks.iter().filter(|c| c.first).filter_map(|c| Some(c.task_offset()?.length())).collect();
        (offsets.len() >= MIN_CLICKS).then(|| Self {
            unix_time,
            side,
            click_deg: offsets.iter().sum::<f32>() / offsets.len() as f32,
            accuracy: stats.accuracy(),
            clicks: offsets.len() as u32,
        })
    }
}

/// A complete day: each candidate's average over its two blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Day {
    pub day: u64,
    /// (click error in degrees, accuracy) of A and of B.
    pub a: (f32, f32),
    pub b: (f32, f32),
}

/// What the comparison says so far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    /// Not enough days, or no clear difference yet.
    Running,
    /// One candidate is clearly more precise.
    Better {
        side: Side,
        /// Its click error over the other's (0.9 = 10% closer to the centre).
        click_error: f32,
        /// Its accuracy minus the other's.
        accuracy: f32,
    },
    /// One clicks clearly closer to the centre, the other clearly hits more: no winner.
    TradeOff,
    /// [`MAX_DAYS`] days without a clear difference: there is none worth changing for.
    NoDifference,
}

/// A comparison of two sensitivities over several days.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LongComparison {
    /// When it was set up (unix seconds).
    pub started: u64,
    /// The candidates, cm/360. A is the sensitivity the player had when setting it up.
    pub a_cm: f32,
    pub b_cm: f32,
    pub blocks: Vec<Block>,
}

impl LongComparison {
    pub fn new(started: u64, a_cm: f32, b_cm: f32) -> Self {
        Self { started, a_cm, b_cm, blocks: Vec::new() }
    }

    pub fn cm(&self, side: Side) -> f32 {
        match side {
            Side::A => self.a_cm,
            Side::B => self.b_cm,
        }
    }

    /// The complete days so far (two blocks of each candidate), oldest first. A day that was
    /// broken off doesn't count: one candidate would have been played colder.
    pub fn days(&self) -> Vec<Day> {
        let mut days: Vec<u64> = self.blocks.iter().map(|b| day_of(b.unix_time)).collect();
        days.sort_unstable();
        days.dedup();
        days.into_iter()
            .filter_map(|day| {
                let side = |side: Side| -> Option<(f32, f32)> {
                    let blocks: Vec<&Block> =
                        self.blocks.iter().filter(|b| day_of(b.unix_time) == day && b.side == side).collect();
                    let mean =
                        |value: fn(&Block) -> f32| blocks.iter().map(|b| value(b)).sum::<f32>() / blocks.len() as f32;
                    (blocks.len() >= DAY_BLOCKS / 2).then(|| (mean(|b| b.click_deg), mean(|b| b.accuracy)))
                };
                Some(Day { day, a: side(Side::A)?, b: side(Side::B)? })
            })
            .collect()
    }

    /// Blocks played on the day of `now`.
    pub fn played_today(&self, now: u64) -> usize {
        self.blocks.iter().filter(|b| day_of(b.unix_time) == day_of(now)).count()
    }

    /// The candidate of today's next block: ABBA, or BAAB after an odd number of complete days.
    /// `None` once today's blocks are done.
    pub fn next_side(&self, now: u64) -> Option<Side> {
        let earlier = self.days().iter().filter(|d| d.day != day_of(now)).count();
        let first = if earlier.is_multiple_of(2) { Side::A } else { Side::B };
        let order = [first, first.other(), first.other(), first];
        order.get(self.played_today(now)).copied()
    }

    /// The run of a block: plain Microflick with fixed settings (the global target size and
    /// decay don't apply), at the candidate's sensitivity, the first [`ADAPT_SECS`] unscored.
    pub fn block_run(&self, side: Side, now: u64, settings: &Settings) -> Run {
        let drill = DrillSpec { name: BLOCK_NAME.to_string(), config: DrillConfig::defaults(DrillKind::Microflick) };
        let title = format!(
            "Comparison day {}, block {} of {DAY_BLOCKS}: sensitivity {}",
            self.days().iter().filter(|d| d.day != day_of(now)).count() + 1,
            self.played_today(now) + 1,
            side.letter(),
        );
        let sens = sens_for(settings, self.cm(side));
        Run::new(drill, sens, ADAPT_SECS + SCORED_SECS, RunPurpose::FinderTrial, title).with_warm_in(ADAPT_SECS)
    }

    /// Paired over the complete days: the daily difference in click error (as a ratio) and in
    /// accuracy, each tested against zero with the t distribution for that many days at the
    /// gates' 1% level. Click error decides: a candidate wins when its clicks are clearly
    /// closer, unless accuracy clearly says the opposite. Accuracy alone names no winner; a
    /// second way to win would be a second chance for noise at every look.
    pub fn verdict(&self) -> Verdict {
        let days = self.days();
        if days.len() < MIN_DAYS {
            return Verdict::Running;
        }
        // B relative to A: log ratio of click error (below 0 = B closer), accuracy difference.
        let errors: Vec<f32> = days.iter().map(|d| (d.b.0.max(1e-6) / d.a.0.max(1e-6)).ln()).collect();
        let accuracies: Vec<f32> = days.iter().map(|d| d.b.1 - d.a.1).collect();
        let critical = t_critical((days.len() - 1) as f32);
        let clear = |values: &[f32]| -> Option<(f32, bool)> {
            let summary = Summary::of(values)?;
            Some((summary.mean, summary.t_against(0.0)?.abs() >= critical))
        };
        let (Some((error, error_clear)), Some((accuracy, accuracy_clear))) = (clear(&errors), clear(&accuracies))
        else {
            return Verdict::Running;
        };
        // Which candidate each measure favours.
        let (closer, hits_more) =
            (if error < 0.0 { Side::B } else { Side::A }, if accuracy > 0.0 { Side::B } else { Side::A });
        let better = |side: Side| {
            let sign = if side == Side::B { 1.0 } else { -1.0 };
            Verdict::Better { side, click_error: (sign * error).exp(), accuracy: sign * accuracy }
        };
        match (error_clear, accuracy_clear) {
            (true, true) if closer != hits_more => Verdict::TradeOff,
            (true, _) => better(closer),
            (false, _) if days.len() >= MAX_DAYS => Verdict::NoDifference,
            (false, _) => Verdict::Running,
        }
    }
}

/// The saved comparison, if one is running or waiting with its verdict.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompareStore {
    pub active: Option<LongComparison>,
}

impl Persisted for CompareStore {
    const PATH: &'static str = "aim_trainer_comparison.ron";
}

impl CompareStore {
    pub fn load() -> (Self, LoadReport) {
        let (mut store, report): (Self, _) = load_ron(Path::new(Self::PATH));
        // A hand-edited file must not start runs at a broken sensitivity.
        store.active = store.active.filter(|c| [c.a_cm, c.b_cm].iter().all(|cm| cm.is_finite() && *cm > 0.0));
        (store, report)
    }
}

/// Present while the player is in the comparison's screens and blocks; the screen it shows.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompareSession {
    /// Choosing the second candidate.
    #[default]
    Setup,
    /// The day's plan, the next block, or the verdict.
    Running,
    /// A block of this candidate is being played.
    Block(Side),
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::finder::stats::SECS_PER_DAY;
    use crate::finder::stats::tests::normal;

    fn block(day: u64, index: u64, side: Side, click_deg: f32, accuracy: f32) -> Block {
        Block { unix_time: day * SECS_PER_DAY + 60_000 + index * 150, side, click_deg, accuracy, clicks: 40 }
    }

    /// Plays one whole day in the comparison's own order; `measure` gives each side's click
    /// error and accuracy for a block.
    pub(crate) fn play_day(comparison: &mut LongComparison, day: u64, mut measure: impl FnMut(Side) -> (f32, f32)) {
        let now = day * SECS_PER_DAY + 60_000;
        let mut index = 0;
        while let Some(side) = comparison.next_side(now) {
            let (click_deg, accuracy) = measure(side);
            comparison.blocks.push(block(day, index, side, click_deg, accuracy));
            index += 1;
        }
        assert_eq!(index, DAY_BLOCKS as u64);
    }

    #[test]
    fn days_alternate_abba_and_baab() {
        let mut comparison = LongComparison::new(0, 40.0, 44.0);
        let mut orders = Vec::new();
        for day in 10..14 {
            let mut order = String::new();
            play_day(&mut comparison, day, |side| {
                order += side.letter();
                (0.4, 0.9)
            });
            orders.push(order);
        }
        assert_eq!(orders, ["ABBA", "BAAB", "ABBA", "BAAB"]);
        assert_eq!(comparison.days().len(), 4);
        assert_eq!(comparison.next_side(13 * SECS_PER_DAY + 80_000), None, "today is done");
        assert_eq!(comparison.next_side(14 * SECS_PER_DAY), Some(Side::A));
    }

    #[test]
    fn a_broken_off_day_does_not_count_and_does_not_shift_the_order() {
        let mut comparison = LongComparison::new(0, 40.0, 44.0);
        play_day(&mut comparison, 10, |_| (0.4, 0.9));
        // Day 11: only the first two blocks (B, then A).
        let now = 11 * SECS_PER_DAY + 60_000;
        assert_eq!(comparison.next_side(now), Some(Side::B));
        comparison.blocks.push(block(11, 0, Side::B, 0.4, 0.9));
        assert_eq!(comparison.next_side(now), Some(Side::A));
        comparison.blocks.push(block(11, 1, Side::A, 0.4, 0.9));
        // Later the same day it goes on where it stopped.
        assert_eq!((comparison.played_today(now + 3600), comparison.next_side(now + 3600)), (2, Some(Side::A)));
        // The next day: day 11 is not complete, so this is the second day again, BAAB.
        assert_eq!(comparison.days().len(), 1);
        assert_eq!(comparison.next_side(12 * SECS_PER_DAY), Some(Side::B));
        let run = comparison.block_run(Side::B, 12 * SECS_PER_DAY, &Settings::default());
        assert!(run.title.starts_with("Comparison day 2, block 1 of 4"), "{}", run.title);
    }

    #[test]
    fn blocks_play_each_candidate_at_its_own_sensitivity_after_an_unscored_stretch() {
        let settings = Settings { sens: 0.4, dpi: 800.0, ..Settings::default() };
        let current = settings.cm_per_360(0.4);
        let comparison = LongComparison::new(0, current, current * 1.1);
        let a = comparison.block_run(Side::A, 0, &settings);
        let b = comparison.block_run(Side::B, 0, &settings);
        assert!((a.sens - 0.4).abs() < 1e-5 && (b.sens - 0.4 / 1.1).abs() < 1e-5, "{} {}", a.sens, b.sens);
        assert_eq!((a.duration, a.warm_in_left(), a.purpose), (120.0, ADAPT_SECS, RunPurpose::FinderTrial));
        // The same fixed task whatever the global target settings are.
        let scaled = Settings { target_scale: 0.5, ..settings };
        assert_eq!(comparison.block_run(Side::A, 0, &scaled).drill, a.drill);
    }

    /// Plays `days` days: A clicks `a_error` degrees off on average, B `ratio` times that, with
    /// each day's form shared by both and every block scattering on its own.
    fn simulate(days: u64, ratio: f32, accuracy_gain: f32, rng: &mut fastrand::Rng) -> LongComparison {
        let mut comparison = LongComparison::new(0, 40.0, 44.0);
        for day in 0..days {
            let form = 0.1 * normal(rng);
            play_day(&mut comparison, 100 + day, |side| {
                let (factor, gain) = if side == Side::B { (ratio, accuracy_gain) } else { (1.0, 0.0) };
                let click = 0.4 * factor * (form + 0.12 * normal(rng)).exp();
                (click, (0.85 + gain - 0.2 * form + 0.03 * normal(rng)).clamp(0.0, 1.0))
            });
        }
        comparison
    }

    #[test]
    fn no_verdict_before_five_complete_days() {
        let mut rng = fastrand::Rng::with_seed(1);
        // B clicks twice as close: obvious, but four days are four days.
        let comparison = simulate(4, 0.5, 0.1, &mut rng);
        assert_eq!(comparison.verdict(), Verdict::Running);
        let comparison = simulate(5, 0.5, 0.1, &mut rng);
        let Verdict::Better { side: Side::B, click_error, accuracy } = comparison.verdict() else {
            panic!("{:?}", comparison.verdict())
        };
        assert!((click_error - 0.5).abs() < 0.08 && (accuracy - 0.1).abs() < 0.04, "{click_error} {accuracy}");
    }

    #[test]
    fn the_verdict_names_the_more_precise_candidate_either_way() {
        let mut rng = fastrand::Rng::with_seed(2);
        // A is the better one here: B's errors are 40% larger.
        let comparison = simulate(8, 1.4, -0.05, &mut rng);
        let Verdict::Better { side: Side::A, click_error, accuracy } = comparison.verdict() else {
            panic!("{:?}", comparison.verdict())
        };
        assert!(click_error < 0.8 && accuracy > 0.0, "{click_error} {accuracy}");
        // Clearly closer clicks but clearly fewer hits: no winner.
        let comparison = simulate(8, 0.6, -0.12, &mut rng);
        assert_eq!(comparison.verdict(), Verdict::TradeOff);
    }

    /// Two candidates that are the same for the player, looked at after every day from the
    /// fifth to the tenth: a winner is announced in fewer than 1 of 20 comparisons, and the
    /// rest end as "no difference". A real 20% difference in click error is found in most.
    #[test]
    fn false_and_true_verdict_rates() {
        let run = |ratio: f32, seed: u64| -> Verdict {
            let mut rng = fastrand::Rng::with_seed(seed);
            let full = simulate(MAX_DAYS as u64, ratio, 0.0, &mut rng);
            let mut so_far = LongComparison::new(0, 40.0, 44.0);
            for day in full.blocks.chunks(DAY_BLOCKS) {
                so_far.blocks.extend_from_slice(day);
                let verdict = so_far.verdict();
                if verdict != Verdict::Running {
                    return verdict;
                }
            }
            panic!("no verdict after {MAX_DAYS} days");
        };
        const COMPARISONS: u64 = 1000;
        let same: Vec<Verdict> = (0..COMPARISONS).map(|seed| run(1.0, seed)).collect();
        let winners = same.iter().filter(|v| **v != Verdict::NoDifference).count();
        assert!(winners <= 50, "{winners} of {COMPARISONS} equal pairs got a winner");
        let found =
            (0..COMPARISONS).filter(|&seed| matches!(run(0.8, seed), Verdict::Better { side: Side::B, .. })).count();
        assert!(found >= 700, "only {found} of {COMPARISONS} real differences were found");
        // And never the wrong way round.
        assert!((0..COMPARISONS).all(|seed| !matches!(run(0.8, seed), Verdict::Better { side: Side::A, .. })));
    }

    #[test]
    fn blocks_need_enough_clicks() {
        use bevy::math::Vec2;

        use crate::metrics::throughput::Click;
        let click = Click {
            start: Vec2::ZERO,
            target: Vec2::new(12.0, 0.0),
            radius: 1.0,
            size: 1.0,
            aim: Vec2::new(12.0, 0.25),
            time: 0.5,
            hit: true,
            first: true,
        };
        let stats = |clicks: usize| RunStats {
            kind: DrillKind::Microflick,
            drill: BLOCK_NAME.to_string(),
            sens: 0.4,
            cm_per_360: 40.82,
            duration: SCORED_SECS,
            hits: clicks as u32,
            shots: clicks as u32,
            expired: 0,
            ttk_total: 1.0,
            best_ttk: 0.3,
            on_target: 0.0,
            landings: Vec::new(),
            clicks: vec![click; clicks],
            rating: None,
            rounds_hit: 0,
        };
        assert_eq!(Block::from_stats(1, Side::A, &stats(MIN_CLICKS - 1)), None);
        let block = Block::from_stats(1, Side::B, &stats(30)).unwrap();
        assert_eq!((block.side, block.clicks, block.accuracy), (Side::B, 30, 1.0));
        assert!((block.click_deg - 0.25).abs() < 1e-5);
    }
}
