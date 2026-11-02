//! PDF document operations for the shared Weave workspace.
//! This adapter does not create windows or own file paths.
use pdfcraft_engine::Session;
pub use pdfcraft_engine::{DocId, Edit};
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct PdfBackend {
    session: Session,
    revisions: HashMap<DocId, u64>,
}

pub struct SaveSnapshot {
    id: DocId,
    revision: u64,
    pub bytes: Arc<Vec<u8>>,
}

impl PdfBackend {
    pub fn open(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        password: Option<&str>,
    ) -> Result<DocId, String> {
        let id = self
            .session
            .open(name, None, Arc::new(bytes), password)
            .map_err(|e| e.to_string())?;
        self.revisions.insert(id, 0);
        Ok(id)
    }
    pub fn create_blank(&mut self, name: &str) -> Result<DocId, String> {
        let bytes = self
            .session
            .create_blank(595.276, 841.89, 1)
            .map_err(|e| e.to_string())?;
        let id = self
            .session
            .open(name, None, bytes, None)
            .map_err(|e| e.to_string())?;
        self.revisions.insert(id, 0);
        Ok(id)
    }
    pub fn apply(&mut self, id: DocId, edit: Edit) -> Result<(), String> {
        let revision = self
            .revisions
            .get(&id)
            .copied()
            .ok_or("PDF 不存在")?
            .checked_add(1)
            .ok_or("版本号耗尽")?;
        self.session.apply(id, edit).map_err(|e| e.to_string())?;
        self.revisions.insert(id, revision);
        Ok(())
    }
    pub fn page_count(&self, id: DocId) -> Option<usize> {
        self.session.get(id).map(|doc| doc.info.pages.len())
    }
    /// Prepare a validated snapshot. The host must serialize writes to a given destination.
    pub fn save_snapshot(&self, id: DocId) -> Result<SaveSnapshot, String> {
        let revision = *self.revisions.get(&id).ok_or("PDF 不存在")?;
        let bytes = self.session.save_bytes(id).map_err(|e| e.to_string())?;
        self.session
            .check_save(id, &bytes)
            .map_err(|e| e.to_string())?;
        Ok(SaveSnapshot {
            id,
            revision,
            bytes,
        })
    }
    /// A successful disk write may clear dirty state only if no newer edit exists.
    pub fn confirm_saved(
        &mut self,
        snapshot: SaveSnapshot,
        path: Option<String>,
    ) -> Result<(), String> {
        if self.revisions.get(&snapshot.id) != Some(&snapshot.revision) {
            return Err("保存期间文档已更改，仍有内容未保存".into());
        }
        self.session
            .mark_saved(snapshot.id, snapshot.bytes, path)
            .map_err(|e| e.to_string())
    }
    pub fn is_dirty(&self, id: DocId) -> Option<bool> {
        self.session
            .docs()
            .iter()
            .find(|doc| doc.id == id)
            .map(|doc| doc.dirty)
    }
    pub fn close(&mut self, id: DocId) -> Result<(), &'static str> {
        match self.is_dirty(id) {
            None => Err("PDF 不存在"),
            Some(true) => Err("PDF 有未保存的更改"),
            Some(false) => {
                self.session.close(id);
                self.revisions.remove(&id);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blank_pdf_round_trips_through_backend() {
        let mut backend = PdfBackend::default();
        let id = backend.create_blank("blank.pdf").unwrap();
        backend
            .apply(
                id,
                Edit::InsertBlankPage {
                    at: 1,
                    width: 595.276,
                    height: 841.89,
                },
            )
            .unwrap();
        assert_eq!(backend.is_dirty(id), Some(true));
        assert!(backend.close(id).is_err());
        let snapshot = backend.save_snapshot(id).unwrap();
        let bytes = snapshot.bytes.clone();
        assert!(bytes.starts_with(b"%PDF-"));
        let reopened = backend
            .open("copy.pdf", bytes.as_ref().clone(), None)
            .unwrap();
        assert_eq!(backend.is_dirty(reopened), Some(false));
        assert_eq!(backend.page_count(reopened), Some(2));
        assert!(backend.close(reopened).is_ok());
        backend
            .apply(
                id,
                Edit::InsertBlankPage {
                    at: 2,
                    width: 595.276,
                    height: 841.89,
                },
            )
            .unwrap();
        assert!(backend.confirm_saved(snapshot, None).is_err());
        assert_eq!(backend.page_count(id), Some(3));
        assert_eq!(backend.is_dirty(id), Some(true));
        let current = backend.save_snapshot(id).unwrap();
        backend.confirm_saved(current, None).unwrap();
        assert_eq!(backend.is_dirty(id), Some(false));
    }
}
