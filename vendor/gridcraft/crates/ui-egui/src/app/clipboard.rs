use super::{Json, SheetApp, json};

impl SheetApp {
    /// Copy or cut the selected cells to the system clipboard. Engine-only commands keep
    /// their existing internal clipboard behavior and do not write to the host clipboard.
    pub fn copy_to_clipboard(&mut self, ctx: &egui::Context, command: &str) {
        // Only ask the engine for HTML when the host can publish it.
        let result = match self.run_command(command, json!({"html": self.services.copy_html.is_some() || self.services.copy_cells.is_some()})) {
            Ok(result) => result,
            Err(e) => return self.alert(e),
        };
        let Some(text) = result.get("text").and_then(Json::as_str) else { return };
        // The system clipboard gets this copy, so its text decides what Paste pastes.
        self.clipboard_text_at_copy = None;
        self.cut_text = (command == "edit.cut").then(|| text.replace("\r\n", "\n"));
        if let (Some(copy), Some(clip)) = (&mut self.services.copy_cells, self.session.clipboard.as_ref()) {
            match copy(clip, result.get("html").and_then(Json::as_str).unwrap_or(""), text) {
                Ok(()) => return,
                Err(error) => log::warn!("structured clipboard unavailable; copying fallback: {error}"),
            }
        }
        if let Some(html) = result.get("html").and_then(Json::as_str)
            && let Some(copy_html) = &mut self.services.copy_html
        {
            match copy_html(html, text) {
                Ok(()) => return,
                Err(error) => log::warn!("HTML clipboard unavailable; copying plain text: {error}"),
            }
        }
        ctx.copy_text(text.to_string());
    }

    /// Paste text that came from the system clipboard (Cmd/Ctrl+V, or the ribbon and menu Paste),
    /// with the clipboard's HTML flavour when the host can read it.
    ///
    /// Excel pastes a cut once: after GridCraft's cut has been pasted, the system clipboard text
    /// that came with it is not pasted again over the moved block; GridCraft's own (empty)
    /// clipboard is pasted instead, which reports "Nothing to paste.". Other text means the
    /// clipboard has moved on, so the cut's text is forgotten and pastes normally later.
    pub fn paste_external(&mut self, text: String) {
        if self.services.paste_cells.is_some() {
            let html = self.services.paste_html.as_mut().and_then(|read| read());
            self.run_or_alert(
                "edit.paste",
                match html {
                    Some(html) => json!({"text": text, "html": html}),
                    None => json!({"text": text}),
                },
            );
            return;
        }
        if Self::same_clipboard_text(Some(&text), self.cut_text.as_deref()) {
            if self.session.clipboard.is_none() {
                return self.run_or_alert("edit.paste", json!({}));
            }
        } else {
            self.cut_text = None;
        }
        let html = self.services.paste_html.as_mut().and_then(|read| read());
        let params = match html {
            Some(html) => json!({"text": text, "html": html}),
            None => json!({"text": text}),
        };
        self.run_or_alert("edit.paste", params);
    }

    /// Ribbon and menu Paste: paste the text on the system clipboard, as Cmd/Ctrl+V does. With no
    /// text there, no host clipboard (web), or the text that was there when GridCraft last copied
    /// without publishing, GridCraft's own clipboard is pasted.
    pub fn paste_from_clipboard(&mut self) {
        if self.services.paste_cells.is_some() {
            match self.system_clipboard_text() {
                Some(text) => self.paste_external(text),
                None => self.run_or_alert("edit.paste", json!({})),
            }
            return;
        }
        match self.system_clipboard_text() {
            Some(text) if self.session.clipboard.is_none() || self.clipboard_text_at_copy.as_ref() != Some(&text) => self.paste_external(text),
            _ => self.run_or_alert("edit.paste", json!({})),
        }
    }

    /// Refresh only for a user's paste, never for a headless engine command.
    pub(crate) fn sync_host_clipboard(&mut self) {
        if let Some(read) = &mut self.services.paste_cells {
            self.session.clipboard = read();
            self.clipboard_text_at_copy = None;
            self.cut_text = None;
        }
    }

    pub(super) fn update_host_clipboard(&mut self) {
        if let Some(update) = &mut self.services.update_cells {
            update(self.session.clipboard.as_ref());
        }
    }

    /// A user-facing paste may use the structured system copy; script calls to `run` do not.
    pub(crate) fn run_user_paste(&mut self, id: &str, mut params: Json) -> Result<Json, String> {
        if !matches!(id, "edit.paste" | "edit.pasteSpecial" | "home.insertCopiedCells") {
            return self.run(id, params);
        }
        if self.services.paste_cells.is_some() {
            self.sync_host_clipboard();
        }
        let explicit_text = params.get("text").and_then(Json::as_str);
        let stale_fragment = self.session.clipboard.as_ref().is_some_and(|clip| {
            explicit_text
                .is_some_and(|text| !(clip.text.is_empty() && text == "\u{2060}") && !Self::same_clipboard_text(Some(text), Some(&clip.text)))
        });
        if stale_fragment {
            self.session.clipboard = None;
        }
        let used_typed_clipboard = self.services.paste_cells.is_some() && self.session.clipboard.is_some();
        let cross_document_cut =
            self.session.clipboard.as_ref().is_some_and(|clip| clip.cut && self.session.active().is_some_and(|doc| doc.uid != clip.doc_uid));
        if self.services.paste_cells.is_some()
            && let Some(clip) = self.session.clipboard.as_ref()
            && let Some(object) = params.as_object_mut()
        {
            if !object.contains_key("text")
                || object.get("text").and_then(Json::as_str) == Some("\u{2060}") && clip.text.is_empty()
                || Self::same_clipboard_text(object.get("text").and_then(Json::as_str), Some(&clip.text))
            {
                object.insert("text".into(), json!(clip.text));
            }
        } else if self.services.paste_cells.is_some()
            && id == "edit.paste"
            && let Some(object) = params.as_object_mut()
            && !object.contains_key("text")
            && let Some(text) = self.system_clipboard_text()
        {
            object.insert("text".into(), json!(text));
            if let Some(html) = self.services.paste_html.as_mut().and_then(|read| read()) {
                object.insert("html".into(), json!(html));
            }
        }
        if cross_document_cut {
            if self.services.request_cross_document_cut.is_some() {
                if self.hosted_has_edit_draft() {
                    return Err("Finish the current edit before moving cells between workbooks.".into());
                }
                let clip = self.session.clipboard.as_ref().ok_or("The cut is no longer available.")?;
                let target = self.session.active().ok_or("No active workbook.")?;
                let request = self.services.request_cross_document_cut.as_mut().ok_or("The host cannot move cells between workbooks.")?;
                return request(id, params, clip, target);
            }
            // The engine rejects a cross-document cut without a host transaction.
            return self.run(id, params);
        }
        let result = self.run(id, params);
        if result.is_ok() {
            // Explicit text from elsewhere did not use this structured copy. In particular,
            // it must not consume another editor's pending cut.
            if used_typed_clipboard {
                self.update_host_clipboard();
            }
        }
        result
    }

    /// Enter pastes a marked range and finishes copy mode only after a completed paste.
    pub(crate) fn paste_and_finish_copy(&mut self) {
        match self.run_user_paste("edit.paste", json!({})) {
            Ok(result) if result.get("pending").and_then(Json::as_str) == Some("host") => {}
            Ok(_) => self.run_or_alert("edit.clearClipboard", json!({})),
            Err(error) => self.alert(error),
        }
    }

    /// The system clipboard's plain text, with the line endings of egui's `Event::Paste` text.
    /// `None` when it holds no text or the host can't read it.
    pub(super) fn system_clipboard_text(&mut self) -> Option<String> {
        self.services.paste_text.as_mut().and_then(|read| read()).filter(|t| !t.is_empty()).map(|t| t.replace("\r\n", "\n"))
    }

    /// Whether two clipboard texts are the same copy: trailing line breaks don't count, as in the
    /// engine's `edit.paste`, and CRLF matches LF (egui's `Event::Paste` text may keep the host's).
    fn same_clipboard_text(a: Option<&str>, b: Option<&str>) -> bool {
        fn chars(t: &str) -> impl Iterator<Item = char> + '_ {
            t.trim_end_matches(['\r', '\n']).chars().filter(|c| *c != '\r')
        }
        match (a, b) {
            (Some(a), Some(b)) => chars(a).eq(chars(b)),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    fn hosted_with_clipboard() -> (SheetApp, Rc<RefCell<Option<gridcraft_engine::Clipboard>>>, Rc<RefCell<usize>>) {
        let typed = Rc::new(RefCell::new(None));
        let invalidations = Rc::new(RefCell::new(0));
        let mut services = crate::Services::default();
        let published = typed.clone();
        services.copy_cells = Some(Box::new(move |clip, _, _| {
            *published.borrow_mut() = Some(clip.clone());
            Ok(())
        }));
        let read = typed.clone();
        services.paste_cells = Some(Box::new(move || read.borrow().clone()));
        let updated = typed.clone();
        let count = invalidations.clone();
        services.update_cells = Some(Box::new(move |clip| {
            if clip.is_none() {
                *count.borrow_mut() += 1;
            }
            *updated.borrow_mut() = clip.cloned();
        }));
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, services);
        app.enable_hosted();
        (app, typed, invalidations)
    }

    #[test]
    fn canceled_cut_cannot_resurrect_old_snapshot_or_delete_new_source() {
        let (mut app, typed, invalidations) = hosted_with_clipboard();
        app.hosted_command("cell.set", json!({"cell": "A1", "input": "old"})).unwrap();
        app.hosted_command("edit.cut", json!({"range": "A1"})).unwrap();
        assert!(typed.borrow().as_ref().is_some_and(|clip| clip.cut));
        app.hosted_command("edit.clearClipboard", json!({})).unwrap();
        assert!(typed.borrow().is_none());
        assert_eq!(*invalidations.borrow(), 1);
        app.hosted_command("cell.set", json!({"cell": "A1", "input": "new"})).unwrap();
        assert!(app.hosted_command("edit.paste", json!({"at": "B1"})).is_err());
        assert_eq!(app.hosted_command("cell.get", json!({"cell": "A1"})).unwrap()["input"], "new");
        assert_eq!(app.hosted_command("cell.get", json!({"cell": "B1"})).unwrap()["input"], "");
    }

    #[test]
    fn cancel_copy_also_invalidates_the_host_snapshot() {
        let (mut app, typed, invalidations) = hosted_with_clipboard();
        app.hosted_command("cell.set", json!({"cell": "A1", "input": "copied"})).unwrap();
        app.hosted_command("edit.copy", json!({"range": "A1"})).unwrap();
        assert!(typed.borrow().as_ref().is_some_and(|clip| !clip.cut));
        app.hosted_command("edit.clearClipboard", json!({})).unwrap();
        assert!(typed.borrow().is_none());
        assert_eq!(*invalidations.borrow(), 1);
    }

    #[test]
    fn ordinary_edit_releases_old_local_copy_without_reading_or_canceling_new_host_copy() {
        let typed = Rc::new(RefCell::new(None::<gridcraft_engine::Clipboard>));
        let generation = Rc::new(Cell::new(0_u64));
        let seen_generation = Rc::new(Cell::new(0_u64));
        let reads = Rc::new(Cell::new(0));
        let mut services = crate::Services::default();
        let (published, published_generation, published_seen) = (typed.clone(), generation.clone(), seen_generation.clone());
        services.copy_cells = Some(Box::new(move |clip, _, _| {
            published_generation.set(published_generation.get() + 1);
            published_seen.set(published_generation.get());
            *published.borrow_mut() = Some(clip.clone());
            Ok(())
        }));
        let (read, read_generation, read_seen, read_count) = (typed.clone(), generation.clone(), seen_generation.clone(), reads.clone());
        services.paste_cells = Some(Box::new(move || {
            read_count.set(read_count.get() + 1);
            read_seen.set(read_generation.get());
            read.borrow().clone()
        }));
        let (updated, update_generation, update_seen) = (typed.clone(), generation.clone(), seen_generation.clone());
        services.update_cells = Some(Box::new(move |clip| {
            if update_generation.get() == update_seen.get() {
                *updated.borrow_mut() = clip.cloned();
            }
        }));
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, services);
        app.enable_hosted();
        app.hosted_command("cell.set", json!({"cell": "A1", "input": "old"})).unwrap();
        app.hosted_command("edit.copy", json!({"range": "A1"})).unwrap();
        let mut newer = typed.borrow().as_ref().unwrap().clone();
        newer.text = "new host copy".into();
        generation.set(generation.get() + 1);
        *typed.borrow_mut() = Some(newer);

        app.hosted_command("home.insertRows", json!({"rows": "1:1"})).unwrap();
        assert!(app.session.clipboard.is_none());
        assert_eq!(reads.get(), 0, "an ordinary edit must not read another editor's copy");
        assert_eq!(typed.borrow().as_ref().map(|clip| clip.text.as_str()), Some("new host copy"));

        app.hosted_command("edit.clearClipboard", json!({})).unwrap();
        assert_eq!(reads.get(), 1, "explicit cancel reads the current host copy");
        assert!(typed.borrow().is_none());
    }

    #[test]
    fn explicit_paste_text_does_not_move_stale_typed_cut() {
        let (mut app, typed, invalidations) = hosted_with_clipboard();
        app.hosted_command("cell.set", json!({"cell": "A1", "input": "A"})).unwrap();
        app.hosted_command("edit.cut", json!({"range": "A1"})).unwrap();
        assert_eq!(typed.borrow().as_ref().map(|clip| clip.text.as_str()), Some("A\n"));
        app.hosted_command("edit.paste", json!({"at": "B1", "text": "B"})).unwrap();
        assert_eq!(app.hosted_command("cell.get", json!({"cell": "A1"})).unwrap()["input"], "A");
        assert_eq!(app.hosted_command("cell.get", json!({"cell": "B1"})).unwrap()["input"], "B");
        assert!(app.session.clipboard.is_none());
        assert_eq!(typed.borrow().as_ref().map(|clip| clip.text.as_str()), Some("A\n"));
        assert_eq!(*invalidations.borrow(), 0);
        app.hosted_command("edit.undo", json!({})).unwrap();
        assert_eq!(app.hosted_command("cell.get", json!({"cell": "A1"})).unwrap()["input"], "A");
        assert_eq!(app.hosted_command("cell.get", json!({"cell": "B1"})).unwrap()["input"], "");
    }

    #[test]
    fn hosted_copy_uses_explicit_range_instead_of_active_cell() {
        let (mut app, typed, _) = hosted_with_clipboard();
        app.hosted_command("cell.set", json!({"cell": "A1", "input": "active"})).unwrap();
        app.hosted_command("cell.set", json!({"cell": "B2", "input": "chosen"})).unwrap();
        let result = app.hosted_command("edit.copy", json!({"range": "B2"})).unwrap();
        assert_eq!(result["range"], "B2");
        assert_eq!(result["text"], "chosen\n");
        assert_eq!(typed.borrow().as_ref().map(|clip| clip.text.as_str()), Some("chosen\n"));
    }

    #[test]
    fn cross_document_cut_is_delegated_before_either_document_changes() {
        let (mut app, typed, invalidations) = hosted_with_clipboard();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let seen = calls.clone();
        app.services.request_cross_document_cut = Some(Box::new(move |id, params, clip, target| {
            assert_eq!(clip.wb.active().unwrap().input_text(crate::CellRef::parse("A1").unwrap()), "source");
            assert_eq!(target.wb.active().unwrap().input_text(crate::CellRef::parse("B1").unwrap()), "");
            seen.borrow_mut().push((id.to_owned(), params));
            Ok(json!({"pending":"host","operation":"moveCells"}))
        }));
        app.hosted_command("cell.set", json!({"cell": "A1", "input": "source"})).unwrap();
        let source_uid = app.session.active().unwrap().uid;
        app.hosted_command("edit.cut", json!({"range": "A1"})).unwrap();
        app.session.execute("file.new", json!({})).unwrap();
        let result = app.hosted_command("edit.paste", json!({"at": "B1"})).unwrap();
        assert_eq!(result, json!({"pending":"host","operation":"moveCells"}));
        assert_eq!(calls.borrow().as_slice(), &[("edit.paste".to_owned(), json!({"at":"B1","text":"source\n"}))]);
        assert_eq!(app.hosted_command("cell.get", json!({"cell": "B1"})).unwrap()["input"], "");
        assert!(app.session.clipboard.as_ref().is_some_and(|clip| clip.cut));
        assert!(typed.borrow().as_ref().is_some_and(|clip| clip.cut));
        assert_eq!(*invalidations.borrow(), 0);
        let source = app.session.documents().iter().find(|doc| doc.uid == source_uid).unwrap();
        assert_eq!(source.wb.active().unwrap().input_text(crate::CellRef::parse("A1").unwrap()), "source");
    }

    #[test]
    fn explicit_other_text_does_not_delegate_cross_document_cut() {
        let (mut app, typed, invalidations) = hosted_with_clipboard();
        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        app.services.request_cross_document_cut = Some(Box::new(move |_, _, _, _| {
            seen.set(seen.get() + 1);
            Ok(json!({"pending":"host"}))
        }));
        app.hosted_command("cell.set", json!({"cell":"A1","input":"A"})).unwrap();
        app.hosted_command("edit.cut", json!({"range":"A1"})).unwrap();
        app.session.execute("file.new", json!({})).unwrap();
        app.hosted_command("edit.paste", json!({"at":"B1","text":"B"})).unwrap();
        assert_eq!(calls.get(), 0);
        assert_eq!(app.hosted_command("cell.get", json!({"cell":"B1"})).unwrap()["input"], "B");
        assert!(typed.borrow().as_ref().is_some_and(|clip| clip.cut));
        assert_eq!(*invalidations.borrow(), 0);
    }

    #[test]
    fn enter_keeps_cut_while_host_move_is_pending() {
        let (mut app, typed, invalidations) = hosted_with_clipboard();
        app.services.request_cross_document_cut = Some(Box::new(|_, _, _, _| Ok(json!({"pending":"host","operation":"moveCells"}))));
        app.hosted_command("cell.set", json!({"cell":"A1","input":"A"})).unwrap();
        app.hosted_command("edit.cut", json!({"range":"A1"})).unwrap();
        app.session.execute("file.new", json!({})).unwrap();
        app.paste_and_finish_copy();
        assert!(app.session.clipboard.as_ref().is_some_and(|clip| clip.cut));
        assert!(typed.borrow().as_ref().is_some_and(|clip| clip.cut));
        assert_eq!(*invalidations.borrow(), 0);
    }

    #[test]
    fn cross_document_cut_without_host_service_is_rejected_without_consuming_it() {
        let (mut app, typed, invalidations) = hosted_with_clipboard();
        app.hosted_command("cell.set", json!({"cell":"A1","input":"A"})).unwrap();
        let source_uid = app.session.active().unwrap().uid;
        app.hosted_command("edit.cut", json!({"range":"A1"})).unwrap();
        app.session.execute("file.new", json!({})).unwrap();
        assert!(app.hosted_command("edit.paste", json!({"at":"B1"})).is_err());
        assert_eq!(app.hosted_command("cell.get", json!({"cell":"B1"})).unwrap()["input"], "");
        let source = app.session.documents().iter().find(|doc| doc.uid == source_uid).unwrap();
        assert_eq!(source.wb.active().unwrap().input_text(crate::CellRef::parse("A1").unwrap()), "A");
        assert!(app.session.clipboard.as_ref().is_some_and(|clip| clip.cut));
        assert!(typed.borrow().as_ref().is_some_and(|clip| clip.cut));
        assert_eq!(*invalidations.borrow(), 0);
    }
}
