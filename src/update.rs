//! Self-updating from the GitHub releases, in two steps so the install folder always holds a
//! working exe:
//! - At startup (and when switched on) a background thread asks GitHub for the latest release.
//!   If it is newer than this build it downloads the zstd-compressed exe, checks it against the
//!   SHA-256 digest GitHub publishes and the release key's signature (`signing.rs`), unpacks it
//!   and stages it next to the exe as `<exe>.update`. It never touches the running exe.
//! - Before the app starts (`Updater::install_staged_update`, from `main`), or after it has quit
//!   for the menu's Restart button, a staged exe is swapped in with two renames and started.
//!
//! Both steps hold an exclusive lock on `aim_trainer.update.lock` next to the exe, so two running
//! copies never stage or swap at the same time.
//!
//! Development builds (an exe inside a cargo `target` directory) never update themselves.

use std::fs::{self, File, TryLockError};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use bevy::prelude::*;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::persist::{with_suffix, write_synced};
use crate::settings::Settings;
use crate::signing;

const LATEST_RELEASE: &str = "https://api.github.com/repos/LeagueRaINi/aim-trainer/releases/latest";
/// The release asset with the Windows exe, zstd-compressed (see `src/bin/release.rs`).
const ASSET: &str = "aim_trainer.exe.zst";
/// The release key's signature of `ASSET` (see `src/bin/release.rs`).
const SIGNATURE_ASSET: &str = "aim_trainer.exe.zst.sig";
/// Largest download and unpacked exe accepted, so a bad response can't exhaust memory.
const MAX_DOWNLOAD: u64 = 256 << 20;
const MAX_EXE: u64 = 512 << 20;
/// A signature is 128 hex digits and a newline.
const MAX_SIGNATURE: u64 = 1024;
/// Held while staging or swapping an update; next to the exe.
const LOCK_FILE: &str = "aim_trainer.update.lock";

/// Checks for updates; `main` creates the `Updater` first, to install a staged update before the
/// app starts and restart after it quits.
pub struct UpdatePlugin(pub Updater);

impl Plugin for UpdatePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone()).add_systems(Update, start_check.run_if(resource_changed::<Settings>));
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
    /// This build doesn't update itself (now); the reason.
    Unavailable(&'static str),
    Checking,
    UpToDate,
    /// GitHub has no release for the repository (it was renamed, made private or deleted).
    NoReleases,
    Downloading(Version),
    /// Downloaded, verified and staged; installed at the next start or on restart.
    Ready(Version),
    Failed(String),
}

/// The update check's state, shared with its thread, and the install it updates.
#[derive(Resource, Clone)]
pub struct Updater {
    status: Arc<Mutex<UpdateStatus>>,
    /// Set by the Restart button; `main` acts on it once the app has quit (and saved).
    restart: Arc<AtomicBool>,
    /// Where this exe runs from; `None` if unknown.
    install: Option<Install>,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(UpdateStatus::Idle)),
            restart: Arc::default(),
            install: std::env::current_exe().ok().and_then(Install::new),
        }
    }
}

impl Updater {
    pub fn status(&self) -> MutexGuard<'_, UpdateStatus> {
        self.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set(&self, status: UpdateStatus) {
        *self.status() = status;
    }

    /// Before the app starts: installs an update staged by an earlier run and starts it. Returns
    /// whether it did, so this process should exit right away.
    pub fn install_staged_update(&self) -> bool {
        let Some(install) = &self.install else { return false };
        let failed = match install.swap_staged() {
            Ok(false) => return false,
            Ok(true) => match spawn(&install.exe) {
                Ok(()) => return true,
                // This process runs on; the new exe is in place for the next start.
                Err(err) => format!("couldn't start the installed update: {err}"),
            },
            // The staged exe stays, for another try at the next start.
            Err(err) => format!("couldn't install the downloaded update: {err}"),
        };
        self.set(UpdateStatus::Failed(failed));
        false
    }

    /// Restart into the staged update once the app has quit.
    pub fn request_restart(&self) {
        self.restart.store(true, Ordering::Relaxed);
    }

    /// After the app has quit: if Restart was clicked, installs the staged update and starts the
    /// exe again. A failed install still restarts, into the old version, which retries it.
    pub fn restart_if_requested(&self) {
        let Some(install) = &self.install else { return };
        if !self.restart.load(Ordering::Relaxed) {
            return;
        }
        if let Err(err) = install.swap_staged() {
            error!("Installing the update failed: {err}");
        }
        if let Err(err) = spawn(&install.exe) {
            error!("Restarting failed: {err}");
        }
    }
}

/// Starts `exe` with this process's arguments. It inherits the working directory, where the data
/// files are.
fn spawn(exe: &Path) -> std::io::Result<()> {
    Command::new(exe).args(std::env::args_os().skip(1)).spawn().map(drop)
}

/// An installed exe and the update files next to it.
#[derive(Clone)]
struct Install {
    exe: PathBuf,
    /// A verified update; written to `partial`, synced, then renamed, so it only ever exists
    /// complete.
    staged: PathBuf,
    /// Left behind if staging was cut short.
    partial: PathBuf,
    /// The previous exe after a swap; it may still be running, so it goes at the next start.
    old: PathBuf,
    lock: PathBuf,
}

impl Install {
    fn new(exe: PathBuf) -> Option<Self> {
        let lock = exe.parent()?.join(LOCK_FILE);
        let staged = with_suffix(&exe, ".update");
        Some(Self { partial: with_suffix(&staged, ".tmp"), old: with_suffix(&exe, ".old"), staged, lock, exe })
    }

    /// Takes the update lock, held until the file is dropped (or the process ends): `Ok(None)`
    /// while another running copy holds it, an error if the folder isn't writable.
    fn lock(&self) -> io::Result<Option<File>> {
        let file = File::options().read(true).write(true).create(true).truncate(false).open(&self.lock)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(err)) => Err(err),
        }
    }

    /// Removes what earlier updates left behind and swaps a staged exe in, under the update lock.
    /// Returns whether it swapped; skips everything while another running copy holds the lock.
    fn swap_staged(&self) -> io::Result<bool> {
        // The usual start: nothing to do, and no lock file created.
        if ![&self.staged, &self.partial, &self.old].into_iter().any(|path| path.exists()) {
            return Ok(false);
        }
        let Some(_lock) = self.lock()? else { return Ok(false) };
        let _ = fs::remove_file(&self.old);
        let _ = fs::remove_file(&self.partial);
        if !self.staged.is_file() {
            return Ok(false);
        }
        swap_in(&self.exe, &self.staged, &self.old, rename_retrying)?;
        Ok(true)
    }
}

/// Puts `staged` in place of `exe`, keeping the old exe as `old`. If the new exe can't be moved
/// in, the old one is moved back, so `exe` exists afterwards whatever fails (short of the move
/// back failing too).
fn swap_in(exe: &Path, staged: &Path, old: &Path, rename: impl Fn(&Path, &Path) -> io::Result<()>) -> io::Result<()> {
    fs::rename(exe, old)?;
    if let Err(err) = rename(staged, exe) {
        rename(old, exe)?;
        return Err(err);
    }
    Ok(())
}

/// `fs::rename`, retried for up to 2 s while another process has the file open, as virus
/// scanners do with a freshly written exe.
fn rename_retrying(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempts = 0;
    loop {
        match fs::rename(from, to) {
            // Access denied, sharing violation, lock violation.
            Err(err) if attempts < 20 && cfg!(windows) && matches!(err.raw_os_error(), Some(5 | 32 | 33)) => {
                attempts += 1;
                thread::sleep(Duration::from_millis(100));
            },
            result => return result,
        }
    }
}

/// Starts the check once updates are on: at startup, or when switched on in the menu.
fn start_check(settings: Res<Settings>, updater: Res<Updater>) {
    if !matches!(*updater.status(), UpdateStatus::Idle | UpdateStatus::Off) {
        return;
    }
    if !settings.updates.auto {
        return updater.set(UpdateStatus::Off);
    }
    let Some(install) = updater.install.clone() else {
        return updater.set(UpdateStatus::Unavailable("its location is unknown"));
    };
    if !cfg!(windows) {
        return updater.set(UpdateStatus::Unavailable("releases are for Windows"));
    }
    if is_development_build(&install.exe) {
        return updater.set(UpdateStatus::Unavailable("development build"));
    }
    updater.set(UpdateStatus::Checking);
    let worker = updater.clone();
    let spawned = thread::Builder::new().name("updater".into()).spawn(move || {
        let current = Version::parse(current_version()).expect("the package version is semver");
        let status = stage_latest(&install, &current, &worker).unwrap_or_else(|err| {
            warn!("Update failed: {err}");
            UpdateStatus::Failed(err)
        });
        if let UpdateStatus::Ready(version) = &status {
            info!("Downloaded v{version}; it is installed at the next start");
        }
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

/// Checks for a newer release and stages it next to the install's exe; returns the resulting
/// status (`Ready` once staged).
fn stage_latest(install: &Install, current: &Version, updater: &Updater) -> Result<UpdateStatus, String> {
    // Held until the staged exe is complete; checked before downloading anything.
    let _lock = match install.lock() {
        Ok(Some(lock)) => lock,
        Ok(None) => return Ok(UpdateStatus::Unavailable("another running copy is updating it")),
        Err(_) => return Ok(UpdateStatus::Unavailable("the install folder isn't writable")),
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(10)))
        // Per read rather than in total, so a slow but steady connection still finishes.
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .user_agent(concat!("aim_trainer/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let release: Release = match agent.get(LATEST_RELEASE).header("Accept", "application/vnd.github+json").call() {
        Err(ureq::Error::StatusCode(404)) => return Ok(UpdateStatus::NoReleases),
        response => response
            .and_then(|mut response| response.body_mut().read_json())
            .map_err(|err| format!("couldn't check GitHub: {err}"))?,
    };
    let Some(update) = pick_update(current, &release)? else { return Ok(UpdateStatus::UpToDate) };
    updater.set(UpdateStatus::Downloading(update.version.clone()));
    let signature = agent
        .get(&update.signature_url)
        .call()
        .and_then(|mut response| response.body_mut().with_config().limit(MAX_SIGNATURE).read_to_string())
        .map_err(|err| format!("download failed: {err}"))?;
    let packed = agent
        .get(&update.url)
        .call()
        .and_then(|mut response| response.body_mut().with_config().limit(MAX_DOWNLOAD).read_to_vec())
        .map_err(|err| format!("download failed: {err}"))?;
    if sha256_hex(&packed) != update.sha256 {
        return Err("the download doesn't match its published checksum".into());
    }
    if !signing::verify(&packed, &signature) {
        return Err("the download isn't signed with the release key".into());
    }
    let exe = unpack(&packed)?;
    write_synced(&install.partial, &exe)
        .and_then(|()| rename_retrying(&install.partial, &install.staged))
        .map_err(|err| format!("couldn't save the update: {err}"))?;
    Ok(UpdateStatus::Ready(update.version))
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
    signature_url: String,
    /// Lowercase hex.
    sha256: String,
}

/// The update `release` offers over `current`: `None` if it isn't newer (or is a pre-release),
/// an error if it is newer but can't be installed safely (no exe asset, no signature, or no
/// digest).
fn pick_update(current: &Version, release: &Release) -> Result<Option<Pending>, String> {
    let tag = &release.tag_name;
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag))
        .map_err(|_| format!("the latest release's tag {tag} isn't a version"))?;
    if version <= *current || !version.pre.is_empty() {
        return Ok(None);
    }
    let asset = |name: &str| {
        release.assets.iter().find(|asset| asset.name == name).ok_or_else(|| format!("release {tag} has no {name}"))
    };
    let exe = asset(ASSET)?;
    let signature = asset(SIGNATURE_ASSET)?;
    let sha256 = exe
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| format!("release {tag} has no SHA-256 digest for {ASSET}"))?;
    Ok(Some(Pending {
        version,
        url: exe.browser_download_url.clone(),
        signature_url: signature.browser_download_url.clone(),
        sha256: sha256.to_ascii_lowercase(),
    }))
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
        let assets = [("aim_trainer.exe.zst", Some(DIGEST)), ("aim_trainer.exe.zst.sig", None), ("other.zip", None)];
        let update = pick_update(&current, &release("v1.2.0", &assets)).unwrap().unwrap();
        assert_eq!(update.version, Version::new(1, 2, 0));
        assert_eq!(update.url, "https://example.com/aim_trainer.exe.zst");
        assert_eq!(update.signature_url, "https://example.com/aim_trainer.exe.zst.sig");
        assert_eq!(update.sha256, DIGEST["sha256:".len()..].to_ascii_lowercase());
        // Tags without the v work too; the same or an older version, or a pre-release, is no update.
        assert!(pick_update(&current, &release("1.0.1", &assets)).unwrap().is_some());
        assert_eq!(pick_update(&current, &release("v1.0.0", &assets)), Ok(None));
        assert_eq!(pick_update(&current, &release("v0.9.0", &assets)), Ok(None));
        assert_eq!(pick_update(&current, &release("v1.1.0-rc.1", &assets)), Ok(None));
        // A newer release that can't be installed safely is an error, not silence.
        let unsigned = [("aim_trainer.exe.zst", Some(DIGEST))];
        assert!(pick_update(&current, &release("v1.1.0", &unsigned)).is_err());
        let sig = ("aim_trainer.exe.zst.sig", None);
        assert!(pick_update(&current, &release("v1.1.0", &[("other.zip", None), sig])).is_err());
        assert!(pick_update(&current, &release("v1.1.0", &[("aim_trainer.exe.zst", None), sig])).is_err());
        assert!(pick_update(&current, &release("v1.1.0", &[("aim_trainer.exe.zst", Some("md5:abc")), sig])).is_err());
        assert!(pick_update(&current, &release("latest", &assets)).is_err());
    }

    #[test]
    fn reads_github_release_json() {
        let json = r#"{"tag_name":"v1.1.0","name":"1.1","assets":[{"name":"aim_trainer.exe.zst","size":1,
            "browser_download_url":"https://github.com/x/releases/download/v1.1.0/aim_trainer.exe.zst",
            "digest":"sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"},
            {"name":"aim_trainer.exe.zst.sig","size":129,
            "browser_download_url":"https://github.com/x/releases/download/v1.1.0/aim_trainer.exe.zst.sig",
            "digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}]}"#;
        let release: Release = serde_json::from_str(json).unwrap();
        let update = pick_update(&Version::new(1, 0, 0), &release).unwrap().unwrap();
        // The digest GitHub publishes is the hash of the asset bytes ("test" here).
        assert_eq!(sha256_hex(b"test"), update.sha256);
        assert!(update.signature_url.ends_with("/aim_trainer.exe.zst.sig"));
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

    /// An install of a fake exe in `dir`.
    fn fake_install(dir: &TempDir) -> Install {
        let install = Install::new(dir.join("aim_trainer.exe")).unwrap();
        fs::write(&install.exe, "old").unwrap();
        install
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn staged_update_is_swapped_in() {
        let dir = TempDir::new("updater_swap");
        let install = fake_install(&dir);
        // Nothing staged: nothing happens, not even a lock file.
        assert!(!install.swap_staged().unwrap());
        assert!(!install.lock.exists());

        fs::write(&install.staged, "new").unwrap();
        fs::write(&install.partial, "half").unwrap();
        assert!(install.swap_staged().unwrap());
        assert_eq!(read(&install.exe), "new");
        assert_eq!(read(&install.old), "old");
        assert!(!install.staged.exists() && !install.partial.exists());

        // The next start removes the old exe and leaves the new one.
        assert!(!install.swap_staged().unwrap());
        assert!(!install.old.exists());
        assert_eq!(read(&install.exe), "new");
    }

    #[test]
    fn failed_swap_puts_the_old_exe_back() {
        let dir = TempDir::new("updater_rollback");
        let install = fake_install(&dir);
        fs::write(&install.staged, "new").unwrap();
        let swapped = swap_in(&install.exe, &install.staged, &install.old, |from, to| {
            if from == install.staged.as_path() {
                Err(io::ErrorKind::PermissionDenied.into())
            } else {
                fs::rename(from, to)
            }
        });
        assert!(swapped.is_err());
        assert_eq!(read(&install.exe), "old");
        assert!(!install.old.exists());
        // The staged exe stays for the next try.
        assert_eq!(read(&install.staged), "new");
    }

    #[test]
    fn a_held_lock_skips_staging_and_swapping() {
        let dir = TempDir::new("updater_lock");
        let install = fake_install(&dir);
        fs::write(&install.staged, "new").unwrap();
        let held = install.lock().unwrap().expect("the lock is free");
        assert!(install.lock().unwrap().is_none());

        // Another running copy is updating: neither a check nor a swap goes ahead.
        let busy = Ok(UpdateStatus::Unavailable("another running copy is updating it"));
        assert_eq!(stage_latest(&install, &Version::new(1, 0, 0), &Updater::default()), busy);
        assert!(!install.swap_staged().unwrap());
        assert_eq!(read(&install.exe), "old");

        drop(held);
        assert!(install.swap_staged().unwrap());
        assert_eq!(read(&install.exe), "new");
    }

    #[test]
    fn unwritable_install_is_unavailable_before_downloading() {
        let dir = TempDir::new("updater_unwritable");
        // A folder that doesn't exist can't hold the lock file, like a read-only one.
        let install = Install::new(dir.join("missing").join("aim_trainer.exe")).unwrap();
        let unwritable = Ok(UpdateStatus::Unavailable("the install folder isn't writable"));
        assert_eq!(stage_latest(&install, &Version::new(1, 0, 0), &Updater::default()), unwritable);
    }
}
