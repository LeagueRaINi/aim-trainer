//! Reaction: click the instant the dot lights up. Handles its own clicks; an unlit cue is
//! [`Dormant`].

use bevy::prelude::*;

use crate::camera::PlayerCamera;
use crate::drill::def::{Category, DecayRule, DrillDef, Environment, TtkFrom, no_respawn, no_spawn_next, no_tuning};
use crate::drill::geometry::{TARGET_DISTANCE, random_range, ray_hit};
use crate::drill::params::ParamId;
use crate::drill::stats::ScoreModel;
use crate::drill::targets::{Dormant, SpawnCtx, Target, TargetAssets};
use crate::drill::{DrillSet, Run};
use crate::replay::Recorder;
use crate::settings::Settings;
use crate::sfx::HitSounds;

/// How long a lit reaction cue waits for a click before it counts as missed.
const REACTION_WINDOW: f32 = 1.0;

pub const DEF: DrillDef = DrillDef {
    name: "Reaction",
    blurb: "Click the instant the dot lights up.",
    category: Category::Eyes,
    base_radius: 0.5,
    decay: DecayRule::None,
    score: ScoreModel::Reaction,
    ttk_from: TtkFrom::Spawn,
    click_kills: false,
    environment: Environment::Open,
    first_flick_probe: false,
    classic_finder: false,
    flick_analysis: false,
    adaptive: false,
    params: &[ParamId::DelayMin, ParamId::DelayMax],
    tune_defaults: no_tuning,
    spawn_initial,
    respawn: no_respawn,
    spawn_next: no_spawn_next,
};

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, reaction.in_set(DrillSet::Input));
}

#[derive(Component)]
struct ReactionCue {
    lit_at: f32,
    lit: bool,
}

/// One cue straight ahead, unlit until a random delay has passed.
fn spawn_initial(ctx: &mut SpawnCtx) {
    let lit_at = random_range(ctx.config.delay_min, ctx.config.delay_max);
    let wait = ctx.assets.wait.clone();
    ctx.spawn_target(Vec3::new(0.0, 0.0, -TARGET_DISTANCE), None).insert((
        ReactionCue { lit_at, lit: false },
        Dormant,
        MeshMaterial3d(wait),
    ));
}

/// The cue lights up after a random delay; click as fast as possible. Clicking before it lights
/// up, or not at all within the window, counts as a miss.
#[allow(clippy::too_many_arguments)]
fn reaction(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    mut run: ResMut<Run>,
    assets: Res<TargetAssets>,
    sounds: Res<HitSounds>,
    settings: Res<Settings>,
    mut recorder: ResMut<Recorder>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<ReactionCue>)>,
    mut cues: Query<(Entity, &mut ReactionCue, &mut Target, &Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    if !run.live() {
        return;
    }
    let now = run.elapsed;
    let (delay_min, delay_max) = (run.drill.config.delay_min, run.drill.config.delay_max);
    let clicked = mouse.just_pressed(MouseButton::Left);
    for (entity, mut cue, mut target, transform, mut material) in &mut cues {
        let just_lit = !cue.lit && now >= cue.lit_at;
        if just_lit {
            cue.lit = true;
            target.born = now;
            material.0 = assets.idle.clone();
        }
        let done = if clicked {
            run.shot();
            let aim = camera.forward().as_vec3();
            let aimed = ray_hit(camera.translation, aim, transform.translation, transform.scale.x).is_some();
            let hit = reaction_hit(cue.lit, just_lit, aimed);
            recorder.mark_shot(hit);
            if hit {
                run.kill(now - target.born);
                sounds.play(&mut commands, &settings.audio);
            }
            true
        } else if cue.lit && now - target.born > REACTION_WINDOW {
            run.expire();
            true
        } else {
            false
        };
        if done {
            cue.lit = false;
            cue.lit_at = now + random_range(delay_min, delay_max);
            material.0 = assets.wait.clone();
        }
        // Dormant mirrors "not lit"; only touched when that changes this frame.
        if just_lit != done {
            if cue.lit {
                commands.entity(entity).remove::<Dormant>();
            } else {
                commands.entity(entity).insert(Dormant);
            }
        }
    }
}

/// A reaction click hits only if the cue was already visible: a click in the frame it lights up
/// was made before it could be seen, so it counts as early (a miss), never a 0 ms reaction.
fn reaction_hit(lit: bool, just_lit: bool, aimed: bool) -> bool {
    lit && !just_lit && aimed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_in_the_frame_the_cue_lights_is_early() {
        assert!(!reaction_hit(true, true, true));
        assert!(reaction_hit(true, false, true));
        assert!(!reaction_hit(false, false, true));
        assert!(!reaction_hit(true, false, false));
    }
}
