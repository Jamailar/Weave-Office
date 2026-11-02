//! The rich-text editor Weave Office hosts: HTML documents laid out the way WeChat article
//! editors (135 编辑器 and the like) lay them out, and any HTML file.
//!
//! A [`RichApp`] shows one HTML file in a design view (the laid-out page) or a source view
//! (the HTML, edited in the text editor's HTML mode). The file's text is the document: the
//! design view parses and lays it out, so both views always agree, and a file nobody edited
//! is saved byte for byte as it was read.

pub mod caret;
pub mod dom;
pub mod edit;
mod editing;
mod i18n;
pub mod images;
pub mod layout;
pub mod style;
mod view;

pub use editing::{ClipboardHooks, Selection, Toggle};
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
    pub(crate) clipboard: Option<ClipboardHooks>,
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
            ribbon_tab: Some("home".into()),
            host_commands: Vec::new(),
            cache: view::Cache::default(),
            images: images::Images::default(),
            clipboard: None,
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

    /// Reach the host's clipboard for rich copy and paste.
    pub fn set_clipboard(&mut self, hooks: ClipboardHooks) {
        self.clipboard = Some(hooks);
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
                self.ribbon_tab = Some("home".into());
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
            _ => {
                if self.view == View::Design
                    && let Some(result) = self.design_command(id, &params)
                {
                    return result;
                }
                return self.source.command(id, params);
            }
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

    /// The design view's ribbon: text and paragraph formatting, and the page width.
    pub(crate) fn design_ribbon(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            if self.ribbon_tab.as_deref() == Some("view") {
                let phone = self.canvas == Canvas::Phone;
                if ui.selectable_label(phone, self.tr("手机宽度")).clicked() {
                    self.canvas = Canvas::Phone;
                }
                if ui.selectable_label(!phone, self.tr("桌面宽度")).clicked() {
                    self.canvas = Canvas::Desktop;
                }
                return;
            }
            self.home_ribbon(ui);
        });
    }

    fn home_ribbon(&mut self, ui: &mut egui::Ui) {
        let mut run: Option<(&str, Value)> = None;
        let kind = self.paragraph_kind().unwrap_or_else(|| "p".into());
        let kinds = [
            ("p", "正文"),
            ("h1", "标题 1"),
            ("h2", "标题 2"),
            ("h3", "标题 3"),
            ("blockquote", "引用"),
        ];
        let current = kinds
            .iter()
            .find(|(tag, _)| *tag == kind)
            .map_or("正文", |(_, label)| label);
        egui::ComboBox::from_id_salt("weave-rich-paragraph")
            .width(72.0)
            .selected_text(self.tr(current))
            .show_ui(ui, |ui| {
                for (tag, label) in kinds {
                    if ui.selectable_label(tag == kind, self.tr(label)).clicked() {
                        run = Some(("paragraph.style", json!({ "value": tag })));
                    }
                }
            });
        egui::ComboBox::from_id_salt("weave-rich-size")
            .width(52.0)
            .selected_text(self.tr("字号"))
            .show_ui(ui, |ui| {
                for size in [12, 13, 14, 15, 16, 17, 18, 20, 22, 24, 28, 32, 36] {
                    if ui.selectable_label(false, format!("{size}px")).clicked() {
                        run = Some(("format.fontSize", json!({ "value": size })));
                    }
                }
            });
        ui.separator();
        for (toggle, label, id, tip) in [
            (
                Toggle::Bold,
                egui::RichText::new("B").strong(),
                "format.bold",
                "加粗",
            ),
            (
                Toggle::Italic,
                egui::RichText::new("I").italics(),
                "format.italic",
                "斜体",
            ),
            (
                Toggle::Underline,
                egui::RichText::new("U").underline(),
                "format.underline",
                "下划线",
            ),
            (
                Toggle::Strike,
                egui::RichText::new("S").strikethrough(),
                "format.strikethrough",
                "删除线",
            ),
        ] {
            let on = self.is_on(toggle);
            if ui
                .selectable_label(on, label)
                .on_hover_text(self.tr(tip))
                .clicked()
            {
                run = Some((id, json!({})));
            }
        }
        for (label, id, none) in [
            ("文字颜色", "format.color", "默认颜色"),
            ("背景色", "format.highlight", "无背景"),
        ] {
            ui.menu_button(self.tr(label), |ui| {
                if ui.button(self.tr(none)).clicked() {
                    run = Some((id, json!({ "value": null })));
                    ui.close();
                }
                egui::Grid::new(("weave-rich-palette", id))
                    .spacing([4.0, 4.0])
                    .show(ui, |ui| {
                        for (i, hex) in PALETTE.iter().enumerate() {
                            let color = crate::style::color(hex).unwrap_or(egui::Color32::BLACK);
                            let (rect, response) = ui
                                .allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                            ui.painter().rect_filled(rect, 3.0, color);
                            ui.painter().rect_stroke(
                                rect,
                                3.0,
                                egui::Stroke::new(1.0, egui::Color32::from_gray(180)),
                                egui::StrokeKind::Inside,
                            );
                            if response.on_hover_text(*hex).clicked() {
                                run = Some((id, json!({ "value": hex })));
                                ui.close();
                            }
                            if i % 8 == 7 {
                                ui.end_row();
                            }
                        }
                    });
            });
        }
        if ui
            .button(self.tr("清除格式"))
            .on_hover_text(self.tr("去掉所选文字的字体、颜色和背景"))
            .clicked()
        {
            run = Some(("format.clear", json!({})));
        }
        ui.separator();
        let align = self.paragraph_style(|s| s.text_align);
        for (value, label, which) in [
            ("left", "左对齐", crate::style::Align::Left),
            ("center", "居中", crate::style::Align::Center),
            ("right", "右对齐", crate::style::Align::Right),
            ("justify", "两端对齐", crate::style::Align::Justify),
        ] {
            if ui
                .selectable_label(align == Some(which), self.tr(label))
                .clicked()
            {
                run = Some(("paragraph.align", json!({ "value": value })));
            }
        }
        egui::ComboBox::from_id_salt("weave-rich-line-height")
            .width(52.0)
            .selected_text(self.tr("行距"))
            .show_ui(ui, |ui| {
                for height in ["1.5", "1.75", "2", "2.5"] {
                    if ui.selectable_label(false, height).clicked() {
                        run = Some(("paragraph.lineHeight", json!({ "value": height })));
                    }
                }
            });
        let indented = self
            .paragraph_style(|s| {
                s.text_indent != crate::style::Length::Px(0.0)
                    && s.text_indent != crate::style::Length::Auto
            })
            .unwrap_or(false);
        if ui.selectable_label(indented, self.tr("首行缩进")).clicked() {
            run = Some(("paragraph.indent", json!({ "value": !indented })));
        }
        if let Some((id, params)) = run {
            let _ = self.command(id, params);
            self.cache.refocus = true;
        }
    }

    /// A computed style property of the paragraph at the caret.
    fn paragraph_style<T>(&mut self, get: impl Fn(&crate::style::Style) -> T) -> Option<T> {
        let sel = self.selection()?;
        self.styles();
        let doc = self.cache.doc.as_ref()?;
        let block = edit::block_of(doc, sel.focus);
        self.cache.styles.as_ref()?.get(block).map(get)
    }
}

/// Colors offered for text and backgrounds: greys, a rainbow, and WeChat's green and link blue.
const PALETTE: [&str; 16] = [
    "#333333", "#000000", "#888888", "#ffffff", "#e53935", "#fb8c00", "#fdd835", "#43a047",
    "#00897b", "#1e88e5", "#3949ab", "#8e24aa", "#d81b60", "#6d4c41", "#07c160", "#576b95",
];

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
                tabs: vec![
                    weave_ribbon::Tab::new("home", self.tr("开始")),
                    weave_ribbon::Tab::new("view", self.tr("视图")),
                ],
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
