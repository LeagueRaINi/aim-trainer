//! Survival: dots fly at the player, faster and more often as the run goes on. Each one that
//! gets close costs a life; the run ends when the lives are gone (or the time is up).

use std::borrow::Cow;

use bevy::prelude::*;

use super::microflick::flick_position;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_respawn, no_tuning};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::SpawnCtx;
use crate::drill::{DrillConfig, DrillSet, Run};

/// Distance from the eye (m) at which an approaching dot costs a life.
const REACH_DISTANCE: f32 = 2.0;
/// Seconds of live time over which the pace grows by the starting pace: twice as fast and twice
/// as many dots after 45 s, three times after 90 s.
const RAMP_SECS: f32 = 45.0;

pub const DEF: DrillDef = DrillDef {
    name: "Survival",
    blurb,
    category: Category::Aim,
    base_radius: 0.3,
    decay: DecayRule::None,
    score: ScoreModel::Survival,
    ttk_from: TtkFrom::Spawn,
    click_kills: true,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    endless: true,
    params: &[
        ParamId::Lives,
        ParamId::ApproachSpeed,
        ParamId::SpawnInterval,
        ParamId::YawRange,
        ParamId::PitchRange,
        ParamId::MinFlick,
    ],
    tune_defaults: no_tuning,
    spawn_initial,
    respawn: no_respawn,
    spawn_next,
};

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, approach.in_set(DrillSet::Move));
}

fn blurb(config: &DrillConfig) -> Cow<'static, str> {
    let lives = match config.lives {
        1 => "1 life".to_string(),
        n => format!("{n} lives"),
    };
    format!("Shoot the dots flying at you before they reach you. {lives}, ever faster.").into()
}

/// A dot flying straight at the eye.
#[derive(Component)]
struct Approach {
    /// m/s.
    speed: f32,
}

/// How much faster (and more often) dots come `now` seconds into the run.
fn pace(now: f32) -> f32 {
    1.0 + now / RAMP_SECS
}

/// One dot to start with (it waits for the countdown), the next after the spawn interval.
fn spawn_initial(ctx: &mut SpawnCtx) {
    spawn_next(ctx);
}

/// A dot somewhere on screen, at the current pace, and the time of the next.
fn spawn_next(ctx: &mut SpawnCtx) {
    let pace = pace(ctx.now);
    let position = flick_position(ctx.aim, &ctx.view, ctx.margin(), ctx.config);
    let approach = Approach { speed: ctx.config.approach_speed * pace };
    ctx.spawn_target(position, None).insert(approach);
    ctx.next.0 = Some(ctx.now + ctx.config.spawn_interval / pace);
}

/// Moves the dots in; one that gets close costs a life (counted as a target that got away), and
/// the run ends with the last life.
fn approach(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut dots: Query<(Entity, &Approach, &mut Transform)>,
) {
    if !run.live() {
        return;
    }
    let dt = time.delta_secs();
    for (entity, approach, mut transform) in &mut dots {
        let distance = transform.translation.length() - approach.speed * dt;
        if distance <= REACH_DISTANCE {
            commands.entity(entity).despawn();
            run.expire();
        } else {
            transform.translation = transform.translation.normalize() * distance;
        }
    }
    if run.expired() >= run.drill.config.lives {
        run.end();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::drill::run::{RunFinished, tick_run};
    use crate::drill::{DrillKind, DrillSpec, RunPurpose};
    use crate::settings::Settings;

    /// Dots that get close cost a life each; losing the last one finishes the run early, with
    /// the time survived as its length, while dots still far away keep coming. The drill length
    /// (here shorter than the test) doesn't end it: only lives do.
    #[test]
    fn losing_the_last_life_ends_the_run() {
        let config = DrillConfig { lives: 2, ..DrillConfig::defaults(DrillKind::Survival) };
        let drill = DrillSpec { name: "Survival".into(), config };
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)))
            .insert_resource(Settings::default())
            .insert_resource(Run::new(drill, 0.4, 0.15, RunPurpose::Practice, String::new()).started())
            .add_message::<RunFinished>()
            .add_systems(Update, (approach, tick_run).chain());
        let near = Vec3::new(0.0, 0.0, -(REACH_DISTANCE + 0.05));
        let far = app.world_mut().spawn((Approach { speed: 1.0 }, Transform::from_xyz(0.0, 0.0, -10.0))).id();
        app.world_mut().spawn((Approach { speed: 1.0 }, Transform::from_translation(near)));
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Run>().expired(), 1);
        assert!(app.world().resource::<Messages<RunFinished>>().is_empty(), "one life left");

        app.world_mut().spawn((Approach { speed: 1.0 }, Transform::from_translation(near)));
        app.update();
        let finished: Vec<_> = app.world_mut().resource_mut::<Messages<RunFinished>>().drain().collect();
        assert_eq!(finished.len(), 1);
        assert!(finished[0].stats.duration < 1.0, "survived {} s", finished[0].stats.duration);
        assert!(app.world().get::<Transform>(far).unwrap().translation.z > -10.0);
    }
}
