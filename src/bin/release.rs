//! The maintainer's release step; not part of the game. Builds, packs, signs and publishes a
//! release from a pushed `v<major>.<minor>.<patch>` tag, on this machine:
//!
//! 1. checks out the tag into a fresh worktree (never the working copy, so local edits can't
//!    slip in) and checks it builds as that version;
//! 2. runs `cargo fmt --check`, clippy and the tests there, then builds the release exe, with
//!    the toolchain pinned in `rust-toolchain.toml` and `--locked`;
//! 3. packs the exe with zstd (level 19), unpacks it again the way installed copies do, signs
//!    the `.zst` with the offline release key and checks the signature against
//!    `signing::PUBLIC_KEY`;
//! 4. uploads `aim_trainer.exe`, `aim_trainer.exe.zst` and `aim_trainer.exe.zst.sig` to a draft
//!    release with the GitHub CLI, then publishes it: installed copies never see it unsigned.
//!
//! Run from the repository:
//! `cargo run --bin release --features release-tool -- --key <key file> v1.2.0`, or set
//! `AIM_TRAINER_RELEASE_KEY` to the key file instead of passing `--key`. The key file holds the
//! 32-byte Ed25519 seed as 64 hex digits. Builds share `target/release-tool`, so later releases
//! only rebuild what changed.

#[path = "../signing.rs"] mod signing;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::{env, fs};

use ed25519_dalek::{Signer, SigningKey};
use semver::Version;

const EXE: &str = "aim_trainer.exe";
const ASSET: &str = "aim_trainer.exe.zst";
const SIGNATURE_ASSET: &str = "aim_trainer.exe.zst.sig";
/// zstd level of the packed exe: slow to pack, small to download, quick to unpack.
const ZSTD_LEVEL: i32 = 19;

fn main() -> ExitCode {
    match run() {
        Ok(tag) => {
            println!("Built, signed and published {tag}.");
            ExitCode::SUCCESS
        },
        Err(err) => {
            eprintln!("release: {err}");
            ExitCode::FAILURE
        },
    }
}

const USAGE: &str = "usage: cargo run --bin release --features release-tool -- [--key <key file>] v<major>.<minor>.<patch>\n\
     (without --key, the key file comes from AIM_TRAINER_RELEASE_KEY)";

fn run() -> Result<String, String> {
    let (tag, key_path) = parse_args(env::args().skip(1))?;
    // Releases only: installed copies ignore pre-releases.
    let version = tag
        .strip_prefix('v')
        .and_then(|version| Version::parse(version).ok())
        .filter(|version| version.pre.is_empty() && version.build.is_empty() && format!("v{version}") == tag)
        .ok_or_else(|| format!("{tag} isn't a release tag (v<major>.<minor>.<patch>)"))?;
    let key = signing_key(&key_path)?;
    let repo = env::current_dir().map_err(|err| format!("no current directory: {err}"))?;
    check_tag_pushed(&tag)?;
    let existing = release_state(&tag)?;
    if existing == Some(false) {
        return Err(format!("{tag} is already published"));
    }

    let dir = env::temp_dir().join(format!("aim_trainer_release_{tag}"));
    let checkout = dir.join("src");
    remove_worktree(&checkout);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|err| format!("couldn't create {}: {err}", dir.display()))?;
    run_cmd(Command::new("git").args(["worktree", "add", "--detach"]).arg(&checkout).arg(&tag))?;
    let released = build_and_publish(&tag, &version, &key, &repo, &checkout, &dir, existing.is_some());
    remove_worktree(&checkout);
    let _ = fs::remove_dir_all(&dir);
    released.map(|()| tag)
}

/// The tag and the key file: `--key <path>` (anywhere) or else `AIM_TRAINER_RELEASE_KEY`.
fn parse_args(args: impl Iterator<Item = String>) -> Result<(String, PathBuf), String> {
    let (mut tag, mut key) = (None, None);
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--key" => key = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            _ if tag.is_none() && !arg.starts_with('-') => tag = Some(arg),
            _ => return Err(USAGE.into()),
        }
    }
    let key = key
        .or_else(|| env::var_os("AIM_TRAINER_RELEASE_KEY").map(PathBuf::from))
        .ok_or_else(|| format!("no signing key: pass --key <key file> or set AIM_TRAINER_RELEASE_KEY\n{USAGE}"))?;
    Ok((tag.ok_or(USAGE)?, key))
}

/// The tag exists here and on GitHub, at the same commit.
fn check_tag_pushed(tag: &str) -> Result<(), String> {
    let local = run_cmd(Command::new("git").args(["rev-parse", "--verify", &format!("refs/tags/{tag}^{{commit}}")]))
        .map_err(|_| format!("no tag {tag} here: tag the release commit and push the tag first"))?;
    let remote = run_cmd(Command::new("git").args(["ls-remote", "--tags", "origin", &format!("refs/tags/{tag}*")]))?;
    // An annotated tag lists the tag object and then its commit (`^{}`); the commit decides.
    let pushed = remote
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(_, name)| *name == format!("refs/tags/{tag}") || *name == format!("refs/tags/{tag}^{{}}"))
        .map(|(hash, _)| hash)
        .next_back();
    match pushed {
        Some(hash) if hash == local.trim() => Ok(()),
        Some(_) => Err(format!("{tag} on GitHub isn't the local {tag}")),
        None => Err(format!("{tag} isn't pushed: git push origin {tag}")),
    }
}

/// `None` if there is no release for `tag`, else whether it is a draft.
fn release_state(tag: &str) -> Result<Option<bool>, String> {
    let output = Command::new("gh")
        .args(["release", "view", tag, "--json", "isDraft", "--jq", ".isDraft"])
        .output()
        .map_err(|err| format!("couldn't run gh (is the GitHub CLI installed?): {err}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return if message.contains("release not found") { Ok(None) } else { Err(format!("gh: {}", message.trim())) };
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).trim() == "true"))
}

fn build_and_publish(
    tag: &str,
    version: &Version,
    key: &SigningKey,
    repo: &Path,
    checkout: &Path,
    dir: &Path,
    draft_exists: bool,
) -> Result<(), String> {
    // The exe reports Cargo.toml's version: a tag that differs would make installs update on
    // every start.
    let manifest = fs::read_to_string(checkout.join("Cargo.toml")).map_err(|err| format!("no Cargo.toml: {err}"))?;
    let built = manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or("no version in Cargo.toml")?;
    if built != version.to_string() {
        return Err(format!("{tag} builds version {built}: bump Cargo.toml and tag that commit"));
    }

    let target = repo.join("target").join("release-tool");
    let cargo = |args: &[&str]| {
        let mut command = Command::new("cargo");
        command.args(args).current_dir(checkout).env("CARGO_TARGET_DIR", &target);
        command
    };
    for args in [
        &["fmt", "--check"][..],
        &["clippy", "--all-targets", "--all-features", "--locked", "--", "-D", "warnings"],
        &["test", "--locked"],
        &["build", "--release", "--locked", "--bin", "aim_trainer"],
    ] {
        println!("release: cargo {}", args.join(" "));
        run_inherited(&mut cargo(args))?;
    }

    let exe =
        fs::read(target.join("release").join(EXE)).map_err(|err| format!("couldn't read the built exe: {err}"))?;
    println!("release: packing {} MB", exe.len() >> 20);
    let packed = zstd::encode_all(&exe[..], ZSTD_LEVEL).map_err(|err| format!("couldn't pack the exe: {err}"))?;
    // Installed copies unpack with ruzstd; make sure that gives back the exact exe.
    let mut unpacked = Vec::with_capacity(exe.len());
    ruzstd::decoding::StreamingDecoder::new(&packed[..])
        .map_err(|err| format!("the packed exe doesn't unpack: {err}"))?
        .read_to_end(&mut unpacked)
        .map_err(|err| format!("the packed exe doesn't unpack: {err}"))?;
    if unpacked != exe {
        return Err("the packed exe unpacks to something else".into());
    }
    let mut signature: String = key.sign(&packed).to_bytes().iter().map(|byte| format!("{byte:02x}")).collect();
    signature.push('\n');
    // The check installed copies make, with the key built into them.
    if !signing::verify(&packed, &signature) {
        return Err("the signature doesn't verify with signing::PUBLIC_KEY".into());
    }
    let assets = [(EXE, exe), (ASSET, packed), (SIGNATURE_ASSET, signature.into_bytes())];
    let mut paths = Vec::with_capacity(assets.len());
    for (name, bytes) in assets {
        let path = dir.join(name);
        fs::write(&path, bytes).map_err(|err| format!("couldn't write {name}: {err}"))?;
        paths.push(path);
    }

    // Everything goes up while the release is a draft, then it is published in one step.
    if draft_exists {
        run_inherited(Command::new("gh").args(["release", "upload", tag, "--clobber"]).args(&paths))?;
    } else {
        run_inherited(
            Command::new("gh")
                .args(["release", "create", tag, "--draft", "--verify-tag", "--title", tag, "--generate-notes"])
                .args(&paths),
        )?;
    }
    run_inherited(Command::new("gh").args(["release", "edit", tag, "--draft=false"]))
}

/// The release key in the file at `path`, checked to be the one installed copies trust.
fn signing_key(path: &Path) -> Result<SigningKey, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("couldn't read {}: {err}", path.display()))?;
    let seed = signing::decode_hex::<32>(text.trim_end())
        .ok_or_else(|| format!("{} doesn't hold a key (64 hex digits)", path.display()))?;
    let key = SigningKey::from_bytes(&seed);
    if key.verifying_key().to_bytes() != signing::PUBLIC_KEY {
        return Err(format!("{} isn't the release key: its public key isn't signing::PUBLIC_KEY", path.display()));
    }
    Ok(key)
}

/// Removes a worktree left behind by an earlier, interrupted run.
fn remove_worktree(path: &Path) {
    let _ = Command::new("git").args(["worktree", "remove", "--force"]).arg(path).output();
    let _ = Command::new("git").args(["worktree", "prune"]).output();
}

/// Runs a command with its output on the terminal.
fn run_inherited(command: &mut Command) -> Result<(), String> {
    let status = command.status().map_err(|err| format!("couldn't run {command:?}: {err}"))?;
    if status.success() { Ok(()) } else { Err(format!("{command:?} failed")) }
}

/// Runs a command and returns what it printed; its errors go straight to the terminal.
fn run_cmd(command: &mut Command) -> Result<String, String> {
    let output = command.stderr(Stdio::inherit()).output().map_err(|err| format!("couldn't run {command:?}: {err}"))?;
    if !output.status.success() {
        return Err(format!("{command:?} failed"));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{command:?} printed something that isn't UTF-8"))
}
