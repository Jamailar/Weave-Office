//! The rich-text editor Weave Office hosts: HTML documents laid out the way WeChat article
//! editors (135 编辑器 and the like) lay them out, and any HTML file.
//!
//! A [`RichApp`] shows one HTML file in a design view (the laid-out page) or a source view
//! (the HTML, edited in the text editor's HTML mode). The file's text is the document: the
//! design view parses and lays it out, so both views always agree, and a file nobody edited
//! is saved byte for byte as it was read.

pub mod dom;
mod i18n;
pub mod images;
pub mod layout;
pub mod style;
mod view;

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use weave_text_editor::{Kind as TextKind, TextApp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The laid-out page.
    Design,
    /// The HTML source.
    Source,
}

/// How wide the design view lays the page out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Canvas {
    /// A phone screen, as WeChat articles are read.
    Phone,
    /// The window's width, like a browser.
    Desktop,
}

/// What a new document starts with.
pub const NEW_DOCUMENT: &str = "<!DOCTYPE html>
<html lang=\"zh-CN\">
<head>
<meta charset=\"utf-8\">
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
<title>未命名</title>
</head>
<body style=\"margin: 0;\">
<section style=\"max-width: 677px; margin: 0 auto; padding: 20px 16px; font-size: 15px; line-height: 1.75; color: #333333; letter-spacing: 0.5px;\">
<h1 style=\"font-size: 22px; line-height: 1.4; margin: 0 0 16px;\">标题</h1>
<p style=\"margin: 0 0 16px;\">在这里输入正文。</p>
</section>
</body>
</html>
";

pub struct RichApp {
    /// The file's text, saving, encodings and the source view.
    source: TextApp,
    view: View,
    canvas: Canvas,
    zh: bool,
    ribbon_tab: Option<String>,
    host_commands: Vec<(String, Value)>,
    pub(crate) cache: view::Cache,
    pub(crate) images: images::Images,
}

impl RichApp {
    /// A new document.
    pub fn new() -> Self {
        let source = TextApp::from_bytes(
            TextKind::PlainText,
            "untitled.html",
            NEW_DOCUMENT.as_bytes(),
        )
        .unwrap_or_else(|_| TextApp::new(TextKind::PlainText));
        Self::with_source(source)
    }

    fn with_source(source: TextApp) -> Self {
        let mut app = Self {
            source,
            view: View::Design,
            canvas: Canvas::Phone,
            zh: true,
            ribbon_tab: Some("view".into()),
            host_commands: Vec::new(),
            cache: view::Cache::default(),
            images: images::Images::default(),
        };
        app.update_base();
        app
    }

    pub fn from_bytes(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let name = if name.is_empty() {
            "untitled.html"
        } else {
            name
        };
        Ok(Self::with_source(TextApp::from_bytes(
            TextKind::PlainText,
            name,
            bytes,
        )?))
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        Ok(Self::with_source(TextApp::open(TextKind::PlainText, path)?))
    }

    fn update_base(&mut self) {
        let base = self
            .source
            .path()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        self.images.set_base(base);
    }

    pub fn text(&self) -> &str {
        self.source.text()
    }

    /// Change the HTML text (one undoable step).
    pub fn apply(&mut self, edit: weave_text_editor::edit::Edit) {
        self.source.apply(edit);
    }

    pub fn path(&self) -> Option<&Path> {
        self.source.path()
    }

    pub fn set_path(&mut self, path: Option<PathBuf>) {
        self.source.set_path(path);
        self.update_base();
    }

    pub fn mark_unsaved(&mut self) {
        self.source.mark_unsaved();
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn set_language_code(&mut self, code: &str) {
        self.source.set_language_code(code);
        self.zh = self.source.interface_zh();
    }

    pub fn interface_zh(&self) -> bool {
        self.zh
    }

    fn tr<'a>(&self, text: &'a str) -> &'a str {
        i18n::tr(self.zh, text)
    }

    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        self.source.bytes()
    }

    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        self.source.save(path)?;
        self.update_base();
        Ok(())
    }

    pub fn dirty(&self) -> bool {
        self.source.dirty()
    }

    pub fn take_host_commands(&mut self) -> Vec<(String, Value)> {
        let mut commands = std::mem::take(&mut self.host_commands);
        commands.extend(self.source.take_host_commands());
        commands
    }

    pub fn command(&mut self, id: &str, params: Value) -> Result<Value, String> {
        match id {
            "view.mode" => {
                self.view = match params.get("value").and_then(Value::as_str) {
                    Some("source") => View::Source,
                    Some("design") => View::Design,
                    _ => match self.view {
                        View::Design => View::Source,
                        View::Source => View::Design,
                    },
                };
                self.ribbon_tab = Some(match self.view {
                    View::Design => "view".into(),
                    View::Source => "home".into(),
                });
            }
            "view.canvas" => {
                self.canvas = match params.get("value").and_then(Value::as_str) {
                    Some("desktop") => Canvas::Desktop,
                    Some("phone") => Canvas::Phone,
                    _ => match self.canvas {
                        Canvas::Phone => Canvas::Desktop,
                        Canvas::Desktop => Canvas::Phone,
                    },
                };
            }
            _ => return self.source.command(id, params),
        }
        Ok(json!({}))
    }

    pub fn hosted_logic(&mut self, ctx: &egui::Context) {
        self.source.hosted_logic(ctx);
    }

    pub fn hosted_ui(&mut self, ui: &mut egui::Ui) {
        match self.view {
            View::Source => self.source.hosted_ui(ui),
            View::Design => view::show(self, ui),
        }
    }

    pub fn hosted_command(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.command(id, params)
    }

    pub fn hosted_flush(&mut self) -> Result<(), String> {
        self.source.hosted_flush()
    }

    pub fn hosted_dirty(&self) -> bool {
        self.dirty()
    }

    pub fn hosted_save(&mut self, path: &Path) -> Result<(), String> {
        self.save(path)
    }

    pub fn hosted_deactivate(&mut self, ctx: &egui::Context) -> Result<(), String> {
        self.source.hosted_deactivate(ctx)
    }

    pub fn hosted_blocks_host_navigation(&self) -> bool {
        self.view == View::Source && self.source.hosted_blocks_host_navigation()
    }

    /// The design view's ribbon: the view switch and canvas width.
    pub(crate) fn design_ribbon(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            let phone = self.canvas == Canvas::Phone;
            if ui.selectable_label(phone, self.tr("手机宽度")).clicked() {
                self.canvas = Canvas::Phone;
            }
            if ui.selectable_label(!phone, self.tr("桌面宽度")).clicked() {
                self.canvas = Canvas::Desktop;
            }
        });
    }
}

impl Default for RichApp {
    fn default() -> Self {
        Self::new()
    }
}

impl weave_ribbon::HostedRibbon for RichApp {
    fn ribbon_tabs(&self, ctx: &egui::Context) -> weave_ribbon::Tabs {
        match self.view {
            View::Source => weave_ribbon::HostedRibbon::ribbon_tabs(&self.source, ctx),
            View::Design => weave_ribbon::Tabs {
                tabs: vec![weave_ribbon::Tab::new("view", self.tr("视图"))],
                active: self.ribbon_tab.clone(),
            },
        }
    }

    fn ribbon_event(&mut self, event: weave_ribbon::Event) {
        if self.view == View::Source {
            weave_ribbon::HostedRibbon::ribbon_event(&mut self.source, event);
            return;
        }
        match event {
            weave_ribbon::Event::Clicked(id) => self.ribbon_tab = Some(id),
            weave_ribbon::Event::DoubleClicked(id) => {
                self.ribbon_tab = if self.ribbon_tab.as_deref() == Some(&id) {
                    None
                } else {
                    Some(id)
                }
            }
        }
    }

    fn undo_redo(&self) -> (bool, bool) {
        weave_ribbon::HostedRibbon::undo_redo(&self.source)
    }

    /// The design / source switch, at the right end of the tab row in both views.
    fn ribbon_trailing(&mut self, ui: &mut egui::Ui) {
        let source = self.view == View::Source;
        // Right to left: source last.
        if ui.selectable_label(source, self.tr("源码")).clicked() && !source {
            let _ = self.command("view.mode", json!({ "value": "source" }));
        }
        if ui.selectable_label(!source, self.tr("设计")).clicked() && source {
            let _ = self.command("view.mode", json!({ "value": "design" }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document_is_clean_html() {
        let app = RichApp::new();
        assert!(!app.dirty());
        assert!(app.text().starts_with("<!DOCTYPE html>"));
        let doc = dom::Document::parse(app.text());
        assert_eq!(doc.title().as_deref(), Some("未命名"));
    }

    #[test]
    fn opening_and_saving_keeps_bytes() {
        let bytes = "<p>你好\r\n<b>世界</b></p>\r\n".as_bytes();
        let app = RichApp::from_bytes("a.html", bytes).unwrap();
        assert!(!app.dirty());
        assert_eq!(app.bytes().unwrap(), bytes);
    }

    #[test]
    fn view_switch() {
        let mut app = RichApp::from_bytes("a.html", b"<p>x</p>").unwrap();
        assert_eq!(app.view(), View::Design);
        app.command("view.mode", json!({ "value": "source" }))
            .unwrap();
        assert_eq!(app.view(), View::Source);
        app.command("view.mode", json!({})).unwrap();
        assert_eq!(app.view(), View::Design);
        app.command("view.canvas", json!({ "value": "desktop" }))
            .unwrap();
        assert_eq!(app.canvas, Canvas::Desktop);
        // Text commands reach the source.
        app.command("edit.selectAll", json!({})).unwrap();
    }
}
