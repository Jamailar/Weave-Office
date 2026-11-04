//! Update checks for Weave Office.
//!
//! The application asks [`Updater`] to check the project's latest GitHub release. When it is
//! newer, the person is told, downloads it with one click, and restarts into the new version.
//! A download is only installed when the release's `SHA256SUMS.txt` is signed with a key built
//! into the application (`update-public-key.txt`) and the installer matches its checksum.
//! Without a key, or where the copy cannot replace itself, the person is sent to the release
//! page instead.
pub mod install;
mod net;
pub mod release;
pub mod verify;

use release::{CHECKSUMS, Platform, Release, SIGNATURE, Version};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Public keys accepted for release checksums, compiled into the application.
pub const PUBLIC_KEYS: &str = include_str!("../update-public-key.txt");

pub(crate) const CANCELLED: &str = "cancelled";

/// What the person is told about an update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub version: String,
    pub notes: String,
    /// The release page, for reading the notes or downloading by hand.
    pub page: String,
    /// Whether this copy can download and install it by itself. When false, the reason.
    pub manual_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The release could not be reached or read.
    Check,
    Download,
    /// The download did not match the signed checksums.
    Verify,
    Install,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Checking {
        manual: bool,
    },
    /// Only reported after a check the person asked for.
    UpToDate,
    Available(Update),
    Downloading {
        update: Update,
        received: u64,
        total: u64,
    },
    /// Downloaded and verified: a restart installs it.
    Ready(Update),
    Failed {
        update: Option<Update>,
        failure: Failure,
        detail: String,
    },
}

struct Shared {
    state: Mutex<State>,
    prepared: Mutex<Option<install::Prepared>>,
    release: Mutex<Option<Release>>,
    cancel: AtomicBool,
    wake: Box<dyn Fn() + Send + Sync>,
}

pub struct Updater {
    shared: Arc<Shared>,
    current: Version,
    platform: Option<Platform>,
    repository: String,
    directory: PathBuf,
    user_agent: String,
}

impl Updater {
    /// `repository` is `https://github.com/{owner}/{repo}`; `wake` asks the interface to redraw.
    pub fn new(
        current_version: &str,
        repository: &str,
        directory: PathBuf,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Option<Self> {
        let repository = repository
            .trim_end_matches('/')
            .strip_prefix("https://github.com/")?
            .to_owned();
        if repository.split('/').count() != 2 {
            return None;
        }
        Some(Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::Idle),
                prepared: Mutex::new(None),
                release: Mutex::new(None),
                cancel: AtomicBool::new(false),
                wake: Box::new(wake),
            }),
            current: Version::parse(current_version)?,
            platform: Platform::current(),
            repository,
            directory,
            user_agent: format!("Weave-Office/{current_version}"),
        })
    }

    /// The folder updates are downloaded to, under the system's cache or temporary folder.
    pub fn default_directory() -> PathBuf {
        if cfg!(target_os = "macos")
            && let Some(home) = std::env::var_os("HOME")
        {
            return PathBuf::from(home).join("Library/Caches/Weave Office/Updates");
        }
        std::env::temp_dir().join("Weave Office Updates")
    }

    /// Whether releases publish an installer for this platform at all.
    pub fn supported(&self) -> bool {
        self.platform.is_some()
    }

    pub fn state(&self) -> State {
        self.shared.state.lock().unwrap().clone()
    }

    fn busy(&self) -> bool {
        matches!(
            self.state(),
            State::Checking { .. } | State::Downloading { .. } | State::Ready(_)
        )
    }

    /// Leave a finished or failed state the person has dismissed. A download keeps going and a
    /// downloaded update stays ready.
    pub fn dismiss(&self) {
        let mut state = self.shared.state.lock().unwrap();
        if matches!(*state, State::UpToDate | State::Failed { .. }) {
            *state = State::Idle;
        }
    }

    /// Look for a newer release in the background. `manual` reports "up to date" and errors;
    /// an automatic check stays silent unless there is an update.
    pub fn check(&self, manual: bool) {
        if self.busy() {
            return;
        }
        if let State::Available(_) = self.state()
            && !manual
        {
            return;
        }
        self.set(State::Checking { manual });
        let shared = self.shared.clone();
        let url = format!(
            "https://api.github.com/repos/{}/releases/latest",
            self.repository
        );
        let user_agent = self.user_agent.clone();
        let current = self.current;
        let platform = self.platform;
        std::thread::spawn(move || {
            let client = net::Client::new(&user_agent);
            let result = client
                .get(&url, "application/vnd.github+json")
                .and_then(|json| Release::from_github(&json));
            let state = match result {
                Ok(release) if release.version > current => {
                    let update = describe(&release, platform);
                    *shared.release.lock().unwrap() = Some(release);
                    State::Available(update)
                }
                Ok(_) if manual => State::UpToDate,
                Ok(_) => State::Idle,
                Err(detail) if manual => State::Failed {
                    update: None,
                    failure: Failure::Check,
                    detail,
                },
                Err(_) => State::Idle,
            };
            *shared.state.lock().unwrap() = state;
            (shared.wake)();
        });
    }

    /// Download, verify and stage the available update.
    pub fn download(&self) {
        let update = match self.state() {
            State::Available(update)
            | State::Failed {
                update: Some(update),
                ..
            } if update.manual_reason.is_none() => update,
            _ => return,
        };
        let Some(release) = self.shared.release.lock().unwrap().clone() else {
            return;
        };
        let Some(platform) = self.platform else {
            return;
        };
        self.shared.cancel.store(false, Ordering::Relaxed);
        self.set(State::Downloading {
            update: update.clone(),
            received: 0,
            total: 0,
        });
        let shared = self.shared.clone();
        let directory = self.directory.clone();
        let user_agent = self.user_agent.clone();
        std::thread::spawn(move || {
            let result = fetch(&shared, &user_agent, &release, platform, &directory);
            let state = match result {
                Ok(prepared) => {
                    *shared.prepared.lock().unwrap() = Some(prepared);
                    State::Ready(update)
                }
                Err((_, detail)) if detail == CANCELLED => State::Available(update),
                Err((failure, detail)) => State::Failed {
                    update: Some(update),
                    failure,
                    detail,
                },
            };
            *shared.state.lock().unwrap() = state;
            (shared.wake)();
        });
    }

    /// Stop a download in progress; the update stays available.
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state(), State::Ready(_))
    }

    /// Install the downloaded update as the application exits. With `relaunch`, the new version
    /// starts afterwards. The caller must exit promptly after `Ok`.
    pub fn apply(&self, relaunch: bool) -> Result<(), String> {
        let prepared = self
            .shared
            .prepared
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| "no update is ready".to_owned())?;
        let result = install::apply(prepared, relaunch);
        if let Err(detail) = &result {
            let update = match self.state() {
                State::Ready(update) => Some(update),
                _ => None,
            };
            self.set(State::Failed {
                update,
                failure: Failure::Install,
                detail: detail.clone(),
            });
        }
        result
    }

    /// Remove downloads left by earlier runs. Call once at startup.
    pub fn clean(&self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }

    fn set(&self, state: State) {
        *self.shared.state.lock().unwrap() = state;
        (self.shared.wake)();
    }
}

fn describe(release: &Release, platform: Option<Platform>) -> Update {
    let manual_reason = (|| {
        let platform = platform.ok_or("no installer for this platform")?;
        let keys = verify::parse_keys(PUBLIC_KEYS).map_err(|_| "invalid update key")?;
        if keys.is_empty() {
            return Err("this build has no update key".to_owned());
        }
        for name in [
            platform.package_name(&release.version),
            CHECKSUMS.to_owned(),
            SIGNATURE.to_owned(),
        ] {
            if release.asset(&name).is_none() {
                return Err(format!("the release has no {name}"));
            }
        }
        install::target().map(|_| ())
    })()
    .err()
    .map(|reason| reason.to_string());
    Update {
        version: release.version.to_string(),
        notes: release.notes.clone(),
        page: release.page.clone(),
        manual_reason,
    }
}

fn fetch(
    shared: &Shared,
    user_agent: &str,
    release: &Release,
    platform: Platform,
    directory: &std::path::Path,
) -> Result<install::Prepared, (Failure, String)> {
    let download = |detail: String| (Failure::Download, detail);
    let verify = |detail: String| (Failure::Verify, detail);
    let client = net::Client::new(user_agent);
    let package = platform.package_name(&release.version);
    let asset = |name: &str| {
        release
            .asset(name)
            .cloned()
            .ok_or_else(|| download(format!("the release has no {name}")))
    };
    let keys = verify::parse_keys(PUBLIC_KEYS).map_err(verify)?;
    let checksums = client
        .get(&asset(CHECKSUMS)?.url, "application/octet-stream")
        .map_err(download)?;
    let signature = client
        .get(&asset(SIGNATURE)?.url, "application/octet-stream")
        .map_err(download)?;
    let expected =
        verify::signed_checksum(&checksums, &signature, &keys, &package).map_err(verify)?;
    let target = install::target().map_err(|detail| (Failure::Install, detail))?;

    std::fs::create_dir_all(directory).map_err(|error| download(error.to_string()))?;
    let path = directory.join(&package);
    let partial = directory.join(format!("{package}.part"));
    let asset = asset(&package)?;
    let digest = client
        .download(
            &asset.url,
            &partial,
            asset.size,
            &shared.cancel,
            |received, total| {
                let mut state = shared.state.lock().unwrap();
                if let State::Downloading {
                    received: r,
                    total: t,
                    ..
                } = &mut *state
                {
                    // Redraw about every 1%: often enough for a smooth bar, rarely enough to idle.
                    let step = (total / 100).max(256 * 1024);
                    let redraw = received / step != *r / step || received == total;
                    *r = received;
                    *t = total;
                    drop(state);
                    if redraw {
                        (shared.wake)();
                    }
                }
            },
        )
        .map_err(|detail| {
            let _ = std::fs::remove_file(&partial);
            download(detail)
        })?;
    if digest != expected {
        let _ = std::fs::remove_file(&partial);
        return Err(verify(format!(
            "{package} does not match its signed checksum"
        )));
    }
    std::fs::rename(&partial, &path).map_err(|error| download(error.to_string()))?;
    install::prepare(&target, &path, &release.version).map_err(|detail| (Failure::Install, detail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_must_be_a_github_project() {
        let new = |repository| Updater::new("0.1.2", repository, PathBuf::from("/tmp/x"), || {});
        assert!(new("https://github.com/example/weave-office").is_some());
        assert!(new("https://github.com/example/weave-office/").is_some());
        assert!(new("https://example.com/example/weave-office").is_none());
        assert!(new("https://github.com/example").is_none());
        assert!(Updater::new("dev", "https://github.com/a/b", PathBuf::new(), || {}).is_none());
    }

    #[test]
    fn the_bundled_key_file_parses() {
        verify::parse_keys(PUBLIC_KEYS).expect("update-public-key.txt");
    }

    #[test]
    fn a_release_without_signature_is_offered_as_a_manual_download() {
        let release = Release {
            version: Version::parse("9.0.0").unwrap(),
            notes: "n".into(),
            page: "https://example.com/r".into(),
            assets: Vec::new(),
        };
        let update = describe(&release, Platform::current());
        assert_eq!(update.version, "9.0.0");
        assert!(update.manual_reason.is_some());
        let update = describe(&release, None);
        assert_eq!(
            update.manual_reason.as_deref(),
            Some("no installer for this platform")
        );
    }
}
