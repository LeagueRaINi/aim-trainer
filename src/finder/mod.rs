//! Sensitivity finders: the classic A/B bisection ([`classic`]) and the model-based smart
//! finder ([`smart`]). Both are pure state machines; this module feeds them finished trials.

pub mod classic;
pub mod smart;

use bevy::prelude::*;

use self::{
    classic::Finder,
    smart::{Segment, SmartFinder},
};
use crate::{
    AppState,
    drill::{DrillSet, RunPurpose, begin_run},
    history::RunRecorded,
    settings::Settings,
};

pub struct FinderPlugin;

impl Plugin for FinderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, finder_trial_finished.in_set(DrillSet::Outcome));
    }
}

/// A finished finder trial (part) feeds whichever finder is running; the smart finder goes
/// straight on to the trial's next part, otherwise it's back to the finder screen.
pub(crate) fn finder_trial_finished(
    mut commands: Commands,
    mut recorded: MessageReader<RunRecorded>,
    settings: Res<Settings>,
    mut finder: Option<ResMut<Finder>>,
    mut smart: Option<ResMut<SmartFinder>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for recorded in recorded.read().filter(|r| r.run.purpose == RunPurpose::FinderTrial) {
        let stats = &recorded.run.stats;
        if let Some(finder) = finder.as_mut() {
            finder.record_trial(stats.score());
        }
        if let Some(smart) = smart.as_mut() {
            smart.record_segment(stats.kind, Segment::from_stats(stats));
            // Straight on to the trial's next part, if there is one.
            if let Some(next) = smart.segment_run(&settings) {
                begin_run(&mut commands, &mut next_state, next);
                continue;
            }
        }
        next_state.set(AppState::Finder);
    }
}
