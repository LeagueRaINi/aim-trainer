//! User-made scenarios (a drill kind with custom parameters) and playlists, saved to disk.

use std::path::Path;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    history::csv_safe,
    routine::{Routine, RoutineStep},
    scenario::{DrillConfig, DrillSpec, Scenario, VARIANT_SEPARATOR},
    settings::{self, Persisted, SaveState, Settings},
};

const CUSTOM_PATH: &str = "aim_trainer_custom.ron";
/// Longest scenario/playlist name, matching the editor's name box.
pub const MAX_NAME_CHARS: usize = 40;
/// Default length of an imported step or scenario whose length is unusable.
const DEFAULT_SECS: f32 = 60.0;

pub struct CustomPlugin;

impl Plugin for CustomPlugin {
    fn build(&self, app: &mut App) {
        let (mut content, report): (CustomContent, _) = settings::load_ron(Path::new(CUSTOM_PATH));
        content.repair();
        app.insert_resource(content)
            .insert_resource(SaveState::<CustomContent>::new(report))
            .add_systems(Startup, settings::report_load_problem::<CustomContent>)
            .add_systems(Update, settings::autosave::<CustomContent>)
            .add_systems(Last, settings::save_on_exit::<CustomContent>);
    }
}

impl Persisted for CustomContent {
    const PATH: &'static str = CUSTOM_PATH;
}

/// The first `max` characters of `s`.
fn truncate_chars(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map_or(s, |(end, _)| &s[..end])
}

/// A name that is safe for the history CSV: no `,`/newlines, trimmed, at most `MAX_NAME_CHARS`
/// characters, and never empty (`fallback` is used then).
fn clean_name(name: &str, fallback: &str) -> String {
    let safe = csv_safe(name);
    let name = truncate_chars(safe.trim(), MAX_NAME_CHARS).trim_end();
    if name.is_empty() { fallback.to_string() } else { name.to_string() }
}

/// `base` with a numeric suffix, shortening `base` so the result still fits `MAX_NAME_CHARS`.
fn suffixed(base: &str, i: u32) -> String {
    let suffix = format!(" {i}");
    let room = MAX_NAME_CHARS.saturating_sub(suffix.len());
    format!("{}{suffix}", truncate_chars(base, room).trim_end())
}

/// Whether `name` is `base` itself or a `suffixed` variant of it, as `unique_name` would make.
fn derived_from(name: &str, base: &str) -> bool {
    if name == base {
        return true;
    }
    let Some((stem, digits)) = name.rsplit_once(' ') else { return false };
    digits.parse::<u32>().is_ok_and(|i| i > 0 && suffixed(base, i) == name && !stem.is_empty())
}

/// A finite length in seconds, clamped to what a step may last.
fn clean_secs(secs: f32) -> f32 {
    if secs.is_finite() { secs } else { DEFAULT_SECS }.clamp(5.0, 600.0)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomScenario {
    pub id: u32,
    pub name: String,
    /// Default length when played on its own.
    pub duration: f32,
    pub config: DrillConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrillRef {
    Builtin(Scenario),
    Custom(u32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistStep {
    pub drill: DrillRef,
    pub secs: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomPlaylist {
    pub id: u32,
    pub name: String,
    pub steps: Vec<PlaylistStep>,
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomContent {
    pub scenarios: Vec<CustomScenario>,
    pub playlists: Vec<CustomPlaylist>,
    next_id: u32,
}

impl CustomContent {
    /// Cleans a loaded (possibly hand-edited) file the same way imports are cleaned: finite,
    /// clamped lengths and sanitized configs. Also makes sure fresh ids never collide with
    /// stored ones (e.g. a file without `next_id`).
    fn repair(&mut self) {
        for scenario in &mut self.scenarios {
            scenario.duration = clean_secs(scenario.duration);
            scenario.config.sanitize();
        }
        for step in self.playlists.iter_mut().flat_map(|p| &mut p.steps) {
            step.secs = clean_secs(step.secs);
        }
        let used = self.scenarios.iter().map(|s| s.id).chain(self.playlists.iter().map(|p| p.id));
        self.next_id = used.fold(self.next_id, u32::max);
    }

    fn take_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    /// Taken by another scenario, or by a built-in drill's name or variant name (their history
    /// would get mixed in).
    fn name_taken(&self, name: &str) -> bool {
        let builtin = Scenario::ALL.iter().any(|s| {
            name.strip_prefix(s.name()).is_some_and(|rest| rest.is_empty() || rest.starts_with(VARIANT_SEPARATOR))
        });
        builtin || self.scenarios.iter().any(|s| s.name == name)
    }

    /// A name not used by any built-in drill or other custom scenario, so history stays separate.
    fn unique_name(&self, base: &str) -> String {
        let base = clean_name(base, "Scenario");
        (1..).map(|i| suffixed(&base, i)).find(|name| !self.name_taken(name)).expect("unbounded")
    }

    pub fn scenario(&self, id: u32) -> Option<&CustomScenario> {
        self.scenarios.iter().find(|s| s.id == id)
    }

    pub fn scenario_mut(&mut self, id: u32) -> Option<&mut CustomScenario> {
        self.scenarios.iter_mut().find(|s| s.id == id)
    }

    pub fn playlist(&self, id: u32) -> Option<&CustomPlaylist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    pub fn playlist_mut(&mut self, id: u32) -> Option<&mut CustomPlaylist> {
        self.playlists.iter_mut().find(|p| p.id == id)
    }

    pub fn new_scenario(&mut self, kind: Scenario) -> u32 {
        let id = self.take_id();
        let name = self.unique_name(&format!("My {}", kind.name()));
        self.scenarios.push(CustomScenario { id, name, duration: 60.0, config: DrillConfig::defaults(kind) });
        id
    }

    pub fn duplicate_scenario(&mut self, id: u32) -> Option<u32> {
        let mut copy = self.scenario(id)?.clone();
        copy.id = self.take_id();
        copy.name = self.unique_name(&copy.name);
        let new_id = copy.id;
        self.scenarios.push(copy);
        Some(new_id)
    }

    /// The scenario named `name` with `config`: updates the drill settings of an existing one
    /// (keeping its length, which you may have changed), or creates it with `duration`. Used by
    /// generated scenarios like weak-spot training, whose history should continue across updates.
    pub fn upsert_scenario(&mut self, name: &str, duration: f32, mut config: DrillConfig) -> u32 {
        config.sanitize();
        let name = clean_name(name, "Scenario");
        if let Some(scenario) = self.scenarios.iter_mut().find(|s| s.name == name) {
            scenario.config = config;
            return scenario.id;
        }
        let id = self.take_id();
        let name = if self.name_taken(&name) { self.unique_name(&name) } else { name };
        self.scenarios.push(CustomScenario { id, name, duration: clean_secs(duration), config });
        id
    }

    /// Renames a scenario. Names must stay unique (history is grouped by name) and can't contain
    /// `,` or line breaks (the history CSV would alter them); returns whether the name was accepted.
    pub fn rename_scenario(&mut self, id: u32, name: &str) -> bool {
        let name = name.trim();
        let clash = name.is_empty()
            || name.contains([',', '\n', '\r'])
            || Scenario::ALL.iter().any(|s| s.name() == name)
            || self.scenarios.iter().any(|s| s.id != id && s.name == name);
        match self.scenario_mut(id) {
            Some(scenario) if !clash => {
                scenario.name = name.to_string();
                true
            }
            _ => false,
        }
    }

    /// Deletes a scenario and removes it from every playlist.
    pub fn delete_scenario(&mut self, id: u32) {
        self.scenarios.retain(|s| s.id != id);
        for playlist in &mut self.playlists {
            playlist.steps.retain(|step| step.drill != DrillRef::Custom(id));
        }
    }

    pub fn new_playlist(&mut self) -> u32 {
        let id = self.take_id();
        let n = self.playlists.len() + 1;
        self.playlists.push(CustomPlaylist { id, name: format!("My playlist {n}"), steps: Vec::new() });
        id
    }

    pub fn delete_playlist(&mut self, id: u32) {
        self.playlists.retain(|p| p.id != id);
    }

    pub fn drill_name(&self, drill: DrillRef) -> Option<String> {
        match drill {
            DrillRef::Builtin(kind) => Some(kind.name().to_string()),
            DrillRef::Custom(id) => self.scenario(id).map(|s| s.name.clone()),
        }
    }

    pub fn resolve(&self, drill: DrillRef, settings: &Settings) -> Option<DrillSpec> {
        match drill {
            DrillRef::Builtin(kind) => Some(DrillSpec::builtin(kind, settings)),
            DrillRef::Custom(id) => {
                let scenario = self.scenario(id)?;
                Some(DrillSpec { name: scenario.name.clone(), config: scenario.config.clone() })
            }
        }
    }

    /// A playlist as a playable routine, skipping steps whose scenario no longer exists.
    pub fn playlist_routine(&self, id: u32, settings: &Settings) -> Option<Routine> {
        let playlist = self.playlist(id)?;
        let steps: Vec<RoutineStep> = playlist
            .steps
            .iter()
            .filter_map(|step| Some(RoutineStep { drill: self.resolve(step.drill, settings)?, secs: step.secs }))
            .collect();
        (!steps.is_empty()).then(|| Routine::new(playlist.name.clone(), String::new(), steps))
    }
}

// ---------------------------------------------------------------------------------------------
// Sharing: a playlist plus every custom scenario it uses, as one copy-pasteable code.
// ---------------------------------------------------------------------------------------------

pub const SHARE_PREFIX: &str = "AIMPL1:";

#[derive(Serialize, Deserialize)]
struct SharedPlaylist {
    name: String,
    steps: Vec<(SharedDrill, f32)>,
}

#[derive(Serialize, Deserialize)]
enum SharedDrill {
    Builtin(Scenario),
    Custom { name: String, duration: f32, config: DrillConfig },
}

impl CustomContent {
    /// Share code for a playlist: `AIMPL1:` followed by base64 of a compact RON document.
    pub fn export_playlist(&self, id: u32) -> Option<String> {
        let playlist = self.playlist(id)?;
        let steps = playlist
            .steps
            .iter()
            .filter_map(|step| {
                let drill = match step.drill {
                    DrillRef::Builtin(kind) => SharedDrill::Builtin(kind),
                    DrillRef::Custom(id) => {
                        let s = self.scenario(id)?;
                        SharedDrill::Custom { name: s.name.clone(), duration: s.duration, config: s.config.clone() }
                    }
                };
                Some((drill, step.secs))
            })
            .collect();
        let shared = SharedPlaylist { name: playlist.name.clone(), steps };
        let text = ron::ser::to_string(&shared).ok()?;
        Some(format!("{SHARE_PREFIX}{}", URL_SAFE_NO_PAD.encode(text)))
    }

    /// Adds a shared playlist (and its scenarios) and returns the new playlist's id.
    /// Scenarios identical to one you already have (same config, and the same name or a renamed
    /// copy of it) are reused rather than duplicated.
    pub fn import_playlist(&mut self, code: &str) -> Result<u32, String> {
        let payload = code.trim().strip_prefix(SHARE_PREFIX).ok_or("Not an Aim Trainer playlist code.")?;
        let bytes = URL_SAFE_NO_PAD.decode(payload).map_err(|_| "The share code is damaged.".to_string())?;
        let text = String::from_utf8(bytes).map_err(|_| "The share code is damaged.".to_string())?;
        let shared: SharedPlaylist = ron::from_str(&text).map_err(|_| "The share code is damaged.".to_string())?;

        let mut steps = Vec::with_capacity(shared.steps.len());
        for (drill, secs) in shared.steps {
            let drill = match drill {
                SharedDrill::Builtin(kind) => DrillRef::Builtin(kind),
                SharedDrill::Custom { name, duration, mut config } => {
                    config.sanitize();
                    let name = clean_name(&name, "Imported scenario");
                    let existing = self.scenarios.iter().find(|s| s.config == config && derived_from(&s.name, &name));
                    let id = match existing {
                        Some(s) => s.id,
                        None => {
                            let id = self.take_id();
                            let name = if self.name_taken(&name) { self.unique_name(&name) } else { name };
                            self.scenarios.push(CustomScenario { id, name, duration: clean_secs(duration), config });
                            id
                        }
                    };
                    DrillRef::Custom(id)
                }
            };
            steps.push(PlaylistStep { drill, secs: clean_secs(secs) });
        }
        let id = self.take_id();
        self.playlists.push(CustomPlaylist { id, name: clean_name(&shared.name, "Imported playlist"), steps });
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_unique_and_never_shadow_builtins() {
        let mut content = CustomContent::default();
        let a = content.new_scenario(Scenario::Gridshot);
        let b = content.new_scenario(Scenario::Gridshot);
        assert_ne!(content.scenario(a).unwrap().name, content.scenario(b).unwrap().name);
        assert!(!content.rename_scenario(a, "Gridshot"));
        assert!(!content.rename_scenario(a, &content.scenario(b).unwrap().name.clone()));
        assert!(!content.rename_scenario(a, "   "));
        assert!(content.rename_scenario(a, "5x5 wide"));
        assert_eq!(content.scenario(a).unwrap().name, "5x5 wide");
    }

    #[test]
    fn deleting_a_scenario_removes_it_from_playlists() {
        let mut content = CustomContent::default();
        let scenario = content.new_scenario(Scenario::Tracking);
        let playlist = content.new_playlist();
        let steps = &mut content.playlist_mut(playlist).unwrap().steps;
        steps.push(PlaylistStep { drill: DrillRef::Custom(scenario), secs: 30.0 });
        steps.push(PlaylistStep { drill: DrillRef::Builtin(Scenario::Gridshot), secs: 30.0 });
        content.delete_scenario(scenario);
        assert_eq!(content.playlist(playlist).unwrap().steps.len(), 1);
    }

    #[test]
    fn playlists_resolve_to_routines_with_custom_configs() {
        let mut content = CustomContent::default();
        let scenario = content.new_scenario(Scenario::Gridshot);
        content.scenario_mut(scenario).unwrap().config.grid_cols = 5;
        let playlist = content.new_playlist();
        content.playlist_mut(playlist).unwrap().steps.push(PlaylistStep { drill: DrillRef::Custom(scenario), secs: 20.0 });
        let routine = content.playlist_routine(playlist, &Settings::default()).unwrap();
        assert_eq!(routine.steps[0].drill.config.grid_cols, 5);
        assert_eq!(routine.steps[0].secs, 20.0);
        // Empty playlists can't be played.
        let empty = content.new_playlist();
        assert!(content.playlist_routine(empty, &Settings::default()).is_none());
    }

    #[test]
    fn saved_content_round_trips() {
        let mut content = CustomContent::default();
        content.new_scenario(Scenario::Peek);
        content.new_playlist();
        let text = ron::ser::to_string(&content).unwrap();
        assert_eq!(ron::from_str::<CustomContent>(&text).unwrap(), content);
    }

    #[test]
    fn share_codes_round_trip_with_embedded_scenarios() {
        let mut source = CustomContent::default();
        let wide = source.new_scenario(Scenario::Gridshot);
        source.scenario_mut(wide).unwrap().config.grid_cols = 6;
        let playlist = source.new_playlist();
        let steps = &mut source.playlist_mut(playlist).unwrap().steps;
        steps.push(PlaylistStep { drill: DrillRef::Custom(wide), secs: 40.0 });
        steps.push(PlaylistStep { drill: DrillRef::Builtin(Scenario::Peek), secs: 60.0 });
        let code = source.export_playlist(playlist).unwrap();
        assert!(code.starts_with(SHARE_PREFIX));

        let mut target = CustomContent::default();
        let imported = target.import_playlist(&code).unwrap();
        let routine = target.playlist_routine(imported, &Settings::default()).unwrap();
        assert_eq!(routine.steps.len(), 2);
        assert_eq!(routine.steps[0].drill.config.grid_cols, 6);
        assert_eq!(routine.steps[1].drill.config.kind, Scenario::Peek);

        // Importing again reuses the identical scenario instead of duplicating it.
        target.import_playlist(&code).unwrap();
        assert_eq!(target.scenarios.len(), 1);
        assert_eq!(target.playlists.len(), 2);
    }

    #[test]
    fn bad_share_codes_are_rejected() {
        let mut content = CustomContent::default();
        assert!(content.import_playlist("hello").is_err());
        assert!(content.import_playlist(&format!("{SHARE_PREFIX}not-base64!!")).is_err());
        assert!(content.playlists.is_empty());
    }

    fn share_code(shared: &SharedPlaylist) -> String {
        format!("{SHARE_PREFIX}{}", URL_SAFE_NO_PAD.encode(ron::ser::to_string(shared).unwrap()))
    }

    #[test]
    fn names_are_csv_safe_and_bounded() {
        let mut content = CustomContent::default();
        let a = content.new_scenario(Scenario::Gridshot);
        assert!(!content.rename_scenario(a, "Head,level"));
        assert!(!content.rename_scenario(a, "two\nlines"));
        assert!(!content.rename_scenario(a, "c\rr"));
        assert_eq!(clean_name("  a,b\n ", "x"), "a b");
        assert_eq!(clean_name(" ,\r\n ", "fallback"), "fallback");
        let long = "é".repeat(60);
        assert_eq!(clean_name(&long, "x").chars().count(), MAX_NAME_CHARS);
        // Copies of a 40-character name still fit, suffix included.
        assert!(content.rename_scenario(a, &"n".repeat(40)));
        let copy = content.duplicate_scenario(a).unwrap();
        let copy_name = &content.scenario(copy).unwrap().name;
        assert_eq!(copy_name.chars().count(), MAX_NAME_CHARS);
        assert!(copy_name.ends_with(" 1"));
    }

    #[test]
    fn imports_normalise_names_and_lengths() {
        let shared = SharedPlaylist {
            name: "   ".into(),
            steps: vec![(
                SharedDrill::Custom {
                    name: "Head,level".into(),
                    duration: f32::NAN,
                    config: DrillConfig { strafe_min: f32::NAN, ..DrillConfig::defaults(Scenario::Tracking) },
                },
                f32::NAN,
            )],
        };
        let mut content = CustomContent::default();
        let id = content.import_playlist(&share_code(&shared)).unwrap();
        let playlist = content.playlist(id).unwrap();
        assert_eq!(playlist.name, "Imported playlist");
        assert_eq!(playlist.steps[0].secs, DEFAULT_SECS);
        let scenario = &content.scenarios[0];
        assert_eq!(scenario.name, "Head level 1");
        assert_eq!(scenario.duration, DEFAULT_SECS);
        assert!(scenario.config.strafe_min.is_finite());
    }

    #[test]
    fn reimporting_reuses_renamed_scenarios() {
        let shared = SharedPlaylist {
            name: "p".into(),
            steps: vec![(
                SharedDrill::Custom { name: "Gridshot".into(), duration: 60.0, config: DrillConfig::defaults(Scenario::Gridshot) },
                30.0,
            )],
        };
        let code = share_code(&shared);
        let mut content = CustomContent::default();
        content.import_playlist(&code).unwrap();
        content.import_playlist(&code).unwrap();
        assert_eq!(content.scenarios.len(), 1);
        assert_eq!(content.scenarios[0].name, "Gridshot 1");
    }

    #[test]
    fn loaded_ids_never_collide() {
        let mut content: CustomContent = ron::from_str("(scenarios: [(id: 7, name: \"a\", duration: 60, config: ())])").unwrap();
        content.repair();
        assert_eq!(content.new_playlist(), 8);
    }

    #[test]
    fn upserting_creates_once_then_updates_in_place() {
        let mut content = CustomContent::default();
        let config = DrillConfig { max_flick: 25.0, flick_direction: Some(225.0), ..DrillConfig::defaults(Scenario::Microflick) };
        let id = content.upsert_scenario("Weak spot: down-left 15-25°", 60.0, config.clone());
        content.scenario_mut(id).unwrap().duration = 45.0;
        let again = content.upsert_scenario("Weak spot: down-left 15-25°", 60.0, DrillConfig { max_flick: 20.0, ..config });
        assert_eq!(again, id);
        assert_eq!(content.scenarios.len(), 1);
        let scenario = content.scenario(id).unwrap();
        assert_eq!(scenario.config.max_flick, 20.0);
        assert_eq!(scenario.duration, 45.0, "your own length is kept");
        // Never takes over a built-in drill's history.
        let other = content.upsert_scenario("Microflick", 60.0, DrillConfig::defaults(Scenario::Microflick));
        assert_eq!(content.scenario(other).unwrap().name, "Microflick 1");
    }

    #[test]
    fn old_custom_files_without_flick_constraints_still_load() {
        let mut content: CustomContent =
            ron::from_str("(scenarios: [(id: 3, name: \"wide\", duration: 60, config: (kind: Microflick, yaw_range: 45.0))])").unwrap();
        content.repair();
        let config = &content.scenarios[0].config;
        assert_eq!(config.yaw_range, 45.0);
        assert_eq!((config.flick_direction, config.max_flick), (None, 0.0));
        assert!(!config.flick_constrained());
    }

    #[test]
    fn invalid_custom_file_is_backed_up() {
        let dir = std::env::temp_dir().join(format!("aim_trainer_test_custom_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("custom.ron");
        std::fs::write(&path, "(scenarios: [(kind: Nope)])").unwrap();
        let (content, report): (CustomContent, _) = settings::load_ron(&path);
        assert_eq!(content, CustomContent::default());
        assert!(report.can_save && report.problem.is_some());
        assert!(dir.join("custom.ron.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The content `fixtures/custom.ron` and `fixtures/share_code.txt` were saved from: one
    /// scenario per kind (Microflick with flick constraints) and a playlist of a built-in and a
    /// custom step. Returns it with the playlist's id.
    fn fixture_content() -> (CustomContent, u32) {
        let mut content = CustomContent::default();
        let ids: Vec<u32> = Scenario::ALL.into_iter().map(|kind| content.new_scenario(kind)).collect();
        let micro = content.scenario_mut(ids[1]).unwrap();
        micro.config.max_flick = 20.0;
        micro.config.flick_direction = Some(90.0);
        micro.config.flick_spread = 30.0;
        micro.duration = 45.0;
        content.scenario_mut(ids[0]).unwrap().config.grid_cols = 5;
        let playlist = content.new_playlist();
        let steps = &mut content.playlist_mut(playlist).unwrap().steps;
        steps.push(PlaylistStep { drill: DrillRef::Builtin(Scenario::Peripheral), secs: 30.0 });
        steps.push(PlaylistStep { drill: DrillRef::Custom(ids[1]), secs: 40.0 });
        (content, playlist)
    }

    /// Pins the custom file format: field names, kind/`DrillRef` variant names, the writer.
    #[test]
    fn loads_saved_custom_fixture() {
        let fixture = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/custom.ron"));
        let (expected, _) = fixture_content();
        let mut loaded: CustomContent = ron::from_str(fixture).unwrap();
        loaded.repair();
        assert_eq!(loaded, expected);
        let kinds: Vec<Scenario> = loaded.scenarios.iter().map(|s| s.config.kind).collect();
        assert_eq!(kinds, Scenario::ALL);
        let written = ron::ser::to_string_pretty(&expected, ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(written.replace("\r\n", "\n"), fixture.replace("\r\n", "\n"));
    }

    /// Pins the `AIMPL1:` share code format in both directions.
    #[test]
    fn imports_share_code_fixture() {
        let code = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/share_code.txt")).trim();
        let (source, playlist) = fixture_content();
        assert_eq!(source.export_playlist(playlist).unwrap(), code);

        let mut content = CustomContent::default();
        let id = content.import_playlist(code).unwrap();
        let imported = content.playlist(id).unwrap();
        assert_eq!(imported.name, "My playlist 1");
        assert_eq!(imported.steps[0], PlaylistStep { drill: DrillRef::Builtin(Scenario::Peripheral), secs: 30.0 });
        let DrillRef::Custom(custom) = imported.steps[1].drill else { panic!("expected a custom step") };
        assert_eq!(imported.steps[1].secs, 40.0);
        let scenario = content.scenario(custom).unwrap();
        assert_eq!(scenario.name, "My Microflick 1");
        assert_eq!(scenario.duration, 45.0);
        assert_eq!(scenario.config, source.scenarios[1].config);
    }
}
