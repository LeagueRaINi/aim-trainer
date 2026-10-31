//! Microflick: one small dot at a time, anywhere in the flick area (or, with flick constraints,
//! in a trained direction and distance). Measures each target's first flick for the smart finder.
//! Head level reuses its spawning.

use bevy::prelude::*;

use crate::AppState;
use crate::camera::{PlayerCamera, ViewCone};
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_spawn_next, no_tuning};
use crate::drill::geometry::{TARGET_DISTANCE, away_from, direction, random_range};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{SpawnCtx, Target, TargetExpired, TargetKilled};
use crate::drill::{DrillConfig, DrillSet, Run};
use crate::metrics::flick_probe::{FlickProbe, angles, screen_offset};

/// Lowest pitch (degrees) a constrained flick target may sit at: lower ones (at
/// `TARGET_DISTANCE`) would sink into the floor 2.5 m below the eye.
const FLOOR_PITCH: f32 = -13.0;
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

/// Microflick / Head level: anywhere within the yaw/pitch ranges, away from the crosshair and on
/// screen. Head level has a pitch range of 0, so its targets always sit exactly at head height.
pub(super) fn flick_position(aim: Vec3, view: &ViewCone, margin: f32, config: &DrillConfig) -> Vec3 {
    if config.flick_constrained() {
        return constrained_flick_position(aim, view, margin, config);
    }
    away_from(aim, config.min_flick, view, margin, || {
        direction(
            random_range(-config.yaw_range, config.yaw_range).to_radians(),
            random_range(-config.pitch_range, config.pitch_range).to_radians(),
        )
    })
}

/// The (yaw, pitch) area constrained flick targets stay in: the yaw/pitch ranges, above the floor.
fn flick_area(config: &DrillConfig) -> (Vec2, Vec2) {
    let min = Vec2::new(-config.yaw_range, (-config.pitch_range).max(FLOOR_PITCH));
    (min, Vec2::new(config.yaw_range, config.pitch_range.max(min.y)))
}

/// Microflick aimed at a weak spot: `min_flick..max_flick` degrees from the aim, towards
/// `flick_direction +- flick_spread` on screen, inside [`flick_area`]. Repeated flicks the same
/// way walk the aim to the edge of the area; once no such target fits, the next one is a reset
/// target on the opposite side of the area, which leaves room for the trained flicks again.
fn constrained_flick_position(aim: Vec3, view: &ViewCone, margin: f32, config: &DrillConfig) -> Vec3 {
    let from = angles(aim);
    let (near, far) = config.flick_distance_range();
    let (min, max) = flick_area(config);
    let to_direction = |p: Vec2| direction(p.x.to_radians(), p.y.to_radians());
    for _ in 0..CONSTRAINED_TRIES {
        let angle = match config.flick_direction {
            Some(center) => center + random_range(-config.flick_spread, config.flick_spread),
            None => random_range(0.0, 360.0),
        };
        let candidate = from + screen_offset(angle, random_range(near, far));
        if candidate.cmpge(min).all()
            && candidate.cmple(max).all()
            && view.contains(aim, to_direction(candidate), margin)
        {
            return to_direction(candidate) * TARGET_DISTANCE;
        }
    }
    let middle = (min + max) * 0.5;
    let reset = match config.flick_direction {
        // As far back as the area allows, for the longest run of trained flicks.
        Some(center) => middle - screen_offset(center, far),
        None => middle,
    };
    view.pull_in(aim, to_direction(reset.clamp(min, max)), margin) * TARGET_DISTANCE
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
    use crate::metrics::flick_probe::{screen_angle, wrap};

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
}
