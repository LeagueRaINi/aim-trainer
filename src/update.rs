//! Self-updating from the GitHub releases: at startup (and when switched on) a background
//! thread asks GitHub for the latest release. If it is newer than this build it downloads the
//! zstd-compressed exe, checks it against the SHA-256 digest GitHub publishes for the asset,
//! unpacks it and swaps it in for the running exe. The new version runs from the next start;
//! the menu offers a restart.
//!
//! Development builds (an exe inside a cargo `target` directory) never replace themselves.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use bevy::prelude::*;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::settings::Settings;

const LATEST_RELEASE: &str = "https://api.github.com/repos/LeagueRaINi/aim-trainer/releases/latest";
/// The release asset with the Windows exe, zstd-compressed (see `.github/workflows/release.yml`).
const ASSET: &str = "aim_trainer.exe.zst";
/// Largest download and unpacked exe accepted, so a bad response can't exhaust memory.
const MAX_DOWNLOAD: u64 = 256 << 20;
const MAX_EXE: u64 = 512 << 20;

pub struct UpdatePlugin;

impl Plugin for UpdatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Updater>().add_systems(Update, start_check.run_if(resource_changed::<Settings>));
    }
}

/// The version of this build.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateStatus {
    /// Not started yet.
    Idle,
    /// Automatic updates are switched off.
    Off,
    /// This build doesn't update itself; the reason.
    Unavailable(&'static str),
    Checking,
    UpToDate,
    Downloading(Version),
    /// Installed; runs from the next start.
    Ready(Version),
    Failed(String),
}

/// The update check's state, shared with its thread, and the exe it updates.
#[derive(Resource, Clone)]
pub struct Updater {
    status: Arc<Mutex<UpdateStatus>>,
    /// Where this exe was started from; also where the update is installed and restarted from.
    exe: Option<PathBuf>,
}

impl Default for Updater {
    fn default() -> Self {
        Self { status: Arc::new(Mutex::new(UpdateStatus::Idle)), exe: std::env::current_exe().ok() }
    }
}

impl Updater {
    pub fn status(&self) -> UpdateStatus {
        self.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    fn set(&self, status: UpdateStatus) {
        *self.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = status;
    }

    /// Starts this exe again from the same working directory (where the data files are) and
    /// returns whether it did; the caller then exits.
    pub fn restart(&self) -> bool {
        let Some(exe) = &self.exe else { return false };
        let mut command = Command::new(exe);
        if let Ok(dir) = std::env::current_dir() {
            command.current_dir(dir);
        }
        match command.spawn() {
            Ok(_) => true,
            Err(err) => {
                error!("Restarting failed: {err}");
                false
            },
        }
    }
}

/// Starts the check once updates are on: at startup, or when switched on in the menu.
fn start_check(settings: Res<Settings>, updater: Res<Updater>) {
    let status = updater.status();
    if !matches!(status, UpdateStatus::Idle | UpdateStatus::Off) {
        return;
    }
    if !settings.updates.auto {
        return updater.set(UpdateStatus::Off);
    }
    let Some(exe) = updater.exe.clone() else {
        return updater.set(UpdateStatus::Unavailable("its location is unknown"));
    };
    if !cfg!(windows) {
        return updater.set(UpdateStatus::Unavailable("releases are for Windows"));
    }
    if is_development_build(&exe) {
        return updater.set(UpdateStatus::Unavailable("development build"));
    }
    updater.set(UpdateStatus::Checking);
    let worker = updater.clone();
    let spawned = thread::Builder::new().name("updater".into()).spawn(move || {
        let current = Version::parse(current_version()).expect("the package version is semver");
        let status = match install_latest(&exe, &current, &worker) {
            Ok(None) => UpdateStatus::UpToDate,
            Ok(Some(version)) => {
                info!("Updated to {version}; it runs from the next start");
                UpdateStatus::Ready(version)
            },
            Err(err) => {
                warn!("Update failed: {err}");
                UpdateStatus::Failed(err)
            },
        };
        worker.set(status);
    });
    if let Err(err) = spawned {
        updater.set(UpdateStatus::Failed(format!("couldn't start the update check: {err}")));
    }
}

/// Whether `exe` was built by cargo here (it sits in a `target` directory next to a
/// `Cargo.toml`): those are updated from source, not from releases.
fn is_development_build(exe: &Path) -> bool {
    exe.ancestors().any(|dir| dir.file_name() == Some("target".as_ref()) && dir.with_file_name("Cargo.toml").is_file())
}

/// Checks for a newer release and installs it in place of `exe`; returns its version, or
/// `None` if this build is the latest.
fn install_latest(exe: &Path, current: &Version, updater: &Updater) -> Result<Option<Version>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(600)))
        .user_agent(concat!("aim_trainer/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let release: Release = agent
        .get(LATEST_RELEASE)
        .header("Accept", "application/vnd.github+json")
        .call()
        .and_then(|mut response| response.body_mut().read_json())
        .map_err(|err| format!("couldn't check GitHub: {err}"))?;
    let Some(update) = pick_update(current, &release)? else { return Ok(None) };
    updater.set(UpdateStatus::Downloading(update.version.clone()));
    let packed = agent
        .get(&update.url)
        .call()
        .and_then(|mut response| response.body_mut().with_config().limit(MAX_DOWNLOAD).read_to_vec())
        .map_err(|err| format!("download failed: {err}"))?;
    if sha256_hex(&packed) != update.sha256 {
        return Err("the download doesn't match its published checksum".into());
    }
    let new_exe = unpack(&packed)?;
    let staged = exe.with_extension("exe.update");
    std::fs::write(&staged, new_exe).map_err(|err| format!("couldn't save the update: {err}"))?;
    let replaced = self_replace::self_replace(&staged);
    let _ = std::fs::remove_file(&staged);
    replaced.map_err(|err| format!("couldn't replace the exe: {err}"))?;
    Ok(Some(update.version))
}

/// The parts of GitHub's release JSON the updater reads.
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>`, computed by GitHub on upload.
    digest: Option<String>,
}

#[derive(Debug, PartialEq)]
struct Pending {
    version: Version,
    url: String,
    /// Lowercase hex.
    sha256: String,
}

/// The update `release` offers over `current`: `None` if it isn't newer, an error if it is
/// newer but can't be installed safely (no exe asset, or no digest to verify it with).
fn pick_update(current: &Version, release: &Release) -> Result<Option<Pending>, String> {
    let tag = &release.tag_name;
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag))
        .map_err(|_| format!("the latest release's tag {tag} isn't a version"))?;
    if version <= *current {
        return Ok(None);
    }
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == ASSET)
        .ok_or_else(|| format!("release {tag} has no {ASSET}"))?;
    let sha256 = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| format!("release {tag} has no SHA-256 digest for {ASSET}"))?;
    Ok(Some(Pending { version, url: asset.browser_download_url.clone(), sha256: sha256.to_ascii_lowercase() }))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The exe inside a downloaded `.zst`, checked to be a Windows executable.
fn unpack(packed: &[u8]) -> Result<Vec<u8>, String> {
    let decoder = ruzstd::decoding::StreamingDecoder::new(packed).map_err(|err| format!("bad download: {err}"))?;
    let mut exe = Vec::new();
    decoder.take(MAX_EXE + 1).read_to_end(&mut exe).map_err(|err| format!("bad download: {err}"))?;
    if exe.len() as u64 > MAX_EXE || !exe.starts_with(b"MZ") {
        return Err("the download isn't an exe".into());
    }
    Ok(exe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn release(tag: &str, assets: &[(&str, Option<&str>)]) -> Release {
        let assets = assets
            .iter()
            .map(|&(name, digest)| Asset {
                name: name.into(),
                browser_download_url: format!("https://example.com/{name}"),
                digest: digest.map(Into::into),
            })
            .collect();
        Release { tag_name: tag.into(), assets }
    }

    const DIGEST: &str = "sha256:9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08";

    #[test]
    fn only_newer_releases_with_a_verifiable_exe_are_installed() {
        let current = Version::new(1, 0, 0);
        let assets = [("aim_trainer.exe.zst", Some(DIGEST)), ("other.zip", None)];
        let update = pick_update(&current, &release("v1.2.0", &assets)).unwrap().unwrap();
        assert_eq!(update.version, Version::new(1, 2, 0));
        assert_eq!(update.url, "https://example.com/aim_trainer.exe.zst");
        assert_eq!(update.sha256, DIGEST["sha256:".len()..].to_ascii_lowercase());
        // Tags without the v work too; the same or an older version is no update.
        assert!(pick_update(&current, &release("1.0.1", &assets)).unwrap().is_some());
        assert_eq!(pick_update(&current, &release("v1.0.0", &assets)), Ok(None));
        assert_eq!(pick_update(&current, &release("v0.9.0", &assets)), Ok(None));
        // A newer release that can't be installed safely is an error, not silence.
        assert!(pick_update(&current, &release("v1.1.0", &[("other.zip", None)])).is_err());
        assert!(pick_update(&current, &release("v1.1.0", &[("aim_trainer.exe.zst", None)])).is_err());
        assert!(pick_update(&current, &release("v1.1.0", &[("aim_trainer.exe.zst", Some("md5:abc"))])).is_err());
        assert!(pick_update(&current, &release("latest", &assets)).is_err());
    }

    #[test]
    fn reads_github_release_json() {
        let json = r#"{"tag_name":"v1.1.0","name":"1.1","assets":[{"name":"aim_trainer.exe.zst","size":1,
            "browser_download_url":"https://github.com/x/releases/download/v1.1.0/aim_trainer.exe.zst",
            "digest":"sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"}]}"#;
        let release: Release = serde_json::from_str(json).unwrap();
        let update = pick_update(&Version::new(1, 0, 0), &release).unwrap().unwrap();
        // The digest GitHub publishes is the hash of the asset bytes ("test" here).
        assert_eq!(sha256_hex(b"test"), update.sha256);
    }

    #[test]
    fn unpacks_only_executables() {
        // A zstd frame holding "MZ" + padding: one raw block.
        let frame = |content: &[u8]| {
            let mut out = vec![0x28, 0xB5, 0x2F, 0xFD, 0x20, content.len() as u8];
            let header = (content.len() as u32) << 3 | 1; // last block, raw
            out.extend_from_slice(&header.to_le_bytes()[..3]);
            out.extend_from_slice(content);
            out
        };
        assert_eq!(unpack(&frame(b"MZ\x90\x00exe")).unwrap(), b"MZ\x90\x00exe");
        assert!(unpack(&frame(b"<html>")).is_err());
        assert!(unpack(b"not zstd").is_err());
    }

    #[test]
    fn cargo_builds_are_development_builds() {
        let repo = TempDir::new("updater_repo");
        std::fs::write(repo.join("Cargo.toml"), "").unwrap();
        assert!(is_development_build(&repo.join("target").join("release").join("aim_trainer.exe")));
        assert!(is_development_build(&repo.join("target").join("debug").join("aim_trainer.exe")));
        // Anywhere else, including a "target" folder that isn't cargo's, it's an install.
        assert!(!is_development_build(&repo.join("aim_trainer.exe")));
        let other = TempDir::new("updater_install");
        assert!(!is_development_build(&other.join("target").join("aim_trainer.exe")));
    }
}
