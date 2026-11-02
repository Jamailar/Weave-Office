//! The design view: the document laid out on a page, painted, and edited in place.

use crate::dom::Document;
use crate::edit::{self, Pos};
use crate::layout::{self, Families, Item, Page};
use crate::style::Border;
use crate::{Canvas, RichApp, Selection, caret};
use egui::epaint::{CornerRadius, RectShape, Shadow};
use egui::{Color32, FontFamily, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

/// Width of the phone canvas, in points (a common phone's CSS width).
pub const PHONE_WIDTH: f32 = 375.0;
const MARGIN: f32 = 24.0;

#[derive(Default)]
pub struct Cache {
    /// The text the document was parsed from (or serialized from, after an edit here).
    pub(crate) text: String,
    pub(crate) doc: Option<Document>,
    pub(crate) has_scripts: bool,
    pub(crate) selection: Option<crate::Selection>,
    /// The input time of the current frame, for grouping typing into undo steps.
    pub(crate) time: f64,
    /// The page and the width it was laid out at.
    pub(crate) page: Option<(f32, Page)>,
    /// Computed styles of the document's nodes.
    pub(crate) styles: Option<Vec<crate::style::Style>>,
    pixels_per_point: f32,
    /// Characters on the page (for the status bar).
    characters: usize,
    /// Where vertical caret moves keep to, in page coordinates.
    goal_x: Option<f32>,
    /// Text the input method is composing at the caret.
    preedit: String,
    focused: bool,
    ime_generation: u64,
    /// Give the page the keyboard again (after a ribbon click).
    pub(crate) refocus: bool,
    /// Scroll the caret into view after the next layout.
    reveal: bool,
}

pub fn show(app: &mut RichApp, ui: &mut Ui) {
    weave_text_editor::ensure_fonts(ui.ctx());
    app.cache.time = ui.input(|i| i.time);
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
    app.sync_document();
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
                let count = app.cache.characters;
                let words = if app.zh {
                    format!("{count} 字")
                } else {
                    format!("{count} characters")
                };
                ui.label(egui::RichText::new(words).small().weak());
                if app.cache.has_scripts {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(
                            app.tr("页面使用了脚本或样式表,设计视图只按行内样式显示"),
                        )
                        .small()
                        .weak(),
                    );
                }
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

/// Lay the page out again if the document, the width or the scale changed.
fn ensure_page(app: &mut RichApp, ctx: &egui::Context, width: f32) {
    let ppp = ctx.pixels_per_point();
    let stale = match &app.cache.page {
        Some((w, _)) => *w != width || app.cache.pixels_per_point != ppp,
        None => true,
    };
    if stale && let Some(doc) = &app.cache.doc {
        let families = families(ctx);
        let page = layout::layout(doc, width, ctx, &families, &mut app.images);
        app.cache.characters = page
            .items
            .iter()
            .map(|item| match item {
                Item::Text { galley, map, .. } if !map.is_empty() => galley
                    .job
                    .text
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .count(),
                _ => 0,
            })
            .sum();
        app.cache.page = Some((width, page));
        app.cache.pixels_per_point = ppp;
    }
}

fn canvas(app: &mut RichApp, ui: &mut Ui) {
    let available = ui.available_width();
    let width = match app.canvas {
        Canvas::Phone => PHONE_WIDTH,
        Canvas::Desktop => (available - MARGIN * 2.0).clamp(320.0, 1200.0),
    }
    .round();
    ensure_page(app, ui.ctx(), width);
    let Some((_, page)) = &app.cache.page else {
        return;
    };
    let size = page.size;
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let total = vec2(
                (size.x + MARGIN * 2.0).max(ui.available_width()),
                size.y + MARGIN * 2.0,
            );
            let (rect, response) = ui.allocate_exact_size(total, Sense::click_and_drag());
            let left = if size.x + MARGIN * 2.0 <= rect.width() {
                rect.center().x - size.x / 2.0
            } else {
                rect.min.x + MARGIN
            };
            let origin = pos2(left.round(), rect.min.y + MARGIN);
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            }
            pointer(app, ui, &response, origin);
            if std::mem::take(&mut app.cache.refocus) {
                response.request_focus();
            }
            let focused = response.has_focus();
            if focused && !app.cache.focused {
                app.cache.ime_generation = app.cache.ime_generation.wrapping_add(1).max(1);
            }
            app.cache.focused = focused;
            if focused {
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        response.id,
                        egui::EventFilter {
                            tab: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            escape: false,
                        },
                    )
                });
                keyboard(app, ui);
            }
            app.sync_document();
            ensure_page(app, ui.ctx(), width);
            let Some((_, page)) = app.cache.page.take() else {
                return;
            };
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
            paint_selection(app, ui, &painter, &page, origin, focused, response.id);
            app.cache.page = Some((width, page));
        });
}

/// Clicks place the caret, drags select, Shift+click extends, double and triple clicks select
/// a word and a paragraph.
fn pointer(app: &mut RichApp, ui: &Ui, response: &egui::Response, origin: Pos2) {
    let Some(point) = response.interact_pointer_pos() else {
        return;
    };
    let Some((_, page)) = &app.cache.page else {
        return;
    };
    let Some(doc) = &app.cache.doc else {
        return;
    };
    let Some(pos) = caret::hit(page, point - origin.to_vec2()).and_then(|s| caret::pos_of(page, s))
    else {
        return;
    };
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let shift = ui.input(|i| i.modifiers.shift);
    if response.triple_clicked() {
        let block = edit::block_of(doc, pos);
        let mut doc = doc.clone();
        let start = edit::start_of(&mut doc, block);
        let end = edit::end_of(&doc, block);
        app.cache.selection = Some(Selection {
            anchor: start,
            focus: end,
        });
    } else if response.double_clicked() {
        app.cache.selection = Some(word_at(doc, pos));
    } else if pressed {
        response.request_focus();
        app.cache.preedit.clear();
        app.cache.selection = Some(match (shift, app.cache.selection) {
            (true, Some(sel)) => Selection {
                anchor: sel.anchor,
                focus: pos,
            },
            _ => Selection::caret(pos),
        });
        app.cache.goal_x = None;
    } else if response.dragged()
        && let Some(sel) = app.cache.selection
    {
        app.cache.selection = Some(Selection {
            anchor: sel.anchor,
            focus: pos,
        });
    }
}

/// The word (a run of letters and digits, or of CJK characters) around a position.
fn word_at(doc: &Document, pos: Pos) -> Selection {
    let Some(text) = doc.text(pos.node) else {
        return Selection::caret(pos);
    };
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = pos.offset;
    while let Some(c) = text[..start].chars().next_back()
        && is_word(c)
    {
        start -= c.len_utf8();
    }
    let mut end = pos.offset;
    while let Some(c) = text[end..].chars().next()
        && is_word(c)
    {
        end += c.len_utf8();
    }
    if start == end
        && let Some(c) = text[end..].chars().next()
    {
        end += c.len_utf8();
    }
    Selection {
        anchor: Pos::new(pos.node, start),
        focus: Pos::new(pos.node, end),
    }
}

#[derive(Clone, Copy)]
enum Move {
    Char(bool),
    Line(bool),
    LineEdge(bool),
    DocumentEdge(bool),
}

fn keyboard(app: &mut RichApp, ui: &Ui) {
    let events = ui.input(|i| i.events.clone());
    for event in events {
        match event {
            egui::Event::Text(text) if app.cache.preedit.is_empty() => {
                app.type_text(&text);
                app.cache.reveal = true;
            }
            egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                app.cache.preedit = text;
            }
            egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                app.cache.preedit.clear();
                if text == "\n" || text == "\r" {
                    continue;
                }
                app.type_text(&text);
                app.cache.reveal = true;
            }
            egui::Event::Copy => {
                if let Some(text) = app.copy_to_clipboard() {
                    ui.ctx().copy_text(text);
                }
            }
            egui::Event::Cut if app.selection().is_some_and(|s| !s.is_empty()) => {
                if let Some(text) = app.copy_to_clipboard() {
                    ui.ctx().copy_text(text);
                }
                app.delete(false);
                app.cache.reveal = true;
            }
            egui::Event::Paste(text) => {
                app.paste_from_clipboard(&text);
                app.cache.reveal = true;
            }
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if !app.cache.preedit.is_empty() {
                    continue;
                }
                key_press(app, key, modifiers);
            }
            _ => {}
        }
    }
}

fn key_press(app: &mut RichApp, key: egui::Key, modifiers: egui::Modifiers) {
    use egui::Key;
    let command = modifiers.command;
    let shift = modifiers.shift;
    // Word-wise moves (Alt on macOS, Ctrl elsewhere) move by character for now.
    let line_edge = if cfg!(target_os = "macos") {
        modifiers.mac_cmd
    } else {
        false
    };
    let run = |app: &mut RichApp, id: &str| {
        let _ = app.command(id, serde_json::json!({}));
    };
    match key {
        Key::Backspace => app.delete(false),
        Key::Delete => app.delete(true),
        Key::Enter => app.enter(shift),
        Key::Tab => app.type_text("\u{a0}\u{a0}\u{a0}\u{a0}"),
        Key::Escape => {
            if let Some(sel) = app.cache.selection {
                app.cache.selection = Some(Selection::caret(sel.focus));
            }
            return;
        }
        Key::A if command => {
            app.select_all();
            return;
        }
        Key::B if command => run(app, "format.bold"),
        Key::I if command => run(app, "format.italic"),
        Key::U if command => run(app, "format.underline"),
        Key::Z if command && shift => run(app, "edit.redo"),
        Key::Y if command => run(app, "edit.redo"),
        Key::Z if command => run(app, "edit.undo"),
        Key::ArrowLeft if line_edge => move_caret(app, Move::LineEdge(false), shift),
        Key::ArrowRight if line_edge => move_caret(app, Move::LineEdge(true), shift),
        Key::ArrowUp if command => move_caret(app, Move::DocumentEdge(false), shift),
        Key::ArrowDown if command => move_caret(app, Move::DocumentEdge(true), shift),
        Key::Home if command => move_caret(app, Move::DocumentEdge(false), shift),
        Key::End if command => move_caret(app, Move::DocumentEdge(true), shift),
        Key::ArrowLeft => move_caret(app, Move::Char(false), shift),
        Key::ArrowRight => move_caret(app, Move::Char(true), shift),
        Key::ArrowUp => move_caret(app, Move::Line(false), shift),
        Key::ArrowDown => move_caret(app, Move::Line(true), shift),
        Key::Home => move_caret(app, Move::LineEdge(false), shift),
        Key::End => move_caret(app, Move::LineEdge(true), shift),
        _ => return,
    }
    app.cache.reveal = true;
}

fn move_caret(app: &mut RichApp, how: Move, extend: bool) {
    app.sync_document();
    let Some((_, page)) = &app.cache.page else {
        return;
    };
    let Some(doc) = &app.cache.doc else {
        return;
    };
    let Some(sel) = app.cache.selection else {
        return;
    };
    // Left or Right with a selection goes to its start or end.
    if let Move::Char(forward) = how
        && !extend
        && !sel.is_empty()
    {
        let (start, end) = edit::ordered(doc, sel.anchor, sel.focus);
        app.cache.selection = Some(Selection::caret(if forward { end } else { start }));
        app.cache.goal_x = None;
        return;
    }
    let target = match how {
        Move::DocumentEdge(end) => {
            let mut copy = doc.clone();
            let body = copy.body();
            if end {
                Some(edit::end_of(&copy, body))
            } else {
                Some(edit::start_of(&mut copy, body))
            }
        }
        _ => {
            let Some(spot) = caret::spot_of(page, doc, sel.focus) else {
                return;
            };
            let moved = match how {
                Move::Char(forward) => caret::step(page, spot, forward),
                Move::Line(down) => {
                    let x = app.cache.goal_x.unwrap_or_else(|| {
                        caret::caret_rect(page, spot).map_or(0.0, |r| r.center().x)
                    });
                    app.cache.goal_x = Some(x);
                    caret::line(page, spot, down, x)
                }
                Move::LineEdge(end) => caret::line_edge(page, spot, end),
                Move::DocumentEdge(_) => spot,
            };
            caret::pos_of(page, moved)
        }
    };
    if !matches!(how, Move::Line(_)) {
        app.cache.goal_x = None;
    }
    let Some(pos) = target else {
        return;
    };
    app.cache.selection = Some(if extend {
        Selection {
            anchor: sel.anchor,
            focus: pos,
        }
    } else {
        Selection::caret(pos)
    });
}

/// The selection's highlight, the caret, the input method's text and its position.
fn paint_selection(
    app: &mut RichApp,
    ui: &Ui,
    painter: &egui::Painter,
    page: &Page,
    origin: Pos2,
    focused: bool,
    id: egui::Id,
) {
    let (Some(sel), Some(doc)) = (app.cache.selection, &app.cache.doc) else {
        return;
    };
    let offset = origin.to_vec2();
    let highlight = if focused {
        Color32::from_rgba_unmultiplied(40, 120, 255, 70)
    } else {
        Color32::from_rgba_unmultiplied(120, 120, 120, 50)
    };
    for (item, range) in caret::selected(page, doc, sel.anchor, sel.focus) {
        for rect in caret::range_rects(page, item, range) {
            painter.rect_filled(rect.translate(offset), 0.0, highlight);
        }
    }
    let Some(spot) = caret::spot_of(page, doc, sel.focus) else {
        return;
    };
    let Some(caret) = caret::caret_rect(page, spot) else {
        return;
    };
    let caret = caret.translate(offset);
    if std::mem::take(&mut app.cache.reveal) {
        ui.scroll_to_rect(caret.expand(24.0), None);
    }
    if !focused {
        return;
    }
    let mut caret_rect = caret;
    if !app.cache.preedit.is_empty() {
        // The composition shows at the caret, underlined, over the text after it.
        let height = caret.height().max(12.0);
        let galley = painter.layout_no_wrap(
            app.cache.preedit.clone(),
            egui::FontId::proportional((height * 0.75).clamp(12.0, 32.0)),
            Color32::from_gray(20),
        );
        let at = pos2(caret.min.x, caret.max.y - galley.size().y);
        let rect = Rect::from_min_size(at, galley.size());
        painter.rect_filled(rect.expand(1.0), 2.0, Color32::from_rgb(255, 255, 240));
        painter.galley(at, galley, Color32::from_gray(20));
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, Color32::from_gray(60)),
        );
        caret_rect =
            Rect::from_min_max(pos2(rect.max.x, caret.min.y), pos2(rect.max.x, caret.max.y));
    } else {
        painter.line_segment(
            [caret.center_top(), caret.center_bottom()],
            Stroke::new(1.5, Color32::from_rgb(20, 20, 20)),
        );
    }
    let to_global = ui
        .ctx()
        .layer_transform_to_global(ui.layer_id())
        .unwrap_or_default();
    let generation = app.cache.ime_generation.max(1);
    ui.ctx().output_mut(|o| {
        o.ime = Some(egui::output::IMEOutput::new(
            id,
            generation,
            egui::IMEPurpose::Normal,
            to_global * ui.clip_rect(),
            to_global * caret_rect,
        ));
    });
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
