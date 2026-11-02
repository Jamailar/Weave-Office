use crate::language::{self, UiLanguage};
use eframe::egui;
use serde_json::{Value, json};
use std::{cell::Cell, path::Path, rc::Rc};
use weave_workspace::DocumentKind;

pub use crate::clipboard::Clipboard;

pub enum Editor {
    Word(Box<wordcraft_ui_egui::WordApp>),
    Sheet(Box<gridcraft_ui_egui::SheetApp>),
    Slides(Box<deckcraft_ui_egui::SlideApp>),
    Pdf(Box<pdfcraft_ui_egui::PdfCraftApp>),
}

impl Editor {
    pub fn generated_pdf(
        name: &str,
        bytes: Vec<u8>,
        password: Option<&str>,
    ) -> Result<Self, String> {
        let mut app = pdfcraft_ui_egui::PdfCraftApp::new();
        app.language = language::current().code().into();
        app.enable_hosted();
        app.set_host_header_prefix(Some(Box::new(crate::chrome::toolbar)));
        app.hosted_open_bytes_with_password(name, bytes, password)?;
        Ok(Self::Pdf(Box::new(app)))
    }

    pub fn create(
        kind: DocumentKind,
        path: Option<&Path>,
        clipboard: &Clipboard,
    ) -> Result<Self, String> {
        let path_text = path
            .map(|p| p.to_str().ok_or("文件路径不是有效的 Unicode"))
            .transpose()?;
        Ok(match kind {
            DocumentKind::Text => {
                let mut session = if let Some(path) = path {
                    let document = wordcraft_engine::io::open_path(path)?;
                    let mut session = wordcraft_engine::Session::new(document);
                    session.path = Some(path.to_path_buf());
                    session
                } else {
                    wordcraft_engine::Session::new(wordcraft_doc::Document::new())
                };
                session.autosave = false;
                let clip = clipboard.clone();
                let mut services = wordcraft_ui_egui::Services {
                    clipboard_text: Some(Box::new(move || clip.text())),
                    header_prefix: Some(Box::new(crate::chrome::toolbar)),
                    ..Default::default()
                };
                let clip = clipboard.clone();
                services.clipboard_html = Some(Box::new(move || clip.html()));
                let clip = clipboard.clone();
                services.clipboard_rtf = Some(Box::new(move || clip.rtf()));
                let clip = clipboard.clone();
                services.set_clipboard_html = Some(Box::new(move |html, text, mathml| {
                    clip.write_rich(html, text, mathml)
                }));
                {
                    let owner = Rc::new(());
                    let clip = clipboard.clone();
                    let source = owner.clone();
                    services.clipboard_fragment = Some(Box::new(move || clip.word(&source)));
                    let clip = clipboard.clone();
                    services.set_clipboard_fragment =
                        Some(Box::new(move |fragment, html, text, mathml| {
                            clip.write_word(&owner, fragment, html, text, mathml)
                        }));
                }
                let mut app = wordcraft_ui_egui::WordApp::new(session, services);
                app.ui.language = language::current().code().into();
                app.enable_hosted();
                Self::Word(Box::new(app))
            }
            DocumentKind::Spreadsheet => {
                let mut session = gridcraft_engine::Session::new();
                if let Some(path) = path_text {
                    session
                        .execute("file.open", json!({"path":path}))
                        .map_err(|e| e.to_string())?;
                } else {
                    session.new_workbook();
                }
                let clip = clipboard.clone();
                let mut services = gridcraft_ui_egui::Services {
                    paste_text: Some(Box::new(move || clip.text())),
                    header_prefix: Some(Box::new(crate::chrome::toolbar)),
                    ..Default::default()
                };
                let clip = clipboard.clone();
                services.paste_html = Some(Box::new(move || clip.html()));
                let clip = clipboard.clone();
                services.copy_html = Some(Box::new(move |html, text| {
                    if clip.write_rich(html, text, "") {
                        Ok(())
                    } else {
                        Err("无法写入系统剪贴板".into())
                    }
                }));
                {
                    let owner = Rc::new(());
                    let source = owner.clone();
                    let received = Rc::new(Cell::new(None));
                    let clip = clipboard.clone();
                    services.copy_cells = Some(Box::new(move |cells, html, text| {
                        clip.write_sheet(&source, cells, html, text)
                    }));
                    let clip = clipboard.clone();
                    let read_generation = received.clone();
                    services.paste_cells = Some(Box::new(move || {
                        let content = clip.read_sheet();
                        read_generation.set(content.as_ref().map(|(generation, _)| *generation));
                        content.map(|(_, cells)| cells)
                    }));
                    let clip = clipboard.clone();
                    let transfer_generation = received.clone();
                    services.request_cross_document_cut =
                        Some(Box::new(move |command, params, cells, target| {
                            clip.request_sheet_transfer(
                                transfer_generation.get(),
                                command,
                                params,
                                cells,
                                target,
                            )
                        }));
                    let clip = clipboard.clone();
                    services.update_cells = Some(Box::new(move |cells| {
                        clip.update_sheet(&owner, received.get(), cells)
                    }));
                }
                let mut app = gridcraft_ui_egui::SheetApp::new(session, services);
                app.ui.language = match language::current() {
                    UiLanguage::ZhHans => gridcraft_ui_egui::i18n::Language::Zh,
                    UiLanguage::En => gridcraft_ui_egui::i18n::Language::En,
                };
                app.enable_hosted();
                Self::Sheet(Box::new(app))
            }
            DocumentKind::Presentation => {
                let mut session = deckcraft_engine::Session::new();
                if let Some(path) = path_text {
                    session
                        .execute("file.open", &json!({"path":path}))
                        .map_err(|e| e.to_string())?;
                } else {
                    session = deckcraft_engine::Session::with_new();
                }
                let mut services = deckcraft_ui_egui::Services::default();
                services.header_prefix = Some(Box::new(crate::chrome::toolbar));
                let clip = clipboard.clone();
                services.clipboard_html = Some(Box::new(move || clip.html()));
                let clip = clipboard.clone();
                services.clipboard_text = Some(Box::new(move || clip.text()));
                let clip = clipboard.clone();
                services.clipboard_set_html =
                    Some(Box::new(move |html, text| clip.write_rich(html, text, "")));
                {
                    let owner = Rc::new(());
                    let clip = clipboard.clone();
                    services.clipboard_set_content = Some(Box::new(move |content, html, text| {
                        clip.write_slides(&owner, content, html, text)
                    }));
                    let clip = clipboard.clone();
                    services.clipboard_content = Some(Box::new(move || clip.slides()));
                    let clip = clipboard.clone();
                    services.clipboard_update_content =
                        Some(Box::new(move |content| clip.update_slides(content)));
                }
                let mut app = deckcraft_ui_egui::SlideApp::new(session, services);
                app.enable_hosted();
                app.set_hosted_language(language::current().code());
                Self::Slides(Box::new(app))
            }
            DocumentKind::Pdf => {
                let mut app = pdfcraft_ui_egui::PdfCraftApp::new();
                app.language = language::current().code().into();
                app.enable_hosted();
                app.set_host_header_prefix(Some(Box::new(crate::chrome::toolbar)));
                if let Some(path) = path {
                    app.hosted_open(path)?;
                } else {
                    app.hosted_new()?;
                }
                Self::Pdf(Box::new(app))
            }
        })
    }
    /// Apply the host preference without running logic or changing document/proofing state.
    pub fn set_interface_language(&mut self, language: UiLanguage) {
        match self {
            Self::Word(app) => app.ui.language = language.code().into(),
            Self::Sheet(app) => {
                app.ui.language = match language {
                    UiLanguage::ZhHans => gridcraft_ui_egui::i18n::Language::Zh,
                    UiLanguage::En => gridcraft_ui_egui::i18n::Language::En,
                }
            }
            Self::Slides(app) => app.set_hosted_language(language.code()),
            Self::Pdf(app) => app.language = language.code().into(),
        }
    }
    pub fn proofing_status(&self) -> Option<String> {
        match self {
            Self::Word(app) => Some(app.hosted_proofing_status()),
            _ => None,
        }
    }
    /// Editor-owned modals keep their own input/render loop; only the host's navigation and
    /// document commands wait for them. Completed dialog requests and app quit still flow.
    pub fn blocks_host_navigation(&self) -> bool {
        match self {
            Self::Word(app) => app.hosted_blocks_host_navigation(),
            Self::Sheet(app) => app.hosted_blocks_host_navigation(),
            Self::Slides(app) => app.hosted_blocks_host_navigation(),
            Self::Pdf(app) => app.hosted_blocks_host_navigation(),
        }
    }
    pub fn logic(&mut self, ctx: &egui::Context) {
        match self {
            Self::Word(a) => a.hosted_logic(ctx),
            Self::Sheet(a) => a.hosted_logic(ctx),
            Self::Slides(a) => a.hosted_logic(ctx),
            Self::Pdf(a) => a.hosted_logic(ctx),
        }
    }
    /// Only task completions may run for an inactive document. Full editor logic consumes
    /// input and owns per-document egui state, so it must remain active-tab only.
    pub fn poll_background(&mut self, ctx: &egui::Context) {
        if let Self::Pdf(app) = self {
            app.hosted_poll_background(ctx);
        }
    }
    pub fn has_pending_background(&self) -> bool {
        matches!(self, Self::Pdf(app) if app.hosted_has_pending_background())
    }
    pub fn prepare_close(&mut self) {
        if let Self::Pdf(app) = self {
            app.hosted_prepare_close();
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        match self {
            Self::Word(a) => a.hosted_ui(ui),
            Self::Sheet(a) => a.hosted_ui(ui),
            Self::Slides(a) => a.hosted_ui(ui),
            Self::Pdf(a) => a.hosted_ui(ui),
        }
    }
    pub fn raw_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Self::Slides(app) = self {
            app.raw_input_hook(ctx, raw);
        }
    }
    pub fn flush(&mut self) -> Result<(), String> {
        match self {
            Self::Word(a) => a.hosted_flush(),
            Self::Sheet(a) => a.hosted_flush(),
            Self::Slides(a) => a.hosted_flush(),
            Self::Pdf(a) => a.hosted_flush(),
        }
    }
    pub fn deactivate(&mut self, ctx: &egui::Context) -> Result<(), String> {
        match self {
            Self::Word(a) => a.hosted_deactivate(ctx),
            Self::Sheet(a) => a.hosted_deactivate(ctx),
            Self::Slides(a) => a.hosted_deactivate(ctx),
            Self::Pdf(a) => a.hosted_deactivate(ctx),
        }
    }
    pub fn dirty(&self) -> bool {
        match self {
            Self::Word(a) => a.hosted_dirty(),
            Self::Sheet(a) => a.hosted_dirty(),
            Self::Slides(a) => a.hosted_dirty(),
            Self::Pdf(a) => a.hosted_dirty(),
        }
    }
    pub fn history_available(&self) -> (bool, bool) {
        match self {
            Self::Word(app) => (app.session.can_undo(), app.session.can_redo()),
            Self::Sheet(app) => app.session.active().map_or((false, false), |doc| {
                (!doc.undo.is_empty(), !doc.redo.is_empty())
            }),
            Self::Slides(app) => app.session.active().map_or((false, false), |doc| {
                (!doc.history.undo.is_empty(), !doc.history.redo.is_empty())
            }),
            Self::Pdf(app) => app
                .active_ids()
                .and_then(|(_, id)| app.session.get(id))
                .map_or((false, false), |doc| {
                    (doc.can_undo().is_some(), doc.can_redo().is_some())
                }),
        }
    }
    /// The document's current contents for automatic recovery, in the editor's own format,
    /// produced on the recovery thread. Uncommitted input (a cell being typed, an IME
    /// composition) is left out; the editor is not flushed, so nothing it shows changes.
    pub fn snapshot(&self) -> Result<crate::recovery::Snapshot, String> {
        Ok(match self {
            Self::Word(app) => {
                let document = app.session.doc.clone();
                Box::new(move || wordcraft_engine::io::save_bytes("recovery.docx", &document))
            }
            Self::Sheet(app) => {
                let workbook = app.session.active().ok_or("没有打开的表格")?.wb.clone();
                Box::new(move || {
                    gridcraft_engine::io::save_bytes(&workbook, "recovery.xlsx")
                        .map_err(|error| error.to_string())
                })
            }
            Self::Slides(app) => {
                let presentation = app.session.active().ok_or("没有打开的演示")?.doc.clone();
                Box::new(move || {
                    deckcraft_engine::cmd::file::save_bytes(&presentation, "pptx")
                        .map_err(|error| error.to_string())
                })
            }
            Self::Pdf(app) => {
                let (_, id) = app.active_ids().ok_or("没有打开的 PDF")?;
                let bytes = app
                    .session
                    .save_bytes(id)
                    .map_err(|error| error.to_string())?;
                Box::new(move || Ok(std::sync::Arc::unwrap_or_clone(bytes)))
            }
        })
    }
    /// Reopen a recovery copy as an unsaved document that belongs to `origin` (its file before
    /// the crash), not to the copy.
    pub fn restore(
        kind: DocumentKind,
        copy: &Path,
        origin: Option<&Path>,
        clipboard: &Clipboard,
    ) -> Result<Self, String> {
        if kind == DocumentKind::Pdf {
            let bytes = std::fs::read(copy).map_err(|error| error.to_string())?;
            let name = origin
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "PDF".into());
            return Self::generated_pdf(&name, bytes, None);
        }
        let mut editor = Self::create(kind, Some(copy), clipboard)?;
        let origin_text = origin.and_then(Path::to_str).map(str::to_owned);
        match &mut editor {
            Self::Word(app) => app.session.path = origin.map(Path::to_path_buf),
            Self::Sheet(app) => {
                if let Some(doc) = app.session.active_mut() {
                    doc.path = origin_text;
                }
            }
            Self::Slides(app) => {
                if let Some(doc) = app.session.active_mut() {
                    doc.path = origin_text;
                }
            }
            Self::Pdf(_) => {}
        }
        Ok(editor)
    }
    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        match self {
            Self::Word(a) => a.hosted_save(path),
            Self::Sheet(a) => a.hosted_save(path),
            Self::Slides(a) => a.hosted_save(path),
            Self::Pdf(a) => a.hosted_save(path),
        }
    }
    pub fn command(&mut self, id: &str, params: Value) -> Result<Value, String> {
        match self {
            Self::Word(a) => a.hosted_command(id, params),
            Self::Sheet(a) => a.hosted_command(id, params),
            Self::Slides(a) => a.hosted_command(id, params),
            Self::Pdf(a) => a.hosted_command(id, params),
        }
    }
    pub fn host_commands(&mut self) -> Vec<(String, Value)> {
        match self {
            Self::Word(a) => a.take_host_commands(),
            Self::Sheet(a) => a.take_host_commands(),
            Self::Slides(a) => a.take_host_commands(),
            Self::Pdf(a) => a.take_host_commands(),
        }
    }
}
