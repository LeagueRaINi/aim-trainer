//! Saving resources to RON files: load with backup of unreadable files, atomic debounced saves,
//! and a last save on exit. Used by the settings and the custom content.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io};

use bevy::prelude::*;
use bevy::window::WindowCloseRequested;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Files are written this long after the last change, so dragging a value doesn't hammer the disk.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

/// A resource saved to a RON file.
pub(crate) trait Persisted: Resource + Serialize {
    const PATH: &'static str;
}

/// Inserts a loaded resource and keeps its file up to date: logs load problems at startup,
/// saves (debounced) after changes and right away on exit.
pub(crate) struct PersistPlugin<T> {
    pub value: T,
    pub report: LoadReport,
}

impl<T: Persisted + Clone> Plugin for PersistPlugin<T> {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.value.clone())
            .insert_resource(SaveState::<T>::new(self.report.clone()))
            .add_systems(Startup, report_load_problem::<T>)
            .add_systems(Update, autosave::<T>)
            .add_systems(Last, save_on_exit::<T>);
    }
}

/// Problems found while loading a file.
#[derive(Clone, Debug)]
pub(crate) struct LoadReport {
    /// Message to log once logging is up.
    pub problem: Option<String>,
    /// False when the file exists but couldn't be read (or backed up): overwriting it with
    /// defaults would destroy the user's data.
    pub can_save: bool,
}

impl Default for LoadReport {
    fn default() -> Self {
        Self { problem: None, can_save: true }
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

/// Reads a RON file, falling back to defaults. An unparsable file is copied to `<path>.bak`
/// first, since the next save replaces it.
pub(crate) fn load_ron<T: DeserializeOwned + Default>(path: &Path) -> (T, LoadReport) {
    let shown = path.display();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return (T::default(), LoadReport::default()),
        Err(err) => {
            let problem = format!("Couldn't read {shown}: {err}. Using defaults; changes won't be saved this session.");
            return (T::default(), LoadReport { problem: Some(problem), can_save: false });
        },
    };
    match ron::from_str(&text) {
        Ok(value) => (value, LoadReport::default()),
        Err(err) => {
            let backup = with_suffix(path, ".bak");
            let report = match fs::copy(path, &backup) {
                Ok(_) => LoadReport {
                    problem: Some(format!(
                        "Ignoring invalid {shown}: {err}. The old file was copied to {}.",
                        backup.display()
                    )),
                    can_save: true,
                },
                Err(copy_err) => LoadReport {
                    problem: Some(format!(
                        "Ignoring invalid {shown}: {err}. Backing it up failed ({copy_err}), so it won't be overwritten this session."
                    )),
                    can_save: false,
                },
            };
            (T::default(), report)
        },
    }
}

/// Writes via a temporary file and a rename, so a crash mid-write never leaves a truncated file.
pub(crate) fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let tmp = with_suffix(path, ".tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

fn save<T: Persisted>(value: &T) {
    let text = match ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default()) {
        Ok(text) => text,
        Err(err) => return error!("Failed to serialize {}: {err}", T::PATH),
    };
    if let Err(err) = write_atomic(Path::new(T::PATH), &text) {
        error!("Failed to write {}: {err}", T::PATH);
    }
}

/// Debounced-save bookkeeping for one persisted resource.
#[derive(Resource)]
struct SaveState<T> {
    /// When the value last changed, if it hasn't been saved since.
    pending: Option<Duration>,
    report: LoadReport,
    marker: PhantomData<fn() -> T>,
}

impl<T> SaveState<T> {
    fn new(report: LoadReport) -> Self {
        Self { pending: None, report, marker: PhantomData }
    }
}

fn report_load_problem<T: Persisted>(mut state: ResMut<SaveState<T>>) {
    if let Some(problem) = state.report.problem.take() {
        error!("{problem}");
    }
}

fn autosave<T: Persisted>(value: Res<T>, time: Res<Time<Real>>, mut state: ResMut<SaveState<T>>) {
    let now = time.elapsed();
    if value.is_changed() && !value.is_added() && state.report.can_save {
        state.pending = Some(now);
    }
    if state.pending.is_some_and(|changed_at| now - changed_at >= SAVE_DEBOUNCE) {
        state.pending = None;
        save(&*value);
    }
}

/// Writes a pending change right away when the app is quitting or the window is closing, instead
/// of losing it to the debounce.
fn save_on_exit<T: Persisted>(
    value: Res<T>,
    mut state: ResMut<SaveState<T>>,
    mut exits: MessageReader<AppExit>,
    mut closes: MessageReader<WindowCloseRequested>,
) {
    let quitting = exits.read().count() > 0;
    let closing = closes.read().count() > 0;
    if !(quitting || closing) || !state.report.can_save {
        return;
    }
    // A change made this very frame hasn't reached `autosave`'s bookkeeping yet.
    if state.pending.is_some() || (value.is_changed() && !value.is_added()) {
        state.pending = None;
        save(&*value);
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::test_util::TempDir;

    #[derive(Debug, Default, PartialEq, Deserialize)]
    #[serde(default)]
    struct Sample {
        sens: f32,
        kind: Option<Kind>,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    enum Kind {
        Known,
    }

    #[test]
    fn invalid_file_is_backed_up_and_defaults_used() {
        let dir = TempDir::new("persist_invalid");
        let path = dir.join("settings.ron");
        fs::write(&path, "(kind: Some(Bogus))").unwrap();
        let (value, report): (Sample, _) = load_ron(&path);
        assert_eq!(value, Sample::default());
        assert!(report.can_save && report.problem.is_some());
        assert_eq!(fs::read_to_string(dir.join("settings.ron.bak")).unwrap(), "(kind: Some(Bogus))");

        // Missing files are fine; valid ones load; saves replace the file atomically.
        let missing = dir.join("missing.ron");
        let (_, report): (Sample, _) = load_ron(&missing);
        assert!(report.can_save && report.problem.is_none());
        write_atomic(&missing, "(sens: 1.5, kind: Some(Known))").unwrap();
        let (value, report): (Sample, _) = load_ron(&missing);
        assert_eq!(value, Sample { sens: 1.5, kind: Some(Kind::Known) });
        assert!(report.problem.is_none());
        assert!(!dir.join("missing.ron.tmp").exists());
    }

    #[test]
    fn unreadable_file_disables_saving() {
        // A directory in place of the file can't be read as text (and isn't NotFound).
        let dir = TempDir::new("persist_unreadable");
        let path = dir.join("settings.ron");
        fs::create_dir_all(&path).unwrap();
        let (_, report): (Sample, _) = load_ron(&path);
        assert!(!report.can_save && report.problem.is_some());
    }
}
