//! Warmup routines and playlists: a sequence of drills with a self-paced break screen between
//! steps. Built-in routines are fixed; custom playlists come from [`crate::custom`].

use bevy::prelude::*;

use crate::{
    AppState,
    history::Comparison,
    drill::{DrillSpec, Run, RunPurpose, RunStats, DrillKind, begin_run},
    settings::Settings,
};

pub struct RoutinePlan {
    pub name: &'static str,
    pub blurb: &'static str,
    pub steps: &'static [(DrillKind, f32)],
}

pub const ROUTINES: [RoutinePlan; 3] = [
    RoutinePlan {
        name: "Valorant warmup",
        blurb: "Eyes, flicks, head-level placement, tracking, then peeks. ~4 min.",
        steps: &[
            (DrillKind::Peripheral, 30.0),
            (DrillKind::Microflick, 45.0),
            (DrillKind::HeadLevel, 45.0),
            (DrillKind::Tracking, 30.0),
            (DrillKind::Peek, 60.0),
        ],
    },
    RoutinePlan {
        name: "CS2 warmup",
        blurb: "Crosshair placement first, then flicks, peeks and a speed finisher. ~4 min.",
        steps: &[
            (DrillKind::HeadLevel, 45.0),
            (DrillKind::Microflick, 45.0),
            (DrillKind::Peek, 60.0),
            (DrillKind::Gridshot, 45.0),
            (DrillKind::Tracking, 30.0),
        ],
    },
    RoutinePlan {
        name: "Eye training",
        blurb: "Reaction speed, peripheral awareness and smooth pursuit. ~2 min.",
        steps: &[
            (DrillKind::Reaction, 30.0),
            (DrillKind::Peripheral, 45.0),
            (DrillKind::Tracking, 30.0),
            (DrillKind::Reaction, 30.0),
        ],
    },
];

#[derive(Clone)]
pub struct RoutineStep {
    pub drill: DrillSpec,
    pub secs: f32,
}

pub struct StepResult {
    pub stats: RunStats,
    pub previous: Comparison,
}

/// A routine or playlist in progress.
#[derive(Resource)]
pub struct Routine {
    pub name: String,
    pub blurb: String,
    pub steps: Vec<RoutineStep>,
    /// Index of the next step to play.
    pub next: usize,
    pub results: Vec<StepResult>,
}

impl Routine {
    pub fn new(name: String, blurb: String, steps: Vec<RoutineStep>) -> Self {
        Self { name, blurb, steps, next: 0, results: Vec::new() }
    }

    pub fn from_plan(plan: &RoutinePlan, settings: &Settings) -> Self {
        let steps = plan
            .steps
            .iter()
            .map(|&(kind, secs)| RoutineStep { drill: DrillSpec::builtin(kind, settings), secs })
            .collect();
        Self::new(plan.name.to_string(), plan.blurb.to_string(), steps)
    }

    /// The same routine from the start, for "run again".
    pub fn restarted(&self) -> Self {
        Self::new(self.name.clone(), self.blurb.clone(), self.steps.clone())
    }

    pub fn finished(&self) -> bool {
        self.next >= self.steps.len()
    }

    pub fn next_step(&self) -> Option<&RoutineStep> {
        self.steps.get(self.next)
    }

    pub fn record(&mut self, stats: RunStats, previous: Comparison) {
        self.results.push(StepResult { stats, previous });
        self.next += 1;
    }

    pub fn skip(&mut self) {
        self.next += 1;
    }

    /// Starts the next step (it still waits for a click before the countdown).
    pub fn start_next(&self, sens: f32, commands: &mut Commands, next_state: &mut NextState<AppState>) {
        let Some(step) = self.next_step() else { return };
        let title = format!("{} {}/{} - {}", self.name, self.next + 1, self.steps.len(), step.drill.name);
        let run = Run::new(step.drill.clone(), sens, step.secs, RunPurpose::Routine, title);
        begin_run(commands, next_state, run);
    }
}

pub fn start_routine(commands: &mut Commands, next_state: &mut NextState<AppState>, routine: Routine) {
    commands.insert_resource(routine);
    next_state.set(AppState::Routine);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_routines_are_short_enough_for_a_warmup() {
        for plan in &ROUTINES {
            let total: f32 = plan.steps.iter().map(|s| s.1).sum();
            assert!(total <= 300.0, "{} is {total} s", plan.name);
            assert!(!plan.steps.is_empty());
        }
    }

    #[test]
    fn skipping_advances_to_the_end_and_restart_resets() {
        let mut routine = Routine::from_plan(&ROUTINES[2], &Settings::default());
        for _ in 0..ROUTINES[2].steps.len() {
            assert!(!routine.finished());
            routine.skip();
        }
        assert!(routine.finished());
        assert!(routine.next_step().is_none());
        let again = routine.restarted();
        assert_eq!((again.next, again.steps.len()), (0, ROUTINES[2].steps.len()));
    }
}
