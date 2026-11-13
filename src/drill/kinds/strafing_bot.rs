//! Strafing bot: one bot at a time strafes left and right like a player, at walking to running
//! speed, and stops now and then as if to shoot. Tap its head: the click has to be timed on a
//! head that moves, or saved for the moment it stands.
//!
//! The bot moves like [`tracking`](super::tracking)'s dot, with inertia, plus the stops. Its
//! speeds are Valorant's with a rifle (5.4 m/s running, about half that walking); how a real
//! player strafes is of course not a formula.

use bevy::prelude::*;

use crate::camera::ViewCone;
use crate::drill::bot::spawn_bot;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom};
use crate::drill::geometry::{random_range, random_sign};
use crate::drill::movement::RUN_SPEED;
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::SpawnCtx;
use crate::drill::{DrillConfig, DrillSet, Run};

/// How fast the bot changes its speed, m/s²: from a run to a standstill in 0.2 s and into a run
/// the other way in 0.4 s. A player stops a little quicker still (see
/// [`movement`](crate::drill::movement)); this leaves the turn readable.
const ACCEL: f32 = RUN_SPEED / 0.2;
/// Distances (m) a bot strafes at.
const DEPTH: (f32, f32) = (7.0, 12.0);
/// Pause (s) between a kill and the next bot.
const GAP: (f32, f32) = (0.3, 0.6);
/// How often a change of mind is a stop rather than a turn.
const STOP_CHANCE: f32 = 0.4;
/// Degrees a head keeps from the screen's edges when it appears, beyond its own size.
const EDGE_ROOM: f32 = 1.5;
/// Random places tried for a new bot before it is put straight ahead of the crosshair.
const TRIES: usize = 16;

pub const DEF: DrillDef = DrillDef {
    name: "Strafing bot",
    blurb: |_| {
        "A bot strafes and stops like a player: time your tap on its head. Speeds and rifle approximate Valorant."
            .into()
    },
    category: Category::Tactical,
    base_radius: 0.14,
    decay: DecayRule::None,
    score: ScoreModel::Taps,
    ttk_from: TtkFrom::Spawn,
    click_kills: false,
    environment: Environment::Range,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    endless: false,
    params: &[
        ParamId::StrafeMin,
        ParamId::StrafeMax,
        ParamId::TurnMin,
        ParamId::TurnMax,
        ParamId::HoldMin,
        ParamId::HoldMax,
        ParamId::Lane,
    ],
    tune_defaults: tune,
    spawn_initial: spawn_strafer,
    respawn,
    spawn_next: spawn_strafer,
};

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, strafe.in_set(DrillSet::Move));
}

/// Walking (half the run speed) to running, a change of mind every 0.4 to 1.2 s, stops of 0.3 to
/// 0.7 s, in a lane 8 m wide.
fn tune(config: &mut DrillConfig) {
    (config.strafe_min, config.strafe_max) = (RUN_SPEED * 0.5, RUN_SPEED);
    (config.turn_min, config.turn_max) = (0.4, 1.2);
    (config.hold_min, config.hold_max) = (0.3, 0.7);
    config.lane = 4.0;
}

/// A bot's strafing: like `tracking`'s `Strafer`, and it also stops.
#[derive(Component, Debug)]
struct Strafe {
    /// Metres per second along the lane.
    velocity: f32,
    /// The velocity it is accelerating to; 0 while it stops.
    wanted: f32,
    /// Seconds until it changes its mind.
    change_in: f32,
}

impl Strafe {
    /// At rest, about to set off either way.
    fn new(config: &DrillConfig) -> Self {
        let mut strafe = Self { velocity: 0.0, wanted: 0.0, change_in: 0.0 };
        strafe.go(random_sign(), config);
        strafe
    }

    /// Moves `x` (metres along the lane) by `dt` seconds: now and then a new random speed the
    /// other way or a stop, reached at [`ACCEL`], and never past the lane's edges.
    fn step(&mut self, x: &mut f32, dt: f32, config: &DrillConfig) {
        self.change_in -= dt;
        if self.change_in <= 0.0 {
            self.change_mind(config);
        }
        let step = ACCEL * dt;
        let mut velocity = self.velocity + (self.wanted - self.velocity).clamp(-step, step);
        // Not past the point where it could still stop at the edge ahead: braking takes
        // velocity² / 2·ACCEL metres after this frame's travel. It turns back there instead.
        let ahead = velocity.signum();
        let room = config.lane - *x * ahead;
        if self.wanted * ahead > 0.0 && room < velocity * velocity / (2.0 * ACCEL) + velocity.abs() * dt {
            self.go(-ahead, config);
            velocity = self.velocity + (self.wanted - self.velocity).clamp(-step, step);
        }
        self.velocity = velocity;
        // The clamp only matters for a lane narrowed mid-run.
        *x = (*x + velocity * dt).clamp(-config.lane, config.lane);
    }

    /// A stop (from a strafe, some of the time), or a strafe: the other way after a strafe,
    /// either way after a stop.
    fn change_mind(&mut self, config: &DrillConfig) {
        if self.wanted != 0.0 && fastrand::f32() < STOP_CHANCE {
            self.wanted = 0.0;
            // The hold is the time it stands, after braking.
            self.change_in = self.velocity.abs() / ACCEL + random_range(config.hold_min, config.hold_max);
        } else {
            let direction = if self.wanted == 0.0 { random_sign() } else { -self.wanted.signum() };
            self.go(direction, config);
        }
    }

    /// A new random speed in `direction`, held for a new while.
    fn go(&mut self, direction: f32, config: &DrillConfig) {
        self.wanted = direction * random_range(config.strafe_min, config.strafe_max);
        self.change_in = random_range(config.turn_min, config.turn_max);
    }
}

/// The next bot comes after a short pause.
fn respawn(ctx: &mut SpawnCtx, _: Option<usize>) {
    ctx.next.0 = Some(ctx.now + random_range(GAP.0, GAP.1));
}

fn spawn_strafer(ctx: &mut SpawnCtx) {
    let head = start(ctx.aim, &ctx.view, ctx.radius(), ctx.config);
    let strafe = Strafe::new(ctx.config);
    spawn_bot(ctx, head).insert(strafe);
}

/// Where a new bot's head appears: at a random distance, somewhere in the lane that is on
/// screen; if the player looks past the lane, at the point of it nearest the crosshair.
fn start(aim: Vec3, view: &ViewCone, radius: f32, config: &DrillConfig) -> Vec3 {
    let depth = random_range(DEPTH.0, DEPTH.1);
    let margin = (radius / depth).atan() + EDGE_ROOM.to_radians();
    let at = |x: f32| Vec3::new(x, 0.0, -depth);
    for _ in 0..TRIES {
        let head = at(random_range(-config.lane, config.lane));
        if view.contains(aim, head.normalize(), margin) {
            return head;
        }
    }
    let ahead = if aim.z < -1e-3 { aim.x / -aim.z * depth } else { config.lane.copysign(aim.x) };
    at(ahead.clamp(-config.lane, config.lane))
}

/// Bots strafe once the run is live. Only their place along the lane changes: the head stays at
/// eye height and the feet on the deck.
fn strafe(time: Res<Time>, run: Res<Run>, mut bots: Query<(&mut Strafe, &mut Transform)>) {
    if !run.live() {
        return;
    }
    let dt = time.delta_secs();
    for (mut strafe, mut transform) in &mut bots {
        strafe.step(&mut transform.translation.x, dt, &run.drill.config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::DrillKind;
    use crate::drill::geometry::direction;

    /// A minute of strafing at `fps`: the velocity never changes faster than [`ACCEL`], the bot
    /// stays in its lane without being stopped by its edge, strafes both ways between walking
    /// and running speed, and stands still now and then.
    fn strafes_like_a_player(config: &DrillConfig, fps: f32) {
        let dt = 1.0 / fps;
        for seed in 0..20 {
            fastrand::seed(seed);
            let mut strafe = Strafe::new(config);
            let (mut x, mut fastest, mut turns) = (0.0f32, (0.0f32, 0.0f32), 0);
            let (mut standing, mut stops) = (0.0, 0);
            for frame in 0..(60.0 * fps) as u32 {
                let before = strafe.velocity;
                strafe.step(&mut x, dt, config);
                let change = (strafe.velocity - before).abs();
                assert!(change <= ACCEL * dt * 1.001, "seed {seed}, frame {frame}: velocity jumped by {change} m/s");
                assert!(x.abs() < config.lane, "seed {seed}, frame {frame}: at {x} m, ran into the lane's edge");
                assert!(strafe.velocity.abs() <= config.strafe_max * 1.001, "seed {seed}: {} m/s", strafe.velocity);
                turns += u32::from(before * strafe.velocity < 0.0);
                stops += u32::from(before != 0.0 && strafe.velocity == 0.0);
                standing += if strafe.velocity == 0.0 { dt } else { 0.0 };
                fastest = (fastest.0.min(strafe.velocity), fastest.1.max(strafe.velocity));
            }
            assert!(turns >= 10, "seed {seed}: only {turns} direction changes in a minute");
            assert!(fastest.0 <= -config.strafe_min && fastest.1 >= config.strafe_min, "seed {seed}: {fastest:?}");
            assert!(stops >= 5, "seed {seed}: stopped only {stops} times in a minute");
            // Each stop is held for at least the shortest hold, give or take a frame (the last
            // one may be cut short by the end of the minute).
            let held = (stops - 1) as f32 * (config.hold_min - 2.0 * dt);
            assert!(standing >= held, "seed {seed}: stood {standing} s over {stops} stops");
        }
    }

    #[test]
    fn the_bot_strafes_with_inertia_and_stops() {
        let defaults = DrillConfig::defaults(DrillKind::StrafingBot);
        for fps in [30.0, 60.0, 178.0, 500.0] {
            strafes_like_a_player(&defaults, fps);
        }
    }

    /// New bots stand on the deck with the head at eye height, inside the lane and on screen
    /// when the player looks anywhere near it.
    #[test]
    fn bots_appear_on_screen_with_their_heads_at_eye_height() {
        let (config, view, radius) =
            (DrillConfig::defaults(DrillKind::StrafingBot), ViewCone::default(), DEF.base_radius);
        for aim in [Vec3::NEG_Z, direction(0.5, 0.1), direction(-0.7, -0.15), direction(1.0, 0.0)] {
            for seed in 0..40 {
                fastrand::seed(seed);
                let head = start(aim, &view, radius, &config);
                assert_eq!(head.y, 0.0);
                assert!(head.x.abs() <= config.lane && (DEPTH.0..=DEPTH.1).contains(&-head.z), "{head}");
                let seen = view.contains(aim, head.normalize(), (radius / head.length()).atan());
                assert!(seen, "aim {aim}: head at {head} off screen");
            }
        }
    }
}
