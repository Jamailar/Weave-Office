//! Editing in the design view: the selection, and the changes typing, the keyboard, the
//! clipboard and the ribbon make to the document.
//!
//! The file's text stays the document. A change edits the parsed tree, serializes it, and
//! applies the difference to the text as one edit, so undo, saving and the source view all
//! keep working on the text. The edited tree is kept (not parsed again) so the selection's
//! nodes stay valid; a change made elsewhere (undo, the source view) parses the text again
//! and carries the selection over by path.

use crate::RichApp;
use crate::dom::{Document, ROOT};
use crate::edit::{self, Anchor, Pos};
use crate::style::Style;
use serde_json::{Value, json};
use weave_text_editor::edit::{Edit, Selection as TextSelection};

/// What the design view has selected: `anchor` stays put while `focus` moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub focus: Pos,
}

impl Selection {
    pub fn caret(pos: Pos) -> Self {
        Self {
            anchor: pos,
            focus: pos,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }
}

/// Reads one flavour of the clipboard.
pub type ReadClipboard = Box<dyn Fn() -> Option<String>>;
/// Puts HTML with its plain-text flavour on the clipboard.
pub type WriteClipboard = Box<dyn Fn(&str, &str) -> bool>;

/// How the host's clipboard is reached (rich HTML in and out).
pub struct ClipboardHooks {
    /// The HTML flavour on the clipboard, if any.
    pub read_html: ReadClipboard,
    /// The plain text on the clipboard, if any.
    pub read_text: ReadClipboard,
    pub write: WriteClipboard,
}

/// Inline formatting the ribbon toggles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Bold,
    Italic,
    Underline,
    Strike,
}

impl Toggle {
    fn is_on(self, style: &Style) -> bool {
        match self {
            Toggle::Bold => style.bold,
            Toggle::Italic => style.italic,
            Toggle::Underline => style.underline,
            Toggle::Strike => style.strike,
        }
    }

    fn property(self) -> &'static str {
        match self {
            Toggle::Bold => "font-weight",
            Toggle::Italic => "font-style",
            Toggle::Underline | Toggle::Strike => "text-decoration",
        }
    }

    fn on(self) -> &'static str {
        match self {
            Toggle::Bold => "bold",
            Toggle::Italic => "italic",
            Toggle::Underline => "underline",
            Toggle::Strike => "line-through",
        }
    }

    fn off(self) -> &'static str {
        match self {
            Toggle::Bold => "normal",
            Toggle::Italic => "normal",
            Toggle::Underline | Toggle::Strike => "none",
        }
    }
}

impl RichApp {
    /// Parse the text again if it changed outside the design view, carrying the selection.
    pub(crate) fn sync_document(&mut self) {
        if self.cache.doc.is_some() && self.cache.text == self.source.text() {
            return;
        }
        let anchors = match (&self.cache.doc, self.cache.selection) {
            (Some(doc), Some(sel)) => {
                Some((Anchor::of(doc, sel.anchor), Anchor::of(doc, sel.focus)))
            }
            _ => None,
        };
        let text = self.source.text().to_string();
        let doc = Document::parse(&text);
        self.cache.has_scripts = doc.has_scripts_or_stylesheets();
        self.cache.selection = anchors.and_then(|(a, f)| {
            Some(Selection {
                anchor: a.resolve(&doc)?,
                focus: f.resolve(&doc)?,
            })
        });
        self.cache.doc = Some(doc);
        self.cache.text = text;
        self.cache.page = None;
        self.cache.styles = None;
    }

    /// The parsed document (parsed again if the text changed).
    pub fn document(&mut self) -> &Document {
        self.sync_document();
        self.cache.doc.as_ref().expect("synced")
    }

    pub fn selection(&self) -> Option<Selection> {
        self.cache.selection
    }

    pub fn set_selection(&mut self, selection: Option<Selection>) {
        self.cache.selection = selection;
    }

    /// The selection, or a caret at the start of the body when there is none.
    fn selection_or_start(&mut self) -> Selection {
        self.sync_document();
        if let Some(sel) = self.cache.selection {
            return sel;
        }
        let doc = self.cache.doc.as_mut().expect("synced");
        let body = doc.body();
        let start = edit::start_of(doc, body);
        Selection::caret(start)
    }

    /// Change the document: `change` gets the ordered selection and returns the new one.
    fn change(&mut self, typing: bool, change: impl FnOnce(&mut Document, Pos, Pos) -> Selection) {
        let sel = self.selection_or_start();
        let doc = self.cache.doc.as_mut().expect("synced");
        let (start, end) = edit::ordered(doc, sel.anchor, sel.focus);
        let selection = change(doc, start, end);
        let text = doc.serialize();
        self.cache.selection = Some(selection);
        self.commit(text, typing);
    }

    /// Make `text` the file's text as one edit (typing merges with the typing before it).
    fn commit(&mut self, text: String, typing: bool) {
        let old = self.source.text();
        if old == text {
            return;
        }
        let prefix = common_prefix(old, &text);
        let suffix = common_suffix(&old[prefix..], &text[prefix..]);
        let edit = Edit {
            range: prefix..old.len() - suffix,
            text: text[prefix..text.len() - suffix].to_string(),
            selection: TextSelection::caret(text.len() - suffix),
        };
        if typing {
            self.source.apply_typing(edit, self.cache.time);
        } else {
            self.source.apply(edit);
        }
        self.cache.text = text;
        self.cache.page = None;
        self.cache.styles = None;
        self.cache.has_scripts = self
            .cache
            .doc
            .as_ref()
            .is_some_and(Document::has_scripts_or_stylesheets);
    }

    /// Type text over the selection.
    pub fn type_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.change(true, |doc, start, end| {
            let at = edit::delete_range(doc, start, end);
            Selection::caret(edit::insert_text(doc, at, text))
        });
    }

    /// Backspace (`forward` false) or Delete.
    pub fn delete(&mut self, forward: bool) {
        self.change(true, |doc, start, end| {
            if start != end {
                return Selection::caret(edit::delete_range(doc, start, end));
            }
            let range = if forward {
                edit::forward_range(doc, start)
            } else {
                edit::backward_range(doc, start)
            };
            match range {
                Some((a, b)) => Selection::caret(edit::delete_range(doc, a, b)),
                None => Selection::caret(start),
            }
        });
    }

    /// Enter splits the paragraph; Shift+Enter breaks the line.
    pub fn enter(&mut self, line_break: bool) {
        self.change(false, |doc, start, end| {
            let at = edit::delete_range(doc, start, end);
            Selection::caret(if line_break {
                edit::insert_break(doc, at)
            } else {
                edit::split_block(doc, at)
            })
        });
    }

    pub fn select_all(&mut self) {
        self.sync_document();
        let doc = self.cache.doc.as_mut().expect("synced");
        let body = doc.body();
        let start = edit::start_of(doc, body);
        let end = edit::end_of(doc, body);
        self.cache.selection = Some(Selection {
            anchor: start,
            focus: end,
        });
    }

    /// The selection as HTML and as plain text, if anything is selected.
    pub fn copy_selection(&mut self) -> Option<(String, String)> {
        self.sync_document();
        let sel = self.cache.selection.filter(|s| !s.is_empty())?;
        let doc = self.cache.doc.as_ref()?;
        Some((
            edit::range_html(doc, sel.anchor, sel.focus),
            edit::range_text(doc, sel.anchor, sel.focus),
        ))
    }

    /// Put the selection on the clipboard. Returns the plain text when no host clipboard
    /// took it (the caller then copies the text itself).
    pub(crate) fn copy_to_clipboard(&mut self) -> Option<String> {
        let (html, text) = self.copy_selection()?;
        match &self.clipboard {
            Some(hooks) if (hooks.write)(&html, &text) => None,
            _ => Some(text),
        }
    }

    /// Paste HTML (kept with its inline styles, minus scripts and the like) or plain text.
    pub fn paste(&mut self, html: Option<&str>, text: &str) {
        let html = html.filter(|h| !h.trim().is_empty()).map(str::to_string);
        let text = text.to_string();
        self.change(false, move |doc, start, end| {
            let at = edit::delete_range(doc, start, end);
            Selection::caret(match html {
                Some(html) => edit::insert_html(doc, at, &html),
                None => edit::insert_plain(doc, at, &text),
            })
        });
    }

    /// Paste what the host clipboard holds; `fallback` is the text the window system gave.
    pub(crate) fn paste_from_clipboard(&mut self, fallback: &str) {
        let (html, text) = match &self.clipboard {
            Some(hooks) => ((hooks.read_html)(), (hooks.read_text)()),
            None => (None, None),
        };
        let text = text.unwrap_or_else(|| fallback.to_string());
        self.paste(html.as_deref(), &text);
    }

    /// The computed style of every node (kept until the document changes).
    pub(crate) fn styles(&mut self) -> &[Style] {
        self.sync_document();
        if self.cache.styles.is_none() {
            let doc = self.cache.doc.as_ref().expect("synced");
            self.cache.styles = Some(crate::style::compute_all(doc));
        }
        self.cache.styles.as_deref().expect("computed")
    }

    /// Whether every selected character has the formatting.
    pub fn is_on(&mut self, toggle: Toggle) -> bool {
        self.styles();
        let Some(sel) = self.cache.selection else {
            return false;
        };
        let doc = self.cache.doc.as_ref().expect("synced");
        let styles = self.cache.styles.as_deref().expect("computed");
        if sel.is_empty() {
            return styles.get(sel.focus.node).is_some_and(|s| toggle.is_on(s));
        }
        edit::all_text_with(doc, styles, sel.anchor, sel.focus, |s| toggle.is_on(s))
    }

    /// Bold, italic, underline or strikethrough on or off for the selected text.
    pub fn toggle(&mut self, toggle: Toggle) {
        let on = self.is_on(toggle);
        self.change(false, |doc, start, end| {
            if start == end {
                return Selection::caret(start);
            }
            let (mut s, mut e) = edit::style_range(doc, start, end, toggle.property(), None);
            let still = edit::all_text(doc, s, e, |st| toggle.is_on(st));
            if on && still {
                (s, e) = edit::style_range(doc, s, e, toggle.property(), Some(toggle.off()));
            } else if !on && !still {
                (s, e) = edit::style_range(doc, s, e, toggle.property(), Some(toggle.on()));
            }
            Selection {
                anchor: s,
                focus: e,
            }
        });
    }

    /// Set (or with `None` remove) an inline style property on the selected text.
    pub fn style_text(&mut self, property: &str, value: Option<&str>) {
        self.change(false, |doc, start, end| {
            if start == end {
                return Selection::caret(start);
            }
            let (s, e) = edit::style_range(doc, start, end, property, value);
            Selection {
                anchor: s,
                focus: e,
            }
        });
    }

    /// Remove inline formatting from the selected text.
    pub fn clear_formatting(&mut self) {
        self.change(false, |doc, start, end| {
            if start == end {
                return Selection::caret(start);
            }
            let (mut s, mut e) = (start, end);
            for property in [
                "font-weight",
                "font-style",
                "text-decoration",
                "color",
                "background-color",
                "font-size",
                "letter-spacing",
            ] {
                (s, e) = edit::style_range(doc, s, e, property, None);
            }
            Selection {
                anchor: s,
                focus: e,
            }
        });
    }

    /// Set a style property on the paragraphs the selection touches.
    pub fn style_paragraphs(&mut self, property: &str, value: Option<&str>) {
        self.change(false, |doc, start, end| {
            edit::style_blocks(doc, start, end, property, value);
            Selection {
                anchor: start,
                focus: end,
            }
        });
    }

    /// Turn the selected paragraphs into headings, quotes or body text.
    pub fn set_paragraph_kind(&mut self, tag: &str) {
        self.change(false, |doc, start, end| {
            edit::set_block_tag(doc, start, end, tag);
            Selection {
                anchor: start,
                focus: end,
            }
        });
    }

    /// The paragraph tag at the caret (`p`, `h1`…).
    pub fn paragraph_kind(&mut self) -> Option<String> {
        self.sync_document();
        let sel = self.cache.selection?;
        let doc = self.cache.doc.as_ref()?;
        let block = edit::block_of(doc, sel.focus);
        (block != ROOT).then(|| doc.name(block).unwrap_or("p").to_string())
    }

    /// Design-view commands; `None` when the id is not one of them.
    pub(crate) fn design_command(
        &mut self,
        id: &str,
        params: &Value,
    ) -> Option<Result<Value, String>> {
        let value = params.get("value").and_then(Value::as_str);
        match id {
            "edit.selectAll" => self.select_all(),
            "format.bold" => self.toggle(Toggle::Bold),
            "format.italic" => self.toggle(Toggle::Italic),
            "format.underline" => self.toggle(Toggle::Underline),
            "format.strikethrough" => self.toggle(Toggle::Strike),
            "format.clear" => self.clear_formatting(),
            "format.color" => self.style_text("color", value),
            "format.highlight" => self.style_text("background-color", value),
            "format.fontSize" => {
                let size = params.get("value").and_then(|v| {
                    v.as_f64()
                        .map(|n| format!("{n}px"))
                        .or_else(|| v.as_str().map(str::to_string))
                });
                self.style_text("font-size", size.as_deref());
            }
            "paragraph.align" => {
                let align = match value {
                    Some("center" | "right" | "justify") => value,
                    _ => None,
                };
                self.style_paragraphs("text-align", align);
            }
            "paragraph.lineHeight" => {
                let height = params.get("value").and_then(|v| {
                    v.as_f64()
                        .map(|n| n.to_string())
                        .or_else(|| v.as_str().map(str::to_string))
                });
                self.style_paragraphs("line-height", height.as_deref());
            }
            "paragraph.indent" => {
                let on = params.get("value").and_then(Value::as_bool).unwrap_or(true);
                self.style_paragraphs("text-indent", on.then_some("2em"));
            }
            "paragraph.style" => {
                let tag = match value {
                    Some(t @ ("p" | "h1" | "h2" | "h3" | "h4" | "blockquote")) => t,
                    _ => return Some(Err("paragraph.style takes p, h1–h4 or blockquote".into())),
                };
                self.set_paragraph_kind(tag);
            }
            "edit.insertText" => {
                let text = params.get("text").and_then(Value::as_str).unwrap_or("");
                self.type_text(text);
            }
            "edit.paste" => {
                let html = params.get("html").and_then(Value::as_str);
                let text = params.get("text").and_then(Value::as_str).unwrap_or("");
                self.paste(html, text);
            }
            "edit.copy" => {
                let copied = self.copy_selection();
                return Some(Ok(match copied {
                    Some((html, text)) => json!({ "html": html, "text": text }),
                    None => json!({}),
                }));
            }
            _ => return None,
        }
        Some(Ok(json!({})))
    }
}

fn common_prefix(a: &str, b: &str) -> usize {
    let mut n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    while !a.is_char_boundary(n) || !b.is_char_boundary(n) {
        n -= 1;
    }
    n
}

fn common_suffix(a: &str, b: &str) -> usize {
    let mut n = a
        .bytes()
        .rev()
        .zip(b.bytes().rev())
        .take_while(|(x, y)| x == y)
        .count();
    while !a.is_char_boundary(a.len() - n) || !b.is_char_boundary(b.len() - n) {
        n -= 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(html: &str) -> RichApp {
        RichApp::from_bytes("a.html", html.as_bytes()).unwrap()
    }

    fn caret_in(app: &mut RichApp, needle: &str, offset: usize) {
        let doc = app.document();
        let node = doc
            .descendants(ROOT)
            .into_iter()
            .find(|&n| doc.text(n).is_some_and(|t| t.contains(needle)))
            .unwrap();
        let base = doc.text(node).unwrap().find(needle).unwrap();
        app.set_selection(Some(Selection::caret(Pos::new(node, base + offset))));
    }

    #[test]
    fn typing_changes_the_file_and_undoes() {
        let mut app = app("<p>ab</p>\n");
        caret_in(&mut app, "ab", 1);
        app.type_text("X");
        app.type_text("Y");
        assert_eq!(app.text(), "<p>aXYb</p>\n");
        assert!(app.dirty());
        app.enter(false);
        assert_eq!(app.text(), "<p>aXY</p><p>b</p>\n");
        app.delete(false);
        assert_eq!(app.text(), "<p>aXYb</p>\n");
        app.command("edit.undo", json!({})).unwrap();
        assert_eq!(app.text(), "<p>aXY</p><p>b</p>\n");
        // The caret survives the reparse and typing goes on.
        app.type_text("Z");
        assert!(app.text().contains("Z"), "{}", app.text());
    }

    #[test]
    fn formatting_a_selection() {
        let mut app = app("<p>abcd</p>");
        caret_in(&mut app, "abcd", 1);
        let start = app.selection().unwrap().focus;
        app.set_selection(Some(Selection {
            anchor: start,
            focus: Pos::new(start.node, 3),
        }));
        app.command("format.bold", json!({})).unwrap();
        assert_eq!(
            app.text(),
            "<p>a<span style=\"font-weight: bold\">bc</span>d</p>"
        );
        assert!(app.is_on(Toggle::Bold));
        app.command("format.bold", json!({})).unwrap();
        assert_eq!(app.text(), "<p>abcd</p>");
        app.command("paragraph.align", json!({"value": "center"}))
            .unwrap();
        assert_eq!(app.text(), "<p style=\"text-align: center\">abcd</p>");
        app.command("paragraph.style", json!({"value": "h2"}))
            .unwrap();
        assert_eq!(app.text(), "<h2 style=\"text-align: center\">abcd</h2>");
    }

    #[test]
    fn bold_inside_bold_text_turns_off() {
        let mut app = app("<p><b>abcd</b></p>");
        caret_in(&mut app, "abcd", 0);
        let start = app.selection().unwrap().focus;
        app.set_selection(Some(Selection {
            anchor: start,
            focus: Pos::new(start.node, 2),
        }));
        assert!(app.is_on(Toggle::Bold));
        app.toggle(Toggle::Bold);
        assert!(!app.is_on(Toggle::Bold));
        assert_eq!(
            app.text(),
            "<p><b><span style=\"font-weight: normal\">ab</span>cd</b></p>"
        );
    }

    #[test]
    fn copy_and_paste() {
        let mut app = app("<p>a<b>bc</b></p><p>x</p>");
        caret_in(&mut app, "bc", 0);
        let start = app.selection().unwrap().focus;
        app.set_selection(Some(Selection {
            anchor: start,
            focus: Pos::new(start.node, 2),
        }));
        let (html, text) = app.copy_selection().unwrap();
        assert_eq!(html, "<b>bc</b>");
        assert_eq!(text, "bc");
        caret_in(&mut app, "x", 1);
        app.paste(Some(&html), &text);
        assert_eq!(app.text(), "<p>a<b>bc</b></p><p>x<b>bc</b></p>");
        app.paste(None, "1\n2");
        // Typing after pasted bold text stays bold, as it would after typing it.
        assert_eq!(app.text(), "<p>a<b>bc</b></p><p>x<b>bc1</b></p><p>2</p>");
    }

    #[test]
    fn select_all_and_type() {
        let mut app = app("<body><h1>T</h1><p>body</p></body>");
        app.select_all();
        app.type_text("new");
        assert_eq!(app.text(), "<body><h1>new</h1></body>");
    }
}
