//! Drills: the frame stages ([`DrillSet`]), the run lifecycle ([`run`]), scoring ([`stats`])
//! and replay recording. Each built-in drill is described by a [`DrillDef`](def::DrillDef) in
//! [`kinds`], which also holds its own systems; [`targets`] has the behaviour all drills share.

pub mod adaptive;
mod config;
pub mod cue;
pub mod def;
mod geometry;
mod health;
pub(crate) mod held_fire;
mod kind;
pub mod kinds;
pub mod params;
pub mod pause;
mod run;
pub mod stats;
mod targets;
mod wander;

mod bot;
pub mod movement;
pub mod range;
pub mod rifle;

use bevy::prelude::*;
pub use config::{ADAPTIVE_TIME_LIMIT, DrillConfig, DrillSpec, SECS_RANGE, VARIANT_SEPARATOR};
pub use kind::DrillKind;
pub use pause::KeepRunsOnFocusLoss;
pub use run::{Phase, Run, RunFinished, RunPurpose, begin_run};
pub use stats::RunStats;
pub(crate) use targets::facing_eye;
use targets::{Dormant, Target, TargetExpired, TargetKilled};
pub use targets::{FacesEye, TargetAssets, spawn_environment};

use crate::camera::{ActiveSens, Look, PlayerCamera, apply_look, mouse_look};
use crate::replay::Recorder;
use crate::settings::Settings;
use crate::{AppState, PlayState};

pub struct DrillPlugin;

/// The stages of a frame of a running drill (`PlayState::Running`), in order. Drills add their
/// systems to the stage they belong in; systems of different drills in one stage touch disjoint
/// components. Commands are applied between stages (e.g. `Measure` sees targets spawned in
/// `Input`/`Expire`).
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DrillSet {
    /// The ready gate and countdown (spawns the initial targets).
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
            .init_resource::<pause::ResumeCountdown>()
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
            // Whole frames only: a pause takes effect between frames, so a paused run has no
            // stage half done and no message unread.
            .configure_sets(
                Update,
                (Start, Input, Expire, Measure, Spawn, Move, Score, Record, Finish, Log, Outcome)
                    .chain()
                    .run_if(in_state(PlayState::Running)),
            )
            .configure_sets(Update, (Input, Expire, Measure, Spawn, Move, Score).after(apply_look))
            .add_systems(
                Update,
                (
                    pause::pause_control.before(Start).run_if(in_state(AppState::Playing)),
                    run::ready_gate.in_set(Start),
                    targets::shoot.in_set(Input),
                    targets::decay.in_set(Expire),
                    targets::spawn_pending.in_set(Spawn),
                    record_frame.in_set(Record),
                    run::tick_run.in_set(Finish),
                ),
            );
        kinds::register(app);
        adaptive::register(app);
        health::register(app);
        held_fire::register(app);
        bot::register(app);
        range::register(app);
        rifle::register(app);
        movement::register(app);
        cue::register(app);
    }
}

/// Samples the view and every target the player should be reacting to ([`Dormant`] ones aren't).
/// Positions are recorded relative to the eye, which a replay keeps at the origin: the same thing
/// as their place in the room unless the drill moves the player. For those drills the scenery's
/// position is recorded too, as a target of radius 0 (see [`range::EyeAnchor`]).
fn record_frame(
    run: Res<Run>,
    look: Res<Look>,
    mut recorder: ResMut<Recorder>,
    camera: Single<&Transform, With<PlayerCamera>>,
    targets: Query<&Transform, (With<Target>, Without<Dormant>)>,
    anchors: Query<&Transform, With<range::EyeAnchor>>,
) {
    if run.live() {
        let eye = camera.translation;
        let targets = targets.iter().map(|transform| (transform.translation - eye, transform.scale.x));
        let anchors = anchors.iter().map(|transform| (transform.translation - eye, 0.0));
        recorder.sample(run.elapsed(), look.angles(), targets.chain(anchors));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A replay looks from the origin, so what is recorded is where things are as seen from the
    /// eye, wherever the player has strafed to: targets with their size, the scenery's anchor
    /// with none.
    #[test]
    fn frames_are_recorded_relative_to_the_eye() {
        let mut app = App::new();
        let drill = DrillSpec::builtin(DrillKind::CounterStrafe, &Settings::default());
        app.insert_resource(Run::new(drill, 0.4, 60.0, RunPurpose::Practice, String::new()).started())
            .init_resource::<Look>()
            .init_resource::<Recorder>()
            .add_systems(Update, record_frame);
        app.world_mut().spawn((PlayerCamera, Transform::from_xyz(2.0, 0.0, 0.0)));
        let target = Target { base_radius: 0.14, born: 0.0, lifetime: None, spawn_aim: None };
        app.world_mut().spawn((target, Transform::from_xyz(-1.0, 0.0, -12.0).with_scale(Vec3::splat(0.14))));
        app.world_mut().spawn((range::EyeAnchor, Transform::IDENTITY));
        app.update();
        let replay = app.world_mut().resource_mut::<Recorder>().finish(DrillKind::CounterStrafe, "", 0.4, 60.0);
        assert_eq!(replay.frames.len(), 1);
        assert_eq!(replay.frames[0].targets, [(Vec3::new(-3.0, 0.0, -12.0), 0.14), (Vec3::new(-2.0, 0.0, 0.0), 0.0)]);
    }
}
