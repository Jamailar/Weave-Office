//! Shared document-tab bookkeeping, independent of editor UI and file codecs.
//! Editors own document data. The host applies their notifications to this registry.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentKind {
    Text,
    Spreadsheet,
    Presentation,
    Pdf,
}

impl DocumentKind {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "docx" | "docm" | "doc" | "odt" | "rtf" | "txt" | "md" => Some(Self::Text),
            "xlsx" | "xlsm" | "xls" | "ods" | "csv" => Some(Self::Spreadsheet),
            "pptx" | "pptm" | "ppt" | "odp" => Some(Self::Presentation),
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TabId(u64);

impl TabId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug)]
pub struct Tab {
    pub id: TabId,
    pub kind: DocumentKind,
    pub title: String,
    pub path: Option<PathBuf>,
    revision: u64,
    saved_revision: u64,
}
impl Tab {
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseResult {
    Closed,
    NeedsSave,
    Missing,
}

#[derive(Default)]
pub struct Workspace {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next_id: u64,
}
impl Workspace {
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }
    pub fn active(&self) -> Option<TabId> {
        self.active
    }
    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }
    /// The editor owns dirty state; this is the workspace's display projection.
    pub fn sync_dirty(&mut self, id: TabId, dirty: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            if dirty && !tab.is_dirty() {
                tab.revision = tab.revision.saturating_add(1);
            } else if !dirty {
                tab.saved_revision = tab.revision;
            }
        }
    }
    pub fn reorder(&mut self, id: TabId, before: TabId) -> bool {
        if id == before {
            return false;
        }
        let Some(from) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        if !self.tabs.iter().any(|tab| tab.id == before) {
            return false;
        }
        let tab = self.tabs.remove(from);
        let to = self
            .tabs
            .iter()
            .position(|tab| tab.id == before)
            .unwrap_or(self.tabs.len());
        self.tabs.insert(to, tab);
        true
    }
    /// Call only after an editor has successfully opened or created its document.
    pub fn add(
        &mut self,
        kind: DocumentKind,
        title: impl Into<String>,
        path: Option<PathBuf>,
    ) -> Option<TabId> {
        self.next_id = self.next_id.checked_add(1)?;
        let id = TabId(self.next_id);
        self.tabs.push(Tab {
            id,
            kind,
            title: title.into(),
            path,
            revision: 0,
            saved_revision: 0,
        });
        self.active = Some(id);
        Some(id)
    }
    /// Update the visible identity only after the host has successfully saved.
    pub fn set_path(&mut self, id: TabId, path: PathBuf) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };
        if let Some(name) = path.file_name() {
            tab.title = name.to_string_lossy().into_owned();
        }
        tab.path = Some(path);
        true
    }
    pub fn activate(&mut self, id: TabId) -> bool {
        if self.tabs.iter().any(|tab| tab.id == id) {
            self.active = Some(id);
            true
        } else {
            false
        }
    }
    pub fn changed(&mut self, id: TabId) -> Option<u64> {
        let tab = self.tabs.iter_mut().find(|tab| tab.id == id)?;
        tab.revision = tab.revision.checked_add(1)?;
        Some(tab.revision)
    }
    /// Capture when starting a save; a later edit must remain dirty when that save finishes.
    pub fn save_revision(&self, id: TabId) -> Option<u64> {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.revision)
    }
    /// Call only after successful persistence. Old or out-of-order saves never mark newer edits clean.
    pub fn saved(&mut self, id: TabId, revision: u64) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };
        if revision != tab.revision {
            return false;
        }
        tab.saved_revision = revision;
        true
    }
    pub fn close(&mut self, id: TabId, discard: bool) -> CloseResult {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return CloseResult::Missing;
        };
        if self.tabs[index].is_dirty() && !discard {
            return CloseResult::NeedsSave;
        }
        self.tabs.remove(index);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(index)
                .or_else(|| index.checked_sub(1).and_then(|i| self.tabs.get(i)))
                .map(|tab| tab.id);
        }
        CloseResult::Closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_dirty_tab_requires_explicit_discard() {
        let mut w = Workspace::default();
        let first = w.add(DocumentKind::Text, "A", None).unwrap();
        let second = w.add(DocumentKind::Pdf, "B", None).unwrap();
        w.changed(second);
        assert_eq!(w.close(second, false), CloseResult::NeedsSave);
        assert_eq!(w.active(), Some(second));
        assert_eq!(w.close(second, true), CloseResult::Closed);
        assert_eq!(w.active(), Some(first));
    }
    #[test]
    fn save_completion_does_not_hide_edits_made_during_save() {
        let mut w = Workspace::default();
        let id = w.add(DocumentKind::Spreadsheet, "Sheet", None).unwrap();
        w.changed(id);
        let saved = w.save_revision(id).unwrap();
        w.changed(id);
        assert!(!w.saved(id, saved));
        assert!(w.tabs()[0].is_dirty());
        let current = w.save_revision(id).unwrap();
        assert!(w.saved(id, current));
        assert!(!w.tabs()[0].is_dirty());
    }
    #[test]
    fn recognizes_four_office_types_without_ambiguous_extensions() {
        assert_eq!(
            DocumentKind::from_path(Path::new("a.DOCX")),
            Some(DocumentKind::Text)
        );
        assert_eq!(
            DocumentKind::from_path(Path::new("b.xlsx")),
            Some(DocumentKind::Spreadsheet)
        );
        assert_eq!(
            DocumentKind::from_path(Path::new("c.pptx")),
            Some(DocumentKind::Presentation)
        );
        assert_eq!(
            DocumentKind::from_path(Path::new("d.pdf")),
            Some(DocumentKind::Pdf)
        );
        assert_eq!(DocumentKind::from_path(Path::new("e.zip")), None);
    }
}
