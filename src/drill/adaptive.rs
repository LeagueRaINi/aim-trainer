//! Adaptive mode: in a drill whose config has [`DrillConfig::adaptive`](super::DrillConfig)
//! set, QUEST+ ([`crate::metrics::quest`]) picks every new target's angular radius, and each
//! target is one trial: a hit if it is killed within the config's time limit, a miss if it
//! expires (missed clicks don't end a target, as in the drill itself).

use std::collections::HashMap;

use bevy::prelude::*;

use super::{
    DrillSet, Run, VARIANT_SEPARATOR,
    targets::{Target, TargetExpired, TargetKilled},
};
use crate::{
    AppState,
    camera::PlayerCamera,
    metrics::quest::{Estimate, Quest, QuestMode},
};

/// Length of a Measure run, seconds: at least 60 targets even if every one expires at the
/// default time limit, enough for a θ75 within about 10% (see [`crate::metrics::quest`]).
pub const MEASURE_SECS: f32 = 90.0;

/// The adaptive procedure of the current (or last finished) run.
#[derive(Resource, Default)]
pub struct AdaptiveRun {
    live: Option<LiveQuest>,
    /// The latest θ75 (degrees) per [`quest_task`], the prior of the next run of that task.
    /// Filled from the adaptive results log.
    pub priors: HashMap<String, f32>,
}

struct LiveQuest {
    quest: Quest,
    mode: QuestMode,
    /// Radius (degrees) of the target on screen until it is killed or expires.
    current: Option<f32>,
}

impl AdaptiveRun {
    /// Starts a procedure for `run` if it is adaptive, centred on the task's latest θ75.
    pub(crate) fn begin(&mut self, run: &Run) {
        self.live = run.drill.config.adaptive().map(|mode| {
            let prior = self.priors.get(quest_task(&run.drill.name)).copied();
            LiveQuest { quest: Quest::new(prior), mode, current: None }
        });
    }

    /// The current estimate; after a run, its final one (until the next run starts).
    pub fn estimate(&self) -> Option<Estimate> {
        self.live.as_ref().map(|live| live.quest.estimate())
    }

    /// Picks the radius (degrees) of a target that just appeared.
    pub(crate) fn next_target(&mut self) -> Option<f32> {
        let live = self.live.as_mut()?;
        let radius = live.quest.next_radius(live.mode);
        live.current = Some(radius);
        Some(radius)
    }

    /// The target on screen was killed in time (`hit`) or expired; `counted` is false during a
    /// warm-in.
    pub(crate) fn outcome(&mut self, hit: bool, counted: bool) {
        if let Some(live) = self.live.as_mut()
            && let Some(radius) = live.current.take()
            && counted
        {
            live.quest.update(radius, hit);
        }
    }
}

/// What a θ75 measures, whichever mode measured it: the drill's name without the mode suffix
/// of [`DrillSpec::adaptive`](super::DrillSpec::adaptive), so Train starts from your last
/// Measure result and vice versa.
pub fn quest_task(drill: &str) -> &str {
    QuestMode::ALL
        .iter()
        .find_map(|mode| {
            drill
                .strip_suffix(mode.name().to_lowercase().as_str())
                .and_then(|rest| rest.strip_suffix(VARIANT_SEPARATOR))
        })
        .unwrap_or(drill)
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<AdaptiveRun>()
        .add_systems(OnEnter(AppState::Playing), |run: Res<Run>, mut adaptive: ResMut<AdaptiveRun>| adaptive.begin(&run))
        .add_systems(Update, adapt_targets.in_set(DrillSet::Measure));
}

/// Scores the previous target (killed or expired this frame) and sizes the new one: its radius
/// becomes the procedure's next angular radius at its actual distance.
fn adapt_targets(
    run: Res<Run>,
    mut adaptive: ResMut<AdaptiveRun>,
    mut killed: MessageReader<TargetKilled>,
    mut expired: MessageReader<TargetExpired>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Target>)>,
    mut spawned: Query<(&mut Target, &mut Transform), Added<Target>>,
) {
    // Always drain, so a later run never sees this one's messages.
    let hit = killed.read().count() > 0;
    let missed = expired.read().count() > 0;
    if run.drill.config.adaptive().is_none() {
        return;
    }
    if hit || missed {
        adaptive.outcome(hit, run.counting());
    }
    for (mut target, mut transform) in &mut spawned {
        let Some(radius) = adaptive.next_target() else { return };
        let distance = transform.translation.distance(camera.translation);
        target.base_radius = distance * radius.to_radians().tan();
        transform.scale = Vec3::splat(target.base_radius);
    }
}

#[cfg(test)]
mod tests {
    use bevy::state::app::StatesPlugin;

    use super::*;
    use crate::{
        drill::{DrillKind, DrillSpec, RunPurpose},
        settings::Settings,
    };

    #[test]
    fn task_ignores_the_mode() {
        let settings = Settings::default();
        let measure = DrillSpec::adaptive(DrillKind::Microflick, QuestMode::Measure, &settings).name;
        let train = DrillSpec::adaptive(DrillKind::Microflick, QuestMode::Train, &settings).name;
        assert_eq!(quest_task(&measure), "Microflick · adaptive");
        assert_eq!(quest_task(&train), "Microflick · adaptive");
        assert_eq!(quest_task("Weak spot: up · adaptive"), "Weak spot: up · adaptive");
        assert_eq!(quest_task("Train"), "Train");
    }

    fn app_with(drill: DrillSpec, priors: &[(&str, f32)]) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(AppState::Menu)
            .add_message::<TargetKilled>()
            .add_message::<TargetExpired>()
            .configure_sets(Update, DrillSet::Measure.run_if(in_state(AppState::Playing)))
            .insert_resource(Run::new(drill, 0.4, 60.0, RunPurpose::Practice, String::new()));
        register(&mut app);
        let mut adaptive = app.world_mut().resource_mut::<AdaptiveRun>();
        adaptive.priors = priors.iter().map(|(task, theta)| (task.to_string(), *theta)).collect();
        app.world_mut().spawn((PlayerCamera, Transform::default()));
        app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::Playing);
        app
    }

    fn spawn_target(app: &mut App, at: Vec3) -> Entity {
        let target = Target { base_radius: 0.2, born: 0.0, lifetime: Some(1.5), spawn_aim: None };
        app.world_mut().spawn((target, Transform::from_translation(at).with_scale(Vec3::splat(0.2)))).id()
    }

    #[test]
    fn sizes_targets_and_scores_them() {
        let settings = Settings::default();
        let drill = DrillSpec::adaptive(DrillKind::Microflick, QuestMode::Measure, &settings);
        let mut app = app_with(drill, &[]);
        let at = Vec3::new(0.0, 0.0, -8.0);
        let first = spawn_target(&mut app, at);
        app.update();
        let radius = Quest::new(None).next_radius(QuestMode::Measure);
        let scale = app.world().get::<Transform>(first).unwrap().scale.x;
        let angular = (scale / 8.0).atan().to_degrees();
        assert!((angular - radius).abs() < 1e-4, "{angular} vs {radius}");
        assert_eq!(app.world().get::<Target>(first).unwrap().base_radius, scale);

        app.world_mut().despawn(first);
        app.world_mut().write_message(TargetKilled { aim: Vec3::NEG_Z });
        spawn_target(&mut app, at);
        app.update();
        app.world_mut().write_message(TargetExpired);
        app.update();
        let estimate = app.world().resource::<AdaptiveRun>().estimate().unwrap();
        assert_eq!((estimate.trials, estimate.hits), (2, 1));
    }

    #[test]
    fn fixed_size_runs_are_left_alone() {
        let drill = DrillSpec::builtin(DrillKind::Microflick, &Settings::default());
        let mut app = app_with(drill, &[]);
        let target = spawn_target(&mut app, Vec3::new(0.0, 0.0, -10.0));
        app.update();
        assert_eq!(app.world().get::<Transform>(target).unwrap().scale.x, 0.2);
        assert!(app.world().resource::<AdaptiveRun>().estimate().is_none());
    }

    #[test]
    fn starts_from_the_tasks_last_result() {
        let settings = Settings::default();
        let drill = DrillSpec::adaptive(DrillKind::Microflick, QuestMode::Train, &settings);
        let mut app = app_with(drill, &[("Microflick · adaptive", 0.9)]);
        app.update();
        let estimate = app.world().resource::<AdaptiveRun>().estimate().unwrap();
        assert_eq!(estimate, Quest::new(Some(0.9)).estimate());
    }
}
