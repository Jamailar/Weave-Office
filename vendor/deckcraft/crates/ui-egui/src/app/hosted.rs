//! Host-owned file and window lifecycle for an embedded presentation editor.
use super::{SlideApp, Value, json, media};

impl SlideApp {
    /// Retain the full editing state while the host provides the application shell.
    pub fn enable_hosted(&mut self) {
        self.hosted = true;
        self.backstage = None;
        self.doc_window = None;
        self.integrated_titlebar = false;
    }

    /// Select the language of host-owned UI labels. Standalone DeckCraft remains English.
    pub fn set_hosted_language(&mut self, language: &str) {
        self.hosted_zh_hans = language.eq_ignore_ascii_case("zh-hans") || language.eq_ignore_ascii_case("zh-cn");
    }

    /// Whether an editor-owned modal must finish before an external host navigation command.
    /// Reuse the editor's modal classification (including its command palette); ordinary task
    /// panes remain non-blocking. Keep editor logic/UI and dialog-generated host requests
    /// running, and route application quit through the host's save transaction.
    pub fn hosted_blocks_host_navigation(&self) -> bool {
        self.dialog.as_ref().is_some_and(|dialog| dialog.modal()) || self.palette.is_some() || self.opening.is_some()
    }

    pub fn hosted_logic(&mut self, ctx: &egui::Context) {
        if !self.hosted {
            self.enable_hosted();
        }
        crate::localization::set(ctx, self.hosted_zh_hans);
        // Queue safely even before the first UI pass (hidden startup has no Fonts yet).
        // In a full pass this installs immediately, before the host draws its header.
        if !self.styled {
            crate::theme::install_fonts_with(ctx, &self.ui_fonts.faces);
        }
        self.logic(ctx);
    }
    pub fn hosted_ui(&mut self, ui: &mut egui::Ui) {
        if !self.hosted {
            self.enable_hosted();
        }
        crate::localization::set(ui.ctx(), self.hosted_zh_hans);
        self.ui(ui);
    }

    /// Unified host toolbar commands retain the editor's draft and clipboard semantics.
    pub fn hosted_command(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if !self.hosted {
            self.enable_hosted();
        }
        self.run(id, params)
    }

    /// Include in-place drafts in save/close decisions before releasing the editor.
    pub fn hosted_flush(&mut self) -> Result<(), String> {
        // The host can finish a file pick just before switching or closing.
        // Consume its targeted inbox before deciding what must be saved.
        self.drain_inbox();
        self.session.settle_composition();
        Ok(())
    }

    pub fn hosted_dirty(&self) -> bool {
        self.session.documents().iter().any(|d| d.is_dirty())
    }

    pub fn hosted_save(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.hosted_flush()?;
        let path = path
            .to_str()
            .ok_or_else(|| if self.hosted_zh_hans { "文件路径不是有效的 Unicode 文本" } else { "The file path is not valid Unicode" }.to_string())?;
        self.host_dispatch = true;
        let result = self.run("file.saveAs", json!({"path": path}));
        self.host_dispatch = false;
        result.map(|_| ()).map_err(|e| if self.hosted_zh_hans { format!("保存失败：{e}") } else { e })
    }

    pub fn hosted_deactivate(&mut self, ctx: &egui::Context) -> Result<(), String> {
        self.hosted_flush()?;
        self.note_settled_composition(ctx);
        ctx.memory_mut(|m| {
            m.interrupt_ime();
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        ctx.stop_dragging();
        egui::Popup::close_all(ctx);
        self.ime_pending = None;
        self.ime_tracker.observe(ctx.viewport_id(), None);
        self.suspend_ime_reset(ctx.viewport_id());
        self.media.stop_owned(media::Owner::Editor);
        self.media.stop_owned(media::Owner::Show);
        self.show = None;
        self.stop_anim_preview();
        Ok(())
    }

    pub fn take_host_commands(&mut self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.host_commands)
    }

    pub(crate) fn queue_host_command(&mut self, id: &str, params: &Value) -> bool {
        // The ordinary export palette belongs to DeckCraft so its format and option
        // controls remain available. Explicit parameters still follow the host contract.
        if id == "app.exportDialog" && (params.is_null() || params.as_object().is_some_and(|o| o.is_empty())) {
            return false;
        }
        let owned = matches!(
            id,
            "file.new"
                | "file.open"
                | "file.save"
                | "file.saveAs"
                | "file.saveTemplate"
                | "file.export"
                | "file.close"
                | "file.print"
                | "app.openDialog"
                | "app.saveAsDialog"
                | "app.exportDialog"
                | "app.closePresentation"
                | "app.home"
                | "app.backstage"
                | "app.preferences"
                | "app.print"
                | "app.about"
                | "app.quit"
                | "app.exit"
                | "app.checkUpdates"
                | "view.newWindow"
                | "view.dark"
        );
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
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn hosted_lifecycle_commands_leave_the_document_in_place() {
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        app.enable_hosted();
        let uid = app.session.active().map(|d| d.uid);
        for id in ["file.new", "file.open", "file.save", "app.saveAsDialog", "app.closePresentation", "view.newWindow"] {
            assert!(app.hosted_command(id, json!({})).is_ok());
        }
        assert_eq!(app.session.active().map(|d| d.uid), uid);
        assert_eq!(app.session.documents().len(), 1);
        assert_eq!(app.take_host_commands().len(), 6);
        assert!(app.take_host_commands().is_empty());
        assert!(app.hosted_command("slide.new", json!({"title": "hosted"})).is_ok());
        assert!(app.hosted_dirty());
        assert!(app.hosted_command("edit.undo", json!({})).is_ok());
        assert!(app.take_host_commands().is_empty());
    }

    #[test]
    fn hosted_export_and_template_paths_are_queued_without_writing() {
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        app.enable_hosted();
        for (id, extension) in [("file.export", "pdf"), ("file.saveTemplate", "potx")] {
            let path = std::env::temp_dir().join(format!(
                "deckcraft-hosted-export-{}-{}.{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
                extension
            ));
            let params = json!({"path": path, "format": extension});
            assert!(app.hosted_command(id, params.clone()).is_ok());
            assert!(!path.exists(), "{id} wrote outside the host");
            assert_eq!(app.take_host_commands(), vec![(id.to_owned(), params)]);
        }
        assert!(app.hosted_command("app.exportDialog", json!({})).is_ok());
        assert!(app.dialog.as_ref().is_some_and(|d| d.id == "export"));
        assert!(app.take_host_commands().is_empty());
        assert!(app.hosted_command("app.exportDialog", json!({"custom": true})).is_ok());
        assert_eq!(app.take_host_commands(), vec![("app.exportDialog".into(), json!({"custom": true}))]);
    }

    #[test]
    fn hosted_export_hands_real_bytes_and_options_to_host() {
        let received = Rc::new(RefCell::new(Vec::<crate::HostExportRequest>::new()));
        let sink = received.clone();
        let services = crate::Services {
            export_files: Some(Box::new(move |request| {
                sink.borrow_mut().push(request);
                Ok(())
            })),
            ..Default::default()
        };
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), services);
        app.enable_hosted();
        let before = app.session.active().unwrap().doc.clone();
        let dirty = app.hosted_dirty();
        assert!(app.export_as("pngAll", json!({"transparentBackground": true})));
        assert!(app.export_as("pdf", json!({"layout": "handouts", "perPage": 2, "includeHidden": true})));
        let requests = received.borrow();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].directory, "even one slide uses a folder picker");
        assert_eq!(requests[0].files.len(), 1);
        assert!(requests[0].files[0].name.ends_with(".png"));
        let image = deckcraft_render::decode_image(&requests[0].files[0].bytes).unwrap();
        assert_eq!(image.get_pixel(2, 2).0[3], 0);
        assert!(!requests[1].directory);
        assert!(requests[1].files[0].name.ends_with(".pdf"));
        assert!(requests[1].files[0].bytes.starts_with(b"%PDF"));
        assert_eq!(app.session.active().unwrap().doc, before);
        assert_eq!(app.hosted_dirty(), dirty);
        assert!(app.take_host_commands().is_empty());
    }

    #[test]
    fn hosted_export_without_handler_reports_failure() {
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        app.enable_hosted();
        assert!(!app.export_as("pdf", json!({})));
        assert!(app.take_host_commands().is_empty());
    }
}
