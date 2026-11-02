//! Editing surface for a shell that owns tabs, files, settings and the native window.

use std::path::Path;

use super::{
    WordApp, canvas, chrome, colors, content_controls, dialogs, i18n, json, keys, keytips, now_ms, options, panes, read_aloud, ribbon, theme, zotero,
};
use serde_json::Value;

/// One COW snapshot when opened/saved, not one snapshot per frame or keystroke.
pub(crate) struct HostedSavePoint {
    document_id: u64,
    document: wordcraft_doc::Document,
    password: Option<wordcraft_engine::Password>,
    /// Font/layout repaints and host title queries reuse the last content comparison.
    same_at_revision: std::cell::Cell<Option<(u64, bool)>>,
}

impl HostedSavePoint {
    fn new(session: &wordcraft_engine::Session) -> Self {
        Self {
            document_id: session.document_id(),
            document: session.doc.clone(),
            password: session.password.clone(),
            same_at_revision: std::cell::Cell::new(Some((session.rev(), true))),
        }
    }

    fn matches(&self, session: &wordcraft_engine::Session) -> bool {
        if self.document_id != session.document_id() {
            return false;
        }
        let same_document = match self.same_at_revision.get() {
            Some((revision, same)) if revision == session.rev() => same,
            _ => {
                let same = self.document.same_content_without_save_stamp(&session.doc);
                self.same_at_revision.set(Some((session.rev(), same)));
                same
            }
        };
        // Encryption is persisted outside Document, and file.encrypt need not bump rev().
        same_document && self.password == session.password
    }
}

impl WordApp {
    /// Embed this editor in an existing application. Call before its first frame.
    ///
    /// Keep one instance per host document. When sharing an egui Context, the host must preserve
    /// each editor's egui Memory and installed fonts between activations: some upstream widgets
    /// use globally named IDs. Only the active editor receives input and draws.
    pub fn enable_hosted(&mut self) {
        if !self.hosted && !self.session.dirty {
            self.hosted_save_point = Some(HostedSavePoint::new(&self.session));
        }
        let previous_base = crate::canvas::screen_px_per_pt(self);
        self.hosted = true;
        self.canvas.scale *= crate::canvas::screen_px_per_pt(self) / previous_base;
        self.windows.single();
        self.autosave = false;
        self.quit_requested = false;
        self.ui.backstage = false;
        self.ui.start_screen = false;
        self.ui.quick_access_toolbar = false;
        self.no_document = None;
        self.recovery_seen = true;
        self.session.view.full_screen = false;
        self.session.view.focus_mode = false;
        self.session.view.full_screen_view = false;
    }

    /// Whether an editor-owned dialog must finish before an external host navigation command.
    /// Navigation/search and other task panes are not dialogs and remain non-blocking.
    /// This query only gates host chrome/shortcuts: keep editor logic/UI and dialog-generated
    /// host requests running, and route application quit through the host's save transaction.
    pub fn hosted_blocks_host_navigation(&self) -> bool {
        self.dialog.is_some() || self.colors.is_some() || self.file_dialog_open() || self.zotero.alert_open()
    }

    /// The same read-only proofing status displayed in Options › Language.
    pub fn hosted_proofing_status(&self) -> String {
        let previous = i18n::current();
        i18n::set_current(i18n::Lang::from_pref(&self.ui.language));
        let status = options::proofing_status(self);
        i18n::set_current(previous);
        status
    }

    /// Editing background work only. Does not own the viewport, theme, global zoom, shortcuts,
    /// updates, recovery selection, document drops or automatic saving.
    ///
    /// Fonts are prepared for this editor, and the host preserves them with its egui Memory.
    pub fn hosted_logic(&mut self, ctx: &egui::Context) {
        if !self.hosted {
            self.enable_hosted();
        }
        self.ctx = Some(ctx.clone());
        let lang = i18n::Lang::from_pref(&self.ui.language);
        i18n::set_current(lang);
        // A hosted interface language is application chrome, not the document's proofing
        // default. Set Proofing Language and Set As Default retain their document semantics.
        options::sync(self);
        self.ui.theme = if ctx.global_style().visuals.dark_mode { theme::Appearance::Dark } else { theme::Appearance::Light };
        self.native_scale = theme::usable_scale(ctx.native_pixels_per_point().unwrap_or(1.0));
        self.read_system_animations();
        self.settle_async_saves();
        zotero::poll(self, ctx);
        read_aloud::poll(self, ctx);
        self.clear_stale_change_picture();
        // Picture, template-attachment and recipient pickers remain document tools. Lifecycle
        // pickers are intercepted before they can be started.
        let _ = self.poll_file_dialog();
        self.drain_pasted();
        self.watch_fonts(ctx);
        self.ensure_caption_fonts();
        self.ensure_comment_fonts();
        self.ensure_find_fonts();
        self.install_ui_fonts(ctx, lang);
        self.report_dirty();
    }

    fn hosted_fonts_ready(&self, ctx: &egui::Context) -> bool {
        ctx.fonts(|fonts| {
            ["medium", "semibold"]
                .iter()
                .all(|name| fonts.definitions().families.get(&egui::FontFamily::Name((*name).into())).is_some_and(|chain| !chain.is_empty()))
        })
    }

    /// Render only the professional editing surface inside the host-provided Ui.
    pub fn hosted_ui(&mut self, ui: &mut egui::Ui) {
        if !self.hosted {
            self.enable_hosted();
        }
        let started = now_ms();
        let ctx = ui.ctx().clone();
        let editor_rect = ui.available_rect_before_wrap();
        i18n::set_current(i18n::Lang::from_pref(&self.ui.language));
        self.ui.backstage = false;
        self.handle_ui_input_events(&ctx);
        // A fixed two-frame wait used to submit an empty editing surface even though
        // hosted_logic had already installed the fonts before the shell was drawn.
        if !self.hosted_fonts_ready(&ctx) {
            // A restored/unprepared embedding may call UI after laying out shell text.
            // Queue the real font set and retry; the final allowed pass still paints a canvas.
            self.styled = false;
            self.install_ui_fonts(&ctx, i18n::Lang::from_pref(&self.ui.language));
            if !self.hosted_fonts_ready(&ctx) {
                ctx.request_discard("prepare hosted document fonts");
                ctx.request_repaint();
                if !ctx.will_discard() {
                    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::Tokens::get(&ctx).canvas)).show(ui, |_| {});
                }
                return;
            }
        }
        self.fonts_frames = 2;
        self.escape_free = self.dialog.is_none() && self.ui.keytips == keytips::Phase::Off && !egui::Popup::is_any_open(&ctx);
        keytips::logic(self, &ctx);
        ribbon::show(self, ui);
        chrome::status_bar(self, ui);
        panes::show(self, ui);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::Tokens::get(&ctx).canvas)).show(ui, |ui| {
            canvas::show(self, ui);
        });
        content_controls::popup(self, &ctx);
        dialogs::show(self, &ctx);
        #[cfg(not(target_arch = "wasm32"))]
        crate::file_browser::show(self, &ctx);
        colors::show(self, &ctx);
        zotero::show_alert(self, &ctx);
        read_aloud::show(self, &ctx);
        keytips::show(self, &ctx, ui);
        keys::global_shortcuts(self, &ctx);
        // The shell's tab strip and menu are outside this rect; clicking them must not make
        // the editor reclaim keyboard focus.
        if ctx.input(|i| i.pointer.interact_pos().is_some_and(|p| editor_rect.contains(p))) {
            self.refocus_after_chrome_click(&ctx);
        }
        if let Some(url) = self.canvas.open_url.take() {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        self.frame_ms = now_ms() - started;
        self.report_dirty();
    }

    /// Commands requested by editor controls that the surrounding application must handle.
    pub fn take_host_commands(&mut self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.host_commands)
    }

    /// Commit comments/replies still being edited before saving or switching tabs.
    /// A refused edit remains a draft and the error is returned to the shell.
    pub fn hosted_flush(&mut self) -> Result<(), String> {
        // A host picker may have answered immediately before a tab switch or
        // close. Include that accepted insertion in the save/dirty decision.
        if let Some(result) = self.poll_file_dialog() {
            result?;
        }
        panes::flush_comment_drafts(self)
    }

    /// Dispatch an editor command (for example the host's Undo/Redo menu), preserving the
    /// normal editing side effects and forwarding application commands to the shell.
    pub fn hosted_command(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if !self.hosted {
            self.enable_hosted();
        }
        self.run(id, params)
    }

    /// Leave this tab while its egui Memory is still installed. Commit comment drafts first;
    /// if that fails, keep the tab active. Completed edits, selection, scroll, dialogs and
    /// render textures remain intact; only in-progress pointer/IME gestures are cancelled.
    pub fn hosted_deactivate(&mut self, ctx: &egui::Context) -> Result<(), String> {
        self.hosted_flush()?;
        self.canvas.cancel_text_drag();
        self.canvas.obj_drag = None;
        self.canvas.crop_drag = None;
        self.canvas.ink = None;
        self.canvas.erasing = None;
        self.canvas.ink_ui.lasso = None;
        self.canvas.table_stroke = None;
        self.canvas.table_resize = None;
        self.canvas.table_drag = None;
        self.canvas.tab_drag = None;
        self.canvas.auto_scroll.cancel();
        self.canvas.wheel = Default::default();
        self.canvas.ime_preedit.clear();
        self.canvas.ime_preedit_range = None;
        self.canvas.ime_join_text = None;
        self.canvas.ime_window_cache = None;
        self.canvas.ime_interrupted_preedit = false;
        self.canvas.ime_interrupt_now = false;
        if let Some(viewport) = self.canvas.ime_viewport.take() {
            self.canvas.ime_tracker.end_viewport(viewport);
        }
        self.canvas.alt_code.clear();
        self.canvas.alt_code_echo = None;
        self.canvas.paste_mods = None;
        self.canvas.v_pressed = false;
        self.canvas.swallow_text = false;
        self.canvas.context_menu_open = false;
        self.canvas.context_menu_from_keys = false;
        self.canvas.focused = false;
        self.canvas.want_focus = self.dialog.is_none() && self.colors.is_none();
        self.ui.keytips = keytips::Phase::Off;
        self.ui.keytip_prefix.clear();
        self.ui.alt_pressed = false;
        self.ui.alt_chord_used = false;
        self.ui.alt_key_down = false;
        self.ui.alt_modifier_seen = false;
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

    /// Includes comment drafts and any unconfirmed asynchronous save.
    pub fn hosted_dirty(&self) -> bool {
        self.hosted_save_point.as_ref().map_or(self.session.dirty, |saved| !saved.matches(&self.session))
            || panes::has_comment_drafts(self)
            || self.save_unconfirmed()
    }

    /// The host's trusted, synchronous save to a chosen local path. Unlike `run("file.save")`,
    /// this writes the file instead of forwarding the command back to the host.
    pub fn hosted_save(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        self.hosted_flush()?;
        if self.services.download.is_some() {
            return Err("hosted_save requires local file services, not a download service".into());
        }
        let path = path.as_ref().to_str().ok_or_else(|| "the save path is not valid UTF-8".to_string())?;
        if path.is_empty() {
            return Err("the save path is empty".into());
        }
        let previous = std::mem::replace(&mut self.hosted_saving, true);
        let result = self.run("file.saveAs", json!({"path": path}));
        self.hosted_saving = previous;
        let result = result?;
        if result.get("saved").and_then(Value::as_bool) != Some(true) {
            return Err("the document was not saved".into());
        }
        if self.session.dirty || panes::has_comment_drafts(self) || self.save_unconfirmed() {
            return Err("this format does not keep all changes; save as a Word document (.docx)".into());
        }
        // Never replace the previous save point before both the write and confirmation succeed.
        self.hosted_save_point = Some(HostedSavePoint::new(&self.session));
        Ok(())
    }

    pub(super) fn forward_host_command(&mut self, id: &str, params: &Value) -> bool {
        if !self.hosted || (self.hosted_saving && matches!(id, "file.save" | "file.saveAs")) {
            return false;
        }
        if !host_command(id, params) {
            return false;
        }
        self.host_commands.push((id.to_string(), params.clone()));
        true
    }

    // Engine UI requests can also arrive from composed commands/macros. Do not let those
    // bypass the boundary and create an independent file/settings/window surface.
    pub(super) fn forward_host_request(&mut self, req: &Value) -> bool {
        if !self.hosted {
            return false;
        }
        let command = match req.get("open").and_then(Value::as_str) {
            Some("openFile") => Some("file.open"),
            Some("saveAs") => Some("file.saveAs"),
            Some("exportPdf") => Some("file.exportPdf"),
            Some("exportPng") => Some("file.exportPng"),
            Some("print") => Some("file.print"),
            Some("options") => Some("file.options"),
            Some("newWindow") => Some("view.newWindow"),
            _ if req.get("close").is_some() => Some("file.quit"),
            _ if req.get("interfaceTheme").is_some() || req.get("toggle").and_then(Value::as_str) == Some("interfaceTheme") => {
                Some("view.interfaceTheme")
            }
            _ => None,
        };
        if let Some(command) = command {
            self.host_commands.push((command.into(), req.clone()));
            true
        } else {
            false
        }
    }
}

fn host_command(id: &str, params: &Value) -> bool {
    matches!(
        id,
        "file.new"
            | "file.newFromTemplate"
            | "file.open"
            | "file.save"
            | "file.saveAs"
            | "file.saveTemplate"
            | "file.rename"
            | "file.close"
            | "file.closeDocument"
            | "file.quit"
            | "file.exit"
            | "file.recover"
            | "file.autosave"
            | "file.options"
            | "file.share"
            | "file.exportPdf"
            | "file.exportPng"
            | "file.print"
            | "file.printPdf"
            | "ui.openFileDialog"
            | "ui.saveAsTemplate"
            | "ui.backstage"
            | "ui.quit"
            | "ui.exit"
            | "ui.theme"
            | "ui.dark"
            | "ui.darkMode"
            | "ui.quickAccessToolbar"
            | "ui.interfaceSize"
            | "ui.animations"
            | "ui.language"
            | "ui.startScreen"
            | "ui.makeDefaultApp"
            | "ui.checkForUpdates"
            | "help.checkForUpdates"
            | "ui.print"
            | "ui.printSubmit"
            | "ui.printSettings"
            | "view.interfaceTheme"
            | "view.newWindow"
            | "view.switchWindows"
            | "view.arrangeAll"
            | "view.fullScreen"
            | "view.fullScreenView"
            | "view.focus"
    ) || (id == "ui.tab" && params.get("tab").and_then(Value::as_str) == Some("File"))
        || (id == "ui.fileBrowser" && matches!(params.get("purpose").and_then(Value::as_str), None | Some("document" | "save")))
        || (id == "ui.dialog"
            && matches!(
                params.get("name").and_then(Value::as_str),
                Some("options" | "customizeKeyboard" | "customizeRibbon" | "customizeQuickAccess")
            ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use wordcraft_engine::Session;

    #[test]
    fn hosted_lifecycle_commands_forward_without_mutating_document() {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::from_text("keep me")), Services::default());
        app.enable_hosted();
        app.session.dirty = true;
        let document = app.session.document_id();
        for id in ["file.new", "file.open", "file.save", "file.close", "file.options", "view.newWindow", "view.fullScreen"] {
            app.run(id, json!({})).unwrap();
            app.execute(id, json!({})).unwrap();
        }
        assert_eq!(app.take_host_commands().len(), 14);
        assert!(app.take_host_commands().is_empty());
        assert_eq!(app.session.document_id(), document);
        assert!(app.session.dirty);
        assert!(app.dialog.is_none());
        assert!(!app.quit_requested);
        assert!(!app.ui.backstage);
        assert!(!app.windows.native());
        app.run("text.insert", json!({"text": "edited"})).unwrap();
        assert!(app.take_host_commands().is_empty());
    }

    #[test]
    fn hosted_settings_and_engine_requests_cannot_open_app_shell() {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        app.enable_hosted();
        app.run("ui.tab", json!({"tab": "File"})).unwrap();
        app.run("ui.backstage", json!({"page": "options"})).unwrap();
        app.handle_request(&json!({"open": "newWindow"}));
        app.handle_request(&json!({"close": true}));
        assert_eq!(app.take_host_commands().len(), 4);
        assert!(!app.ui.backstage);
        assert!(!app.quit_requested);
        assert!(!host_command("file.accessibility", &json!({})));
        assert!(!host_command("ui.fileBrowser", &json!({"purpose": "picture"})));
    }

    #[test]
    fn hosted_trusted_save_writes_without_forwarding_and_retains_unsaved_on_failure() {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        app.enable_hosted();
        app.hosted_command("text.insert", json!({"text": "saved through the host"})).unwrap();
        assert!(app.hosted_dirty());
        assert!(app.hosted_save("").is_err());
        assert!(app.hosted_dirty());
        let path = std::env::temp_dir().join(format!("weave-word-hosted-{}-{}.docx", std::process::id(), app.session.document_id()));
        app.hosted_save(&path).unwrap();
        assert!(!app.hosted_dirty());
        assert!(app.take_host_commands().is_empty());
        assert_eq!(app.session.path.as_deref(), Some(path.as_path()));
        let mut reopened = Session::new(wordcraft_doc::Document::new());
        reopened.run("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
        assert_eq!(reopened.doc.plain_text(wordcraft_doc::StoryRef::Body), "saved through the host");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_deactivation_preserves_document_and_scroll_but_releases_input() {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::from_text("keep me")), Services::default());
        app.enable_hosted();
        let ctx = egui::Context::default();
        let id = egui::Id::unique("hosted_test_focus");
        ctx.memory_mut(|m| m.request_focus(id));
        app.canvas.focused = true;
        app.canvas.ime_preedit = "unfinished".into();
        app.canvas.scroll_offset = egui::vec2(0.0, 120.0);
        let selection = app.session.sel.clone();
        app.hosted_deactivate(&ctx).unwrap();
        assert!(!app.canvas.focused);
        assert!(app.canvas.ime_preedit.is_empty());
        assert_eq!(ctx.memory(|m| m.focused()), None);
        assert_eq!(app.canvas.scroll_offset.y, 120.0);
        assert_eq!(app.session.sel, selection);
        assert_eq!(app.session.doc.plain_text(wordcraft_doc::StoryRef::Body), "keep me");
    }

    #[test]
    fn hosted_undo_redo_returns_to_saved_contents_through_the_ui_dispatch() {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        app.enable_hosted();
        let path = std::env::temp_dir().join(format!("weave-word-save-point-{}-{}.docx", std::process::id(), app.session.document_id()));
        app.hosted_save(&path).unwrap();
        // Keyboard, ribbon and context menus all call run, not hosted_command.
        app.run("text.insert", json!({"text": "first"})).unwrap();
        assert!(app.hosted_dirty());
        app.run("edit.undo", json!({})).unwrap();
        assert!(!app.hosted_dirty(), "Undo restored the initially saved blank document");
        app.run("edit.redo", json!({})).unwrap();
        assert!(app.hosted_dirty());
        app.hosted_save(&path).unwrap();
        app.run("edit.undo", json!({})).unwrap();
        assert!(app.hosted_dirty(), "the old blank is no longer the save point");
        app.run("edit.redo", json!({})).unwrap();
        assert!(!app.hosted_dirty(), "Redo restored the new save point");
        app.run("file.properties", json!({"title": "A real user property"})).unwrap();
        assert!(app.hosted_dirty(), "user metadata is not a save timestamp");
        app.run("edit.undo", json!({})).unwrap();
        assert!(!app.hosted_dirty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_failed_save_preserves_the_previous_content_save_point() {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        app.enable_hosted();
        let path = std::env::temp_dir().join(format!("weave-word-failed-save-{}-{}.docx", std::process::id(), app.session.document_id()));
        app.run("text.insert", json!({"text": "saved"})).unwrap();
        app.hosted_save(&path).unwrap();
        app.run("text.insert", json!({"text": " new work"})).unwrap();
        // A file cannot be used as a directory, so this fails without relying on permissions.
        assert!(app.hosted_save(path.join("cannot-write.docx")).is_err());
        assert!(app.hosted_dirty());
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.doc.plain_text(wordcraft_doc::StoryRef::Body), "saved");
        assert!(!app.hosted_dirty(), "failed save must not replace the successful save point");
        app.run("edit.redo", json!({})).unwrap();
        assert_eq!(app.session.doc.plain_text(wordcraft_doc::StoryRef::Body), "saved new work");
        assert!(app.hosted_dirty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_save_point_ignores_only_save_stamps_and_tracks_password_without_revision() {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        app.enable_hosted();
        app.session.stamp_save();
        app.session.touch();
        assert!(!app.hosted_dirty(), "save-created timestamps/revision/zoom are not content edits");
        let revision = app.session.rev();
        app.run("file.encrypt", json!({"password": "test-save-point-password"})).unwrap();
        assert_eq!(app.session.rev(), revision);
        assert!(app.hosted_dirty(), "encryption changes must not hide behind the revision cache");
        app.run("file.encrypt", json!({"password": null})).unwrap();
        assert!(!app.hosted_dirty());
        app.run("file.properties", json!({"author": "Changed Author"})).unwrap();
        assert!(app.hosted_dirty());
    }
}
