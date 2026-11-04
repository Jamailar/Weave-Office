//! Release signing for automatic updates.
//!
//! ```sh
//! # Once: create the key pair. Keep the private key safe and out of the repository;
//! # add the printed public key to crates/updater/update-public-key.txt.
//! cargo run -p weave-updater --example release-signing -- keygen ~/.weave-office/update-signing.key
//! # Each release: sign SHA256SUMS.txt, writing SHA256SUMS.txt.sig next to it.
//! cargo run -p weave-updater --example release-signing -- sign ~/.weave-office/update-signing.key path/to/SHA256SUMS.txt
//! # Check a signature against the keys built into the application.
//! cargo run -p weave-updater --example release-signing -- verify path/to/SHA256SUMS.txt
//! ```
use ed25519_dalek::{Signer, SigningKey};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["keygen", key] => keygen(Path::new(key)),
        ["sign", key, sums] => sign(Path::new(key), Path::new(sums)),
        ["verify", sums] => verify(Path::new(sums)),
        _ => Err("usage: release-signing keygen <private-key> | sign <private-key> <SHA256SUMS.txt> | verify <SHA256SUMS.txt>".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn keygen(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!(
            "{} already exists; refusing to replace a key",
            path.display()
        ));
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| error.to_string())?;
    let key = SigningKey::from_bytes(&seed);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    write_private(path, format!("{}\n", hex::encode(seed)).as_bytes())?;
    println!("private key: {}", path.display());
    println!("public key (add to crates/updater/update-public-key.txt):");
    println!("{}", hex::encode(key.verifying_key().to_bytes()));
    Ok(())
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

fn load(path: &Path) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let seed: [u8; 32] = hex::decode(text.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or("the private key file must hold 64 hex digits")?;
    Ok(SigningKey::from_bytes(&seed))
}

fn signature_path(sums: &Path) -> PathBuf {
    let mut name = sums.file_name().unwrap_or_default().to_os_string();
    name.push(".sig");
    sums.with_file_name(name)
}

fn sign(key: &Path, sums: &Path) -> Result<(), String> {
    let key = load(key)?;
    let message = std::fs::read(sums).map_err(|error| error.to_string())?;
    // Refuse a malformed file now rather than in every installed copy later.
    let entries = weave_updater::verify::parse_checksums(&message)?;
    let public = hex::encode(key.verifying_key().to_bytes());
    if !weave_updater::PUBLIC_KEYS.contains(&public) {
        eprintln!(
            "warning: this key is not in update-public-key.txt; installed copies will refuse it"
        );
    }
    let output = signature_path(sums);
    std::fs::write(
        &output,
        format!("{}\n", hex::encode(key.sign(&message).to_bytes())),
    )
    .map_err(|error| error.to_string())?;
    println!("signed {} files: {}", entries.len(), output.display());
    Ok(())
}

fn verify(sums: &Path) -> Result<(), String> {
    let keys = weave_updater::verify::parse_keys(weave_updater::PUBLIC_KEYS)?;
    let message = std::fs::read(sums).map_err(|error| error.to_string())?;
    let signature = std::fs::read(signature_path(sums)).map_err(|error| error.to_string())?;
    weave_updater::verify::verify_signature(&message, &signature, &keys)?;
    println!("signature valid");
    Ok(())
}
