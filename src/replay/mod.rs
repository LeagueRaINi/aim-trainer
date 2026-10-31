//! Run replays: compact recordings of view angles, targets and shots, saved per practice run
//! ([`format`], [`recorder`]), and a first-person [`player`].

pub mod format;
pub mod player;
pub mod recorder;

use std::path::PathBuf;

use bevy::prelude::*;
pub use format::{Replay, ReplayFrame, Shot};
pub use player::{Playback, watch};
pub use recorder::Recorder;

use crate::AppState;
use crate::camera::apply_look;
use crate::drill::{DrillSet, RunPurpose};
use crate::history::RunRecorded;

pub struct ReplayPlugin;

impl Plugin for ReplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<player::TracerGizmos>()
            .insert_resource(ReplayDir(PathBuf::from(format::REPLAY_DIR)))
            .add_systems(Startup, (player::create_marker_assets, player::configure_tracer))
            .add_systems(OnEnter(AppState::Replay), player::spawn_replay_scene)
            .add_systems(OnExit(AppState::Replay), player::clear_look)
            .add_systems(Update, save_finished_replay.in_set(DrillSet::Outcome))
            .add_systems(
                Update,
                (player::playback_controls, player::advance_playback, player::draw_tracer)
                    .chain()
                    .before(apply_look)
                    .run_if(in_state(AppState::Replay)),
            );
    }
}

/// The one directory replays are saved in and loaded from.
#[derive(Resource)]
pub struct ReplayDir(pub PathBuf);

/// Saves the replay of every run logged as practice, named by its history timestamp.
pub(crate) fn save_finished_replay(
    mut recorded: MessageReader<RunRecorded>,
    mut recorder: ResMut<Recorder>,
    dir: Res<ReplayDir>,
) {
    for recorded in recorded.read().filter(|r| r.run.purpose.logged_as() == RunPurpose::Practice) {
        let stats = &recorded.run.stats;
        recorder.finish(stats.kind, &stats.drill, stats.sens, stats.duration).save(&dir.0, recorded.timestamp);
    }
}
