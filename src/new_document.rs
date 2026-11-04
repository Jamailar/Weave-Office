//! The title-bar New button and its anchored format picker.
use crate::{icons, language::tr};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, vec2};
use weave_workspace::DocumentKind;

pub type Anchor = (Rect, egui::LayerId, egui::Id);

pub fn button(ui: &mut egui::Ui, open: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(42.0, 28.0), egui::Sense::click());
    let visuals = ui.visuals();
    let fill = if open || response.hovered() || response.has_focus() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    let ink = visuals.text_color();
    let stroke = Stroke::new(1.0, visuals.window_stroke.color);
    ui.painter().rect_filled(rect.shrink(0.5), 5, fill);
    ui.painter()
        .rect_stroke(rect.shrink(0.5), 5, stroke, egui::StrokeKind::Inside);
    let center = Pos2::new(rect.left() + 14.0, rect.center().y);
    ui.painter().line_segment(
        [center - vec2(6.0, 0.0), center + vec2(6.0, 0.0)],
        Stroke::new(1.4, ink),
    );
    ui.painter().line_segment(
        [center - vec2(0.0, 6.0), center + vec2(0.0, 6.0)],
        Stroke::new(1.4, ink),
    );
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + 28.0, rect.top() + 1.0),
            Pos2::new(rect.left() + 28.0, rect.bottom() - 1.0),
        ],
        stroke,
    );
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.right() - 7.0, rect.center().y),
            vec2(10.0, 10.0),
        ),
        icons::Icon::ChevronDown,
        ink,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tr("新建文件"))
    });
    response.on_hover_text(tr("新建文件"))
}

/// One tile of the picker: a document kind, and for code and data files the extension that
/// picks the new file's language and its default file name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub kind: DocumentKind,
    pub label: &'static str,
    pub extension: Option<&'static str>,
}

const fn office(kind: DocumentKind, label: &'static str) -> Choice {
    Choice {
        kind,
        label,
        extension: None,
    }
}

const fn code(label: &'static str, extension: &'static str) -> Choice {
    Choice {
        kind: DocumentKind::PlainText,
        label,
        extension: Some(extension),
    }
}

/// Tiles per row of the picker.
const COLUMNS: usize = 6;
const TILE_WIDTH: f32 = 73.5;
const TILE_GAP: f32 = 6.0;

/// The picker's groups, in order. Only formats an editor can also save are listed.
/// Labels are Chinese source strings, translated when drawn.
pub fn groups() -> Vec<(&'static str, Vec<Choice>)> {
    vec![
        (
            tr("Office 文档"),
            vec![
                office(DocumentKind::Text, "文字"),
                office(DocumentKind::Presentation, "演示"),
                office(DocumentKind::Spreadsheet, "表格"),
                office(DocumentKind::Pdf, "PDF"),
            ],
        ),
        (
            tr("文本与网页"),
            vec![
                office(DocumentKind::Markdown, "Markdown"),
                office(DocumentKind::PlainText, "纯文本"),
                office(DocumentKind::RichText, "富文本"),
            ],
        ),
        (
            tr("代码与数据"),
            vec![
                code("JSON", "json"),
                code("YAML", "yaml"),
                code("TOML", "toml"),
                code("XML", "xml"),
                code("SQL", "sql"),
                code("Shell", "sh"),
                code("Python", "py"),
                code("JavaScript", "js"),
                code("TypeScript", "ts"),
                code("CSS", "css"),
                code("Go", "go"),
                code("Rust", "rs"),
            ],
        ),
    ]
}

fn tile(ui: &mut egui::Ui, entry: &Choice) -> bool {
    let label = tr(entry.label);
    let (rect, response) = ui.allocate_exact_size(vec2(TILE_WIDTH, 82.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, 6, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            6,
            Stroke::new(1.0, ui.visuals().selection.bg_fill),
            egui::StrokeKind::Inside,
        );
    }
    let icon = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.top() + 26.0),
        vec2(40.0, 40.0),
    );
    match entry.extension {
        Some(extension) => icons::text_document(ui.painter(), icon, extension),
        None => icons::document(ui.painter(), icon, entry.kind),
    }
    ui.painter().text(
        Pos2::new(rect.center().x, rect.top() + 64.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.5),
        ui.visuals().text_color(),
    );
    response.clicked()
}

pub fn show(ctx: &egui::Context, anchor: Anchor, open: &mut bool) -> Option<Choice> {
    let mut selected = None;
    let visuals = &crate::theme::style(ctx).visuals;
    let frame = egui::Frame::new()
        .fill(if visuals.dark_mode {
            visuals.window_fill
        } else {
            Color32::from_rgb(241, 243, 244)
        })
        .stroke(visuals.window_stroke)
        .corner_radius(10)
        .inner_margin(24)
        .shadow(egui::epaint::Shadow {
            offset: [0, 4],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(35),
        });
    egui::Popup::new(
        egui::Id::unique("weave-new-formats"),
        ctx.clone(),
        anchor.0,
        anchor.1,
    )
    .anchor_widget(anchor.2)
    .accessible_name(tr("新建"))
    .gap(7.0)
    .open_bool(open)
    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
    .frame(frame)
    .show(|ui| {
        ui.set_style(crate::theme::style(ctx));
        ui.set_width(COLUMNS as f32 * TILE_WIDTH + (COLUMNS - 1) as f32 * TILE_GAP);
        ui.label(egui::RichText::new(tr("新建")).size(22.0).strong());
        ui.add_space(20.0);
        for (index, (title, entries)) in groups().into_iter().enumerate() {
            if index > 0 {
                ui.add_space(14.0);
            }
            ui.label(egui::RichText::new(title).size(13.0).strong());
            ui.add_space(12.0);
            for row in entries.chunks(COLUMNS) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = TILE_GAP;
                    for entry in row {
                        if tile(ui, entry) {
                            selected = Some(*entry);
                            ui.close();
                        }
                    }
                });
            }
        }
    });
    selected
}
