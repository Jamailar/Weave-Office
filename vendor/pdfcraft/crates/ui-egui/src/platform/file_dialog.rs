//! File selection descriptions independent of the native dialog backend.
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HostFileKind {
    File,
    Files,
    Save,
    Folder,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostFileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

/// The host renders this request in its own UI and returns paths to the originating editor.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostFileRequest {
    pub id: u64,
    pub kind: HostFileKind,
    pub title: Option<String>,
    pub directory: Option<PathBuf>,
    pub file_name: Option<String>,
    pub filters: Vec<HostFileFilter>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FileDialog {
    pub title: Option<String>,
    pub directory: Option<PathBuf>,
    pub file_name: Option<String>,
    pub filters: Vec<HostFileFilter>,
}

impl FileDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn set_directory(mut self, directory: impl AsRef<Path>) -> Self {
        self.directory = Some(directory.as_ref().to_owned());
        self
    }

    pub fn set_file_name(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }

    pub fn add_filter(mut self, name: impl Into<String>, extensions: &[impl AsRef<str>]) -> Self {
        self.filters.push(HostFileFilter { name: name.into(), extensions: extensions.iter().map(|ext| ext.as_ref().to_owned()).collect() });
        self
    }

    /// Called only by the standalone native picker or the browser's file-upload adapter.
    pub fn into_native(self) -> rfd::AsyncFileDialog {
        let mut dialog = rfd::AsyncFileDialog::new();
        if let Some(title) = self.title {
            dialog = dialog.set_title(title);
        }
        if let Some(directory) = self.directory {
            dialog = dialog.set_directory(directory);
        }
        if let Some(file_name) = self.file_name {
            dialog = dialog.set_file_name(file_name);
        }
        for filter in self.filters {
            dialog = dialog.add_filter(filter.name, &filter.extensions);
        }
        dialog
    }

    #[cfg(target_arch = "wasm32")]
    pub fn pick_file(self) -> impl std::future::Future<Output = Option<rfd::FileHandle>> {
        self.into_native().pick_file()
    }

    #[cfg(target_arch = "wasm32")]
    pub fn pick_files(self) -> impl std::future::Future<Output = Option<Vec<rfd::FileHandle>>> {
        self.into_native().pick_files()
    }
}
