use crate::editor::{Clipboard, Editor};
use eframe::egui;
use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, PoisonError, RwLock},
};
use weave_workspace::{CloseResult, DocumentKind, TabId, Workspace};

pub struct Document {
    pub editor: Editor,
    pub file_services: crate::office_files::OfficeFiles,
    pub memory: egui::Memory,
    /// The fonts this tab's editor installed: its part of the shared set ([`crate::font_set`]).
    pub fonts: Option<egui::FontDefinitions>,
    /// The file a recovered document came from before the crash: the save folder to suggest.
    pub origin: Option<PathBuf>,
    /// Reopened from a recovery copy and not saved since: unsaved whatever the editor says.
    pub recovered: bool,
}
#[derive(Default)]
pub struct Host {
    pub workspace: Workspace,
    pub documents: HashMap<TabId, Document>,
    clipboard: Clipboard,
    untitled: u64,
    output_paths: Arc<RwLock<HashSet<PathBuf>>>,
}
impl Host {
    #[cfg(test)]
    pub(crate) fn with_clipboard(clipboard: Clipboard) -> Self {
        Self {
            clipboard,
            ..Default::default()
        }
    }

    pub fn receive_sheet_transfers(&mut self) -> Vec<String> {
        let requests = self.clipboard.take_sheet_transfers();
        let mut errors = Vec::new();
        for request in requests {
            // Escape or a newer copy cancels a move that has not committed yet.
            if !self.clipboard.sheet_transfer_is_current(&request) {
                continue;
            }
            match crate::sheet_transfer::apply(self, &request) {
                Ok(()) => self.clipboard.finish_sheet_transfer(request.generation),
                Err(error) => errors.push(error),
            }
        }
        self.sync_dirty();
        errors
    }
    pub(crate) fn has_sheet_transfers(&self) -> bool {
        self.documents.values().any(|document| matches!(&document.editor,
            Editor::Sheet(app) if app.session.active().is_some_and(|doc| self.clipboard.has_sheet_transfer(doc.uid))))
    }

    fn settle_sheet_transfers(&mut self) -> Result<(), String> {
        let errors = self.receive_sheet_transfers();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    pub fn poll_background(&mut self, ctx: &egui::Context) {
        for document in self.documents.values_mut() {
            document.editor.poll_background(ctx);
        }
        self.sync_dirty();
    }

    pub fn prepare_close(&mut self, ids: &[TabId]) {
        for id in ids {
            if let Some(document) = self.documents.get_mut(id) {
                document.editor.prepare_close();
            }
        }
        self.sync_dirty();
    }
    pub fn pending_background(&self, ids: &[TabId]) -> Option<TabId> {
        ids.iter().copied().find(|id| {
            self.documents
                .get(id)
                .is_some_and(|document| document.editor.has_pending_background()
                    || matches!(&document.editor, Editor::Sheet(app) if app.session.active().is_some_and(|doc| self.clipboard.has_sheet_transfer(doc.uid))))
        })
    }
    /// Background exporters and file registration share this lock. The policy reads live
    /// identities while writing, so a picker or worker cannot retain an obsolete path snapshot.
    pub fn export_writer(&self) -> pdfcraft_ui_egui::HostExportWriter {
        let paths = self.output_paths.clone();
        Arc::new(move |path, bytes| {
            let protected = paths.read().unwrap_or_else(PoisonError::into_inner);
            let path = canonical_destination(path)?;
            if protected.contains(&path) {
                return Err("写入目标已在标签中打开，请选择其他文件名".into());
            }
            atomic_export(&path, bytes)?;
            drop(protected);
            Ok(())
        })
    }
    /// Native Save confirms one overwrite. Folder exports never replace existing files.
    pub fn office_export_writer(&self) -> crate::office_files::OfficeExportWriter {
        let paths = self.output_paths.clone();
        Arc::new(move |outputs, overwrite| {
            let protected = paths.read().unwrap_or_else(PoisonError::into_inner);
            let result = export_office_files(outputs, overwrite, &protected);
            drop(protected);
            result
        })
    }
    fn install_services(&self, editor: &mut Editor) -> crate::office_files::OfficeFiles {
        let files = crate::office_files::OfficeFiles::default();
        files.install(editor);
        files.install_exports(editor, self.office_export_writer());
        if let Editor::Pdf(app) = editor {
            app.set_host_export_writer(self.export_writer());
        }
        files
    }
    /// Install generated PDFs as independent, unsaved workspace files before acknowledging
    /// the originating editor. Destructive extraction cannot run before this succeeds.
    pub fn receive_generated_pdfs(&mut self) -> Vec<String> {
        let origins: Vec<_> = self.workspace.tabs().iter().map(|tab| tab.id).collect();
        let mut errors = Vec::new();
        for origin in origins {
            let requests = match self.documents.get_mut(&origin).map(|doc| &mut doc.editor) {
                Some(Editor::Pdf(app)) => app.take_host_documents(),
                _ => continue,
            };
            for request in requests {
                let result = self.open_generated_pdf(
                    &request.name,
                    request.bytes,
                    request.password.as_deref(),
                );
                let result = result.map(|_| ());
                if let Some(Editor::Pdf(app)) =
                    self.documents.get_mut(&origin).map(|doc| &mut doc.editor)
                {
                    if let Err(error) = app.hosted_document_result(request.id, result) {
                        errors.push(error);
                    }
                }
            }
        }
        self.sync_dirty();
        errors
    }

    pub fn open_generated_pdf(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        password: Option<&str>,
    ) -> Result<TabId, String> {
        let mut editor = Editor::generated_pdf(name, bytes, password)?;
        let file_services = self.install_services(&mut editor);
        // A generated name is a label, never an on-disk identity or implicit save target.
        let title = Path::new(name)
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("生成文档.pdf");
        let id = self
            .workspace
            .add(DocumentKind::Pdf, title, None)
            .ok_or("标签标识已耗尽")?;
        self.documents.insert(
            id,
            Document {
                editor,
                file_services,
                memory: Default::default(),
                fonts: None,
                origin: None,
                recovered: false,
            },
        );
        self.sync_dirty();
        Ok(id)
    }

    /// Export a copy without changing a source path or saved version. Every open destination
    /// is protected, including the active PDF itself, whose Save operation has different semantics.
    pub fn export_pdf_as(&mut self, id: TabId, path: &Path) -> Result<(), String> {
        self.settle_sheet_transfers()?;
        if !path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("pdf"))
        {
            return Err("请使用 .pdf 格式导出".into());
        }
        let editor = &mut self.documents.get_mut(&id).ok_or("标签不存在")?.editor;
        let bytes = crate::output::pdf_bytes(editor, crate::output::PdfPurpose::Export)?;
        self.export_writer()(path, &bytes)?;
        self.sync_dirty();
        Ok(())
    }

    pub fn active(&self) -> Option<&Document> {
        self.documents.get(&self.workspace.active()?)
    }
    pub fn active_mut(&mut self) -> Option<&mut Document> {
        self.documents.get_mut(&self.workspace.active()?)
    }
    pub fn new_document(&mut self, kind: DocumentKind) -> Result<TabId, String> {
        let mut editor = Editor::create(kind, None, &self.clipboard)?;
        let file_services = self.install_services(&mut editor);
        self.untitled = self.untitled.saturating_add(1);
        let label = crate::language::tr(match kind {
            DocumentKind::Text => "文档",
            DocumentKind::Spreadsheet => "表格",
            DocumentKind::Presentation => "演示",
            DocumentKind::Pdf => "PDF",
            DocumentKind::Markdown => "笔记",
            DocumentKind::PlainText => "文本",
        });
        let id = self
            .workspace
            .add(
                kind,
                crate::language::tr("未命名{kind}{number}")
                    .replace("{kind}", label)
                    .replace("{number}", &self.untitled.to_string()),
                None,
            )
            .ok_or("标签标识已耗尽")?;
        self.documents.insert(
            id,
            Document {
                editor,
                file_services,
                memory: Default::default(),
                fonts: None,
                origin: None,
                recovered: false,
            },
        );
        self.sync_dirty();
        Ok(id)
    }
    pub fn open(&mut self, path: &Path) -> Result<TabId, String> {
        let registry = self.output_paths.clone();
        let mut protected = registry.write().unwrap_or_else(PoisonError::into_inner);
        let path = path
            .canonicalize()
            .map_err(|e| format!("无法打开 {}：{e}", path.display()))?;
        if let Some(id) = self
            .workspace
            .tabs()
            .iter()
            .find(|t| t.path.as_ref() == Some(&path))
            .map(|t| t.id)
        {
            self.workspace.activate(id);
            return Ok(id);
        }
        let kind = match DocumentKind::from_path(&path) {
            Some(kind) => kind,
            // An unknown extension may still be text (a dotfile, a script without an
            // extension): look at the start of the file before refusing it.
            None if looks_like_text_file(&path) => DocumentKind::PlainText,
            None => return Err("不支持此文件类型".into()),
        };
        let mut editor = Editor::create(kind, Some(&path), &self.clipboard)?;
        let file_services = self.install_services(&mut editor);
        let title = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .ok_or("文件名为空")?;
        let id = self
            .workspace
            .add(kind, title, Some(path.clone()))
            .ok_or("标签标识已耗尽")?;
        self.documents.insert(
            id,
            Document {
                editor,
                file_services,
                memory: Default::default(),
                fonts: None,
                origin: None,
                recovered: false,
            },
        );
        protected.insert(path);
        drop(protected);
        self.sync_dirty();
        Ok(id)
    }
    pub fn dirty(&self, id: TabId) -> bool {
        self.documents
            .get(&id)
            .is_some_and(|d| d.recovered || d.editor.dirty())
    }
    pub fn sync_dirty(&mut self) {
        for (&id, doc) in &self.documents {
            self.workspace
                .sync_dirty(id, doc.recovered || doc.editor.dirty());
        }
    }
    /// The folder to suggest when saving `id` for the first time.
    pub fn origin(&self, id: TabId) -> Option<&Path> {
        self.documents.get(&id)?.origin.as_deref()
    }
    /// Reopen a copy automatic recovery kept as an unsaved document titled `title`. It has no
    /// file until it is saved; `origin` is only the folder to suggest.
    pub fn restore(
        &mut self,
        kind: DocumentKind,
        title: &str,
        copy: &Path,
        origin: Option<&Path>,
    ) -> Result<TabId, String> {
        let mut editor = Editor::restore(kind, copy, origin, &self.clipboard)?;
        let file_services = self.install_services(&mut editor);
        let id = self
            .workspace
            .add(kind, title, None)
            .ok_or("标签标识已耗尽")?;
        self.documents.insert(
            id,
            Document {
                editor,
                file_services,
                memory: Default::default(),
                fonts: None,
                origin: origin.map(Path::to_path_buf),
                recovered: true,
            },
        );
        self.sync_dirty();
        Ok(id)
    }
    pub fn path(&self, id: TabId) -> Option<&Path> {
        self.workspace.tab(id)?.path.as_deref()
    }
    pub fn save_as(&mut self, id: TabId, path: &Path) -> Result<(), String> {
        self.settle_sheet_transfers()?;
        let registry = self.output_paths.clone();
        let mut protected = registry.write().unwrap_or_else(PoisonError::into_inner);
        let kind = self.workspace.tab(id).ok_or("标签不存在")?.kind;
        if !kind.can_save_to(path) {
            return Err(format!("请使用 .{} 格式保存", extension(kind)));
        }
        let path = canonical_destination(path)?;
        if self
            .workspace
            .tabs()
            .iter()
            .any(|t| t.id != id && t.path.as_ref() == Some(&path))
        {
            return Err("该文件已在其他标签中打开".into());
        }
        let document = self.documents.get_mut(&id).ok_or("标签不存在")?;
        document.editor.save(&path)?;
        document.recovered = false;
        if self.dirty(id) {
            return Err("仍有未提交的修改，请检查编辑区后重试".into());
        }
        self.workspace.set_path(id, path);
        *protected = self
            .workspace
            .tabs()
            .iter()
            .filter_map(|tab| tab.path.clone())
            .collect();
        drop(protected);
        self.sync_dirty();
        Ok(())
    }
    pub fn close(&mut self, id: TabId, discard: bool) -> CloseResult {
        if !self.documents.contains_key(&id) {
            return CloseResult::Missing;
        }
        if self.pending_background(&[id]).is_some() {
            return CloseResult::NeedsSave;
        }
        if self.dirty(id) && !discard {
            return CloseResult::NeedsSave;
        }
        // Cancel before taking the path registry lock: an in-flight completion may
        // hold its read lock, and must finish while the origin is still registered.
        self.documents[&id].file_services.cancel_all();
        let registry = self.output_paths.clone();
        let mut protected = registry.write().unwrap_or_else(PoisonError::into_inner);
        self.workspace.close(id, true);
        self.documents.remove(&id);
        *protected = self
            .workspace
            .tabs()
            .iter()
            .filter_map(|tab| tab.path.clone())
            .collect();
        CloseResult::Closed
    }
}
fn export_office_files(
    outputs: &[(PathBuf, Vec<u8>)],
    overwrite: bool,
    protected: &HashSet<PathBuf>,
) -> Result<(), String> {
    if outputs.is_empty() || (overwrite && outputs.len() != 1) {
        return Err("导出文件数量与覆盖策略不符".into());
    }
    let live_protected: HashSet<_> = protected
        .iter()
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    let mut destinations = HashSet::new();
    let mut resolved = Vec::with_capacity(outputs.len());
    // Check the entire batch before creating any output. symlink_metadata also
    // detects dangling symlinks, which must not be replaced by a folder export.
    for (path, bytes) in outputs {
        if !overwrite {
            match std::fs::symlink_metadata(path) {
                Ok(_) => {
                    return Err(format!(
                        "导出目标已存在，请选择其他文件夹：{}",
                        path.display()
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("无法检查 {}：{error}", path.display())),
            }
        }
        let path = canonical_destination(path)?;
        if protected.contains(&path) || live_protected.contains(&path) {
            return Err("写入目标已在标签中打开，请选择其他文件名".into());
        }
        if path.is_dir() {
            return Err(format!("导出目标是文件夹：{}", path.display()));
        }
        if !destinations.insert(path.clone()) {
            return Err("多个导出文件指向同一目标".into());
        }
        resolved.push((path, bytes));
    }
    let mut staged = Vec::with_capacity(resolved.len());
    for (path, bytes) in resolved {
        let parent = path.parent().ok_or("导出目标缺少父目录")?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| format!("无法准备 {}：{error}", path.display()))?;
        temporary
            .write_all(bytes)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|error| format!("无法暂存 {}：{error}", path.display()))?;
        staged.push((path, temporary));
    }
    for (committed, (path, temporary)) in staged.into_iter().enumerate() {
        let result = if overwrite {
            temporary.persist(&path)
        } else {
            temporary.persist_noclobber(&path)
        };
        if let Err(error) = result {
            return Err(format!(
                "无法完成导出 {}：{}。已成功导出 {committed} 个文件并保留，请检查目标文件夹后重试",
                path.display(),
                error.error,
            ));
        }
    }
    Ok(())
}
fn atomic_export(path: &Path, bytes: &[u8]) -> Result<(), String> {
    pdfcraft_platform::staging::write_atomically(
        path,
        bytes,
        pdfcraft_platform::staging::StagingName::TagThenSuffix,
    )
    .map_err(|error| error.to_string())
}
pub fn extension(kind: DocumentKind) -> &'static str {
    kind.default_extension()
}

/// Whether a file with an extension the host does not know is text.
fn looks_like_text_file(path: &Path) -> bool {
    use std::io::Read;
    let mut head = Vec::with_capacity(8192);
    std::fs::File::open(path)
        .and_then(|file| file.take(8192).read_to_end(&mut head))
        .is_ok()
        && path.is_file()
        && weave_text_editor::file::looks_like_text(&head)
}
fn canonical_destination(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return path.canonicalize().map_err(|e| e.to_string());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(parent
        .canonicalize()
        .map_err(|e| e.to_string())?
        .join(path.file_name().ok_or("缺少文件名")?))
}
