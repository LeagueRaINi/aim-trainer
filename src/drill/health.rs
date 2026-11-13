//! Hit points for targets that take more than one hit (or a while of tracking) to destroy, and
//! the bar over each that shows how much is left.

use bevy::prelude::*;

use super::targets;
use crate::camera::PlayerCamera;

/// The bar's size in metres at the target (targets sit about 10 m away): a little wider than
/// most targets, and thin.
const BAR_SIZE: Vec2 = Vec2::new(0.9, 0.11);
/// The dark track shows this much around the fill, metres.
const BAR_BORDER: f32 = 0.018;
/// Gap between the top of the target and its bar, metres.
const BAR_GAP: f32 = 0.14;

/// A target with hit points. Whatever takes them off (clicks, time on target) destroys the target
/// once [`damage`](Self::damage) says none are left; a bar over the target shows them.
#[derive(Component)]
pub(crate) struct Health {
    left: f32,
    full: f32,
}

impl Health {
    pub(crate) fn new(full: f32) -> Self {
        Self { left: full, full }
    }

    /// Takes `amount` off; true once nothing is left.
    pub(crate) fn damage(&mut self, amount: f32) -> bool {
        self.left = (self.left - amount).max(0.0);
        self.left <= 0.0
    }

    /// What is left of the full amount, 0 to 1.
    pub(crate) fn fraction(&self) -> f32 {
        if self.full > 0.0 { self.left / self.full } else { 0.0 }
    }
}

/// The two quads of a health bar, children of their target.
#[derive(Component, Clone, Copy)]
enum BarPart {
    Track,
    Fill,
}

#[derive(Resource)]
struct BarAssets {
    /// A unit square facing +Z.
    mesh: Handle<Mesh>,
    track: Handle<StandardMaterial>,
    fill: Handle<StandardMaterial>,
}

pub(super) fn register(app: &mut App) {
    app.add_systems(Startup, create_bar_assets).add_systems(
        PostUpdate,
        // After the targets have their final place and rotation for the frame.
        (add_bars, place_bars).chain().after(targets::face_eye).before(TransformSystems::Propagate),
    );
}

fn create_bar_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let flat = |color: Color, alpha_mode: AlphaMode| StandardMaterial {
        base_color: color,
        unlit: true,
        alpha_mode,
        ..default()
    };
    commands.insert_resource(BarAssets {
        mesh: meshes.add(Rectangle::new(1.0, 1.0)),
        track: materials.add(flat(Color::srgba(0.03, 0.04, 0.06, 0.85), AlphaMode::Blend)),
        fill: materials.add(flat(Color::srgb(0.36, 0.86, 0.52), AlphaMode::Opaque)),
    });
}

/// Gives every target that just got [`Health`] its bar, hidden until [`place_bars`] sizes it.
fn add_bars(mut commands: Commands, assets: Res<BarAssets>, added: Query<Entity, Added<Health>>) {
    for target in &added {
        for (part, material) in [(BarPart::Track, &assets.track), (BarPart::Fill, &assets.fill)] {
            commands.spawn((
                ChildOf(target),
                part,
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_scale(Vec3::ZERO),
            ));
        }
    }
}

/// Keeps each bar above its target and parallel to the screen, at the same size whatever the
/// target's size, with the fill shrinking to the left as the health goes.
#[allow(clippy::type_complexity)]
fn place_bars(
    camera: Option<Single<&Transform, (With<PlayerCamera>, Without<BarPart>)>>,
    targets: Query<(&Transform, &Health, &Children), Without<BarPart>>,
    mut bars: Query<(&mut Transform, &BarPart)>,
) {
    // Parallel to the screen, like the HUD: the camera's own rotation (it looks along -Z, so the
    // quads' +Z faces it), not a turn towards the eye, which tilts a bar that is off-centre.
    let facing = camera.map_or(Quat::IDENTITY, |camera| camera.rotation);
    for (target, health, children) in &targets {
        let radius = target.scale.x.max(1e-4);
        // Straight above the target as seen on screen.
        let lift = facing * Vec3::Y * (radius + BAR_GAP + BAR_SIZE.y * 0.5);
        // The bars are children: undo the target's rotation and (uniform) scale.
        let to_local = target.rotation.inverse();
        let fraction = health.fraction();
        for &child in children {
            let Ok((mut bar, part)) = bars.get_mut(child) else { continue };
            let (offset, size) = match part {
                BarPart::Track => (Vec3::ZERO, BAR_SIZE),
                BarPart::Fill => {
                    let full = BAR_SIZE - 2.0 * BAR_BORDER;
                    // Anchored on the left, and a hair in front of the track.
                    (Vec3::new((fraction - 1.0) * full.x * 0.5, 0.0, 0.004), Vec2::new(fraction * full.x, full.y))
                },
            };
            let placed = Transform {
                translation: to_local * (lift + facing * offset) / radius,
                rotation: to_local * facing,
                scale: size.extend(1.0) / radius,
            };
            if *bar != placed {
                *bar = placed;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_empties_health() {
        let mut health = Health::new(0.5);
        assert!(!health.damage(0.2));
        assert!((health.fraction() - 0.6).abs() < 1e-6);
        assert!(health.damage(0.4));
        assert_eq!(health.fraction(), 0.0);
    }

    /// The bar sits straight above the target on screen and parallel to it, whatever the target's
    /// own rotation and size and wherever it is in the view: its world transform is the target's
    /// times the child's local one.
    #[test]
    fn bars_sit_above_the_target_parallel_to_the_screen() {
        let mut app = App::new();
        app.add_systems(Update, place_bars);
        let view = Quat::from_euler(EulerRot::YXZ, 0.5, 0.3, 0.0);
        app.world_mut().spawn((PlayerCamera, Transform::from_rotation(view)));
        let at = Vec3::new(6.0, 3.0, -8.0);
        let target =
            Transform::from_translation(at).with_rotation(targets::facing_eye(at)).with_scale(Vec3::splat(0.3));
        let mut health = Health::new(1.0);
        health.damage(0.25);
        let parent = app.world_mut().spawn((target, health)).id();
        let part =
            |app: &mut App, part: BarPart| app.world_mut().spawn((ChildOf(parent), part, Transform::IDENTITY)).id();
        let (track, fill) = (part(&mut app, BarPart::Track), part(&mut app, BarPart::Fill));
        app.update();
        let world_of = |app: &App, bar: Entity| target.mul_transform(*app.world().get::<Transform>(bar).unwrap());
        let (track, fill) = (world_of(&app, track), world_of(&app, fill));
        // Above the target's top along the screen's up, centred on it.
        let above = view.inverse() * (track.translation - at);
        assert!(above.x.abs() < 1e-4 && above.z.abs() < 1e-4 && above.y > 0.3, "{above}");
        // Turned exactly like the camera, at its own size.
        assert!(track.rotation.angle_between(view) < 1e-3, "{:?}", track.rotation);
        assert!((track.scale.truncate() - BAR_SIZE).length() < 1e-4, "{}", track.scale);
        // Three quarters full, flush with the track's left inside edge.
        let full = BAR_SIZE.x - 2.0 * BAR_BORDER;
        assert!((fill.scale.x - 0.75 * full).abs() < 1e-4, "{}", fill.scale);
        let left_edge =
            |bar: &Transform| (bar.translation - track.translation).dot(track.rotation * Vec3::X) - bar.scale.x * 0.5;
        assert!((left_edge(&fill) - (left_edge(&track) + BAR_BORDER)).abs() < 1e-4);
    }
}
