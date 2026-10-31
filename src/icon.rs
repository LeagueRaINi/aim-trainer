//! Window title-bar and taskbar icons. Bevy has no window-icon API, so they're set on the winit
//! window directly (on the main thread, as winit requires) once it exists. The art lives in
//! `assets/icons` (SVG sources; the .exe icon is embedded by `build.rs`).

use bevy::{ecs::system::NonSendMarker, prelude::*, winit::WINIT_WINDOWS};
use winit::window::Icon;

/// The simplified logo, legible at title-bar size.
const WINDOW_ICON: &[u8] = include_bytes!("../assets/icons/window_32.png");
const TASKBAR_ICON: &[u8] = include_bytes!("../assets/icons/taskbar_64.png");

pub struct IconPlugin;

impl Plugin for IconPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, set_window_icon);
    }
}

fn decode(png: &[u8]) -> Option<Icon> {
    let image = image::load_from_memory(png).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).ok()
}

/// Runs every frame until the window exists, then sets the icons once.
fn set_window_icon(_main_thread: NonSendMarker, mut done: Local<bool>) {
    if *done {
        return;
    }
    WINIT_WINDOWS.with_borrow(|windows| {
        for window in windows.windows.values() {
            window.set_window_icon(decode(WINDOW_ICON));
            #[cfg(target_os = "windows")]
            {
                use winit::platform::windows::WindowExtWindows;
                window.set_taskbar_icon(decode(TASKBAR_ICON));
            }
            *done = true;
        }
    });
}
