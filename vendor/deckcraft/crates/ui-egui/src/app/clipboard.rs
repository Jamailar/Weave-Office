use super::{SlideApp, Value, json};

/// Text put on the system clipboard when the copied objects (a rectangle, a picture, slides…)
/// carry no text. egui-winit drops Ctrl+V when the system clipboard is empty, so without it the
/// shortcut never reached the session and only the context menu could paste objects (#182). An
/// invisible word joiner, so a paste into another app adds nothing visible.
pub(crate) const OBJECTS_MARKER: &str = "\u{2060}";

/// What a copy/cut leaves on the system clipboard for the session's plain-text rendering `t`.
pub(crate) fn system_clipboard_text(t: &str) -> String {
    if t.is_empty() { OBJECTS_MARKER.to_string() } else { t.to_string() }
}

impl SlideApp {
    /// Offer the copy or cut on the system clipboard.
    ///
    /// Slides go out as plain text. Text goes out as HTML plus the same plain words, so a paste
    /// back into DeckCraft still uses the internal clipboard. A textless shape leaves the
    /// invisible [`OBJECTS_MARKER`] rather than a picture: with only an image on the clipboard
    /// egui-winit drops Ctrl+V, and the shortcut could no longer paste the shape here (#182).
    pub(super) fn publish_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
        if self.session.active().is_some_and(|d| d.selection.text.as_ref().is_some_and(|text| !text.is_range())) {
            return;
        }
        let cmd = if cut { "edit.cut" } else { "edit.copy" };
        if self.run(cmd, json!({})).is_err() {
            return;
        }
        if let Some(text) = self.pending_copy.take() {
            ctx.copy_text(text);
        }
    }

    /// Structured hosts must accept the preview before the command can delete anything.
    pub(super) fn publish_hosted_clipboard(&mut self, id: &str, params: &Value, empty: bool) -> Result<bool, String> {
        if !matches!(id, "edit.copy" | "edit.cut") || self.services.clipboard_set_content.is_none() {
            return Ok(false);
        }
        // Even a rejected write must not later flush a stale queued plain-text copy.
        self.pending_copy = None;
        if empty {
            return Ok(true);
        }
        let (clipboard, result) = self.session.preview_copy(params).map_err(|e| e.to_string())?;
        let text = result.get("text").and_then(Value::as_str).unwrap_or("");
        let html = result.get("html").and_then(Value::as_str).unwrap_or("");
        if self.services.clipboard_set_content.as_mut().is_some_and(|write| write(&clipboard, html, text)) {
            Ok(true)
        } else {
            Err("could not write to the clipboard; the selection was kept".into())
        }
    }

    /// Copy/cut from every entry point shares the same sink, including non-text objects.
    pub(super) fn clipboard_output(&mut self, v: &Value) -> Option<String> {
        let text = v.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        let html = v.get("html").and_then(Value::as_str).unwrap_or("").to_string();
        if self.session.clipboard.slides.is_empty() && !html.is_empty() && self.services.clipboard_set_html.as_mut().is_some_and(|f| f(&html, &text))
        {
            return None;
        }
        Some(system_clipboard_text(&text))
    }

    pub(super) fn prepare_clipboard(&mut self, params: &mut Value) {
        if let Some(read) = &mut self.services.clipboard_content {
            self.session.clipboard = read().unwrap_or_default();
        }
        if params.get("text").is_none()
            && let Some(text) = self.services.clipboard_text.as_mut().and_then(|read| read())
        {
            params["text"] = Value::String(text);
        }
        if params.get("text").and_then(Value::as_str) == Some(OBJECTS_MARKER) {
            params["text"] = Value::String(String::new());
        }
        if params.get("html").is_none()
            && let Some(html) = self.services.clipboard_html.as_mut().and_then(|read| read()).filter(|html| !html.is_empty())
        {
            params["html"] = Value::String(html);
        }
    }

    pub(super) fn clipboard_pasted(&mut self) {
        if let Some(update) = &mut self.services.clipboard_update_content {
            update(&self.session.clipboard);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use deckcraft_engine::Session;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn rejected_hosted_cut_keeps_text_and_textless_shapes_from_both_entry_points() {
        for text in [false, true] {
            for keyboard in [false, true] {
                let writes = Rc::new(Cell::new(0));
                let sink = writes.clone();
                let mut app = SlideApp::new(
                    Session::with_new(),
                    Services {
                        clipboard_set_content: Some(Box::new(move |clipboard, _, plain| {
                            sink.set(sink.get() + 1);
                            assert_eq!(plain.is_empty(), !text);
                            assert!(!clipboard.shapes.is_empty() || clipboard.text.is_some());
                            false
                        })),
                        clipboard_content: Some(Box::new(|| None)),
                        ..Default::default()
                    },
                );
                let id = app.run("shape.insert", json!({"preset": "rect", "rect": [80, 80, 120, 80]})).unwrap()["id"].clone();
                if text {
                    app.run("text.set", json!({"id": id, "text": "Keep this text"})).unwrap();
                    app.run("text.edit", json!({"id": id})).unwrap();
                    app.run("text.selectAll", json!({})).unwrap();
                }
                app.run("shape.insert", json!({"preset": "ellipse", "rect": [250, 80, 120, 80]})).unwrap();
                app.run("edit.undo", json!({})).unwrap();
                let before = app.session.active().unwrap().clone();
                let journal = app.session.journal.clone();
                let clipboard = format!("{:?}", app.session.clipboard);
                let ctx = egui::Context::default();
                if keyboard {
                    app.publish_clipboard(&ctx, true);
                    assert!(app.status.as_ref().unwrap().0.contains("clipboard"));
                } else {
                    assert!(app.run("edit.cut", json!({})).unwrap_err().contains("clipboard"));
                }
                let after = app.session.active().unwrap();
                assert!(std::sync::Arc::ptr_eq(&after.doc, &before.doc));
                assert!(std::sync::Arc::ptr_eq(&after.saved_doc, &before.saved_doc));
                assert_eq!(after.selection, before.selection);
                assert_eq!(after.revision, before.revision);
                assert_eq!(format!("{:?}", after.history), format!("{:?}", before.history));
                assert_eq!(app.session.journal, journal);
                assert_eq!(format!("{:?}", app.session.clipboard), clipboard);
                assert!(app.pending_copy.is_none());
                assert_eq!(writes.get(), 1);
                assert!(!ctx.output(|o| o.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(_)))));
            }
        }
    }

    #[test]
    fn hosted_cut_publishes_once_and_caret_copy_keeps_previous_payload() {
        let writes = Rc::new(Cell::new(0));
        let sink = writes.clone();
        let mut app = SlideApp::new(
            Session::with_new(),
            Services {
                clipboard_set_content: Some(Box::new(move |_, _, _| {
                    sink.set(sink.get() + 1);
                    true
                })),
                ..Default::default()
            },
        );
        let id = app.run("shape.insert", json!({"preset": "rect", "rect": [80, 80, 120, 80]})).unwrap()["id"].clone();
        let before = app.session.active().unwrap().doc.clone();
        let depth = app.session.active().unwrap().history.undo.len();
        app.publish_clipboard(&egui::Context::default(), true);
        assert_eq!(writes.get(), 1);
        assert_eq!(app.session.active().unwrap().history.undo.len(), depth + 1);
        app.run("edit.undo", json!({})).unwrap();
        assert!(std::sync::Arc::ptr_eq(&app.session.active().unwrap().doc, &before));
        app.run("text.edit", json!({"id": id})).unwrap();
        app.run("edit.copy", json!({})).unwrap();
        app.publish_clipboard(&egui::Context::default(), false);
        assert_eq!(writes.get(), 1);
    }
}
