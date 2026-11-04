//! HTTPS through the system's trust store and proxy settings.
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

pub struct Client {
    agent: ureq::Agent,
}

/// Larger than any release JSON or checksum file.
const SMALL_LIMIT: u64 = 4 * 1024 * 1024;
/// Larger than any installer we publish.
const PACKAGE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

impl Client {
    pub fn new(user_agent: &str) -> Self {
        // The application already carries aws-lc-rs for PDF signatures; reuse it for TLS.
        let crypto = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let tls = TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .root_certs(RootCerts::PlatformVerifier)
            .unversioned_rustls_crypto_provider(crypto)
            .build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .https_only(true)
            .user_agent(user_agent)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .build()
            .new_agent();
        Self { agent }
    }

    pub fn get(&self, url: &str, accept: &str) -> Result<Vec<u8>, String> {
        let mut response = self
            .agent
            .get(url)
            .header("Accept", accept)
            .call()
            .map_err(|error| error.to_string())?;
        response
            .body_mut()
            .with_config()
            .limit(SMALL_LIMIT)
            .read_to_vec()
            .map_err(|error| error.to_string())
    }

    /// Download to `destination`, reporting `(received, total)`, and return the SHA-256 of
    /// what was written. Stops with an error when `cancel` is set.
    pub fn download(
        &self,
        url: &str,
        destination: &Path,
        expected_size: u64,
        cancel: &AtomicBool,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<[u8; 32], String> {
        let mut response = self
            .agent
            .get(url)
            .header("Accept", "application/octet-stream")
            .call()
            .map_err(|error| error.to_string())?;
        let total = response
            .body()
            .content_length()
            .filter(|&length| length > 0)
            .unwrap_or(expected_size);
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(PACKAGE_LIMIT)
            .reader();
        let mut file = std::fs::File::create(destination).map_err(|error| error.to_string())?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 256 * 1024];
        let mut received = 0u64;
        progress(0, total);
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(crate::CANCELLED.into());
            }
            let read = reader
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
            hasher.update(&buffer[..read]);
            received += read as u64;
            progress(received, total.max(received));
        }
        file.sync_all().map_err(|error| error.to_string())?;
        Ok(hasher.finalize().into())
    }
}
