//! Host-level automatic recovery for every editor.
//!
//! While a document has unsaved changes the host periodically writes a copy of it, in the
//! editor's own format, into this run's recovery folder. A clean save, a discarded close or a
//! normal exit removes the copy; after a crash the copies stay behind, and the next start offers
//! to reopen them as unsaved documents. Each run's folder holds a lock for the life of the
//! process, so a second running instance never offers (or deletes) the first one's copies.
//!
//! Serializing and writing happen on one background thread, in the order they were asked for, so
//! typing never waits on the disk and a removal never overtakes the write before it.

use std::{
    collections::HashMap,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use weave_workspace::{DocumentKind, TabId};

/// How long the person must pause before a copy is written, so writing never competes with
/// typing.
pub const IDLE: Duration = Duration::from_secs(2);
/// The shortest time between two copies of one document.
pub const INTERVAL: Duration = Duration::from_secs(30);
/// Documents whose saved form is larger than this are not copied.
pub const MAX_COPY: usize = 256 << 20;

const LOCK: &str = "session.lock";

/// Produces a document's bytes on the recovery thread.
pub type Snapshot = Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send>;

/// What a copy is of: enough to reopen it as the document it was.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    pub kind: String,
    pub title: String,
    /// The file the document was opened from or last saved to, if any.
    pub origin: Option<PathBuf>,
    /// When the copy was written, in milliseconds since the Unix epoch.
    pub written_ms: u64,
}

impl Manifest {
    pub fn document_kind(&self) -> Option<DocumentKind> {
        kinds()
            .into_iter()
            .find(|kind| kind_key(*kind) == self.kind)
    }
}

/// A copy a previous run left behind.
#[derive(Clone, Debug)]
pub struct Found {
    pub manifest: Manifest,
    pub file: PathBuf,
}

enum Task {
    Write {
        tab: u64,
        manifest: Manifest,
        snapshot: Snapshot,
    },
    Remove(u64),
    Stop,
}

#[derive(Default)]
struct TabState {
    /// Edited since the last copy (or never copied).
    touched: bool,
    last: Option<Instant>,
    /// A copy exists (or has been asked for).
    copied: bool,
}

/// One run's recovery folder.
pub struct Recovery {
    dir: PathBuf,
    lock: Option<File>,
    tasks: mpsc::Sender<Task>,
    errors: mpsc::Receiver<String>,
    worker: Option<JoinHandle<()>>,
    tabs: HashMap<TabId, TabState>,
    found: Vec<Found>,
    stale: Vec<PathBuf>,
}

/// The recovery folder: `WEAVE_RECOVERY_DIR`, or `Recovery` beside the settings file.
pub fn default_root(settings: &Path) -> PathBuf {
    std::env::var_os("WEAVE_RECOVERY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            settings
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(std::env::temp_dir)
                .join("Recovery")
        })
}

impl Recovery {
    /// Start this run's folder under `root`, and collect the copies runs that are no longer
    /// running left there.
    pub fn open(root: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(root)?;
        let (found, stale) = previous_runs(root);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let dir = root.join(format!("run-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let lock = File::create(dir.join(LOCK))?;
        lock.try_lock().map_err(|error| match error {
            std::fs::TryLockError::Error(error) => error,
            std::fs::TryLockError::WouldBlock => std::io::Error::other("recovery folder in use"),
        })?;
        let (tasks, receive) = mpsc::channel();
        let (report, errors) = mpsc::channel();
        let target = dir.clone();
        let worker = std::thread::Builder::new()
            .name("recovery".into())
            .spawn(move || work(&target, receive, report))?;
        Ok(Self {
            dir,
            lock: Some(lock),
            tasks,
            errors,
            worker: Some(worker),
            tabs: HashMap::new(),
            found,
            stale,
        })
    }

    /// This run's folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The copies previous runs left behind, oldest first.
    pub fn found(&self) -> &[Found] {
        &self.found
    }

    /// Delete what previous runs left behind (after its copies were reopened or declined).
    pub fn clear_found(&mut self) {
        self.found.clear();
        for dir in std::mem::take(&mut self.stale) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// The document in `tab` was edited.
    pub fn touch(&mut self, tab: TabId) {
        self.tabs.entry(tab).or_default().touched = true;
    }

    /// Whether `tab` needs a copy now: it is dirty, was edited since its last copy (or has none),
    /// the person has paused for [`IDLE`] and the last copy is [`INTERVAL`] old. Otherwise how
    /// long until it may (`Err`), or `Err(None)` when nothing is waiting.
    pub fn due(
        &mut self,
        tab: TabId,
        dirty: bool,
        now: Instant,
        last_input: Instant,
    ) -> Result<(), Option<Duration>> {
        if !dirty {
            self.forget(tab);
            return Err(None);
        }
        let state = self.tabs.entry(tab).or_default();
        if state.copied && !state.touched {
            return Err(None);
        }
        let idle_left = IDLE.saturating_sub(now.saturating_duration_since(last_input));
        let interval_left = state.last.map_or(Duration::ZERO, |last| {
            INTERVAL.saturating_sub(now.saturating_duration_since(last))
        });
        let wait = idle_left.max(interval_left);
        if wait.is_zero() {
            Ok(())
        } else {
            Err(Some(wait))
        }
    }

    /// Write a copy of `tab` (on the recovery thread).
    pub fn copy(
        &mut self,
        tab: TabId,
        kind: DocumentKind,
        title: &str,
        origin: Option<&Path>,
        snapshot: Snapshot,
        now: Instant,
    ) {
        let state = self.tabs.entry(tab).or_default();
        state.touched = false;
        state.last = Some(now);
        state.copied = true;
        let manifest = Manifest {
            kind: kind_key(kind).into(),
            title: title.into(),
            origin: origin.map(Path::to_path_buf),
            written_ms: 0,
        };
        let _ = self.tasks.send(Task::Write {
            tab: tab.get(),
            manifest,
            snapshot,
        });
    }

    /// Remove `tab`'s copy: it was saved, closed or discarded.
    pub fn forget(&mut self, tab: TabId) {
        if self.tabs.remove(&tab).is_some_and(|state| state.copied) {
            let _ = self.tasks.send(Task::Remove(tab.get()));
        }
    }

    /// Forget every tab not in `open`.
    pub fn retain(&mut self, open: &[TabId]) {
        let closed: Vec<TabId> = self
            .tabs
            .keys()
            .copied()
            .filter(|tab| !open.contains(tab))
            .collect();
        for tab in closed {
            self.forget(tab);
        }
    }

    /// Errors the recovery thread met since the last call.
    pub fn take_errors(&self) -> Vec<String> {
        self.errors.try_iter().collect()
    }

    /// Wait until every copy and removal asked for so far is on disk.
    pub fn flush(&mut self) {
        self.stop_worker();
        let (tasks, receive) = mpsc::channel();
        let (report, errors) = mpsc::channel();
        let target = self.dir.clone();
        self.worker = std::thread::Builder::new()
            .name("recovery".into())
            .spawn(move || work(&target, receive, report))
            .ok();
        self.tasks = tasks;
        self.errors = errors;
    }

    fn stop_worker(&mut self) {
        let _ = self.tasks.send(Task::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    /// Stop as a crash would: the copies stay and the folder is unlocked.
    #[cfg(test)]
    pub fn crash(mut self) {
        self.stop_worker();
        self.lock = None;
        self.dir = PathBuf::new();
    }
}

impl Drop for Recovery {
    /// A normal exit: nothing needs recovering.
    fn drop(&mut self) {
        self.stop_worker();
        if !self.dir.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
        self.lock = None;
    }
}

fn work(dir: &Path, tasks: mpsc::Receiver<Task>, errors: mpsc::Sender<String>) {
    let mut written: HashMap<u64, u64> = HashMap::new();
    for task in tasks {
        match task {
            Task::Write {
                tab,
                mut manifest,
                snapshot,
            } => {
                let result = snapshot().and_then(|bytes| {
                    if bytes.len() > MAX_COPY {
                        return Ok(());
                    }
                    let hash = fingerprint(&bytes);
                    if written.get(&tab) == Some(&hash) {
                        return Ok(());
                    }
                    manifest.written_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
                    let ext = manifest
                        .document_kind()
                        .map_or("bin", crate::host::extension);
                    // The document first: a manifest always names a complete copy.
                    write_atomically(&dir.join(format!("{tab}.{ext}")), &bytes)?;
                    write_atomically(&dir.join(format!("{tab}.json")), &json)?;
                    written.insert(tab, hash);
                    Ok(())
                });
                if let Err(error) = result {
                    let _ = errors.send(format!("{}: {error}", manifest.title));
                }
            }
            Task::Remove(tab) => {
                written.remove(&tab);
                let _ = std::fs::remove_file(dir.join(format!("{tab}.json")));
                for kind in kinds() {
                    let ext = crate::host::extension(kind);
                    let _ = std::fs::remove_file(dir.join(format!("{tab}.{ext}")));
                }
            }
            Task::Stop => break,
        }
    }
}

fn fingerprint(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("missing recovery folder")?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

/// The copies left by runs that are no longer running (their lock is free), and their folders.
fn previous_runs(root: &Path) -> (Vec<Found>, Vec<PathBuf>) {
    let mut found = Vec::new();
    let mut stale = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return (found, stale);
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() || !entry.file_name().to_string_lossy().starts_with("run-") {
            continue;
        }
        // A running instance holds its lock; leave its folder alone.
        if let Ok(lock) = File::options()
            .read(true)
            .write(true)
            .create(true)
            .open(dir.join(LOCK))
        {
            if lock.try_lock().is_err() {
                continue;
            }
        }
        if let Ok(files) = std::fs::read_dir(&dir) {
            for file in files.flatten() {
                let path = file.path();
                if path.extension().is_none_or(|ext| ext != "json") {
                    continue;
                }
                let Some(manifest) = std::fs::read(&path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).ok())
                else {
                    continue;
                };
                let Some(kind) = manifest.document_kind() else {
                    continue;
                };
                let document = path.with_extension(crate::host::extension(kind));
                if document.is_file() {
                    found.push(Found {
                        manifest,
                        file: document,
                    });
                }
            }
        }
        stale.push(dir);
    }
    found.sort_by_key(|found| found.manifest.written_ms);
    (found, stale)
}

fn kinds() -> [DocumentKind; 6] {
    [
        DocumentKind::Text,
        DocumentKind::Spreadsheet,
        DocumentKind::Presentation,
        DocumentKind::Pdf,
        DocumentKind::Markdown,
        DocumentKind::PlainText,
    ]
}

fn kind_key(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Text => "text",
        DocumentKind::Spreadsheet => "spreadsheet",
        DocumentKind::Presentation => "presentation",
        DocumentKind::Pdf => "pdf",
        DocumentKind::Markdown => "markdown",
        DocumentKind::PlainText => "plain-text",
    }
}

#[cfg(test)]
mod tests;
