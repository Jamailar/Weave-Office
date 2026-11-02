//! Atomic writes: a document reaches its path complete or not at all. The engine writes a
//! temporary file beside the target (same folder, same extension, so the same format), which
//! is then renamed over it.

use std::path::{Path, PathBuf};

use crate::engines::Doc;
use crate::error::CliError;

fn temp_beside(path: &Path) -> PathBuf {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    dir.join(format!(
        ".{stem}.weave-tmp-{}-{nonce}{ext}",
        std::process::id()
    ))
}

fn check_target(path: &Path) -> Result<(), CliError> {
    if path.is_dir() {
        return Err(CliError::new(
            "io",
            format!("{}: is a folder, not a file", path.display()),
        ));
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
        && !parent.is_dir()
    {
        return Err(CliError::new(
            "not_found",
            format!(
                "{}: folder {} does not exist",
                path.display(),
                parent.display()
            ),
        ));
    }
    Ok(())
}

/// Save `doc` to `path` atomically.
pub fn save_doc(doc: &mut Doc, path: &Path) -> Result<(), CliError> {
    check_target(path)?;
    let temp = temp_beside(path);
    let result = doc.write(&temp).and_then(|()| {
        if !temp.is_file() {
            return Err(CliError::new(
                "io",
                format!("{}: the engine wrote nothing", path.display()),
            ));
        }
        std::fs::rename(&temp, path).map_err(CliError::from)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Write `bytes` to `path` atomically.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    check_target(path)?;
    let temp = temp_beside(path);
    let result = std::fs::write(&temp, bytes)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(CliError::from);
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
