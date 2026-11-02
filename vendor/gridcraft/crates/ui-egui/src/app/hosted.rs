//! Host-owned file and window lifecycle for an embedded workbook editor.
use super::{Json, SheetApp, json};

impl SheetApp {
    /// Use the editor inside a host which owns files, preferences, and the native window.
    pub fn enable_hosted(&mut self) {
        self.hosted = true;
        self.chrome = crate::Chrome { title_bar: false, file_tab: false };
        self.backstage = None;
        self.native_windows = false;
        crate::fonts::start_scan();
    }

    /// Whether an editor-owned dialog must finish before an external host navigation command.
    /// This follows the grid's dialog/message/macro-question input gate; ordinary task panes
    /// and in-cell editing do not block host navigation. Keep editor logic/UI and confirmed
    /// dialog requests running, including reference picking, and let the host handle quit.
    pub fn hosted_blocks_host_navigation(&self) -> bool {
        self.dialog.is_some() || self.message.is_some() || self.macros.question.is_some()
    }

    pub fn hosted_logic(&mut self, ctx: &egui::Context) {
        if !self.hosted {
            self.enable_hosted();
        }
        self.logic(ctx);
    }
    pub fn hosted_ui(&mut self, ui: &mut egui::Ui) {
        if !self.hosted {
            self.enable_hosted();
        }
        // The host must prepare this editor in hosted_logic before painting its chrome.
        // Re-check restored/pending definitions at the actual pass boundary.
        self.sync_hosted_font_state(ui.ctx());
        if !self.fonts_ready {
            // Also support an embedding that restored an unprepared context. Setup before
            // text layout is immediate; otherwise retry before submitting this UI pass.
            self.fonts_set = false;
            self.apply_font_state(ui.ctx());
            self.sync_hosted_font_state(ui.ctx());
            if !self.fonts_ready {
                let ctx = ui.ctx().clone();
                ctx.request_discard("prepare hosted spreadsheet fonts");
                ctx.request_repaint();
                if !ctx.will_discard() {
                    // A host can exhaust its pass budget before invoking the editor. Never
                    // expose only its shell or address a family absent from the current atlas.
                    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(super::theme::Tokens::get(&ctx).grid_bg)).show(ui, |_| {});
                }
                return;
            }
        }
        self.ui(ui);
    }

    /// Unified host toolbar commands retain the editor's draft and clipboard semantics.
    pub fn hosted_command(&mut self, id: &str, mut params: Json) -> Result<Json, String> {
        if !self.hosted {
            self.enable_hosted();
        }
        if matches!(id, "edit.copy" | "edit.cut") && self.services.copy_cells.is_some() {
            // The host toolbar has no egui Context. Its structured sink owns the system write.
            if let Some(object) = params.as_object_mut() {
                object.insert("html".into(), json!(true));
            }
            let result = self.run_command(id, params)?;
            if let (Some(copy), Some(clip), Some(text)) =
                (&mut self.services.copy_cells, self.session.clipboard.as_ref(), result.get("text").and_then(Json::as_str))
            {
                copy(clip, result.get("html").and_then(Json::as_str).unwrap_or(""), text)?;
            }
            return Ok(result);
        }
        if matches!(id, "edit.paste" | "edit.pasteSpecial" | "home.insertCopiedCells") {
            return self.run_user_paste(id, params);
        }
        self.run(id, params)
    }

    /// Include in-place drafts in save/close decisions before releasing the editor.
    pub fn hosted_flush(&mut self) -> Result<(), String> {
        self.settle_drafts()
    }

    pub fn hosted_dirty(&self) -> bool {
        self.editor.is_some() || self.text_box_editor.is_some() || self.session.documents().iter().any(|d| d.is_dirty())
    }

    /// Whether an in-place cell or text-box edit still needs to be committed.
    pub fn hosted_has_edit_draft(&self) -> bool {
        self.editor.is_some() || self.text_box_editor.is_some() || self.parked.values().any(|w| w.has_edit_draft())
    }

    /// Refresh the UI after a host transaction changed the engine directly.
    pub fn hosted_sync_external_edit(&mut self) {
        self.follow_active_window();
        self.after_engine();
        self.clipboard_text_at_copy = None;
        self.cut_text = None;
    }

    pub fn hosted_save(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.hosted_flush()?;
        let path = path.to_str().ok_or("The file path is not valid Unicode")?;
        self.host_dispatch = true;
        let result = self.run("file.saveAs", json!({"path": path}));
        self.host_dispatch = false;
        result.map(|_| ())
    }

    pub fn hosted_deactivate(&mut self, ctx: &egui::Context) -> Result<(), String> {
        // Switching host tabs is not Enter or Save. Keep even a rejected cell
        // draft so the user can return to correct it, or discard it when closing.
        if let Some(editor) = &mut self.editor {
            editor.request_focus = true;
            editor.keep_selection = true;
        }
        ctx.memory_mut(|m| {
            m.interrupt_ime();
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        ctx.stop_dragging();
        egui::Popup::close_all(ctx);
        Ok(())
    }

    pub fn take_host_commands(&mut self) -> Vec<(String, Json)> {
        std::mem::take(&mut self.host_commands)
    }

    pub(crate) fn queue_host_command(&mut self, id: &str, params: &Json) -> bool {
        let owned = matches!(
            id,
            "file.new"
                | "file.open"
                | "file.save"
                | "file.saveAs"
                | "file.exportPdf"
                | "file.exportCsv"
                | "file.exportHtml"
                | "file.close"
                | "file.options"
                | "file.print"
                | "app.about"
                | "app.quit"
                | "app.exit"
                | "app.checkForUpdates"
                | "help.checkForUpdates"
                | "ui.backstage"
                | "view.darkMode"
                | "view.theme"
                | "view.interfaceScale"
                | "view.newWindow"
                | "view.hideWindow"
                | "view.unhideWindow"
                | "view.switchWindows"
                | "view.arrangeAll"
        ) || id.starts_with("window.")
            || id.starts_with("app.language.");
        if self.hosted && !self.host_dispatch && owned {
            self.host_commands.push((id.to_owned(), params.clone()));
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosted_lifecycle_commands_leave_the_document_in_place() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, crate::Services::default());
        app.enable_hosted();
        let uid = app.session.active().map(|d| d.uid);
        for id in ["file.new", "file.open", "file.save", "file.saveAs", "file.close", "view.newWindow"] {
            assert!(app.hosted_command(id, json!({})).is_ok());
        }
        assert_eq!(app.session.active().map(|d| d.uid), uid);
        assert_eq!(app.session.documents().len(), 1);
        assert_eq!(app.take_host_commands().len(), 6);
        assert!(app.take_host_commands().is_empty());
        assert!(app.hosted_command("cell.set", json!({"cell": "A1", "input": "hosted"})).is_ok());
        assert!(app.hosted_dirty());
        assert!(app.hosted_command("edit.undo", json!({})).is_ok());
        assert!(app.take_host_commands().is_empty());
    }

    #[test]
    fn hosted_exports_with_paths_are_queued_without_writing() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, crate::Services::default());
        app.enable_hosted();
        for (id, extension) in [("file.exportPdf", "pdf"), ("file.exportCsv", "csv"), ("file.exportHtml", "html")] {
            let path = std::env::temp_dir().join(format!(
                "gridcraft-hosted-export-{}-{}-{}.{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
                extension,
                extension
            ));
            let params = json!({"path": path});
            assert!(app.hosted_command(id, params.clone()).is_ok());
            assert!(!path.exists(), "{id} wrote outside the host");
            assert_eq!(app.take_host_commands(), vec![(id.to_owned(), params)]);
        }
    }
}
