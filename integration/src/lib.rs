//! Shared, UI-independent ownership of open editor engines.
//! An application shell can attach one file tab to each `TabId`.
use std::{collections::HashMap, io::Write, path::Path};
use weave_pdf_backend::{DocId, PdfBackend};
use weave_workspace::{CloseResult, DocumentKind, TabId, Workspace};

enum Engine {
    Text(Box<wordcraft_engine::Session>),
    Spreadsheet(Box<gridcraft_engine::Session>),
    Presentation(Box<deckcraft_engine::Session>),
    Pdf(Box<PdfBackend>, DocId),
}

#[derive(Default)]
pub struct Host {
    workspace: Workspace,
    engines: HashMap<TabId, Engine>,
}
impl Host {
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }
    pub fn activate(&mut self, id: TabId) -> bool {
        self.workspace.activate(id)
    }

    /// Creates an engine first and publishes a tab only if creation succeeds.
    pub fn new_document(&mut self, kind: DocumentKind) -> Result<TabId, String> {
        let (title, engine) = match kind {
            DocumentKind::Text => (
                "未命名文档",
                Engine::Text(Box::new(wordcraft_engine::Session::new(
                    wordcraft_doc::Document::new(),
                ))),
            ),
            DocumentKind::Spreadsheet => {
                let mut session = gridcraft_engine::Session::new();
                session.new_workbook();
                ("未命名表格", Engine::Spreadsheet(Box::new(session)))
            }
            DocumentKind::Presentation => (
                "未命名演示",
                Engine::Presentation(Box::new(deckcraft_engine::Session::with_new())),
            ),
            DocumentKind::Pdf => {
                let mut backend = PdfBackend::default();
                let id = backend.create_blank("未命名.pdf")?;
                ("未命名 PDF", Engine::Pdf(Box::new(backend), id))
            }
        };
        let id = self
            .workspace
            .add(kind, title, None)
            .ok_or("标签标识已耗尽")?;
        self.engines.insert(id, engine);
        Ok(id)
    }
    /// Opens each existing local file once, across all document types.
    pub fn open_document(&mut self, path: &Path) -> Result<TabId, String> {
        let canonical = path.canonicalize().map_err(|e| e.to_string())?;
        if let Some(id) = self
            .workspace
            .tabs()
            .iter()
            .find(|tab| tab.path.as_ref() == Some(&canonical))
            .map(|tab| tab.id)
        {
            self.workspace.activate(id);
            return Ok(id);
        }
        let kind = DocumentKind::from_path(&canonical).ok_or("不支持此文件类型")?;
        let name = canonical
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("文件名不是有效的 Unicode")?
            .to_string();
        let path_string = canonical.to_str().ok_or("文件路径不是有效的 Unicode")?;
        let engine = match kind {
            DocumentKind::Text => {
                let document = wordcraft_engine::io::open_path(&canonical)?;
                let mut session = wordcraft_engine::Session::new(document);
                session.path = Some(canonical.clone());
                Engine::Text(Box::new(session))
            }
            DocumentKind::Spreadsheet => {
                let mut session = gridcraft_engine::Session::new();
                session
                    .execute("file.open", serde_json::json!({"path": path_string}))
                    .map_err(|e| e.to_string())?;
                Engine::Spreadsheet(Box::new(session))
            }
            DocumentKind::Presentation => {
                let mut session = deckcraft_engine::Session::new();
                session
                    .execute("file.open", &serde_json::json!({"path": path_string}))
                    .map_err(|e| e.to_string())?;
                Engine::Presentation(Box::new(session))
            }
            DocumentKind::Pdf => {
                let mut backend = PdfBackend::default();
                let bytes = std::fs::read(&canonical).map_err(|e| e.to_string())?;
                let id = backend.open(&name, bytes, None)?;
                Engine::Pdf(Box::new(backend), id)
            }
        };
        let id = self
            .workspace
            .add(kind, name, Some(canonical))
            .ok_or("标签标识已耗尽")?;
        self.engines.insert(id, engine);
        Ok(id)
    }
    /// Save through the owning engine; paths and dirty state change only on success.
    /// Only native formats are accepted here. Exporting lossy formats is a separate operation.
    pub fn save_as(&mut self, id: TabId, path: &Path) -> Result<(), String> {
        let tab = self
            .workspace
            .tabs()
            .iter()
            .find(|tab| tab.id == id)
            .ok_or("标签不存在")?;
        let required = match tab.kind {
            DocumentKind::Text => "docx",
            DocumentKind::Spreadsheet => "xlsx",
            DocumentKind::Presentation => "pptx",
            DocumentKind::Pdf => "pdf",
        };
        if !path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(required))
        {
            return Err(format!("请使用 .{required} 格式保存"));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let name = path.file_name().ok_or("缺少文件名")?;
        let candidate = parent.canonicalize().map_err(|e| e.to_string())?.join(name);
        let target = if candidate.exists() {
            candidate.canonicalize().map_err(|e| e.to_string())?
        } else {
            candidate
        };
        if self
            .workspace
            .tabs()
            .iter()
            .any(|other| other.id != id && other.path.as_ref() == Some(&target))
        {
            return Err("该文件已在其他标签中打开".into());
        }
        let path_text = target.to_str().ok_or("文件路径不是有效的 Unicode")?;
        let engine = self.engines.get_mut(&id).ok_or("标签不存在")?;
        match engine {
            Engine::Text(session) => {
                session
                    .run("file.saveAs", &serde_json::json!({"path": path_text}))
                    .map_err(|e| e.to_string())?;
            }
            Engine::Spreadsheet(session) => {
                session
                    .execute("file.saveAs", serde_json::json!({"path": path_text}))
                    .map_err(|e| e.to_string())?;
            }
            Engine::Presentation(session) => {
                session
                    .execute("file.saveAs", &serde_json::json!({"path": path_text}))
                    .map_err(|e| e.to_string())?;
            }
            Engine::Pdf(backend, doc) => {
                let snapshot = backend.save_snapshot(*doc)?;
                let mut tmp =
                    tempfile::NamedTempFile::new_in(&parent).map_err(|e| e.to_string())?;
                tmp.write_all(&snapshot.bytes)
                    .and_then(|_| tmp.as_file().sync_all())
                    .map_err(|e| e.to_string())?;
                tmp.persist(&target).map_err(|e| e.to_string())?;
                backend.confirm_saved(snapshot, Some(path_text.to_string()))?;
            }
        }
        self.workspace.set_path(id, target);
        if let Some(rev) = self.workspace.save_revision(id) {
            self.workspace.saved(id, rev);
        }
        Ok(())
    }
    pub fn is_dirty(&self, id: TabId) -> Option<bool> {
        self.engines.get(&id).map(|engine| match engine {
            Engine::Text(session) => session.documents().iter().any(|doc| doc.dirty),
            Engine::Spreadsheet(session) => session.documents().iter().any(|doc| doc.is_dirty()),
            Engine::Presentation(session) => session.documents().iter().any(|doc| doc.is_dirty()),
            Engine::Pdf(backend, doc) => backend.is_dirty(*doc).unwrap_or(true),
        })
    }
    pub fn apply_pdf(&mut self, id: TabId, edit: weave_pdf_backend::Edit) -> Result<(), String> {
        if self
            .workspace
            .save_revision(id)
            .is_none_or(|rev| rev == u64::MAX)
        {
            return Err("标签不存在或版本号耗尽".into());
        }
        let Some(Engine::Pdf(backend, doc)) = self.engines.get_mut(&id) else {
            return Err("标签不是 PDF".into());
        };
        backend.apply(*doc, edit)?;
        self.workspace.changed(id).ok_or("标签版本号耗尽")?;
        Ok(())
    }
    /// Closing modified content always requires an explicit discard decision from the shell.
    pub fn close(&mut self, id: TabId, discard: bool) -> CloseResult {
        match self.is_dirty(id) {
            None => CloseResult::Missing,
            Some(true) if !discard => CloseResult::NeedsSave,
            Some(_) => {
                let result = self.workspace.close(id, true);
                if result == CloseResult::Closed {
                    self.engines.remove(&id);
                }
                result
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creates_saves_and_reopens_each_native_document_type() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = Host::default();
        for (kind, file) in [
            (DocumentKind::Text, "one.docx"),
            (DocumentKind::Spreadsheet, "two.xlsx"),
            (DocumentKind::Presentation, "three.pptx"),
            (DocumentKind::Pdf, "four.pdf"),
        ] {
            let id = host.new_document(kind).unwrap();
            let path = dir.path().join(file);
            host.save_as(id, &path).unwrap();
            assert!(path.is_file());
            assert_eq!(host.close(id, false), CloseResult::Closed);
            let reopened = host.open_document(&path).unwrap();
            assert_eq!(host.workspace().tabs().last().unwrap().kind, kind);
            assert_eq!(host.close(reopened, false), CloseResult::Closed);
        }
    }
    #[test]
    fn reopening_the_same_pdf_reuses_tab_and_failed_open_adds_nothing() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("example.pdf");
        let mut source = PdfBackend::default();
        let source_id = source.create_blank("example.pdf").unwrap();
        std::fs::write(
            &path,
            source.save_snapshot(source_id).unwrap().bytes.as_ref(),
        )
        .unwrap();
        let mut host = Host::default();
        let first = host.open_document(&path).unwrap();
        let other = host.new_document(DocumentKind::Text).unwrap();
        assert_ne!(first, other);
        assert_eq!(host.open_document(&path).unwrap(), first);
        assert_eq!(host.workspace().active(), Some(first));
        assert_eq!(host.workspace().tabs().len(), 2);
        assert!(
            host.open_document(&folder.path().join("missing.docx"))
                .is_err()
        );
        assert_eq!(host.workspace().tabs().len(), 2);
    }
    #[test]
    fn one_workspace_owns_mixed_documents_and_protects_pdf_changes() {
        let mut host = Host::default();
        let text = host.new_document(DocumentKind::Text).unwrap();
        let sheet = host.new_document(DocumentKind::Spreadsheet).unwrap();
        let slides = host.new_document(DocumentKind::Presentation).unwrap();
        let pdf = host.new_document(DocumentKind::Pdf).unwrap();
        assert_eq!(host.workspace().tabs().len(), 4);
        assert!(host.activate(text));
        assert_eq!(host.workspace().active(), Some(text));
        host.apply_pdf(
            pdf,
            weave_pdf_backend::Edit::InsertBlankPage {
                at: 1,
                width: 595.0,
                height: 842.0,
            },
        )
        .unwrap();
        assert_eq!(host.close(pdf, false), CloseResult::NeedsSave);
        assert_eq!(host.close(pdf, true), CloseResult::Closed);
        assert_eq!(host.close(sheet, false), CloseResult::Closed);
        assert_eq!(host.close(slides, false), CloseResult::Closed);
        assert_eq!(host.workspace().active(), Some(text));
    }
}
