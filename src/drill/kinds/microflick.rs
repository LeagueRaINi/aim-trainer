//! Microflick: one small dot at a time, anywhere in the flick area (or, with flick constraints,
//! in a trained direction and distance). Measures each target's first flick for the smart finder.
//! Head level reuses its spawning.

use bevy::prelude::*;

use crate::AppState;
use crate::camera::{PlayerCamera, ViewCone};
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_spawn_next, no_tuning};
use crate::drill::geometry::{
    CEILING_PITCH, EDGE_SLACK, FLOOR_PITCH, TARGET_DISTANCE, away_from, bisect, direction, pull_in, random_pitch,
    random_range,
};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{SpawnCtx, Target, TargetExpired, TargetKilled};
use crate::drill::{DrillConfig, DrillSet, Run};
use crate::metrics::flick_probe::{FlickProbe, angles, screen_offset, wrap};

/// Attempts at finding a constrained flick target before falling back to a reset target.
const CONSTRAINED_TRIES: usize = 32;

pub const DEF: DrillDef = DrillDef {
    name: "Microflick",
    blurb: "One small dot at a time. Flick precision.",
    category: Category::Aim,
    base_radius: 0.2,
    decay: DecayRule::Global,
    score: ScoreModel::Flick,
    ttk_from: TtkFrom::Spawn,
    click_kills: true,
    environment: Environment::Open,
    first_flick_probe: true,
    classic_finder: true,
    flick_analysis: true,
    adaptive: true,
    params: &[ParamId::YawRange, ParamId::PitchRange, ParamId::MinFlick],
    tune_defaults: no_tuning,
    spawn_initial: spawn_flick,
    respawn: respawn_flick,
    spawn_next: no_spawn_next,
};

pub(super) fn register(app: &mut App) {
    app.init_resource::<ActiveProbe>()
        .add_systems(OnEnter(AppState::Playing), |mut probe: ResMut<ActiveProbe>| probe.0 = None)
        .add_systems(Update, probe_flicks.in_set(DrillSet::Measure));
}

/// One flick target, away from the aim.
pub(super) fn spawn_flick(ctx: &mut SpawnCtx) {
    let (position, lifetime) = (flick_position(ctx.aim, &ctx.view, ctx.margin(), ctx.config), ctx.lifetime());
    ctx.spawn_target(position, lifetime);
}

pub(super) fn respawn_flick(ctx: &mut SpawnCtx, _: Option<usize>) {
    spawn_flick(ctx);
}

/// Microflick / Head level: anywhere within the yaw/pitch ranges (between floor and ceiling), away
/// from the crosshair and on screen. Head level has a pitch range of 0, so its targets always sit
/// exactly at head height.
pub(super) fn flick_position(aim: Vec3, view: &ViewCone, margin: f32, config: &DrillConfig) -> Vec3 {
    if config.flick_constrained() {
        return constrained_flick_position(aim, view, margin, config);
    }
    away_from(aim, config.min_flick, view, margin, || {
        direction(random_range(-config.yaw_range, config.yaw_range).to_radians(), random_pitch(config.pitch_range))
    })
}

/// The (yaw, pitch) area constrained flick targets stay in: the yaw/pitch ranges, between floor
/// and ceiling.
fn flick_area(config: &DrillConfig) -> (Vec2, Vec2) {
    let min = Vec2::new(-config.yaw_range, (-config.pitch_range).max(FLOOR_PITCH));
    (min, Vec2::new(config.yaw_range, config.pitch_range.min(CEILING_PITCH).max(min.y)))
}

/// Microflick aimed at a weak spot: `min_flick..max_flick` degrees from the aim, towards
/// `flick_direction +- flick_spread`, inside [`flick_area`] and on screen. A flick longer than the
/// view shows in its direction goes as far as it does. Repeated flicks the same way walk the aim
/// to the edge of the area; once no such target fits, the next one is a reset target on the
/// opposite side of the area, which leaves room for the trained flicks again. If the area is too
/// small for that to be a real flick either, the target leaves the area, as far as the view and
/// the room allow: along the trained direction or back if either is as long as a trained flick
/// could be, else sideways or diagonally (the floor and ceiling can cut both short).
fn constrained_flick_position(aim: Vec3, view: &ViewCone, margin: f32, config: &DrillConfig) -> Vec3 {
    let from = angles(aim);
    let (near, far) = config.flick_distance_range();
    let (min, max) = flick_area(config);
    let to_direction = |p: Vec2| direction(p.x.to_radians(), p.y.to_radians());
    // How far (up to `far`) a target can sit from the aim towards on-screen `angle` and still be
    // on screen (and, `in_room`, between floor and ceiling).
    let reach = |angle: f32, in_room: bool| {
        far * bisect(|t| {
            let p = from + screen_offset(angle, far * t);
            (!in_room || (FLOOR_PITCH..=CEILING_PITCH).contains(&p.y))
                && view.contains(aim, to_direction(p), margin + EDGE_SLACK)
        })
    };
    for _ in 0..CONSTRAINED_TRIES {
        let angle = match config.flick_direction {
            Some(center) => center + random_range(-config.flick_spread, config.flick_spread),
            None => random_range(0.0, 360.0),
        };
        let shown = reach(angle, false);
        let candidate = from + screen_offset(angle, random_range(near.min(shown), far.min(shown)));
        if candidate.cmpge(min).all()
            && candidate.cmple(max).all()
            && view.contains(aim, to_direction(candidate), margin)
        {
            return to_direction(candidate) * TARGET_DISTANCE;
        }
    }
    let middle = (min + max) * 0.5;
    let (reset, center) = match config.flick_direction {
        // As far back as the area allows, for the longest run of trained flicks.
        Some(center) => (middle - screen_offset(center, far), center),
        None => (middle, random_range(0.0, 360.0)),
    };
    let reset = pull_in(aim, to_direction(reset.clamp(min, max)), view, margin);
    // The reset is a real flick too: as long as a trained one could be from here.
    let need = near.min(reach(center, false));
    if wrap(angles(reset) - from).length() >= need {
        return reset * TARGET_DISTANCE;
    }
    let ways = [0.0, 180.0, 45.0, -45.0, 135.0, -135.0, 90.0, -90.0].map(|turn| {
        let angle = center + turn;
        (angle, reach(angle, true))
    });
    let (angle, distance) =
        ways.iter().find(|way| way.1 >= need).copied().unwrap_or_else(|| {
            ways.into_iter().fold((center, 0.0), |best, way| if way.1 > best.1 { way } else { best })
        });
    to_direction(from + screen_offset(angle, distance)) * TARGET_DISTANCE
}

/// Measures the current target's first flick.
#[derive(Resource, Default)]
struct ActiveProbe(Option<FlickProbe>);

/// Measures where the first flick towards each new target lands. A kill this frame finishes the
/// previous target's probe at the shot's aim; an expired target drops it.
fn probe_flicks(
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut probe: ResMut<ActiveProbe>,
    mut killed: MessageReader<TargetKilled>,
    mut expired: MessageReader<TargetExpired>,
    camera: Single<&Transform, With<PlayerCamera>>,
    spawned: Query<&Transform, Added<Target>>,
) {
    // Always drain, so a later drill never sees another drill's messages.
    let kill_aim = killed.read().last().map(|kill| kill.aim);
    let any_expired = expired.read().count() > 0;
    if !run.kind().def().first_flick_probe {
        return;
    }
    if let Some(aim) = kill_aim
        && let Some(finished) = probe.0.take()
    {
        run.land(finished.landing(angles(aim)));
    }
    if any_expired {
        probe.0 = None;
    }
    let aim = angles(camera.forward().as_vec3());
    if let Some(target) = spawned.iter().last() {
        probe.0 = FlickProbe::new(aim, angles((target.translation - camera.translation).normalize_or_zero()));
    } else if run.live()
        && let Some(landing) = probe.0.as_mut().and_then(|probe| probe.update(aim, time.delta_secs()))
    {
        probe.0 = None;
        run.land(landing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::DrillKind;
    use crate::metrics::flick_probe::screen_angle;

    /// A Microflick config trained on down-left flicks of 15-25 degrees.
    fn down_left() -> DrillConfig {
        let mut config = DrillConfig {
            kind: DrillKind::Microflick,
            yaw_range: 40.0,
            pitch_range: 25.0,
            min_flick: 15.0,
            max_flick: 25.0,
            flick_direction: Some(225.0),
            flick_spread: 22.5,
            ..DrillConfig::defaults(DrillKind::Microflick)
        };
        config.sanitize();
        config
    }

    /// Whether the flick from `from` to `to` (both view directions) follows the constraints.
    fn follows(config: &DrillConfig, from: Vec3, to: Vec3) -> bool {
        let offset = wrap(angles(to.normalize()) - angles(from));
        let distance = offset.length();
        let angle = screen_angle(offset);
        let center = config.flick_direction.unwrap();
        let off_center = (angle - center + 180.0).rem_euclid(360.0) - 180.0;
        let (near, far) = config.flick_distance_range();
        off_center.abs() <= config.flick_spread + 1e-2 && distance >= near - 1e-2 && distance <= far + 1e-2
    }

    #[test]
    fn constrained_flicks_spawn_only_in_the_trained_sector_and_distance() {
        let config = down_left();
        for aim in [Vec3::NEG_Z, direction(-0.2, 0.15)] {
            for _ in 0..2000 {
                assert!(follows(&config, aim, flick_position(aim, &view(), MARGIN, &config)), "aim {aim}");
            }
        }
    }

    #[test]
    fn repeated_constrained_flicks_reset_and_stay_in_the_area() {
        let config = down_left();
        let (min, max) = flick_area(&config);
        let mut aim = Vec3::NEG_Z;
        let mut trained = 0;
        const SPAWNS: usize = 2000;
        for _ in 0..SPAWNS {
            let target = flick_position(aim, &view(), MARGIN, &config);
            let at = angles(target.normalize());
            assert!(at.cmpge(min - 1e-3).all() && at.cmple(max + 1e-3).all(), "{at} outside the area");
            trained += follows(&config, aim, target) as usize;
            aim = target.normalize();
        }
        // Mostly trained flicks, with a reset flick every few targets.
        assert!(trained as f32 / SPAWNS as f32 > 0.6, "{trained} of {SPAWNS}");
    }

    #[test]
    fn default_microflick_is_unconstrained() {
        let config = DrillConfig::defaults(DrillKind::Microflick);
        assert!(!config.flick_constrained());
        // Targets still land on both sides, as before.
        let sides: Vec<bool> =
            (0..200).map(|_| flick_position(Vec3::NEG_Z, &view(), MARGIN, &config).x > 0.0).collect();
        assert!(sides.contains(&true) && sides.contains(&false));
    }

    /// Valorant's view (103° at 16:9) and a small target's margin.
    fn view() -> ViewCone {
        ViewCone::default()
    }
    const MARGIN: f32 = 0.04;

    #[test]
    fn targets_always_spawn_on_screen() {
        let view = view();
        // Plain Microflick and a long-flick weak spot (40-60°, beyond the 51.5° screen edge),
        // aiming straight ahead and from far off to the side.
        let mut long_left = down_left();
        (long_left.yaw_range, long_left.min_flick, long_left.max_flick, long_left.flick_direction) =
            (80.0, 40.0, 60.0, Some(180.0));
        long_left.sanitize();
        for config in [DrillConfig::defaults(DrillKind::Microflick), long_left] {
            for aim in [Vec3::NEG_Z, direction(0.45, 0.1), direction(-0.5, -0.15)] {
                for _ in 0..1000 {
                    let target = flick_position(aim, &view, MARGIN, &config).normalize();
                    assert!(view.contains(aim, target, MARGIN), "aim {aim}: {target} off screen");
                }
            }
        }
    }

    /// Up (90) or down (270) flicks of 40-60°: longer than the 33° the view shows vertically.
    fn long_vertical(towards: f32) -> DrillConfig {
        let mut config = DrillConfig {
            kind: DrillKind::Microflick,
            yaw_range: 40.0,
            pitch_range: 25.0,
            min_flick: 40.0,
            max_flick: 60.0,
            flick_direction: Some(towards),
            flick_spread: 22.5,
            ..DrillConfig::defaults(DrillKind::Microflick)
        };
        config.sanitize();
        config
    }

    #[test]
    fn flicks_longer_than_the_view_still_need_a_flick() {
        let view = view();
        let to_direction = |p: Vec2| direction(p.x.to_radians(), p.y.to_radians());
        for towards in [90.0, 270.0] {
            let config = long_vertical(towards);
            let starts = [Vec3::NEG_Z, direction(0.4, -0.2), direction(-0.6, 0.35), direction(0.1, 0.5)];
            for start in starts {
                let mut aim = start;
                for _ in 0..200 {
                    let target = flick_position(aim, &view, MARGIN, &config).normalize();
                    let from = angles(aim);
                    // The farthest the view shows in the trained direction.
                    let shown = 60.0
                        * bisect(|t| view.contains(aim, to_direction(from + screen_offset(towards, 60.0 * t)), MARGIN));
                    let distance = wrap(angles(target) - from).length();
                    assert!(view.contains(aim, target, MARGIN), "{towards}° from {from}: {target} off screen");
                    assert!(
                        distance >= config.min_flick.min(shown) - 0.05,
                        "{towards}° from {from}: only {distance}° away, {shown}° shown"
                    );
                    let pitch = angles(target).y;
                    assert!((FLOOR_PITCH - 1e-3..=CEILING_PITCH + 1e-3).contains(&pitch), "{towards}°: pitch {pitch}");
                    aim = target;
                }
            }
        }
    }

    #[test]
    fn unconstrained_targets_stay_above_the_floor() {
        let mut config = DrillConfig { pitch_range: 30.0, ..DrillConfig::defaults(DrillKind::Microflick) };
        config.sanitize();
        // Straight ahead, and looking 34° down at the floor.
        for aim in [Vec3::NEG_Z, direction(0.3, -0.6)] {
            for _ in 0..500 {
                let target = flick_position(aim, &view(), MARGIN, &config).normalize();
                assert!(angles(target).y >= FLOOR_PITCH - 1e-3, "aim {aim}: {target} below the floor");
                assert!(view().contains(aim, target, MARGIN), "aim {aim}: {target} off screen");
            }
        }
    }
}
