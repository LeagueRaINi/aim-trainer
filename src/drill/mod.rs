//! Drills: the frame stages ([`DrillSet`]), the run lifecycle ([`run`]), scoring ([`stats`])
//! and replay recording. Each built-in drill is described by a [`DrillDef`](def::DrillDef) in
//! [`kinds`], which also holds its own systems; [`targets`] has the behaviour all drills share.

pub mod adaptive;
mod config;
pub mod def;
mod geometry;
mod kind;
pub mod kinds;
pub mod params;
mod run;
pub mod stats;
mod targets;

use bevy::prelude::*;
pub use config::{ADAPTIVE_TIME_LIMIT, DrillConfig, DrillSpec, SECS_RANGE, VARIANT_SEPARATOR};
pub use kind::DrillKind;
pub use run::{KeepRunsOnFocusLoss, Phase, Run, RunFinished, RunPurpose, begin_run};
pub use stats::RunStats;
pub(crate) use targets::facing_eye;
use targets::{Dormant, Target, TargetExpired, TargetKilled};
pub use targets::{FacesEye, TargetAssets, spawn_environment};

use crate::AppState;
use crate::camera::{ActiveSens, Look, apply_look, mouse_look};
use crate::replay::Recorder;
use crate::settings::Settings;

pub struct DrillPlugin;

/// The stages of a frame in `Playing`, in order. Drills add their systems to the stage they
/// belong in; systems of different drills in one stage touch disjoint components. Commands are
/// applied between stages (e.g. `Measure` sees targets spawned in `Input`/`Expire`).
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DrillSet {
    /// Escape/alt-tab abort, then the ready gate and countdown (spawns the initial targets).
    Start,
    /// Clicks.
    Input,
    /// Targets running out of time.
    Expire,
    /// Measurements of the player's aim (the first-flick probe) and adaptive target sizing.
    Measure,
    /// Delayed spawns.
    Spawn,
    /// Target movement.
    Move,
    /// Continuous scoring (time on target).
    Score,
    /// Replay sampling.
    Record,
    /// The run timer; writes [`RunFinished`] when the run is over.
    Finish,
    /// The history records the finished run (`history::RunRecorded`).
    Log,
    /// Reactions to the recorded run: replay saving, finders, routines, the results screen.
    Outcome,
}

impl Plugin for DrillPlugin {
    fn build(&self, app: &mut App) {
        use DrillSet::*;
        app.add_message::<TargetKilled>()
            .add_message::<TargetExpired>()
            .add_message::<RunFinished>()
            .add_systems(Startup, targets::create_target_assets)
            .add_systems(Update, targets::sync_target_look.run_if(resource_changed::<Settings>))
            // After every drill (and the replay player) has moved its targets this frame.
            .add_systems(PostUpdate, targets::face_eye.before(TransformSystems::Propagate))
            .add_systems(
                OnEnter(AppState::Playing),
                (targets::spawn_static, |mut commands: Commands| commands.insert_resource(Recorder::default())),
            )
            // Mouse look turns at the run's sensitivity (read every frame: a finder trial's next
            // part may replace the run without leaving `Playing`).
            .add_systems(
                Update,
                (|run: Res<Run>, mut sens: ResMut<ActiveSens>| sens.0 = run.sens)
                    .before(mouse_look)
                    .run_if(in_state(AppState::Playing)),
            )
            .configure_sets(
                Update,
                (Start, Input, Expire, Measure, Spawn, Move, Score, Record, Finish, Log, Outcome)
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            )
            .configure_sets(Update, (Input, Expire, Measure, Spawn, Move, Score).after(apply_look))
            .add_systems(
                Update,
                (
                    // First, so an abort on the run's final frame wins over finishing it.
                    (run::abort_run, run::ready_gate).chain().in_set(Start),
                    targets::shoot.in_set(Input),
                    targets::decay.in_set(Expire),
                    targets::spawn_pending.in_set(Spawn),
                    record_frame.in_set(Record),
                    run::tick_run.in_set(Finish),
                ),
            );
        kinds::register(app);
        adaptive::register(app);
    }
}

/// Samples the view and every target the player should be reacting to ([`Dormant`] ones aren't).
fn record_frame(
    run: Res<Run>,
    look: Res<Look>,
    mut recorder: ResMut<Recorder>,
    targets: Query<&Transform, (With<Target>, Without<Dormant>)>,
) {
    if run.live() {
        let targets = targets.iter().map(|transform| (transform.translation, transform.scale.x));
        recorder.sample(run.elapsed(), look.angles(), targets);
    }
}
