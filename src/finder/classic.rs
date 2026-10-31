//! Sensitivity finder: a blind A/B bisection in the spirit of the "PSA" method.
//!
//! The search range starts at 0.5x..1.5x of the current sensitivity. Each round the player plays
//! the same drill at both ends of the range (in random order, labelled only A/B), then picks the
//! one that felt better. The rejected end moves to the midpoint, halving the range. After the
//! configured number of rounds the midpoint of what's left is the recommendation.

use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinderStep {
    TrialA,
    TrialB,
    Choose,
    Done,
}

#[derive(Clone, Debug)]
pub struct FinderRound {
    pub low: f32,
    pub high: f32,
    pub low_score: f32,
    pub high_score: f32,
    pub picked_high: bool,
}

#[derive(Resource, Clone, Debug)]
pub struct Finder {
    pub start_sens: f32,
    pub low: f32,
    pub high: f32,
    /// Zero-based index of the round in progress.
    pub round: u32,
    pub rounds: u32,
    pub step: FinderStep,
    /// Which end of the range is presented as "A" this round. Randomised to avoid order bias.
    a_is_low: bool,
    pub score_a: f32,
    pub score_b: f32,
    pub history: Vec<FinderRound>,
}

impl Finder {
    pub fn new(start_sens: f32, rounds: u32) -> Self {
        Self {
            start_sens,
            low: start_sens * 0.5,
            high: start_sens * 1.5,
            round: 0,
            rounds: rounds.max(1),
            step: FinderStep::TrialA,
            a_is_low: fastrand::bool(),
            score_a: 0.0,
            score_b: 0.0,
            history: Vec::new(),
        }
    }

    pub fn sens_a(&self) -> f32 {
        if self.a_is_low { self.low } else { self.high }
    }

    pub fn sens_b(&self) -> f32 {
        if self.a_is_low { self.high } else { self.low }
    }

    /// Sensitivity for the trial that should be played next, if any.
    pub fn trial_sens(&self) -> Option<f32> {
        match self.step {
            FinderStep::TrialA => Some(self.sens_a()),
            FinderStep::TrialB => Some(self.sens_b()),
            FinderStep::Choose | FinderStep::Done => None,
        }
    }

    pub fn trial_label(&self) -> &'static str {
        match self.step {
            FinderStep::TrialB => "B",
            _ => "A",
        }
    }

    pub fn record_trial(&mut self, score: f32) {
        match self.step {
            FinderStep::TrialA => {
                self.score_a = score;
                self.step = FinderStep::TrialB;
            },
            FinderStep::TrialB => {
                self.score_b = score;
                self.step = FinderStep::Choose;
            },
            FinderStep::Choose | FinderStep::Done => {},
        }
    }

    /// Keep the preferred end of the range and move the other end to the midpoint.
    pub fn choose(&mut self, picked_a: bool) {
        if self.step != FinderStep::Choose {
            return;
        }
        let picked_high = picked_a != self.a_is_low;
        let (low_score, high_score) =
            if self.a_is_low { (self.score_a, self.score_b) } else { (self.score_b, self.score_a) };
        self.history.push(FinderRound { low: self.low, high: self.high, low_score, high_score, picked_high });

        let mid = (self.low + self.high) * 0.5;
        if picked_high {
            self.low = mid;
        } else {
            self.high = mid;
        }

        self.round += 1;
        self.step = if self.round >= self.rounds { FinderStep::Done } else { FinderStep::TrialA };
        self.a_is_low = fastrand::bool();
    }

    /// Replay both trials of the current round, e.g. after a bad run.
    pub fn redo_round(&mut self) {
        if self.step == FinderStep::Choose {
            self.step = FinderStep::TrialA;
        }
    }

    pub fn recommendation(&self) -> f32 {
        (self.low + self.high) * 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play_round(finder: &mut Finder, prefer_high: bool) {
        let sens_a = finder.trial_sens().unwrap();
        finder.record_trial(1.0);
        let sens_b = finder.trial_sens().unwrap();
        finder.record_trial(2.0);
        finder.choose((sens_a > sens_b) == prefer_high);
    }

    #[test]
    fn a_and_b_cover_both_ends_of_the_range() {
        let finder = Finder::new(1.0, 3);
        let mut pair = [finder.sens_a(), finder.sens_b()];
        pair.sort_by(f32::total_cmp);
        assert_eq!(pair, [0.5, 1.5]);
    }

    #[test]
    fn picking_a_end_keeps_that_end_regardless_of_blind_order() {
        for _ in 0..32 {
            let mut finder = Finder::new(1.0, 3);
            play_round(&mut finder, true);
            assert_eq!((finder.low, finder.high), (1.0, 1.5));

            let mut finder = Finder::new(1.0, 3);
            play_round(&mut finder, false);
            assert_eq!((finder.low, finder.high), (0.5, 1.0));
        }
    }

    #[test]
    fn records_scores_against_the_right_end() {
        let mut finder = Finder::new(1.0, 1);
        let a_is_low = finder.sens_a() < finder.sens_b();
        finder.record_trial(10.0); // A
        finder.record_trial(20.0); // B
        finder.choose(true);
        let round = &finder.history[0];
        let (low_score, high_score) = if a_is_low { (10.0, 20.0) } else { (20.0, 10.0) };
        assert_eq!((round.low_score, round.high_score), (low_score, high_score));
        assert_eq!(round.picked_high, !a_is_low);
    }

    #[test]
    fn finishes_after_configured_rounds_and_converges() {
        let mut finder = Finder::new(1.0, 7);
        for _ in 0..7 {
            assert_ne!(finder.step, FinderStep::Done);
            play_round(&mut finder, true);
        }
        assert_eq!(finder.step, FinderStep::Done);
        assert!(finder.trial_sens().is_none());
        // Always preferring the higher end converges towards the top of the initial range.
        assert!((finder.high - finder.low - 1.0 / 128.0).abs() < 1e-6);
        assert!(finder.recommendation() > 1.49);
    }

    #[test]
    fn redo_replays_the_round_without_narrowing() {
        let mut finder = Finder::new(1.0, 3);
        finder.record_trial(1.0);
        finder.record_trial(2.0);
        finder.redo_round();
        assert_eq!(finder.step, FinderStep::TrialA);
        assert_eq!((finder.low, finder.high, finder.round), (0.5, 1.5, 0));
    }
}
