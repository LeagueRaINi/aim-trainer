//! The tap rifle the bot drills are shot with, and what a run with it is made of ([`TapStats`]).
//!
//! The first shot from rest goes exactly where the crosshair points. Every shot heats the rifle;
//! the heat is gone [`RECOVERY_SECS`] after the latest shot, and a shot fired before that lands
//! off the crosshair by a random amount that grows with the heat. Holding the button fires at
//! the rifle's own rate, which piles the heat up: tap, and wait for the rifle to settle.
//!
//! This approximates a Valorant Vandal; it is not the game's recoil. Riot doesn't publish its
//! recoil tables, and their climb-then-sway pattern is not modelled at all: an unsettled shot is
//! simply scattered. What is taken from the game's listed stats: the fire rate (9.75 rounds/s)
//! and the recovery time (0.375 s). Left out on purpose: the 0.25° spread the game gives even a
//! first shot, so a miss from rest is always the player's aim.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::bot::{self, Bot, Zone};
use super::def::TtkFrom;
use super::health::Health;
use super::movement::{self, PlayerMove};
use super::targets::{self, NextSpawn, Slot, SpawnCtx, Target, TargetAssets};
use super::{DrillKind, DrillSet, Run, range};
use crate::AppState;
use crate::camera::{PlayerCamera, ViewCone};
use crate::replay::Recorder;
use crate::settings::Settings;
use crate::sfx::HitSounds;

/// Seconds between the shots of a held trigger.
pub(crate) const FIRE_INTERVAL: f32 = 1.0 / 9.75;
/// Seconds after the latest shot until the rifle is accurate again.
pub(crate) const RECOVERY_SECS: f32 = 0.375;
/// How far off the crosshair (degrees) a shot may land per unit of heat: fired right after
/// another (0.73 heat left) up to 1.5°, which misses a head 10 m away (0.8° from its centre to
/// its edge) two times in three; tapped again after 0.25 s, at most 0.7°.
const ERROR_PER_HEAT: f32 = 2.0;
/// The heat stops piling up here: a long spray scatters up to 6°, like running.
const MAX_HEAT: f32 = 3.0;
/// The smallest share of the shots (or first shots) worth calling a leak.
const LEAK_SHARE: f32 = 0.2;
/// The share of first shots on the head below which they are the leak.
const FIRST_SHOT_GOAL: f32 = 0.5;

/// The rifle's state: when it last fired and how hot that left it.
#[derive(Debug)]
pub(crate) struct Rifle {
    /// Run time of the latest shot.
    last_shot: f32,
    /// Heat right after it: one per shot on top of what was left.
    heat: f32,
    /// Whether the button was down in the previous frame.
    held: bool,
}

impl Default for Rifle {
    fn default() -> Self {
        Self { last_shot: f32::NEG_INFINITY, heat: 0.0, held: false }
    }
}

impl Rifle {
    /// The heat left at run time `now`: it falls linearly to nothing over [`RECOVERY_SECS`].
    pub(crate) fn heat(&self, now: f32) -> f32 {
        self.heat * (1.0 - (now - self.last_shot) / RECOVERY_SECS).clamp(0.0, 1.0)
    }

    /// The most a shot fired at `now` may land off the crosshair because of the heat, degrees.
    pub(crate) fn error(&self, now: f32) -> f32 {
        ERROR_PER_HEAT * self.heat(now)
    }

    /// Whether a shot is fired in the frame at run time `now`, and at which run time: a new pull
    /// fires right away, a trigger held since the previous frame on the rifle's own clock (so a
    /// held burst heats up the same at any frame rate). Nothing fires sooner than
    /// [`FIRE_INTERVAL`] after the latest shot.
    pub(crate) fn trigger(&mut self, pressed: bool, just_pressed: bool, now: f32) -> Option<f32> {
        let ready_at = self.last_shot + FIRE_INTERVAL;
        let held = std::mem::replace(&mut self.held, pressed) && pressed && !just_pressed;
        ((pressed || just_pressed) && now >= ready_at).then_some(if held { ready_at } else { now })
    }

    /// Fires at run time `at`; returns the heat the shot was fired with (0: settled).
    pub(crate) fn fire(&mut self, at: f32) -> f32 {
        let heat = self.heat(at);
        self.heat = (heat + 1.0).min(MAX_HEAT);
        self.last_shot = at;
        heat
    }
}

/// The most the second shot of a held burst may land off the crosshair, degrees: what "not
/// settled" costs right after a shot.
pub fn burst_error() -> f32 {
    ERROR_PER_HEAT * (1.0 - FIRE_INTERVAL / RECOVERY_SECS)
}

/// The direction of a shot that may land up to `error` degrees off the view direction of the
/// camera turned by `view`: uniformly spread over that cone, picked by `along` and `around`
/// (both uniform in 0..1). Without error it is the view direction itself, to the bit.
pub(crate) fn scatter(view: Quat, error: f32, along: f32, around: f32) -> Vec3 {
    if error <= 0.0 {
        return view * Vec3::NEG_Z;
    }
    // Uniform over the cone's cross-section: the square root spreads the angle by area.
    let off = error.to_radians() * along.sqrt();
    let (sin, cos) = (around * TAU).sin_cos();
    view * (Quat::from_axis_angle(Vec3::new(cos, sin, 0.0), off) * Vec3::NEG_Z)
}

/// A shot, as [`TapStats`] counts it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ShotReport {
    pub(crate) hit: Option<Zone>,
    pub(crate) killed: bool,
    /// The first shot at a bot: none fired since it appeared, or since the kill before.
    pub(crate) first: bool,
    /// The heat the shot was fired with.
    pub(crate) heat: f32,
    /// The player's speed (m/s) in drills with movement.
    pub(crate) speed: Option<f32>,
    /// Seconds since the player stopped, for the first shot after each stop.
    pub(crate) since_stop: Option<f32>,
}

/// What the shots of a bot-drill run add up to, beyond the kills and shots of
/// [`RunStats`](super::RunStats): for the results breakdown.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TapStats {
    pub shots: u32,
    pub kills: u32,
    pub head_hits: u32,
    pub body_hits: u32,
    /// First shots at a bot, and how many of them hit its head.
    pub first_shots: u32,
    pub first_heads: u32,
    /// Shots fired before the rifle had settled.
    pub unsettled: u32,
    /// In drills with movement.
    pub movement: Option<MoveStats>,
}

/// What the player's movement was at the shots of a run.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoveStats {
    /// Shots fired faster than the dead zone allows.
    pub moving: u32,
    /// Sum of the speeds at every shot, m/s.
    pub speed_total: f32,
    /// Stops followed by a shot, and the seconds from each stop to that shot, summed.
    pub stops: u32,
    pub stop_delay_total: f32,
}

/// The habit that cost a run the most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leak {
    /// Shooting while still moving.
    Moving,
    /// Shooting again before the rifle has settled.
    Unsettled,
    /// First shots that miss the head.
    FirstShots,
    /// Nothing stands out.
    Clean,
}

impl TapStats {
    fn record(&mut self, shot: ShotReport) {
        self.shots += 1;
        self.kills += u32::from(shot.killed);
        self.head_hits += u32::from(shot.hit == Some(Zone::Head));
        self.body_hits += u32::from(shot.hit == Some(Zone::Body));
        self.first_shots += u32::from(shot.first);
        self.first_heads += u32::from(shot.first && shot.hit == Some(Zone::Head));
        self.unsettled += u32::from(shot.heat > 0.0);
        if let Some(speed) = shot.speed {
            let movement = self.movement.get_or_insert_default();
            movement.moving += u32::from(speed > movement::DEAD_ZONE * movement::RUN_SPEED);
            movement.speed_total += speed;
            if let Some(delay) = shot.since_stop {
                movement.stops += 1;
                movement.stop_delay_total += delay;
            }
        }
    }

    /// Share of the hits that were on the head; `None` without hits.
    pub fn headshot_share(&self) -> Option<f32> {
        let hits = self.head_hits + self.body_hits;
        (hits > 0).then(|| self.head_hits as f32 / hits as f32)
    }

    /// Shots fired per kill (1 = every kill a single head tap); `None` without kills.
    pub fn shots_per_kill(&self) -> Option<f32> {
        (self.kills > 0).then(|| self.shots as f32 / self.kills as f32)
    }

    /// Share of the first shots that hit the head; `None` without first shots.
    pub fn first_shot_share(&self) -> Option<f32> {
        (self.first_shots > 0).then(|| self.first_heads as f32 / self.first_shots as f32)
    }

    /// The biggest leak; `None` for a run without shots. Discipline comes before aim: shots
    /// fired while moving or before the rifle settled (whichever were more, if at least
    /// [`LEAK_SHARE`] of the shots) scatter whatever the aim was, so they are named before first
    /// shots that miss the head (the leak below [`FIRST_SHOT_GOAL`]).
    pub fn leak(&self) -> Option<Leak> {
        if self.shots == 0 {
            return None;
        }
        let moving = self.movement.map_or(0, |movement| movement.moving);
        let share = |count: u32| count as f32 / self.shots as f32;
        Some(if moving >= self.unsettled && share(moving) >= LEAK_SHARE {
            Leak::Moving
        } else if share(self.unsettled) >= LEAK_SHARE {
            Leak::Unsettled
        } else if self.first_shot_share().is_some_and(|share| share < FIRST_SHOT_GOAL) {
            Leak::FirstShots
        } else {
            Leak::Clean
        })
    }
}

impl MoveStats {
    /// Average speed at the shot, m/s, over a run's `shots`.
    pub fn speed_at_shot(&self, shots: u32) -> Option<f32> {
        (shots > 0).then(|| self.speed_total / shots as f32)
    }

    /// Average seconds from stopping to the first shot after it; `None` if no shot followed a
    /// stop.
    pub fn stop_to_shot(&self) -> Option<f32> {
        (self.stops > 0).then(|| self.stop_delay_total / self.stops as f32)
    }
}

/// The rifle and tap statistics of the bot drill being played. Stays after the run, for its
/// results; replaced or removed when the next run starts.
#[derive(Resource)]
pub struct TapRun {
    /// The drill the statistics are of.
    pub kind: DrillKind,
    pub stats: TapStats,
    rifle: Rifle,
    /// The stop whose first shot has been counted.
    counted_stop: Option<f32>,
}

impl TapRun {
    /// A run that already has its statistics, for staged results screens.
    pub fn with_stats(kind: DrillKind, stats: TapStats) -> Self {
        Self { kind, stats, rifle: Rifle::default(), counted_stop: None }
    }

    /// The most a shot fired at run time `now` may land off the crosshair, degrees: the rifle's
    /// heat plus the player's movement. 0 means it goes where the crosshair points.
    pub fn error(&self, now: f32, player: Option<&PlayerMove>) -> f32 {
        self.rifle.error(now) + player.map_or(0.0, |player| movement::error(player.speed()))
    }
}

pub(super) fn register(app: &mut App) {
    app.add_systems(OnEnter(AppState::Playing), arm)
        .add_systems(Update, fire.in_set(DrillSet::Input).run_if(resource_exists::<TapRun>));
}

/// A fresh rifle for a drill on the range; none for any other drill.
fn arm(mut commands: Commands, run: Res<Run>) {
    if range::played_on(run.kind()) {
        commands.insert_resource(TapRun::with_stats(run.kind(), TapStats::default()));
    } else {
        commands.remove_resource::<TapRun>();
    }
}

/// Fires the rifle: one shot per click, or a burst while the button is held. A shot hits the
/// nearest bot on its (possibly scattered) line unless the scenery is in the way; the head kills,
/// the body wears the bot's health down. A kill is handled like a click kill in
/// [`targets::shoot`]: timed, with the hit sound, and replaced through the drill's `respawn`.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn fire(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    mut run: ResMut<Run>,
    mut taps: ResMut<TapRun>,
    player: Option<Res<PlayerMove>>,
    assets: Res<TargetAssets>,
    sounds: Res<HitSounds>,
    settings: Res<Settings>,
    mut recorder: ResMut<Recorder>,
    mut next: ResMut<NextSpawn>,
    view: Res<ViewCone>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Bot>)>,
    mut bots: Query<(Entity, &Target, &Transform, &mut Health, Option<&Slot>), With<Bot>>,
) {
    if !run.live() {
        return;
    }
    let now = run.elapsed();
    let (pressed, just_pressed) = (mouse.pressed(MouseButton::Left), mouse.just_pressed(MouseButton::Left));
    let Some(fired_at) = taps.rifle.trigger(pressed, just_pressed, now) else { return };
    let heat = taps.rifle.fire(fired_at);
    let player = player.as_deref();
    let speed = player.map(PlayerMove::speed);
    let error = ERROR_PER_HEAT * heat + speed.map_or(0.0, movement::error);
    let (origin, aim) = (camera.translation, camera.forward().as_vec3());
    let shot = scatter(camera.rotation, error, fastrand::f32(), fastrand::f32());
    let def = run.kind().def();
    let blocked_at = targets::blocked_at(def.environment, origin, shot);
    let hit = bots
        .iter()
        .filter_map(|(entity, target, transform, ..)| {
            let (distance, zone) = bot::hit(origin, shot, transform.translation, transform.scale.x)?;
            Some((distance, zone, entity, target.born))
        })
        .filter(|hit| hit.0 < blocked_at)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    // A miss was aimed at the bot nearest the crosshair.
    let aimed_born = hit.map(|hit| hit.3).or_else(|| {
        let off = |position: Vec3| aim.angle_between(position - origin);
        bots.iter().min_by(|a, b| off(a.2.translation).total_cmp(&off(b.2.translation))).map(|bot| bot.1.born)
    });
    let first = aimed_born.is_some_and(|born| first_shot(run.last_shot, born, run.last_kill));
    // The first shot after a stop tells how long the player took to shoot once stopped.
    let since_stop = player
        .filter(|player| player.still())
        .and_then(PlayerMove::stopped_at)
        .filter(|stop| taps.counted_stop.replace(*stop) != Some(*stop))
        .map(|stop| now - stop);
    run.shot();
    recorder.mark_shot(hit.is_some());

    let mut killed = None;
    if let Some((_, zone, entity, born)) = hit
        && let Ok((.., mut health, slot)) = bots.get_mut(entity)
        && health.damage(zone.damage())
    {
        killed = Some((entity, born, slot.map(|slot| slot.0)));
    }
    if run.counting() {
        let report = ShotReport { hit: hit.map(|hit| hit.1), killed: killed.is_some(), first, heat, speed, since_stop };
        taps.stats.record(report);
    }
    let Some((entity, born, slot)) = killed else { return };

    // Bots that wait while the others are shot are timed from the previous kill.
    let ttk = if def.ttk_from == TtkFrom::PreviousKill { now - run.last_kill } else { now - born };
    run.kill(ttk);
    commands.entity(entity).despawn();
    sounds.play(&mut commands, &settings.audio);
    let mut occupied: Vec<usize> = bots.iter().filter_map(|bot| bot.4.map(|slot| slot.0)).collect();
    let mut ctx = SpawnCtx {
        commands: &mut commands,
        assets: &assets,
        config: &run.drill.config,
        now,
        aim,
        view: *view,
        live: true,
        occupied: &mut occupied,
        next: &mut next,
    };
    (def.respawn)(&mut ctx, slot);
}

/// Whether a shot is the first at its bot: none was fired since the bot appeared at `born` or,
/// if that was later, since the kill at `last_kill`. `last_shot` is the run time of the shot
/// before; one in the very frame the bot appeared or the kill was made came before either.
fn first_shot(last_shot: f32, born: f32, last_kill: f32) -> bool {
    last_shot <= born.max(last_kill)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::{DrillSpec, RunPurpose};

    #[test]
    fn the_first_shot_from_rest_goes_where_the_crosshair_points() {
        let mut rifle = Rifle::default();
        assert_eq!(rifle.error(0.0), 0.0);
        assert_eq!(rifle.fire(5.0), 0.0);
        // Whatever the random draw: exactly the view direction, to the bit.
        let view = Quat::from_euler(EulerRot::YXZ, 0.7, -0.2, 0.0);
        for (along, around) in [(0.0, 0.0), (1.0, 0.25), (0.6, 0.9)] {
            assert_eq!(scatter(view, 0.0, along, around), view * Vec3::NEG_Z);
        }
    }

    #[test]
    fn shots_before_the_rifle_has_settled_scatter_more_and_more() {
        let mut rifle = Rifle::default();
        // A held trigger: every shot is fired with more heat than the one before, up to a limit.
        let heats: Vec<f32> = (0..12).map(|shot| rifle.fire(shot as f32 * FIRE_INTERVAL)).collect();
        assert_eq!(heats[0], 0.0);
        assert!(heats.windows(2).take(4).all(|pair| pair[1] > pair[0] + 0.05), "{heats:?}");
        assert!(heats.iter().all(|heat| *heat < MAX_HEAT), "{heats:?}");
        assert!(rifle.error(11.0 * FIRE_INTERVAL) <= ERROR_PER_HEAT * MAX_HEAT);
        // The scatter stays within the error, reaches its edge, and goes every way.
        let view = Quat::from_euler(EulerRot::YXZ, -1.1, 0.3, 0.0);
        let aim = view * Vec3::NEG_Z;
        let error = 1.5f32;
        let (mut widest, mut sum) = (0.0f32, Vec3::ZERO);
        for step in 0..400 {
            let (along, around) = ((step % 20) as f32 / 19.0, (step / 20) as f32 / 20.0);
            let shot = scatter(view, error, along, around);
            let off = shot.angle_between(aim).to_degrees();
            assert!(off <= error * 1.001, "{off}° off with an error of {error}°");
            widest = widest.max(off);
            sum += shot - aim;
        }
        assert!(widest > error * 0.999, "never further than {widest}°");
        let sideways = sum - aim * sum.dot(aim);
        assert!(sideways.length() < 1e-3, "the scatter leans: {sideways}");
        // A full draw lands exactly on the cone's edge, sideways for a quarter turn around.
        let right = scatter(Quat::IDENTITY, 2.0, 1.0, 0.25);
        assert!((right - Vec3::new(-(2f32.to_radians().sin()), 0.0, -(2f32.to_radians().cos()))).length() < 1e-5);
    }

    #[test]
    fn the_rifle_is_settled_again_after_the_recovery_time() {
        let mut rifle = Rifle::default();
        rifle.fire(1.0);
        assert!((rifle.heat(1.0) - 1.0).abs() < 1e-6);
        assert!((rifle.heat(1.0 + RECOVERY_SECS * 0.5) - 0.5).abs() < 1e-5, "the heat falls linearly");
        assert!(rifle.heat(1.0 + RECOVERY_SECS - 0.01) > 0.0);
        assert_eq!(rifle.heat(1.0 + RECOVERY_SECS), 0.0);
        // A tap spaced by the recovery time is a first shot again; after a burst too.
        assert_eq!(rifle.fire(1.0 + RECOVERY_SECS), 0.0);
        for shot in 0..8 {
            rifle.fire(2.0 + shot as f32 * FIRE_INTERVAL);
        }
        let last = 2.0 + 7.0 * FIRE_INTERVAL;
        assert!(rifle.heat(last + 0.2) > 1.0);
        // (A hair later than the recovery time: the burst's times aren't round numbers.)
        assert_eq!(rifle.fire(last + RECOVERY_SECS + 1e-4), 0.0);
    }

    /// The heat every shot is fired with when the button goes down and up at the given run times,
    /// played at `fps`.
    fn play(fps: f32, presses: &[(f32, f32)]) -> Vec<(f32, f32)> {
        let mut rifle = Rifle::default();
        let mut shots = Vec::new();
        let mut was_down = false;
        for frame in 0..(3.0 * fps) as u32 {
            let now = frame as f32 / fps;
            let down = presses.iter().any(|&(from, to)| (from..to).contains(&now));
            if let Some(at) = rifle.trigger(down, down && !was_down, now) {
                shots.push((at, rifle.fire(at)));
            }
            was_down = down;
        }
        shots
    }

    /// Taps and a held burst heat the rifle the same at 60 and 500 fps: a held trigger fires on
    /// the rifle's clock, not the frames'. (A new pull fires in the frame it is seen in, so the
    /// taps here start on frames both rates have.)
    #[test]
    fn the_rifle_behaves_the_same_at_60_and_500_fps() {
        // A tap, a second one 0.2 s later (unsettled), then a burst held for 0.55 s.
        let presses = [(0.1, 0.15), (0.3, 0.35), (1.0, 1.55)];
        let (slow, fast) = (play(60.0, &presses), play(500.0, &presses));
        assert_eq!(slow.len(), fast.len(), "{slow:?} vs {fast:?}");
        assert_eq!(slow.len(), 2 + 6, "0.55 s of holding fires at 0, 0.10, 0.21, 0.31, 0.41 and 0.51 s");
        for (slow, fast) in slow.iter().zip(&fast) {
            assert!((slow.0 - fast.0).abs() < 1e-4, "fired at {} vs {}", slow.0, fast.0);
            assert!((slow.1 - fast.1).abs() < 1e-3, "heat {} vs {}", slow.1, fast.1);
        }
        assert_eq!(slow[0].1, 0.0);
        assert!((slow[1].1 - (1.0 - 0.2 / RECOVERY_SECS)).abs() < 1e-3, "{}", slow[1].1);
        assert_eq!(slow[2].1, 0.0, "settled again before the burst");
        assert!((slow[3].0 - slow[2].0 - FIRE_INTERVAL).abs() < 1e-5);
    }

    #[test]
    fn the_trigger_fires_no_faster_than_the_rifle() {
        let mut rifle = Rifle::default();
        assert_eq!(rifle.trigger(false, false, 0.0), None);
        // A click that is down and up again within one frame still fires.
        assert_eq!(rifle.trigger(false, true, 1.0), Some(1.0));
        rifle.fire(1.0);
        // Clicking again at once does nothing...
        assert_eq!(rifle.trigger(true, true, 1.02), None);
        assert_eq!(rifle.trigger(false, false, 1.04), None);
        // ...but a button still down when the rifle is ready fires then.
        assert_eq!(rifle.trigger(true, true, 1.06), None);
        let at = rifle.trigger(true, false, 1.11).unwrap();
        assert!((at - (1.0 + FIRE_INTERVAL)).abs() < 1e-6, "{at}");
    }

    #[test]
    fn first_shots_are_the_ones_after_a_bot_appears_or_a_kill() {
        // Nothing fired yet at a bot that was there from the start.
        assert!(first_shot(f32::NEG_INFINITY, 0.0, 0.0));
        // A miss at 1 s: the next shot is a correction.
        assert!(!first_shot(1.0, 0.0, 0.0));
        // The kill at 2 s was the latest shot: the next one starts on another bot.
        assert!(first_shot(2.0, 0.0, 2.0));
        // A stray shot at 2.5 s into thin air, then a bot appears at 3 s.
        assert!(first_shot(2.5, 3.0, 2.0));
        assert!(!first_shot(3.2, 3.0, 2.0));
    }

    fn shot(hit: Option<Zone>, first: bool, heat: f32) -> ShotReport {
        ShotReport { hit, first, heat, ..default() }
    }

    #[test]
    fn tap_stats_add_up() {
        let mut stats = TapStats::default();
        assert_eq!((stats.headshot_share(), stats.shots_per_kill(), stats.first_shot_share()), (None, None, None));
        // A head tap; then a miss, a body hit and the head; then four body shots in a burst.
        stats.record(ShotReport { killed: true, ..shot(Some(Zone::Head), true, 0.0) });
        stats.record(shot(None, true, 0.0));
        stats.record(shot(Some(Zone::Body), false, 0.4));
        stats.record(ShotReport { killed: true, ..shot(Some(Zone::Head), false, 0.0) });
        for burst in 0..4 {
            stats.record(ShotReport { killed: burst == 3, ..shot(Some(Zone::Body), burst == 0, burst as f32) });
        }
        assert_eq!((stats.shots, stats.kills, stats.head_hits, stats.body_hits), (8, 3, 2, 5));
        assert_eq!((stats.first_shots, stats.first_heads, stats.unsettled), (3, 1, 4));
        assert!((stats.headshot_share().unwrap() - 2.0 / 7.0).abs() < 1e-6);
        assert!((stats.shots_per_kill().unwrap() - 8.0 / 3.0).abs() < 1e-6);
        assert!((stats.first_shot_share().unwrap() - 1.0 / 3.0).abs() < 1e-6);
        assert_eq!(stats.movement, None);

        // With movement: one shot at a run, one 0.2 s after stopping, one more standing.
        let dead_zone = movement::DEAD_ZONE * movement::RUN_SPEED;
        let mut moved = TapStats::default();
        moved.record(ShotReport { speed: Some(5.0), ..default() });
        moved.record(ShotReport { speed: Some(dead_zone), since_stop: Some(0.2), ..default() });
        moved.record(ShotReport { speed: Some(0.0), ..default() });
        let movement = moved.movement.unwrap();
        assert_eq!((movement.moving, movement.stops), (1, 1));
        assert!((movement.speed_at_shot(moved.shots).unwrap() - (5.0 + dead_zone) / 3.0).abs() < 1e-5);
        assert!((movement.stop_to_shot().unwrap() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn the_leak_is_the_habit_that_cost_the_most() {
        let stats = |shots, unsettled, moving, first_shots, first_heads| TapStats {
            shots,
            unsettled,
            first_shots,
            first_heads,
            movement: Some(MoveStats { moving, ..default() }),
            ..default()
        };
        assert_eq!(TapStats::default().leak(), None);
        // 7 of 20 shots while moving, 4 unsettled: moving.
        assert_eq!(stats(20, 4, 7, 10, 8).leak(), Some(Leak::Moving));
        assert_eq!(stats(20, 9, 7, 10, 8).leak(), Some(Leak::Unsettled));
        // A few of either are not a leak; then the first shots are, if most miss the head.
        assert_eq!(stats(20, 3, 3, 10, 4).leak(), Some(Leak::FirstShots));
        assert_eq!(stats(20, 3, 3, 10, 5).leak(), Some(Leak::Clean));
        assert_eq!(stats(20, 4, 0, 10, 2).leak(), Some(Leak::Unsettled), "discipline before aim");
        // Without movement only the rifle and the first shots count.
        let standing = TapStats { movement: None, ..stats(20, 2, 0, 12, 3) };
        assert_eq!(standing.leak(), Some(Leak::FirstShots));
    }

    /// The whole shot in a running drill: a click with the crosshair on the head kills the bot,
    /// four on the body do too, a click beside it misses, and each is counted once.
    #[test]
    fn clicks_kill_bots_by_the_head_or_four_body_shots() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Settings::default())
            .insert_resource(ViewCone::default())
            .insert_resource(NextSpawn::default())
            .insert_resource(HitSounds::default())
            .insert_resource(Recorder::default())
            .init_resource::<ButtonInput<MouseButton>>()
            .insert_resource(TargetAssets {
                mesh: default(),
                idle: default(),
                hot: default(),
                wait: default(),
                cover_mesh: default(),
                cover_material: default(),
            })
            .insert_resource(TapRun::with_stats(DrillKind::StrafingBot, TapStats::default()))
            // Shots half a second of run time apart, so every one is fired settled.
            .add_systems(Update, (fire, |mut run: ResMut<Run>| run.elapsed += 0.5).chain());
        let drill = DrillSpec::builtin(DrillKind::StrafingBot, &Settings::default());
        app.insert_resource(Run::new(drill, 0.4, 60.0, RunPurpose::Practice, String::new()).started());
        let camera = app.world_mut().spawn((PlayerCamera, Transform::IDENTITY)).id();
        let bot = |app: &mut App, x: f32| {
            let target = Target { base_radius: 0.14, born: 0.0, lifetime: None, spawn_aim: None };
            let at = Transform::from_xyz(x, 0.0, -10.0).with_scale(Vec3::splat(0.14));
            app.world_mut().spawn((Bot, target, at, Health::new(bot::BODY_SHOTS))).id()
        };
        let (left, right) = (bot(&mut app, -3.0), bot(&mut app, 3.0));
        let click_at = |app: &mut App, point: Vec3| {
            *app.world_mut().get_mut::<Transform>(camera).unwrap() = Transform::IDENTITY.looking_at(point, Vec3::Y);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            app.update();
            // No input plugin here to do this between frames.
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        };
        click_at(&mut app, Vec3::new(-3.0, 0.05, -10.0));
        assert!(app.world().get_entity(left).is_err(), "a head shot did not kill");
        click_at(&mut app, Vec3::new(0.0, 0.0, -10.0));
        for body_shot in 1..=4 {
            assert!(app.world().get_entity(right).is_ok(), "dead before body shot {body_shot}");
            click_at(&mut app, Vec3::new(3.0, -0.5, -10.0));
        }
        assert!(app.world().get_entity(right).is_err(), "four body shots did not kill");
        let world = app.world();
        let stats = world.resource::<TapRun>().stats;
        assert_eq!((stats.shots, stats.kills, stats.head_hits, stats.body_hits), (6, 2, 1, 4));
        assert_eq!((stats.first_shots, stats.first_heads, stats.unsettled), (2, 1, 0));
        let run = world.resource::<Run>().live_stats(&Settings::default());
        assert_eq!((run.hits, run.shots), (2, 6));
    }
}
