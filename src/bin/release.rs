//! The maintainer's release step; not part of the game. The release workflow leaves a draft
//! release with `aim_trainer.exe` and `aim_trainer.exe.zst`. This downloads the draft's `.zst`
//! with the GitHub CLI, signs it with the offline key in
//! `%USERPROFILE%\.aim_trainer\release_signing.key`, checks the signature the way installed copies
//! do, uploads it as `aim_trainer.exe.zst.sig` and publishes the draft (see ARCHITECTURE.md,
//! Releases).
//!
//! Run from the repository: `cargo run --bin release -- v1.2.0`

#[path = "../signing.rs"] mod signing;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::{env, fs};

use ed25519_dalek::{Signer, SigningKey};
use semver::Version;

const ASSET: &str = "aim_trainer.exe.zst";
const SIGNATURE_ASSET: &str = "aim_trainer.exe.zst.sig";

fn main() -> ExitCode {
    match run() {
        Ok(tag) => {
            println!("Signed and published {tag}.");
            ExitCode::SUCCESS
        },
        Err(err) => {
            eprintln!("release: {err}");
            ExitCode::FAILURE
        },
    }
}

fn run() -> Result<String, String> {
    let mut args = env::args().skip(1);
    let (Some(tag), None) = (args.next(), args.next()) else {
        return Err("usage: cargo run --bin release -- v<major>.<minor>.<patch>".into());
    };
    // The same tags the release workflow accepts.
    let release_tag = tag
        .strip_prefix('v')
        .and_then(|version| Version::parse(version).ok())
        .is_some_and(|version| version.pre.is_empty() && version.build.is_empty() && format!("v{version}") == tag);
    if !release_tag {
        return Err(format!("{tag} isn't a release tag (v<major>.<minor>.<patch>)"));
    }
    let key = signing_key()?;
    // Only the workflow's drafts: a release must never be public without its signature.
    if run_gh(gh().args(["release", "view", tag.as_str(), "--json", "isDraft", "--jq", ".isDraft"]))?.trim() != "true" {
        return Err(format!("{tag} isn't a draft release"));
    }
    let dir = env::temp_dir().join(format!("aim_trainer_release_{tag}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|err| format!("couldn't create {}: {err}", dir.display()))?;
    let published = sign_and_publish(&tag, &key, &dir);
    let _ = fs::remove_dir_all(&dir);
    published.map(|()| tag)
}

/// The release key, checked to be the one installed copies trust.
fn signing_key() -> Result<SigningKey, String> {
    let home = env::var_os("USERPROFILE").ok_or("USERPROFILE isn't set")?;
    let path = PathBuf::from(home).join(".aim_trainer").join("release_signing.key");
    let text = fs::read_to_string(&path).map_err(|err| format!("couldn't read {}: {err}", path.display()))?;
    let seed = signing::decode_hex::<32>(text.trim_end())
        .ok_or_else(|| format!("{} doesn't hold a key (64 hex digits)", path.display()))?;
    let key = SigningKey::from_bytes(&seed);
    if key.verifying_key().to_bytes() != signing::PUBLIC_KEY {
        return Err(format!("{} isn't the release key: its public key isn't signing::PUBLIC_KEY", path.display()));
    }
    Ok(key)
}

/// Signs the draft's `.zst` and uploads the signature, then publishes the draft.
fn sign_and_publish(tag: &str, key: &SigningKey, dir: &Path) -> Result<(), String> {
    run_gh(gh().args(["release", "download", tag, "--pattern", ASSET, "--dir"]).arg(dir))?;
    let packed = fs::read(dir.join(ASSET)).map_err(|err| format!("couldn't read the downloaded {ASSET}: {err}"))?;
    let mut signature: String = key.sign(&packed).to_bytes().iter().map(|byte| format!("{byte:02x}")).collect();
    signature.push('\n');
    // The check installed copies make, with the key built into them.
    if !signing::verify(&packed, &signature) {
        return Err("the signature doesn't verify with signing::PUBLIC_KEY".into());
    }
    let signature_path = dir.join(SIGNATURE_ASSET);
    fs::write(&signature_path, signature).map_err(|err| format!("couldn't write {SIGNATURE_ASSET}: {err}"))?;
    run_gh(gh().args(["release", "upload", tag, "--clobber"]).arg(&signature_path))?;
    run_gh(gh().args(["release", "edit", tag, "--draft=false"]))?;
    Ok(())
}

fn gh() -> Command {
    Command::new("gh")
}

/// Runs a `gh` command and returns what it printed; its errors go straight to the terminal.
fn run_gh(command: &mut Command) -> Result<String, String> {
    let output = command
        .stderr(Stdio::inherit())
        .output()
        .map_err(|err| format!("couldn't run gh (is the GitHub CLI installed?): {err}"))?;
    if !output.status.success() {
        return Err(format!("{command:?} failed"));
    }
    String::from_utf8(output.stdout).map_err(|_| "gh printed something that isn't UTF-8".into())
}
