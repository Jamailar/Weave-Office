//! The design view: the document laid out on a page and painted.

use crate::dom::Document;
use crate::layout::{self, Families, Item, Page};
use crate::style::Border;
use crate::{Canvas, RichApp};
use egui::epaint::{CornerRadius, RectShape, Shadow};
use egui::{Color32, FontFamily, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

/// Width of the phone canvas, in points (a common phone's CSS width).
pub const PHONE_WIDTH: f32 = 375.0;
const MARGIN: f32 = 24.0;

#[derive(Default)]
pub struct Cache {
    /// The text the document was parsed from.
    text: String,
    doc: Option<Document>,
    has_scripts: bool,
    /// The page and the width it was laid out at.
    page: Option<(f32, Page)>,
    pixels_per_point: f32,
}

pub fn show(app: &mut RichApp, ui: &mut Ui) {
    weave_text_editor::ensure_fonts(ui.ctx());
    if app.ribbon_tab.is_some() {
        egui::Panel::top("weave-rich-commands")
            .exact_size(36.0)
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ui, |ui| app.design_ribbon(ui));
    }
    refresh(app);
    egui::Panel::bottom("weave-rich-status")
        .exact_size(24.0)
        .frame(
            egui::Frame::NONE
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::symmetric(10, 2)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new(app.tr("设计视图")).small());
                ui.separator();
                let note = if app.cache.has_scripts {
                    app.tr("页面使用了脚本或样式表,设计视图只按行内样式显示")
                } else {
                    app.tr("设计视图暂不能直接编辑,请切换到源码修改")
                };
                ui.label(egui::RichText::new(note).small().weak());
            });
        });
    let surround = if ui.visuals().dark_mode {
        Color32::from_gray(30)
    } else {
        Color32::from_gray(236)
    };
    egui::CentralPanel::no_frame()
        .frame(egui::Frame::NONE.fill(surround))
        .show(ui, |ui| canvas(app, ui));
}

/// Parse the text again when it changed.
fn refresh(app: &mut RichApp) {
    if app.cache.doc.is_none() || app.cache.text != app.text() {
        let text = app.text().to_string();
        let doc = Document::parse(&text);
        app.cache.has_scripts = doc.has_scripts_or_stylesheets();
        app.cache.doc = Some(doc);
        app.cache.text = text;
        app.cache.page = None;
    }
}

fn families(ctx: &egui::Context) -> Families {
    let bold = FontFamily::Name(weave_text_editor::layout::BOLD_FAMILY.into());
    let mono = FontFamily::Name(weave_text_editor::layout::MONO_FAMILY.into());
    ctx.fonts(|fonts| {
        let defined = &fonts.definitions().families;
        Families {
            regular: FontFamily::Proportional,
            bold: if defined.contains_key(&bold) {
                bold
            } else {
                FontFamily::Proportional
            },
            mono: if defined.contains_key(&mono) {
                mono
            } else {
                FontFamily::Monospace
            },
        }
    })
}

fn canvas(app: &mut RichApp, ui: &mut Ui) {
    let available = ui.available_width();
    let width = match app.canvas {
        Canvas::Phone => PHONE_WIDTH,
        Canvas::Desktop => (available - MARGIN * 2.0).clamp(320.0, 1200.0),
    }
    .round();
    let ppp = ui.ctx().pixels_per_point();
    let stale = match &app.cache.page {
        Some((w, _)) => *w != width || app.cache.pixels_per_point != ppp,
        None => true,
    };
    if stale && let Some(doc) = &app.cache.doc {
        let families = families(ui.ctx());
        let page = layout::layout(doc, width, ui.ctx(), &families, &mut app.images);
        app.cache.page = Some((width, page));
        app.cache.pixels_per_point = ppp;
    }
    let Some((_, page)) = app.cache.page.take() else {
        return;
    };
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let total = vec2(
                (page.size.x + MARGIN * 2.0).max(ui.available_width()),
                page.size.y + MARGIN * 2.0,
            );
            let (rect, _) = ui.allocate_exact_size(total, Sense::hover());
            let left = if page.size.x + MARGIN * 2.0 <= rect.width() {
                rect.center().x - page.size.x / 2.0
            } else {
                rect.min.x + MARGIN
            };
            let origin = pos2(left.round(), rect.min.y + MARGIN);
            let sheet = Rect::from_min_size(origin, page.size);
            let painter = ui.painter_at(ui.clip_rect());
            painter.add(
                Shadow {
                    offset: [0, 2],
                    blur: 12,
                    spread: 0,
                    color: Color32::from_black_alpha(40),
                }
                .as_shape(sheet, CornerRadius::same(2)),
            );
            painter.rect_filled(sheet, 2.0, page.background.unwrap_or(Color32::WHITE));
            let visible = ui.clip_rect();
            for item in &page.items {
                let moved = item.rect().translate(origin.to_vec2());
                if !moved.intersects(visible) && moved.is_finite() {
                    continue;
                }
                paint(app, ui, &painter, item, origin.to_vec2());
            }
        });
    app.cache.page = Some((width, page));
}

fn paint(app: &mut RichApp, ui: &Ui, painter: &egui::Painter, item: &Item, offset: Vec2) {
    match item {
        Item::Box {
            rect,
            fill,
            border,
            radius,
            ..
        } => {
            if !rect.is_finite() {
                return;
            }
            let rect = rect.translate(offset);
            let radius = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
            if let Some(fill) = fill {
                painter.rect_filled(rect, radius, *fill);
            }
            paint_border(painter, rect, border, radius);
        }
        Item::Text { pos, galley, .. } => {
            painter.galley(*pos + offset, galley.clone(), Color32::BLACK);
        }
        Item::Image {
            rect,
            src,
            alt,
            radius,
            ..
        } => {
            let rect = rect.translate(offset);
            match app.images.texture(ui.ctx(), src) {
                Some(texture) => {
                    let shape = RectShape::filled(rect, *radius, Color32::WHITE)
                        .with_texture(texture.id(), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
                    painter.add(shape);
                }
                None => {
                    painter.rect_filled(rect, *radius, Color32::from_gray(232));
                    painter.rect_stroke(
                        rect,
                        *radius,
                        Stroke::new(1.0, Color32::from_gray(205)),
                        StrokeKind::Inside,
                    );
                    let label = if alt.is_empty() { "🖼" } else { alt.as_str() };
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(13.0),
                        Color32::from_gray(120),
                    );
                }
            }
        }
    }
}

fn paint_border(painter: &egui::Painter, rect: Rect, border: &[Border; 4], radius: f32) {
    if border.iter().all(|b| b.width == 0.0) {
        return;
    }
    let uniform = border.iter().all(|b| *b == border[0]);
    if uniform && !border[0].dashed {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(border[0].width, border[0].color),
            StrokeKind::Inside,
        );
        return;
    }
    // Sides drawn one by one: top, right, bottom, left.
    let [top, right, bottom, left] = *border;
    let sides = [
        (
            top,
            Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + top.width)),
        ),
        (
            right,
            Rect::from_min_max(pos2(rect.max.x - right.width, rect.min.y), rect.max),
        ),
        (
            bottom,
            Rect::from_min_max(pos2(rect.min.x, rect.max.y - bottom.width), rect.max),
        ),
        (
            left,
            Rect::from_min_max(rect.min, pos2(rect.min.x + left.width, rect.max.y)),
        ),
    ];
    for (side, strip) in sides {
        if side.width <= 0.0 {
            continue;
        }
        if side.dashed {
            let horizontal = strip.width() >= strip.height();
            let (a, b) = if horizontal {
                (
                    pos2(strip.min.x, strip.center().y),
                    pos2(strip.max.x, strip.center().y),
                )
            } else {
                (
                    pos2(strip.center().x, strip.min.y),
                    pos2(strip.center().x, strip.max.y),
                )
            };
            let dash = (side.width * 3.0).max(3.0);
            painter.extend(egui::Shape::dashed_line(
                &[a, b],
                Stroke::new(side.width, side.color),
                dash,
                dash,
            ));
        } else {
            painter.rect_filled(strip, 0.0, side.color);
        }
    }
}
