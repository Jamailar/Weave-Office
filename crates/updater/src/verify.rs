//! A release is trusted when its `SHA256SUMS.txt` carries an Ed25519 signature from a key built
//! into the application, and the downloaded installer matches the checksum listed there.
//! Neither GitHub nor the network can substitute an installer without the release key.
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Public keys, one 64-digit hex key per line. `#` starts a comment. Several keys allow a
/// rotation: releases signed with either key are accepted while both are listed.
pub fn parse_keys(text: &str) -> Result<Vec<VerifyingKey>, String> {
    let mut keys = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let bytes: [u8; 32] = hex::decode(line)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| format!("not an Ed25519 public key: {line}"))?;
        keys.push(VerifyingKey::from_bytes(&bytes).map_err(|error| error.to_string())?);
    }
    Ok(keys)
}

/// The signature file holds the 64-byte signature of `SHA256SUMS.txt` as 128 hex digits.
pub fn verify_signature(
    message: &[u8],
    signature: &[u8],
    keys: &[VerifyingKey],
) -> Result<(), String> {
    let text = std::str::from_utf8(signature).map_err(|_| "signature is not text".to_owned())?;
    let bytes: [u8; 64] = hex::decode(text.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| "signature is not 128 hex digits".to_owned())?;
    let signature = Signature::from_bytes(&bytes);
    if keys.is_empty() {
        return Err("this build has no update key".into());
    }
    if keys
        .iter()
        .any(|key| key.verify_strict(message, &signature).is_ok())
    {
        Ok(())
    } else {
        Err("checksums are not signed by the release key".into())
    }
}

/// `sha256sum` output: `<64 hex>  <name>` (binary mode `*name` too).
pub fn parse_checksums(text: &[u8]) -> Result<HashMap<String, [u8; 32]>, String> {
    let text = std::str::from_utf8(text).map_err(|_| "checksums are not text".to_owned())?;
    let mut sums = HashMap::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let (digest, name) = line
            .split_once(char::is_whitespace)
            .ok_or_else(|| format!("checksum line: {line}"))?;
        let name = name.trim_start();
        let name = name.strip_prefix('*').unwrap_or(name);
        let digest: [u8; 32] = hex::decode(digest)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| format!("checksum line: {line}"))?;
        if name.is_empty() || sums.insert(name.to_owned(), digest).is_some() {
            return Err(format!("checksum line: {line}"));
        }
    }
    Ok(sums)
}

/// Verify the signature, then return the signed checksum for `package`.
pub fn signed_checksum(
    checksums: &[u8],
    signature: &[u8],
    keys: &[VerifyingKey],
    package: &str,
) -> Result<[u8; 32], String> {
    verify_signature(checksums, signature, keys)?;
    parse_checksums(checksums)?
        .remove(package)
        .ok_or_else(|| format!("{package} is not in the signed checksums"))
}

pub fn sha256_file(path: &std::path::Path) -> std::io::Result<[u8; 32]> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        match file.read(&mut buffer)? {
            0 => return Ok(hasher.finalize().into()),
            read => hasher.update(&buffer[..read]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }
    const SUMS: &[u8] = b"e88d47ba9ec0270c913edc3a21aad8499102a16a6afaa7d7734c4d137da6f5d2  WeaveOffice-0.1.2-macos-arm64.dmg\n\
6f057592f6099a13ac2f56c56e1b9202c4194610278b4ee070f0169174ac93a8 *WeaveOffice-0.1.2-windows-x64-setup.exe\n";

    #[test]
    fn signed_checksums_are_accepted_from_any_listed_key() {
        let signer = key(7);
        let signature = hex::encode(signer.sign(SUMS).to_bytes());
        let keys = parse_keys(&format!(
            "# release keys\n{}\n{}  # new key\n",
            hex::encode(key(1).verifying_key().to_bytes()),
            hex::encode(signer.verifying_key().to_bytes())
        ))
        .unwrap();
        assert_eq!(keys.len(), 2);
        let digest = signed_checksum(
            SUMS,
            signature.as_bytes(),
            &keys,
            "WeaveOffice-0.1.2-windows-x64-setup.exe",
        )
        .unwrap();
        assert_eq!(
            hex::encode(digest),
            "6f057592f6099a13ac2f56c56e1b9202c4194610278b4ee070f0169174ac93a8"
        );
        assert!(signed_checksum(SUMS, signature.as_bytes(), &keys, "other.exe").is_err());
    }

    #[test]
    fn tampering_or_a_foreign_key_is_refused() {
        let signer = key(7);
        let keys = vec![signer.verifying_key()];
        let signature = hex::encode(signer.sign(SUMS).to_bytes());
        let mut tampered = SUMS.to_vec();
        tampered[0] = b'f';
        assert!(verify_signature(&tampered, signature.as_bytes(), &keys).is_err());
        let foreign = hex::encode(key(9).sign(SUMS).to_bytes());
        assert!(verify_signature(SUMS, foreign.as_bytes(), &keys).is_err());
        assert!(verify_signature(SUMS, b"not hex", &keys).is_err());
        assert!(verify_signature(SUMS, signature.as_bytes(), &[]).is_err());
    }

    #[test]
    fn malformed_keys_and_checksums_are_errors() {
        assert!(parse_keys("abcd").is_err());
        assert!(parse_keys("# none yet\n\n").unwrap().is_empty());
        assert!(parse_checksums(b"xyz  file").is_err());
        assert!(
            parse_checksums(b"e88d47ba9ec0270c913edc3a21aad8499102a16a6afaa7d7734c4d137da6f5d2")
                .is_err()
        );
        let twice = [SUMS, SUMS].concat();
        assert!(
            parse_checksums(&twice).is_err(),
            "a name listed twice is ambiguous"
        );
    }

    #[test]
    fn file_digest_streams_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            hex::encode(sha256_file(&path).unwrap()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
