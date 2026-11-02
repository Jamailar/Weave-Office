//! Editor surface used by an Office host. The host owns tabs, paths and window services.
use super::*;
use std::path::Path;

impl PdfCraftApp {
    /// Install the host's File and quick access controls in the editor mode row.
    pub fn set_host_header_prefix(&mut self, prefix: Option<HostHeaderPrefix>) {
        self.host_header_prefix = prefix;
    }

    /// Route every advanced file export through the host's live path-protection policy.
    /// Install before starting editor work. Trusted host saves use `hosted_save` separately.
    pub fn set_host_export_writer(&mut self, writer: HostExportWriter) {
        self.host_export_writer = writer;
    }

    pub(crate) fn write_export(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        (self.host_export_writer)(path, bytes)
    }

    /// Queue owned bytes without changing this editor's document or view.
    pub(crate) fn queue_host_document(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        password: Option<String>,
        delete: Option<(DocId, u64, Vec<usize>)>,
    ) -> Result<(), String> {
        let id = self.next_host_document_id.checked_add(1).ok_or("Too many generated PDF requests")?;
        self.next_host_document_id = id;
        self.host_document_pending.push(HostedDocumentPending { id, delete });
        self.host_documents.push(HostedDocument { id, name: name.into(), bytes, password });
        Ok(())
    }

    /// Drain PDFs that must each be opened in a new host-owned editor. Every result must be
    /// acknowledged with `hosted_document_result`, including failed or cancelled opens.
    pub fn take_host_documents(&mut self) -> Vec<HostedDocument> {
        std::mem::take(&mut self.host_documents)
    }

    /// Acknowledge a generated document only after the host has installed its new tab.
    /// Source-page deletion is deferred until success and refused if the source changed.
    pub fn hosted_document_result(&mut self, id: u64, result: Result<(), String>) -> Result<(), String> {
        let index = self.host_document_pending.iter().position(|p| p.id == id).ok_or("Unknown generated PDF request")?;
        let pending = self.host_document_pending.remove(index);
        let result = result.and_then(|()| {
            let Some((source, generation, pages)) = pending.delete else { return Ok(()) };
            if self.active_ids().map(|(_, id)| id) != Some(source) {
                return Err("The extracted PDF was opened, but source pages were retained because the active document changed.".into());
            }
            // Commit new page drafts first; a changed generation then prevents deleting edits
            // absent from the exported bytes. Unposted comments stay intact and block deletion.
            self.hosted_flush()?;
            if self.session.get(source).map(|doc| doc.edit_generation()) != Some(generation) {
                return Err("The extracted PDF was opened, but source pages were retained because the document changed.".into());
            }
            if self.apply_edit(pdfcraft_engine::Edit::DeletePages { pages }) {
                Ok(())
            } else {
                Err("The extracted PDF was opened, but its source pages could not be deleted.".into())
            }
        });
        if let Err(error) = &result {
            self.notify(error.clone());
        }
        result
    }

    fn hosted_comment_draft(&self) -> bool {
        self.views.iter().any(|view| {
            let comments = &view.comments;
            comments
                .composer
                .as_ref()
                .is_some_and(|draft| !draft.text.trim().is_empty() || matches!(draft.kind, crate::comments::ComposerKind::Edit(_)))
                || comments.editing.is_some()
                || !comments.reply.trim().is_empty()
                || !comments.add_box.trim().is_empty()
        })
    }

    /// Use this instance as one document inside a host-owned egui context.
    /// The host must restore this editor's egui memory/fonts before calling its frame methods.
    pub fn enable_hosted(&mut self) {
        self.hosted = true;
        self.native_menu = None;
        self.os_events = None;
        self.recovery = None;
        self.integrated_titlebar = false;
        self.window_controls = None;
        self.reload_changed_files = false;
        self.default_mode = Mode::Read;
        self.mode = Mode::Read;
        self.left_open = false;
        self.right = None;
    }

    /// Whether an editor-owned modal must finish before an external host navigation command.
    /// Search/navigation panes, the dismissible command popup and the Combine tab are work
    /// surfaces, not blocking modals. Keep editor logic/UI and dialog-generated host requests
    /// running, and route application quit through the host's save transaction.
    pub fn hosted_blocks_host_navigation(&self) -> bool {
        self.modal_open()
    }

    pub(crate) fn is_host_command(id: &str) -> bool {
        matches!(
            id,
            "file.open"
                | "file.open_recent"
                | "file.save"
                | "file.save_as"
                | "file.close"
                | "file.close_all"
                | "app.preferences"
                | "app.quit"
                | "help.about"
                | "help.check_updates"
                | "view.full_screen"
                | "view.next_tab"
                | "view.previous_tab"
                | "view.home"
                | "view.palette"
                | "print.dialog"
        ) || id.starts_with("view.theme")
    }

    /// Drain requests for host-owned menus, files and system services.
    pub fn take_host_commands(&mut self) -> Vec<(String, serde_json::Value)> {
        std::mem::take(&mut self.host_commands)
    }

    /// Dispatch the same registered commands used by PDF tools and native menus.
    /// Parameterized engine edits remain available through `apply_edit`.
    pub fn hosted_command(&mut self, id: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        if !params.is_null() && params.as_object().is_none_or(|object| !object.is_empty()) {
            return Err("PDF UI commands do not accept parameters".into());
        }
        if self.execute(id) { Ok(serde_json::Value::Null) } else { Err(format!("PDF command is unknown or unavailable: {id}")) }
    }

    /// Open one PDF. Password errors are reported to the host; existing work is never replaced.
    pub fn hosted_open(&mut self, path: &Path) -> Result<(), String> {
        if !self.views.is_empty() || !self.session.docs().is_empty() {
            return Err("This PDF editor already contains a document".into());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Document.pdf".into());
        let id = self.session.open(&name, Some(path.to_string_lossy().into_owned()), std::sync::Arc::new(bytes), None).map_err(|e| e.to_string())?;
        self.hosted_attach(id)
    }

    /// Open generated bytes in an empty, independent editor as an unsaved document.
    pub fn hosted_open_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<(), String> {
        self.hosted_open_bytes_with_password(name, bytes, None)
    }

    /// Like `hosted_open_bytes`, preserving access to encrypted revisions without persisting
    /// their password outside this editor's normal in-memory document state.
    pub fn hosted_open_bytes_with_password(&mut self, name: &str, bytes: Vec<u8>, password: Option<&str>) -> Result<(), String> {
        if !self.views.is_empty() || !self.session.docs().is_empty() {
            return Err("This PDF editor already contains a document".into());
        }
        let id = self.session.open_new_with_password(name, std::sync::Arc::new(bytes), password).map_err(|e| e.to_string())?;
        self.hosted_attach(id)
    }

    /// Create an unsaved A4 PDF in this editor.
    pub fn hosted_new(&mut self) -> Result<(), String> {
        if !self.views.is_empty() || !self.session.docs().is_empty() {
            return Err("This PDF editor already contains a document".into());
        }
        let bytes = self.session.create_blank(595.276, 841.89, 1).map_err(|e| e.to_string())?;
        self.hosted_open_bytes("Untitled.pdf", (*bytes).clone())
    }

    fn hosted_attach(&mut self, id: DocId) -> Result<(), String> {
        let doc = self.session.get(id).ok_or("PDF could not be opened")?;
        self.views.push(DocView::new(id, &doc.info, self.view_defaults));
        self.active = Some(self.views.len() - 1);
        Ok(())
    }

    /// Process document-local work, without owning the host window or global shortcuts.
    pub fn hosted_logic(&mut self, ctx: &egui::Context) {
        self.ctx = Some(ctx.clone());
        i18n::set_current(i18n::Lang::from_pref(&self.language));
        let cjk = theme::CjkPreference::for_language(i18n::current().code());
        if !self.styled || self.fonts_cjk != cjk {
            egui_extras::install_image_loaders(ctx);
            self.fonts_cjk = cjk;
            theme::install_fonts_for(ctx, self.fonts_cjk);
            self.styled = true;
            ctx.request_repaint();
        }
        self.theme = if ctx.global_style().visuals.dark_mode { ThemeKind::Dark } else { ThemeKind::Light };
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("pdfcraft-theme"), theme::Tokens::for_kind(self.theme)));
        if let Some((_, id)) = self.active_ids() {
            let out = self.session.take_js_output(id);
            self.handle_js(id, out);
        }
        if !self.combine_showing() {
            self.shortcuts(ctx);
        }
        let blocked = self.combine_showing() || self.modal_open() || self.palette_open || !ctx.input(|i| i.focused);
        for view in &mut self.views {
            view.objects.set_input_blocked(blocked);
            if blocked {
                view.auto_scroll.cancel();
            }
        }
        self.process_pending_edits();
        self.hosted_poll_background(ctx);
        self.process_file_requests();
        #[cfg(not(target_arch = "wasm32"))]
        self.process_picked();
    }

    /// Consume worker results even while another host document is active. This reads no
    /// input and installs no fonts, style or egui memory; the context is only used to wake
    /// the host. It is safe to call again from `hosted_logic` in the same frame.
    ///
    /// File-picker callbacks stay with the host's originating-editor context: some install
    /// document fonts. Optimization can enqueue a host save request; the host must still
    /// drain and answer it. Automatic OCR keeps its normal Read-mode/modal deferral.
    pub fn hosted_poll_background(&mut self, ctx: &egui::Context) {
        self.ctx = Some(ctx.clone());
        let language = i18n::current();
        i18n::set_current(i18n::Lang::from_pref(&self.language));
        self.poll_export();
        self.poll_ocr();
        self.poll_optimize();
        self.poll_action();
        self.poll_print();
        i18n::set_current(language);
    }

    /// Work whose result must settle before the host can close this editor. Includes ready
    /// results awaiting consumption, generated documents awaiting acknowledgement and file
    /// dialogs holding an edit/export continuation. Rendering and read-only discovery are
    /// deliberately excluded. Poll first, then check this before starting a close prompt.
    pub fn hosted_has_pending_background(&self) -> bool {
        let pending = self.export_status.is_some()
            || self.ocr_run.is_some()
            || self.ocr_batch.is_some()
            || self.optimize_run.is_some()
            || self.action_run.is_some()
            || self.print_tasks.has_pending_jobs()
            || !self.host_document_pending.is_empty()
            || self.requests.lock().map(|requests| !requests.is_empty()).unwrap_or(false);
        #[cfg(not(target_arch = "wasm32"))]
        let pending = pending || self.pickers.has_pending_work();
        pending
    }

    /// Read mode can defer implicit OCR. Closing also ends that implicit work, retaining
    /// recognized pages for the normal save/discard decision. Explicit tasks still finish.
    pub fn hosted_prepare_close(&mut self) {
        if self.ocr_run.as_ref().is_some_and(|run| run.automatic.is_some()) {
            self.cancel_ocr();
            let language = i18n::current();
            i18n::set_current(i18n::Lang::from_pref(&self.language));
            self.poll_ocr();
            i18n::set_current(language);
        }
    }

    fn hosted_fonts_ready(&self, ctx: &egui::Context) -> bool {
        ctx.fonts(|fonts| {
            ["medium", "semibold"]
                .iter()
                .all(|name| fonts.definitions().families.get(&egui::FontFamily::Name((*name).into())).is_some_and(|chain| !chain.is_empty()))
        })
    }

    /// Draw the mature PDF canvas and editing panes, without the application shell.
    pub fn hosted_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // Hosted logic runs before shell text and activates fonts immediately. Check here,
        // where an atlas also exists after a hidden logic tick queued the initial fonts.
        self.fonts_ready = self.hosted_fonts_ready(&ctx);
        if !self.fonts_ready {
            self.fonts_cjk = theme::CjkPreference::for_language(i18n::Lang::from_pref(&self.language).code());
            theme::install_fonts_for(&ctx, self.fonts_cjk);
            self.fonts_ready = self.hosted_fonts_ready(&ctx);
            if !self.fonts_ready {
                ctx.request_discard("prepare hosted PDF fonts");
                ctx.request_repaint();
                if !ctx.will_discard() {
                    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::Tokens::get(&ctx).pasteboard)).show(ui, |_| {});
                }
                return;
            }
        }
        i18n::set_current(i18n::Lang::from_pref(&self.language));
        self.motion.begin_pass(&ctx, self.animation_effects);
        for view in &mut self.views {
            view.begin_render_frame();
        }
        if !self.combine_showing() && !self.host_tabs {
            egui::Panel::top("pdf-editor-tools")
                .exact_size(32.0)
                .frame(egui::Frame::NONE.fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(8, 2)))
                .show(ui, |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("hosted_pdf_modes")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if self.hosted {
                                    let (can_undo, can_redo) = self
                                        .active
                                        .and_then(|i| self.views.get(i))
                                        .and_then(|view| self.session.get(view.id))
                                        .map(|doc| (doc.can_undo().is_some(), doc.can_redo().is_some()))
                                        .unwrap_or((false, false));
                                    if let Some(prefix) = self.host_header_prefix.as_mut() {
                                        self.host_commands.extend(prefix(ui, can_undo, can_redo));
                                        ui.add_space(6.0);
                                    }
                                }
                                for (id, label) in HOSTED_TABS {
                                    if hosted_mode_tab(ui, tl!(label), self.hosted_tab_open(id)).clicked() {
                                        self.open_hosted_tab(id);
                                    }
                                }
                            });
                        });
                });
        }
        self.all_tools_full = false;
        if !self.combine_showing() {
            side_panels::show(self, ui);
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::Tokens::get(&ctx).pasteboard)).show(ui, |ui| {
            if self.combine_showing() {
                if ui.button(tl!("Close")).clicked() {
                    self.close_combine_tab();
                }
                combine_ui::page(self, ui);
            } else if let Some(index) = self.active.filter(|i| *i < self.views.len()) {
                canvas::document_area(self, index, ui);
            }
        });
        self.process_pending_edits();
        dialogs::show(self, &ctx);
        self.show_progress(&ctx);
        widgets::toast(self, &ctx);
        self.finish_render_frame(&ctx);
    }

    /// Commit page/form drafts before saving. Unposted comments require an explicit Post.
    pub fn hosted_flush(&mut self) -> Result<(), String> {
        // Comment Post is an explicit action. Do not silently save an earlier version or
        // discard its composer/reply when the host saves this editor.
        if self.hosted_comment_draft() {
            return Err("Post or cancel the unfinished PDF comment before saving.".into());
        }
        self.hosted_commit_page_drafts()
    }

    fn hosted_commit_page_drafts(&mut self) -> Result<(), String> {
        if !self.process_pending_edits() {
            return Err("The PDF field rejected its value; correct it before continuing".into());
        }
        if let Some((_, id)) = self.active_ids() {
            if !self.commit_typing_in(id) {
                return Err("The PDF has an unfinished edit that could not be committed".into());
            }
        }
        Ok(())
    }

    pub fn hosted_dirty(&self) -> bool {
        self.hosted_comment_draft() || self.views.iter().enumerate().any(|(i, _)| self.has_unsaved_work(i))
    }

    /// A synchronous atomic save: no UI edits can interleave between serialization and commit.
    pub fn hosted_save(&mut self, path: &Path) -> Result<(), String> {
        self.hosted_flush()?;
        let (_, id) = self.active_ids().ok_or("No PDF is open")?;
        let bytes = self.session.save_bytes(id).map_err(|e| e.to_string())?;
        self.session.check_save(id, &bytes).map_err(|e| e.to_string())?;
        let destination = path.to_str().ok_or("The PDF destination is not valid UTF-8")?;
        crate::editing::write_atomically(destination, &bytes).map_err(|e| e.to_string())?;
        self.session.mark_saved(id, bytes, Some(destination.to_owned())).map_err(|e| e.to_string())?;
        if let Some(doc) = self.session.get(id) {
            for view in &mut self.views {
                if view.id == id {
                    view.document_changed(&doc.info);
                }
            }
        }
        Ok(())
    }

    /// Release render demand and pointer gestures before the host swaps context memory.
    /// Unposted comments stay in this editor and do not prevent visiting another document.
    pub fn hosted_deactivate(&mut self, ctx: &egui::Context) -> Result<(), String> {
        // Keep coexisting inputs too: committing a typewriter annotation invokes
        // comment_added, which may close an inline comment being edited to empty text.
        // All drafts remain owned by this PdfCraftApp while its host tab is inactive.
        if !self.hosted_comment_draft() {
            self.hosted_commit_page_drafts()?;
        }
        for view in &mut self.views {
            view.cancel_pointer_gestures();
            view.auto_scroll.cancel();
            view.objects.set_input_blocked(true);
            if let Some(doc) = self.session.get(view.id) {
                view.suspend_rendering(&doc.renderer);
            }
        }
        self.space_hand = None;
        ctx.memory_mut(|memory| {
            memory.interrupt_ime();
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        ctx.stop_dragging();
        egui::Popup::close_all(ctx);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn background_app() -> PdfCraftApp {
        let mut app = PdfCraftApp::new();
        app.enable_hosted();
        app.language = "en".into();
        app.hosted_new().unwrap();
        app
    }

    fn finished_ocr(app: &PdfCraftApp, automatic: bool) -> ocr_ui::OcrRun {
        let (_, doc) = app.active_ids().unwrap();
        let page = pdfcraft_engine::ocr::OcrPage {
            page: 0,
            words: vec![pdfcraft_engine::ocr::PlacedWord { text: "scanned".into(), origin: [72.0, 700.0], across: [60.0, 0.0], up: [0.0, 12.0] }],
            already_on_page: 0,
            skipped: None,
        };
        ocr_ui::OcrRun {
            doc,
            progress: Arc::new(Mutex::new(ocr_ui::OcrProgress { done: 1, total: 1, result: Some(Ok(vec![page])), cancel: false })),
            automatic: automatic.then_some(ocr_ui::AutoOcr { page: 0, generation: app.session.get(doc).unwrap().edit_generation() }),
        }
    }

    #[test]
    fn hosted_background_applies_ocr_once_without_input_fonts_or_style() {
        let mut app = background_app();
        let (_, doc) = app.active_ids().unwrap();
        app.ocr_run = Some(finished_ocr(&app, false));
        app.theme = ThemeKind::Dark;
        let ctx = egui::Context::default();
        let host_language = i18n::Lang::from_pref("fr");
        i18n::set_current(host_language);
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::W,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            let ctx = ui.ctx();
            let style = ctx.global_style();
            let events = ctx.input(|input| input.events.clone());
            assert!(app.hosted_has_pending_background());
            app.hosted_poll_background(ctx);
            assert!(!app.hosted_has_pending_background());
            assert_eq!(app.session.get(doc).unwrap().can_undo(), Some("Recognize text"));
            let generation = app.session.get(doc).unwrap().edit_generation();
            app.hosted_poll_background(ctx);
            assert_eq!(app.session.get(doc).unwrap().edit_generation(), generation);
            assert_eq!(ctx.input(|input| input.events.clone()), events);
            assert_eq!(ctx.global_style(), style);
        })
        .drop_without_applying_deltas();
        assert_eq!(i18n::current(), host_language);
        assert_eq!(app.theme, ThemeKind::Dark);
        assert!(!app.styled && !app.fonts_ready);
        assert!(app.take_host_commands().is_empty());
    }

    #[test]
    fn hosted_background_tracks_deferred_automatic_ocr_and_cancellation_settles_it() {
        let mut app = background_app();
        app.ocr_run = Some(finished_ocr(&app, true));
        let ctx = egui::Context::default();
        app.hosted_poll_background(&ctx);
        assert!(app.hosted_has_pending_background(), "Read mode keeps the finished result");
        app.cancel_ocr();
        app.hosted_poll_background(&ctx);
        assert!(!app.hosted_has_pending_background(), "explicit cancellation retains read pages and settles the run");
    }

    #[test]
    fn hosted_background_close_settles_invisible_automatic_ocr_in_read_mode() {
        let mut app = background_app();
        app.ocr_run = Some(finished_ocr(&app, true));
        app.mode = Mode::Read;
        app.hosted_poll_background(&egui::Context::default());
        assert!(app.hosted_has_pending_background());
        app.hosted_prepare_close();
        assert!(!app.hosted_has_pending_background());
        let (_, doc) = app.active_ids().unwrap();
        assert!(app.session.get(doc).unwrap().can_undo().is_some(), "recognized pages are retained for save/discard");
    }

    #[test]
    fn hosted_background_drains_finished_export_batch_and_action() {
        let mut app = background_app();
        let ctx = egui::Context::default();
        let export = Arc::new(Mutex::new(Some((0, 2, None))));
        let batch = Arc::new(Mutex::new(ocr_ui::BatchProgress { total: 2, ..Default::default() }));
        let action = Arc::new(Mutex::new(actions_ui::RunProgress { total: 2, ..Default::default() }));
        app.export_status = Some(export.clone());
        app.ocr_batch = Some(batch.clone());
        app.action_run = Some(action.clone());
        app.hosted_poll_background(&ctx);
        assert!(app.hosted_has_pending_background());
        *export.lock().unwrap() = Some((2, 2, Some("export done".into())));
        batch.lock().unwrap().message = Some("OCR done".into());
        action.lock().unwrap().message = Some("action done".into());
        app.hosted_poll_background(&ctx);
        assert!(!app.hosted_has_pending_background());
        let toast = app.toast.take().unwrap().0;
        app.hosted_poll_background(&ctx);
        assert!(app.toast.is_none(), "completed worker messages are not consumed twice: {toast}");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn hosted_background_keeps_optimized_bytes_until_host_save_is_answered() {
        let mut app = background_app();
        let (_, doc) = app.active_ids().unwrap();
        let bytes = app.session.get(doc).unwrap().bytes.clone();
        app.optimize_run = Some(optimize_ui::OptimizeRun {
            doc,
            kind: optimize_ui::OptimizeKind::Advanced,
            progress: Arc::new(Mutex::new(optimize_ui::OptimizeProgress { result: Some(Ok((bytes, Default::default()))), ..Default::default() })),
        });
        app.hosted_poll_background(&egui::Context::default());
        assert!(app.optimize_run.is_none());
        assert!(app.hosted_has_pending_background(), "the save continuation still owns the optimized bytes");
        let requests = app.take_host_file_requests();
        assert_eq!(requests.len(), 1);
        assert!(app.hosted_has_pending_background(), "draining a request is not acknowledging it");
        app.hosted_file_result(requests[0].id, None).unwrap();
        assert!(!app.hosted_has_pending_background(), "cancel releases the bytes without needing active logic");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn hosted_background_writes_optimized_copy_once_through_host_policy() {
        let mut app = background_app();
        let (_, doc) = app.active_ids().unwrap();
        let bytes = app.session.get(doc).unwrap().bytes.clone();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let recorded = writes.clone();
        app.set_host_export_writer(Arc::new(move |path, bytes| {
            recorded.lock().unwrap().push((path.to_owned(), bytes.to_vec()));
            Ok(())
        }));
        app.save_override = Some("optimized.pdf".into());
        app.optimize_run = Some(optimize_ui::OptimizeRun {
            doc,
            kind: optimize_ui::OptimizeKind::Advanced,
            progress: Arc::new(Mutex::new(optimize_ui::OptimizeProgress {
                result: Some(Ok((bytes.clone(), Default::default()))),
                ..Default::default()
            })),
        });
        let ctx = egui::Context::default();
        app.hosted_poll_background(&ctx);
        app.hosted_poll_background(&ctx);
        assert!(!app.hosted_has_pending_background());
        assert_eq!(writes.lock().unwrap().as_slice(), &[(std::path::PathBuf::from("optimized.pdf"), (*bytes).clone())]);
    }

    #[test]
    fn hosted_background_keeps_generated_documents_until_acknowledged() {
        let mut app = background_app();
        let bytes = app.session.create_blank(300.0, 400.0, 1).unwrap();
        app.queue_host_document("generated.pdf", (*bytes).clone(), None, None).unwrap();
        let generated = app.take_host_documents();
        assert!(app.hosted_has_pending_background());
        app.hosted_document_result(generated[0].id, Ok(())).unwrap();
        assert!(!app.hosted_has_pending_background());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn hosted_background_releases_export_after_worker_failure() {
        let mut app = background_app();
        let export = Arc::new(Mutex::new(Some((0, 1, None))));
        let worker = export.clone();
        let _ = std::thread::spawn(move || {
            let _held = worker.lock().unwrap();
            panic!("export worker stopped");
        })
        .join();
        app.export_status = Some(export);
        app.hosted_poll_background(&egui::Context::default());
        assert!(!app.hosted_has_pending_background());
        assert!(app.toast.as_ref().unwrap().0.starts_with("Export stopped:"));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn hosted_background_export_writer_panic_finishes_instead_of_staying_busy() {
        let mut app = background_app();
        app.set_host_export_writer(Arc::new(|_, _| panic!("export writer stopped")));
        app.export_dir_override = Some(std::env::temp_dir().to_string_lossy().into_owned());
        app.start_export(export_ui::ExportKind::Text);
        assert!(app.hosted_has_pending_background());
        app.hosted_poll_background(&egui::Context::default());
        assert!(!app.hosted_has_pending_background());
        assert!(app.toast.as_ref().unwrap().0.contains("export writer stopped"));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn hosted_background_action_writer_panic_finishes_instead_of_staying_busy() {
        let mut app = background_app();
        let (_, doc) = app.active_ids().unwrap();
        let bytes = (*app.session.get(doc).unwrap().bytes).clone();
        app.set_host_export_writer(Arc::new(|_, _| panic!("action writer stopped")));
        app.export_dir_override = Some(std::env::temp_dir().to_string_lossy().into_owned());
        app.run_inline = true;
        app.run_action_on(
            pdfcraft_engine::actions::Action { name: "copy".into(), description: String::new(), steps: Vec::new(), builtin: false },
            vec![(format!("pdfcraft-action-{}-{}.pdf", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()), bytes)],
        );
        assert!(!app.hosted_has_pending_background());
        assert!(app.toast.as_ref().unwrap().0.contains("action writer stopped"));
    }

    #[test]
    fn hosted_commands_leave_document_and_window_ownership_to_host() {
        let mut app = PdfCraftApp::new();
        app.enable_hosted();
        app.hosted_new().unwrap();
        assert!(app.hosted_dirty());
        assert!(app.execute("file.close"));
        assert!(app.execute("file.save"));
        assert!(app.execute("view.full_screen"));
        assert_eq!(app.views.len(), 1);
        assert!(!app.full_screen);
        let commands = app.take_host_commands();
        assert_eq!(commands.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["file.close", "file.save", "view.full_screen"]);
        assert!(app.take_host_commands().is_empty());
    }

    #[test]
    fn hosted_save_commits_queued_page_edit_and_round_trips() {
        let mut app = PdfCraftApp::new();
        app.enable_hosted();
        app.hosted_new().unwrap();
        app.views[0].pending_edit = Some(pdfcraft_engine::Edit::InsertBlankPage { at: 1, width: 300.0, height: 400.0 });
        let path =
            std::env::temp_dir().join(format!("weave-hosted-pdf-{}-{}.pdf", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
        app.hosted_save(&path).unwrap();
        assert!(!app.hosted_dirty());
        let mut reopened = PdfCraftApp::new();
        reopened.enable_hosted();
        reopened.hosted_open(&path).unwrap();
        let (_, id) = reopened.active_ids().unwrap();
        assert_eq!(reopened.session.get(id).unwrap().info.pages.len(), 2);
        assert!(!reopened.hosted_dirty());
        std::fs::remove_file(path).unwrap();
    }
}

/// Compact navigation shares the hosted Office ribbon's text-and-underline treatment.
/// The hosted editor's tab row: (id, label). Comments and Pages open a tool rather than a mode.
const HOSTED_TABS: [(&str, &str); 4] = [("read", "Read"), ("edit", "Edit"), ("comments", "Add comments"), ("pages", "Organize pages")];

impl PdfCraftApp {
    fn hosted_tab_open(&self, id: &str) -> bool {
        match id {
            "read" => self.mode == Mode::Read && !self.left_open,
            "edit" => self.mode == Mode::Edit,
            _ => false,
        }
    }

    fn open_hosted_tab(&mut self, id: &str) {
        match id {
            "read" => {
                self.select_mode(Mode::Read);
                self.left_open = false;
            }
            "edit" => self.select_mode(Mode::Edit),
            "comments" => {
                self.enter_mode(Mode::AllTools);
                self.select_tool("comment");
                self.choose_right_panel(Some(RightPanel::Comments));
            }
            "pages" => {
                self.enter_mode(Mode::AllTools);
                self.select_tool("organize");
            }
            _ => {}
        }
    }
}

#[cfg(feature = "host-ribbon")]
impl weave_ribbon::HostedRibbon for PdfCraftApp {
    fn ribbon_tabs(&self, _: &egui::Context) -> weave_ribbon::Tabs {
        // Combining files is a page of its own, without the switcher.
        if self.combine_showing() {
            return weave_ribbon::Tabs::default();
        }
        weave_ribbon::Tabs {
            tabs: HOSTED_TABS.iter().map(|(id, label)| weave_ribbon::Tab::new(*id, tl!(label))).collect(),
            active: HOSTED_TABS.iter().find(|(id, _)| self.hosted_tab_open(id)).map(|(id, _)| (*id).to_owned()),
        }
    }

    fn ribbon_event(&mut self, event: weave_ribbon::Event) {
        let (weave_ribbon::Event::Clicked(id) | weave_ribbon::Event::DoubleClicked(id)) = event;
        if !self.combine_showing() {
            self.open_hosted_tab(&id);
        }
    }

    fn undo_redo(&self) -> (bool, bool) {
        self.active
            .and_then(|i| self.views.get(i))
            .and_then(|view| self.session.get(view.id))
            .map(|doc| (doc.can_undo().is_some(), doc.can_redo().is_some()))
            .unwrap_or((false, false))
    }

    fn ribbon_tab_rects(&mut self, _: Vec<(String, egui::Rect)>) {
        self.host_tabs = true;
    }
}

fn hosted_mode_tab(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let font = egui::FontId::proportional(13.0);
    let width = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), ui.visuals().text_color()).size().x;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width + 22.0, 28.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), active, label));
    if response.hovered() {
        ui.painter().rect_filled(rect.shrink2(egui::vec2(1.0, 3.0)), 3, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let accent = if ui.visuals().dark_mode { egui::Color32::from_rgb(238, 155, 156) } else { egui::Color32::from_rgb(191, 65, 69) };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, font, if active { accent } else { ui.visuals().text_color() });
    if active {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(rect.left_bottom() + egui::vec2(10.0, -2.0), rect.right_bottom() - egui::vec2(10.0, 0.0)),
            1,
            accent,
        );
    }
    response
}
