//! Sharing: a playlist plus every custom scenario it uses, as one copy-pasteable `AIMPL1:` code.
//! Persisted format: the `SharedPlaylist`/`SharedDrill` field and variant names are part of every
//! code ever shared. Never change them.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

use super::{CustomContent, CustomPlaylist, CustomScenario, DrillRef, PlaylistStep, clean_name, clean_secs, suffixed};
use crate::drill::{DrillConfig, DrillKind};

pub const SHARE_PREFIX: &str = "AIMPL1:";
/// Most steps an imported playlist may have. Every custom step is checked against every
/// scenario you have, so a crafted code with thousands would stall the app.
const MAX_SHARED_STEPS: usize = 100;
/// Longest share code accepted, checked before decoding: room for `MAX_SHARED_STEPS` custom
/// steps (about 600 characters each).
const MAX_CODE_LEN: usize = 128 << 10;

#[derive(Serialize, Deserialize)]
struct SharedPlaylist {
    name: String,
    steps: Vec<(SharedDrill, f32)>,
}

#[derive(Serialize, Deserialize)]
enum SharedDrill {
    Builtin(DrillKind),
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
                    },
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
        let code = code.trim();
        if code.len() > MAX_CODE_LEN {
            return Err("The share code is too long.".into());
        }
        let payload = code.strip_prefix(SHARE_PREFIX).ok_or("Not an Aim Trainer playlist code.")?;
        let bytes = URL_SAFE_NO_PAD.decode(payload).map_err(|_| "The share code is damaged.".to_string())?;
        let text = String::from_utf8(bytes).map_err(|_| "The share code is damaged.".to_string())?;
        let shared: SharedPlaylist = ron::from_str(&text).map_err(|_| "The share code is damaged.".to_string())?;
        if shared.steps.len() > MAX_SHARED_STEPS {
            return Err(format!("Playlists with more than {MAX_SHARED_STEPS} steps can't be imported."));
        }

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
                        },
                    };
                    DrillRef::Custom(id)
                },
            };
            steps.push(PlaylistStep { drill, secs: clean_secs(secs) });
        }
        let id = self.take_id();
        self.playlists.push(CustomPlaylist { id, name: clean_name(&shared.name, "Imported playlist"), steps });
        Ok(id)
    }
}

/// Whether `name` is `base` itself or a `suffixed` variant of it, as `unique_name` would make.
fn derived_from(name: &str, base: &str) -> bool {
    if name == base {
        return true;
    }
    let Some((stem, digits)) = name.rsplit_once(' ') else { return false };
    digits.parse::<u32>().is_ok_and(|i| i > 0 && suffixed(base, i) == name && !stem.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom::DEFAULT_SECS;
    use crate::custom::tests::fixture_content;
    use crate::settings::Settings;

    #[test]
    fn share_codes_round_trip_with_embedded_scenarios() {
        let mut source = CustomContent::default();
        let wide = source.new_scenario(DrillKind::Gridshot);
        source.scenario_mut(wide).unwrap().config.grid_cols = 6;
        let playlist = source.new_playlist();
        let steps = &mut source.playlist_mut(playlist).unwrap().steps;
        steps.push(PlaylistStep { drill: DrillRef::Custom(wide), secs: 40.0 });
        steps.push(PlaylistStep { drill: DrillRef::Builtin(DrillKind::Peek), secs: 60.0 });
        let code = source.export_playlist(playlist).unwrap();
        assert!(code.starts_with(SHARE_PREFIX));

        let mut target = CustomContent::default();
        let imported = target.import_playlist(&code).unwrap();
        let routine = target.playlist_routine(imported, &Settings::default()).unwrap();
        assert_eq!(routine.steps.len(), 2);
        assert_eq!(routine.steps[0].drill.config.grid_cols, 6);
        assert_eq!(routine.steps[1].drill.config.kind, DrillKind::Peek);

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

    #[test]
    fn oversized_share_codes_are_rejected() {
        let playlist = |name: String, steps: usize| SharedPlaylist {
            name,
            steps: (0..steps).map(|_| (SharedDrill::Builtin(DrillKind::Gridshot), 30.0)).collect(),
        };
        let mut content = CustomContent::default();
        assert!(content.import_playlist(&share_code(&playlist("p".into(), MAX_SHARED_STEPS))).is_ok());
        assert!(content.import_playlist(&share_code(&playlist("p".into(), MAX_SHARED_STEPS + 1))).is_err());
        // Valid, but too long to decode.
        assert!(content.import_playlist(&share_code(&playlist("p".repeat(MAX_CODE_LEN), 1))).is_err());
        assert_eq!(content.playlists.len(), 1);
    }

    fn share_code(shared: &SharedPlaylist) -> String {
        format!("{SHARE_PREFIX}{}", URL_SAFE_NO_PAD.encode(ron::ser::to_string(shared).unwrap()))
    }

    #[test]
    fn imports_normalise_names_and_lengths() {
        let shared = SharedPlaylist {
            name: "   ".into(),
            steps: vec![(
                SharedDrill::Custom {
                    name: "Head,level".into(),
                    duration: f32::NAN,
                    config: DrillConfig { strafe_min: f32::NAN, ..DrillConfig::defaults(DrillKind::Tracking) },
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
                SharedDrill::Custom {
                    name: "Gridshot".into(),
                    duration: 60.0,
                    config: DrillConfig::defaults(DrillKind::Gridshot),
                },
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
        assert_eq!(imported.steps[0], PlaylistStep { drill: DrillRef::Builtin(DrillKind::Peripheral), secs: 30.0 });
        let DrillRef::Custom(custom) = imported.steps[1].drill else { panic!("expected a custom step") };
        assert_eq!(imported.steps[1].secs, 40.0);
        let scenario = content.scenario(custom).unwrap();
        assert_eq!(scenario.name, "My Microflick 1");
        assert_eq!(scenario.duration, 45.0);
        assert_eq!(scenario.config, source.scenarios[1].config);
    }
}
