//! Video settings: window mode, Valorant-style 16:9 lock, vsync, MSAA and an FPS cap.
//!
//! Both fullscreen modes use a borderless window covering the monitor. winit's exclusive
//! fullscreen on Windows forces the window topmost (`WS_EX_TOPMOST`) and switches display
//! modes, so the game stayed on top after alt-tab and appeared to hang. "Fullscreen" here
//! instead minimizes itself when it loses focus, which is how exclusive fullscreen behaves in
//! games; "Borderless" stays where it is.

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use bevy::camera::Viewport;
use bevy::prelude::*;
use bevy::render::view::Msaa;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowFocused, WindowMode};

use crate::camera::PlayerCamera;
use crate::settings::{AspectMode, DisplayMode, Settings, VideoSettings};

const SIXTEEN_NINE: f32 = 16.0 / 9.0;
/// Menus and HUD are laid out for this many logical pixels of height and scaled from there.
const UI_DESIGN_HEIGHT: f32 = 720.0;
/// Smallest UI scale, so menus still fit on short (high-DPI) screens without getting unreadable.
const MIN_UI_SCALE: f32 = 0.7;
/// Window size when switching to windowed mode.
const WINDOWED_SIZE: (u32, u32) = (1280, 720);

pub struct DisplayPlugin;

impl Plugin for DisplayPlugin {
    fn build(&self, app: &mut App) {
        let cap = FrameCap::default();
        app.insert_resource(cap.clone())
            .add_systems(Startup, spawn_bar_camera)
            .add_systems(Update, (apply_video, minimize_on_focus_loss, fit_viewport))
            .add_systems(Last, sync_frame_cap.run_if(resource_changed::<Settings>));
        // No render app (e.g. a headless run): nothing to pace.
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(cap).add_systems(Render, limit_fps.in_set(RenderSystems::PostCleanup));
        }
    }
}

/// The primary window as configured, so the game starts directly in the saved mode instead of
/// switching modes (and grabbing focus) after the window is already up.
pub fn primary_window(video: &VideoSettings) -> Window {
    Window {
        title: "Aim Trainer".into(),
        // There is no "current" monitor before the window exists, so start on the primary one.
        mode: window_mode(video.display, MonitorSelection::Primary),
        present_mode: present_mode(video.vsync),
        // One queued frame instead of the default two: less input latency.
        desired_maximum_frame_latency: NonZero::new(1),
        ..default()
    }
}

fn window_mode(display: DisplayMode, monitor: MonitorSelection) -> WindowMode {
    match display {
        DisplayMode::Windowed => WindowMode::Windowed,
        DisplayMode::Borderless | DisplayMode::Fullscreen => WindowMode::BorderlessFullscreen(monitor),
    }
}

fn present_mode(vsync: bool) -> PresentMode {
    if vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync }
}

/// Clears the whole window to black underneath the (possibly smaller) game viewport,
/// which produces the black bars.
#[derive(Component)]
struct BarCamera;

fn spawn_bar_camera(mut commands: Commands) {
    commands.spawn((
        BarCamera,
        Camera2d,
        Camera { order: -1, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
        Msaa::Off,
    ));
}

/// Applies video settings when they change. Compares against what was last applied rather than
/// the live `Window`, so unrelated settings edits never touch the window.
fn apply_video(
    settings: Res<Settings>,
    mut applied: Local<Option<VideoSettings>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    camera: Single<Entity, With<PlayerCamera>>,
    mut commands: Commands,
) {
    let video = &settings.video;
    if applied.as_ref() == Some(video) {
        return;
    }
    // Compare only windowed vs. fullscreen; the monitor it was created on is fine as is.
    let fullscreen = video.display != DisplayMode::Windowed;
    if fullscreen != matches!(window.mode, WindowMode::BorderlessFullscreen(_)) {
        window.mode = window_mode(video.display, MonitorSelection::Current);
        if !fullscreen {
            // Borderless windows are created without a size; give the window a proper one.
            window.resolution.set(WINDOWED_SIZE.0 as f32, WINDOWED_SIZE.1 as f32);
            window.position = WindowPosition::Centered(MonitorSelection::Current);
        }
    }
    let present_mode = present_mode(video.vsync);
    if window.present_mode != present_mode {
        window.present_mode = present_mode;
    }
    let msaa = match video.msaa {
        8 => Msaa::Sample8,
        4 => Msaa::Sample4,
        2 => Msaa::Sample2,
        _ => Msaa::Off,
    };
    commands.entity(*camera).insert(msaa);
    *applied = Some(video.clone());
}

fn minimize_on_focus_loss(
    mut focus: MessageReader<WindowFocused>,
    settings: Res<Settings>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let lost_focus = focus.read().fold(false, |_, event| !event.focused);
    if lost_focus && settings.video.display == DisplayMode::Fullscreen {
        window.set_minimized(true);
    }
}

/// Largest centered rectangle of `aspect` that fits in `size`, or `None` if it would cover all of it.
fn fitted_rect(size: UVec2, aspect: f32) -> Option<(UVec2, UVec2)> {
    let (w, h) = (size.x as f32, size.y as f32);
    let fitted = if w / h > aspect {
        UVec2::new((h * aspect).round() as u32, size.y)
    } else {
        UVec2::new(size.x, (w / aspect).round() as u32)
    };
    (fitted != size).then(|| ((size - fitted) / 2, fitted))
}

fn fit_viewport(
    settings: Res<Settings>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<&mut Camera, (With<PlayerCamera>, Without<BarCamera>)>,
    mut bar_camera: Single<&mut Camera, With<BarCamera>>,
    mut ui_scale: ResMut<UiScale>,
) {
    let size = window.physical_size();
    if size.x == 0 || size.y == 0 {
        return; // minimised
    }
    let wanted = match settings.video.aspect {
        AspectMode::Native => None,
        AspectMode::Lock16x9 => fitted_rect(size, SIXTEEN_NINE),
    };
    let current = camera.viewport.as_ref().map(|v| (v.physical_position, v.physical_size));
    if current != wanted {
        camera.viewport =
            wanted.map(|(physical_position, physical_size)| Viewport { physical_position, physical_size, ..default() });
    }
    // The bar camera is a whole extra render pass: only run it when there are bars to draw.
    let bars = wanted.is_some();
    if bar_camera.is_active != bars {
        bar_camera.is_active = bars;
    }

    // Keep menus the same relative size at 1440p/4K, and shrink them (down to a limit) when
    // the logical viewport is shorter than the design height; the UI size setting then shrinks
    // them further (the layout already fills the design height, so it can't grow).
    let viewport_height = wanted.map_or(size.y, |(_, fitted)| fitted.y) as f32;
    let fit = (viewport_height / window.scale_factor() / UI_DESIGN_HEIGHT).max(MIN_UI_SCALE);
    let scale = fit * settings.video.ui_scale;
    if (ui_scale.0 - scale).abs() > 1e-3 {
        ui_scale.0 = scale;
    }
}

/// FPS cap shared with the render world, so the limiter can run after the frame is presented.
#[derive(Resource, Clone, Default)]
struct FrameCap(Arc<AtomicU32>);

fn sync_frame_cap(settings: Res<Settings>, cap: Res<FrameCap>) {
    cap.0.store(settings.video.fps_cap, Ordering::Relaxed);
}

/// Sleeps right after presenting to hold the configured frame rate, so the next frame starts
/// (and reads input) as late as possible: sleeping between input and render would add the
/// whole wait to input latency. Sleeps coarsely, then spins for the last stretch, since OS
/// sleep granularity is too rough for high caps.
fn limit_fps(cap: Res<FrameCap>, mut deadline: Local<Option<Instant>>) {
    let cap = cap.0.load(Ordering::Relaxed);
    if cap == 0 {
        *deadline = None;
        return;
    }
    let period = Duration::from_secs_f64(1.0 / cap as f64);
    let now = Instant::now();
    // If we fell behind, restart the schedule instead of trying to catch up.
    let next = deadline.map_or(now, |previous| (previous + period).max(now));
    loop {
        let now = Instant::now();
        if now >= next {
            break;
        }
        let left = next - now;
        if left > Duration::from_millis(2) {
            std::thread::sleep(left - Duration::from_millis(1));
        } else {
            std::hint::spin_loop();
        }
    }
    *deadline = Some(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ultrawide_gets_side_bars() {
        let (position, size) = fitted_rect(UVec2::new(3440, 1440), SIXTEEN_NINE).unwrap();
        assert_eq!(size, UVec2::new(2560, 1440));
        assert_eq!(position, UVec2::new(440, 0));
    }

    #[test]
    fn sixteen_ten_gets_top_and_bottom_bars() {
        let (position, size) = fitted_rect(UVec2::new(1920, 1200), SIXTEEN_NINE).unwrap();
        assert_eq!(size, UVec2::new(1920, 1080));
        assert_eq!(position, UVec2::new(0, 60));
    }

    #[test]
    fn exact_sixteen_nine_needs_no_viewport() {
        assert_eq!(fitted_rect(UVec2::new(2560, 1440), SIXTEEN_NINE), None);
    }
}
