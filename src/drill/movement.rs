//! Player movement for drills that have the player strafe: sideways along the X axis with A and
//! D, in a lane, with the eye (the player camera) moving along. No head bob and no roll: only
//! the eye's position changes.
//!
//! The numbers approximate Valorant with a rifle in hand; none of them is exact:
//! - [`RUN_SPEED`]: 5.4 m/s, the run speed the game lists for the Vandal and Phantom.
//! - Stopping: Riot's weapon designer measured (2020, on the Phantom) about 0.16 s from a full
//!   run to a standstill when counter-strafing and about 0.17 s without, with the speed falling
//!   close to linearly. So releasing the key brakes at a constant `RUN_SPEED / 0.17 s`, and
//!   pressing the opposite key at `RUN_SPEED / 0.16 s`: counter-strafing stops sooner, but only
//!   just. What matters is being stopped before the shot.
//! - Starting: not published. Taken to be as quick as stopping without a counter-strafe.
//! - [`DEAD_ZONE`]: below 30% of the run speed the game treats a player as standing still
//!   (patch 0.50). Above it shots are inaccurate, see [`error`].
//!
//! Each frame is integrated exactly (the speed changes linearly in every phase), so the movement
//! is the same at any frame rate. The systems are in [`DrillSet::Move`], so a paused run doesn't
//! move.

use bevy::prelude::*;

use super::{DrillSet, Run};
use crate::AppState;
use crate::camera::PlayerCamera;

/// Top speed, m/s.
pub(crate) const RUN_SPEED: f32 = 5.4;
/// Seconds from rest to [`RUN_SPEED`].
const START_SECS: f32 = 0.17;
/// Seconds from [`RUN_SPEED`] to rest after releasing the key.
const COAST_STOP_SECS: f32 = 0.17;
/// Seconds from [`RUN_SPEED`] to rest while pressing the opposite key.
const COUNTER_STOP_SECS: f32 = 0.16;
/// Fraction of [`RUN_SPEED`] up to which the player counts as standing still.
pub(crate) const DEAD_ZONE: f32 = 0.3;
/// How far off the crosshair (degrees) a shot at a full run may land. Valorant lists +6° for a
/// running Vandal and +3° for a walking one; here the error grows linearly from nothing at the
/// dead zone's edge to this.
const RUN_ERROR: f32 = 6.0;

/// The most a shot fired at `speed` m/s may land off the crosshair because of the movement,
/// degrees.
pub(crate) fn error(speed: f32) -> f32 {
    RUN_ERROR * ((speed / RUN_SPEED - DEAD_ZONE) / (1.0 - DEAD_ZONE)).clamp(0.0, 1.0)
}

/// The strafing player. Present only while a drill with movement is played.
#[derive(Resource, Debug)]
pub(crate) struct PlayerMove {
    /// Where the eye is along the lane, metres from its centre.
    x: f32,
    /// m/s along the lane.
    velocity: f32,
    /// Half the lane's width, metres: the player stops dead at its ends.
    lane: f32,
    /// Run time at which the speed last fell into the dead zone.
    stopped_at: Option<f32>,
}

impl PlayerMove {
    /// At rest in the middle of a lane reaching `lane` metres to each side.
    pub(crate) fn new(lane: f32) -> Self {
        Self { x: 0.0, velocity: 0.0, lane, stopped_at: None }
    }

    pub(crate) fn x(&self) -> f32 {
        self.x
    }

    pub(crate) fn speed(&self) -> f32 {
        self.velocity.abs()
    }

    /// Whether the player is slow enough to shoot accurately.
    pub(crate) fn still(&self) -> bool {
        self.speed() <= DEAD_ZONE * RUN_SPEED
    }

    /// Run time of the latest stop (the speed falling into the dead zone); `None` before the
    /// first.
    pub(crate) fn stopped_at(&self) -> Option<f32> {
        self.stopped_at
    }

    /// Moves for `dt` seconds from run time `now` with the keys asking for `wish`: -1 left, 1
    /// right, 0 neither (or both).
    pub(crate) fn step(&mut self, wish: f32, dt: f32, now: f32) {
        let dead_zone = DEAD_ZONE * RUN_SPEED;
        let was_still = self.still();
        let mut left = dt;
        // At most three phases a frame: braking to rest, speeding up the other way, cruising.
        while left > 0.0 {
            let (target, rate) = if wish == 0.0 {
                (0.0, RUN_SPEED / COAST_STOP_SECS)
            } else if self.velocity * wish < 0.0 {
                (0.0, RUN_SPEED / COUNTER_STOP_SECS)
            } else {
                (wish * RUN_SPEED, RUN_SPEED / START_SECS)
            };
            let change = target - self.velocity;
            let phase = left.min(change.abs() / rate);
            let acceleration = rate * change.signum();
            let before = self.speed();
            self.x += self.velocity * phase + 0.5 * acceleration * phase * phase;
            // Exactly the target at the end of a phase, so the next one starts cleanly.
            self.velocity = if phase < left { target } else { self.velocity + acceleration * phase };
            if before > dead_zone && self.speed() <= dead_zone {
                self.stopped_at = Some(now + (dt - left) + (before - dead_zone) / rate);
            }
            left -= phase;
            if change == 0.0 {
                // Standing or cruising for the rest of the frame.
                self.x += self.velocity * left;
                break;
            }
        }
        if self.x.abs() >= self.lane {
            self.x = self.x.clamp(-self.lane, self.lane);
            if self.velocity * self.x > 0.0 {
                // Running into the end is a stop; leaning on it from rest isn't.
                if !was_still && !self.still() {
                    self.stopped_at = Some(now + dt);
                }
                self.velocity = 0.0;
            }
        }
    }
}

pub(super) fn register(app: &mut App) {
    app.add_systems(OnEnter(AppState::Playing), reset_eye)
        .add_systems(
            OnExit(AppState::Playing),
            (reset_eye, |mut commands: Commands| commands.remove_resource::<PlayerMove>()),
        )
        .add_systems(Update, strafe.in_set(DrillSet::Move).run_if(resource_exists::<PlayerMove>));
}

/// Every drill starts with the eye at the origin, and leaves it there for the menus and replays.
fn reset_eye(mut camera: Single<&mut Transform, With<PlayerCamera>>) {
    if camera.translation != Vec3::ZERO {
        camera.translation = Vec3::ZERO;
    }
}

/// A and D strafe once the run is live.
fn strafe(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    run: Res<Run>,
    mut player: ResMut<PlayerMove>,
    mut camera: Single<&mut Transform, With<PlayerCamera>>,
) {
    if !run.live() {
        return;
    }
    let wish = f32::from(keys.pressed(KeyCode::KeyD)) - f32::from(keys.pressed(KeyCode::KeyA));
    player.step(wish, time.delta_secs(), run.elapsed());
    if camera.translation.x != player.x() {
        camera.translation.x = player.x();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::PlayState;
    use crate::drill::{DrillKind, DrillSpec, RunPurpose};
    use crate::settings::Settings;

    /// Wide enough never to be reached in these tests.
    const OPEN: f32 = 1000.0;

    /// Steps `player` at `fps` with `wish` until `done` says so; the seconds that took.
    fn run_until(player: &mut PlayerMove, wish: f32, fps: f32, start: f32, done: impl Fn(&PlayerMove) -> bool) -> f32 {
        let dt = 1.0 / fps;
        let mut frames = 0;
        while !done(player) {
            player.step(wish, dt, start + frames as f32 * dt);
            frames += 1;
            assert!(frames < 10_000, "never got there: {player:?}");
        }
        frames as f32 * dt
    }

    /// A player running to the right at full speed.
    fn running() -> PlayerMove {
        PlayerMove { velocity: RUN_SPEED, ..PlayerMove::new(OPEN) }
    }

    #[test]
    fn reaches_run_speed_and_no_more() {
        let mut player = PlayerMove::new(OPEN);
        let secs = run_until(&mut player, 1.0, 1000.0, 0.0, |p| p.speed() >= RUN_SPEED);
        assert!((secs - START_SECS).abs() < 0.002, "{secs} s to full speed");
        for _ in 0..500 {
            player.step(1.0, 0.004, 0.0);
            assert_eq!(player.velocity, RUN_SPEED);
        }
    }

    #[test]
    fn counter_strafing_stops_sooner_than_letting_go() {
        let mut coasting = running();
        let coast = run_until(&mut coasting, 0.0, 1000.0, 0.0, |p| p.speed() == 0.0);
        assert!((coast - COAST_STOP_SECS).abs() < 0.002, "{coast} s");
        // Braking at a constant rate covers speed · time / 2.
        assert!((coasting.x() - RUN_SPEED * COAST_STOP_SECS * 0.5).abs() < 0.005, "{} m", coasting.x());
        // Standing still, it stays put.
        coasting.step(0.0, 0.5, 1.0);
        assert_eq!(coasting.speed(), 0.0);

        let mut countering = running();
        let counter = run_until(&mut countering, -1.0, 1000.0, 0.0, |p| p.velocity <= 0.0);
        assert!((counter - COUNTER_STOP_SECS).abs() < 0.002, "{counter} s");
        assert!(counter < coast);
        // Accurate (inside the dead zone) earlier still, at 70% of the way down.
        let mut accurate = running();
        let secs = run_until(&mut accurate, -1.0, 1000.0, 0.0, PlayerMove::still);
        assert!((secs - 0.7 * COUNTER_STOP_SECS).abs() < 0.002, "{secs} s");
        // Holding the opposite key on carries the player out of the dead zone the other way.
        let secs = run_until(&mut accurate, -1.0, 1000.0, secs, |p| !p.still());
        assert!(accurate.velocity < 0.0 && secs < 0.12, "{secs} s, {accurate:?}");
    }

    /// The same key presses give the same path at 60 and 500 fps, also across the kinks (braking
    /// done within a frame, the turn into the other direction, reaching top speed).
    #[test]
    fn movement_does_not_depend_on_the_frame_rate() {
        // (key, seconds): out, stop by counter-strafe, back, let go, a short tap.
        let script = [(1.0, 0.5), (-1.0, 0.1), (-1.0, 0.4), (0.0, 0.3), (1.0, 0.1), (0.0, 0.5)];
        let play = |fps: u32| {
            let mut player = PlayerMove::new(OPEN);
            let (dt, mut now, mut path) = (1.0 / fps as f32, 0.0, Vec::new());
            for (wish, secs) in script {
                for _ in 0..(secs * fps as f32).round() as u32 {
                    player.step(wish, dt, now);
                    now += dt;
                }
                path.push((player.x(), player.velocity, player.stopped_at()));
            }
            path
        };
        for (slow, fast) in play(60).into_iter().zip(play(500)) {
            assert!((slow.0 - fast.0).abs() < 1e-3, "position {} vs {}", slow.0, fast.0);
            assert!((slow.1 - fast.1).abs() < 1e-3, "velocity {} vs {}", slow.1, fast.1);
            match (slow.2, fast.2) {
                (Some(slow), Some(fast)) => assert!((slow - fast).abs() < 1e-3, "stopped at {slow} vs {fast}"),
                (slow, fast) => assert_eq!(slow, fast),
            }
        }
    }

    /// The stop is timed where the speed crosses into the dead zone, not at the end of the frame
    /// it happened in.
    #[test]
    fn stops_are_timed_within_the_frame() {
        let mut player = running();
        // One long frame of coasting: through the dead zone and on to rest.
        player.step(0.0, 0.5, 2.0);
        let expected = 2.0 + (1.0 - DEAD_ZONE) * COAST_STOP_SECS;
        assert!((player.stopped_at().unwrap() - expected).abs() < 1e-4, "{:?}", player.stopped_at());
        assert!(PlayerMove::new(OPEN).stopped_at().is_none());
    }

    #[test]
    fn the_lane_ends_stop_the_player() {
        let mut player = PlayerMove::new(2.0);
        for frame in 0..400 {
            player.step(1.0, 0.005, frame as f32 * 0.005);
            assert!(player.x() <= 2.0);
        }
        assert_eq!((player.x(), player.speed()), (2.0, 0.0));
        assert!(player.stopped_at().is_some(), "running into the end is a stop");
        // And lets it leave again.
        player.step(-1.0, 0.1, 2.0);
        assert!(player.x() < 2.0);
    }

    #[test]
    fn only_shots_above_the_dead_zone_are_inaccurate() {
        assert_eq!(error(0.0), 0.0);
        assert_eq!(error(DEAD_ZONE * RUN_SPEED), 0.0);
        assert!(error(DEAD_ZONE * RUN_SPEED + 0.5) > 0.0);
        assert!((error(RUN_SPEED) - RUN_ERROR).abs() < 1e-5);
        assert!(error(3.0) < error(4.0));
    }

    /// The eye follows the keys while the run is live and running, stands still while it is
    /// paused (whatever the keys do) and is back at the origin after the drill.
    #[test]
    fn the_eye_strafes_only_while_the_run_is_running() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(AppState::Playing)
            .add_sub_state::<PlayState>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(10)))
            .init_resource::<ButtonInput<KeyCode>>()
            .configure_sets(Update, DrillSet::Move.run_if(in_state(PlayState::Running)));
        register(&mut app);
        let drill = DrillSpec::builtin(DrillKind::CounterStrafe, &Settings::default());
        app.insert_resource(Run::new(drill, 0.4, 60.0, RunPurpose::Practice, String::new()).started())
            .insert_resource(PlayerMove::new(3.0));
        let camera = app.world_mut().spawn((PlayerCamera, Transform::IDENTITY)).id();
        let eye = |app: &App| app.world().get::<Transform>(camera).unwrap().translation;
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyD);
        for _ in 0..30 {
            app.update();
        }
        let out = eye(&app);
        assert!(out.x > 0.5 && out.y == 0.0 && out.z == 0.0, "{out}");

        app.world_mut().resource_mut::<NextState<PlayState>>().set(PlayState::Paused);
        app.update();
        let paused_at = eye(&app);
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(eye(&app), paused_at, "moved while paused");
        assert!(app.world().resource::<PlayerMove>().speed() > 0.0, "the pause took the speed away");

        app.world_mut().resource_mut::<NextState<PlayState>>().set(PlayState::Running);
        for _ in 0..3 {
            app.update();
        }
        assert!(eye(&app).x > paused_at.x, "did not go on after the pause");

        app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::Menu);
        app.update();
        assert_eq!(eye(&app), Vec3::ZERO);
        assert!(app.world().get_resource::<PlayerMove>().is_none());
    }
}
