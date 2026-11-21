//! Build tasks: `cargo xtask <command>`.
//!
//! - `keygen --private-out <file outside the repo>`: creates the Ed25519 update key pair. The private
//!   key (PKCS#8 PEM) goes to the given path, which must not exist; the public key (PEM) replaces
//!   `crates/medvita_updater/keys/update-public.key`. Same formats as the core's `initialize_keys`.
//! - `sign-feed --feed <releases.win.json> --channel <stable|beta> [--out <file>]`: writes the signed
//!   envelope next to the feed. The private key PEM comes from the `DESKTOP_SIGNING_KEY` environment
//!   variable (CI secret) or `--key-file <file>`.
//! - `verify-feed --feed <file> --envelope <file> [--channel <c>] [--public-key <file>]`: checks an
//!   envelope with the committed public key.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
use medvita_platform::build_info::CLIENT_ID;
use medvita_updater::envelope::{Expectation, parse_public_key, sign_envelope, verify_envelope};

const PUBLIC_KEY_FILE: &str = "crates/medvita_updater/keys/update-public.key";
const KEY_ENV: &str = "DESKTOP_SIGNING_KEY";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let options = parse_options(args)?;
    match command.as_str() {
        "keygen" => keygen(&options),
        "sign-feed" => sign_feed(&options),
        "verify-feed" => verify_feed(&options),
        _ => bail!("usage: cargo xtask <keygen|sign-feed|verify-feed> [--option value ...]"),
    }
}

fn parse_options(args: impl Iterator<Item = String>) -> Result<HashMap<String, String>> {
    let mut options = HashMap::new();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let Some(name) = arg.strip_prefix("--") else {
            bail!("unexpected argument {arg:?}");
        };
        let value = args
            .next()
            .with_context(|| format!("--{name} needs a value"))?;
        options.insert(name.to_owned(), value);
    }
    Ok(options)
}

fn required<'a>(options: &'a HashMap<String, String>, name: &str) -> Result<&'a str> {
    options
        .get(name)
        .map(String::as_str)
        .with_context(|| format!("missing --{name}"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace")
        .to_path_buf()
}

fn keygen(options: &HashMap<String, String>) -> Result<()> {
    let private_out = PathBuf::from(required(options, "private-out")?);
    let root = workspace_root();
    let absolute = std::path::absolute(&private_out)?;
    if absolute.starts_with(&root) {
        bail!("the private key must be written outside the repository");
    }
    if private_out.exists() {
        bail!("{} already exists", private_out.display());
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| anyhow::anyhow!("random source: {error}"))?;
    let key = SigningKey::from_bytes(&seed);
    let private_pem = key.to_pkcs8_pem(LineEnding::LF)?;
    let public_pem = key.verifying_key().to_public_key_pem(LineEnding::LF)?;

    if let Some(dir) = private_out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&private_out)
        .and_then(|mut file| std::io::Write::write_all(&mut file, private_pem.as_bytes()))
        .with_context(|| format!("writing {}", private_out.display()))?;
    let public_out = options
        .get("public-out")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(PUBLIC_KEY_FILE));
    std::fs::write(&public_out, public_pem.as_bytes())?;
    println!(
        "private key: {} (store it as the {KEY_ENV} secret, then delete it)",
        private_out.display()
    );
    println!("public key:  {}", public_out.display());
    Ok(())
}

fn signing_key(options: &HashMap<String, String>) -> Result<SigningKey> {
    let pem = match options.get("key-file") {
        Some(path) => std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?,
        None => std::env::var(KEY_ENV).with_context(|| format!("{KEY_ENV} is not set"))?,
    };
    SigningKey::from_pkcs8_pem(pem.trim()).context("invalid private key (expected PKCS#8 PEM)")
}

fn sign_feed(options: &HashMap<String, String>) -> Result<()> {
    let feed_path = PathBuf::from(required(options, "feed")?);
    let channel = required(options, "channel")?;
    let feed =
        std::fs::read(&feed_path).with_context(|| format!("reading {}", feed_path.display()))?;
    let key = signing_key(options)?;
    let envelope = sign_envelope(&feed, CLIENT_ID, channel, Utc::now(), &key);
    let out = options
        .get("out")
        .map(PathBuf::from)
        .unwrap_or_else(|| feed_path.with_file_name(medvita_updater::ENVELOPE_FILE));
    let mut bytes = serde_json::to_vec_pretty(&envelope)?;
    bytes.push(b'\n');
    std::fs::write(&out, bytes)?;
    println!(
        "{} signed for {channel}, valid until {}",
        out.display(),
        envelope.payload.expires_at
    );
    Ok(())
}

fn verify_feed(options: &HashMap<String, String>) -> Result<()> {
    let feed = std::fs::read(required(options, "feed")?)?;
    let envelope = std::fs::read(required(options, "envelope")?)?;
    let channel = options
        .get("channel")
        .map(String::as_str)
        .unwrap_or("stable");
    let public = match options.get("public-key") {
        Some(path) => std::fs::read_to_string(path)?,
        None => medvita_updater::PUBLIC_KEY_PEM.to_owned(),
    };
    let key = parse_public_key(&public)?;
    let payload = verify_envelope(
        &envelope,
        &feed,
        &key,
        &Expectation {
            client_id: CLIENT_ID,
            channel,
            now: Utc::now(),
            last_signed_at: None,
        },
    )?;
    println!(
        "ok: {} / {} signed at {}, valid until {}",
        payload.client_id, payload.channel, payload.signed_at, payload.expires_at
    );
    Ok(())
}
