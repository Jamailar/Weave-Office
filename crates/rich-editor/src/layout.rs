//! Lays a document out for one page width: a display list of boxes, text and pictures.
//!
//! Block boxes stack with margins (adjacent sibling margins collapse), padding, borders,
//! backgrounds and rounded corners; `width`, `max-width` and `margin: auto` size and center
//! them. Inline content becomes one egui text job per run (fonts, colors, backgrounds,
//! decorations, letter spacing, line height, alignment, first-line indent). Pictures and
//! inline blocks are laid out as atoms on rows of their own, flowed left to right and aligned
//! like the text around them. Lists draw markers; tables get columns; `display: flex` lays its
//! children out in a row. Everything else falls back to normal flow.

use crate::dom::{Document, Kind, NodeId};
use crate::style::{self, Align, Border, Display, Justify, Length, ListStyle, Style, VAlign};
use egui::text::{LayoutJob, LayoutSection, TextFormat, TextWrapping};
use egui::{Color32, FontFamily, FontId, Galley, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use std::sync::Arc;

#[derive(Clone)]
pub enum Item {
    /// A box's background and borders.
    Box {
        node: NodeId,
        rect: Rect,
        fill: Option<Color32>,
        border: [Border; 4],
        radius: f32,
    },
    Text {
        /// The block whose inline content this is.
        node: NodeId,
        pos: Pos2,
        galley: Arc<Galley>,
    },
    Image {
        node: NodeId,
        rect: Rect,
        src: String,
        alt: String,
        radius: f32,
    },
}

impl Item {
    pub fn rect(&self) -> Rect {
        match self {
            Item::Box { rect, .. } | Item::Image { rect, .. } => *rect,
            Item::Text { pos, galley, .. } => galley.rect.translate(pos.to_vec2()),
        }
    }

    fn translate(&mut self, delta: Vec2) {
        match self {
            Item::Box { rect, .. } | Item::Image { rect, .. } => *rect = rect.translate(delta),
            Item::Text { pos, .. } => *pos += delta,
        }
    }
}

/// What the page looks like at one width.
#[derive(Clone, Default)]
pub struct Page {
    pub items: Vec<Item>,
    pub size: Vec2,
    /// The page's own background (`<body>` or `<html>`), if it sets one.
    pub background: Option<Color32>,
}

/// Sizes of pictures, by `src`.
pub trait ImageSizes {
    fn image_size(&mut self, src: &str) -> Option<Vec2>;
}

impl ImageSizes for () {
    fn image_size(&mut self, _src: &str) -> Option<Vec2> {
        None
    }
}

/// The font families text uses.
#[derive(Clone, Debug)]
pub struct Families {
    pub regular: FontFamily,
    pub bold: FontFamily,
    pub mono: FontFamily,
}

impl Default for Families {
    fn default() -> Self {
        Self {
            regular: FontFamily::Proportional,
            bold: FontFamily::Proportional,
            mono: FontFamily::Monospace,
        }
    }
}

/// Size of a picture whose size is unknown (a web address, a missing file).
const PLACEHOLDER: Vec2 = vec2(240.0, 135.0);

pub fn layout(
    doc: &Document,
    width: f32,
    ctx: &egui::Context,
    families: &Families,
    images: &mut dyn ImageSizes,
) -> Page {
    let styles = style::compute_all(doc);
    let mut cx = Cx {
        doc,
        styles: &styles,
        ctx,
        families,
        images,
        items: Vec::new(),
    };
    let body = doc.body();
    let height = if body == crate::dom::ROOT {
        cx.flow(body, 0.0, 0.0, width, true)
    } else {
        let result = cx.block(body, 0.0, 0.0, width, false);
        result.margin[0] + result.height + result.margin[2]
    };
    let background = styles[body]
        .background
        .or_else(|| doc.find("html").and_then(|html| styles[html].background));
    // Content wider than the page (a fixed-width table) widens it.
    let right = cx
        .items
        .iter()
        .map(|item| item.rect().max.x)
        .fold(width, f32::max);
    Page {
        items: cx.items,
        size: vec2(right, height.max(1.0)),
        background,
    }
}

struct Cx<'a> {
    doc: &'a Document,
    styles: &'a [Style],
    ctx: &'a egui::Context,
    families: &'a Families,
    images: &'a mut dyn ImageSizes,
    items: Vec<Item>,
}

/// A laid-out box: border-box size and resolved vertical margins.
#[derive(Clone, Copy, Debug)]
struct Laid {
    width: f32,
    height: f32,
    /// Top, right, bottom, left.
    margin: [f32; 4],
    /// The item holding this box's background and border.
    deco: usize,
}

enum Piece {
    /// One text job's worth of inline content.
    Text(LayoutJob),
    /// A picture or inline block.
    Atom(NodeId),
    /// An empty line held open by a `<br>`.
    Blank(f32),
}

impl Cx<'_> {
    fn style(&self, id: NodeId) -> &Style {
        &self.styles[id]
    }

    fn display(&self, id: NodeId) -> Display {
        match &self.doc.node(id).kind {
            Kind::Text(_) => Display::Inline,
            Kind::Element { .. } => {
                let style = self.style(id);
                if style.opacity == 0.0 {
                    Display::None
                } else {
                    style.display
                }
            }
            _ => Display::None,
        }
    }

    /// Lay out a block-level box whose margin edge starts at `(x, y)`, in a container `avail`
    /// wide. `shrink`: size an auto width to the content (inline blocks, flex items).
    fn block(&mut self, id: NodeId, x: f32, y: f32, avail: f32, shrink: bool) -> Laid {
        self.block_with(id, x, y, avail, shrink, None)
    }

    /// [`Self::block`], with the border box exactly `force` wide when given (flex items and
    /// table cells, whose widths their container already resolved).
    fn block_with(
        &mut self,
        id: NodeId,
        x: f32,
        y: f32,
        avail: f32,
        shrink: bool,
        force: Option<f32>,
    ) -> Laid {
        let style = self.style(id).clone();
        let pad = |i: usize| style.padding[i].or_zero(avail).max(0.0);
        let (pt, pr, pb, pl) = (pad(0), pad(1), pad(2), pad(3));
        let bw = |i: usize| style.border[i].width;
        let (bt, br, bb, bl) = (bw(0), bw(1), bw(2), bw(3));
        let mt = style.margin[0].or_zero(avail);
        let mb = style.margin[2].or_zero(avail);
        let mut ml = style.margin[3].or_zero(avail);
        let mut mr = style.margin[1].or_zero(avail);
        let frame = pl + pr + bl + br;
        let fixed = style.width.resolve(avail).map(|w| {
            if style.border_box || self.doc.name(id) == Some("img") {
                (w - frame).max(0.0)
            } else {
                w
            }
        });
        let mut content = match (force, fixed) {
            (Some(w), _) => (w - frame).max(0.0),
            (None, Some(w)) => w,
            (None, None) => (avail - ml - mr - frame).max(0.0),
        };
        if let Some(max) = style.max_width.resolve(avail).filter(|_| force.is_none()) {
            let max = if style.border_box { max - frame } else { max };
            content = content.min(max.max(0.0));
        }
        if shrink && fixed.is_none() && force.is_none() {
            content = content.min(self.natural_width(id, content));
        }
        // Auto margins center a box narrower than its container.
        let used = content + frame;
        let left_auto = style.margin[3] == Length::Auto && force.is_none();
        let right_auto = style.margin[1] == Length::Auto && force.is_none();
        let spare = (avail - used - ml - mr).max(0.0);
        if left_auto && right_auto {
            ml += spare / 2.0;
            mr += spare / 2.0;
        } else if left_auto {
            ml += spare;
        }
        let _ = mr;
        let box_x = x + ml;
        let deco = self.items.len();
        self.items.push(Item::Box {
            node: id,
            rect: Rect::NOTHING,
            fill: style.background,
            border: style.border,
            radius: style.radius,
        });
        let cx0 = box_x + bl + pl;
        let cy0 = y + mt + bt + pt;
        let start = self.items.len();
        let inner = match style.display {
            Display::Table => self.table(id, cx0, cy0, content),
            Display::Flex if !style.flex_column => self.flex(id, cx0, cy0, content),
            _ => self.flow(id, cx0, cy0, content, false),
        };
        if style.display == Display::ListItem {
            self.marker(id, cx0, cy0);
        }
        let mut content_height = inner;
        if let Some(h) = style
            .height
            .resolve(0.0)
            .filter(|_| style.height != Length::Auto)
        {
            let h = if style.border_box {
                h - pt - pb - bt - bb
            } else {
                h
            };
            content_height = h.max(0.0);
        }
        let _ = start;
        let height = bt + pt + content_height + pb + bb;
        let width = content + frame;
        if let Item::Box { rect, .. } = &mut self.items[deco] {
            *rect = Rect::from_min_size(pos2(box_x, y + mt), vec2(width, height));
        }
        Laid {
            width,
            height,
            margin: [mt, mr, mb, ml],
            deco,
        }
    }

    /// The width content would take with no wrapping pressure beyond `avail`.
    fn natural_width(&mut self, id: NodeId, avail: f32) -> f32 {
        let mark = self.items.len();
        let style = self.style(id).clone();
        match style.display {
            Display::Table => {
                self.table(id, 0.0, 0.0, avail);
            }
            Display::Flex if !style.flex_column => {
                self.flex(id, 0.0, 0.0, avail);
            }
            _ => {
                self.flow(id, 0.0, 0.0, avail, false);
            }
        }
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for item in &self.items[mark..] {
            let rect = item.rect();
            if rect.is_finite() && rect.width() > 0.0 {
                min = min.min(rect.min.x);
                max = max.max(rect.max.x);
            }
        }
        self.items.truncate(mark);
        if max > min {
            (max - min).ceil() + 1.0
        } else {
            0.0
        }
    }

    /// Normal flow: block children stack, runs of inline children become text. Returns the
    /// content height.
    fn flow(&mut self, parent: NodeId, x: f32, y: f32, width: f32, _root: bool) -> f32 {
        let children: Vec<NodeId> = self.doc.children(parent).to_vec();
        let mut cursor = y;
        let mut previous_margin = 0.0f32;
        let mut first = true;
        let mut run: Vec<NodeId> = Vec::new();
        let mut i = 0;
        while i <= children.len() {
            let child = children.get(i).copied();
            let display = child.map(|c| self.display(c));
            let inline = display.is_some_and(Display::is_inline_level);
            if inline {
                run.push(child.unwrap());
                i += 1;
                continue;
            }
            if !run.is_empty() {
                let height = self.inline(
                    parent,
                    &std::mem::take(&mut run),
                    x,
                    cursor + previous_margin,
                    width,
                );
                if height > 0.0 {
                    cursor += previous_margin + height;
                    previous_margin = 0.0;
                    first = false;
                }
            }
            let Some(child) = child else { break };
            i += 1;
            if display == Some(Display::None) {
                continue;
            }
            // A block's top margin collapses with the previous sibling's bottom margin.
            let top = self.style(child).margin[0].or_zero(width);
            let gap = if first { top } else { previous_margin.max(top) };
            let laid = self.block(child, x, cursor + gap - top, width, false);
            cursor += gap + laid.height;
            previous_margin = laid.margin[2];
            first = false;
        }
        cursor + previous_margin - y
    }

    /// Lay out a run of inline-level siblings inside `block`. Returns the height used.
    fn inline(&mut self, block: NodeId, run: &[NodeId], x: f32, y: f32, width: f32) -> f32 {
        let block_style = self.style(block).clone();
        let mut pieces = Vec::new();
        let mut job = self.new_job(&block_style, width);
        let mut first_text = true;
        for &id in run {
            self.collect(
                id,
                None,
                &mut job,
                &mut pieces,
                &block_style,
                width,
                &mut first_text,
            );
        }
        self.finish_job(job, &mut pieces, &block_style);
        let mut cursor = y;
        let mut atoms: Vec<NodeId> = Vec::new();
        for piece in pieces {
            match piece {
                Piece::Atom(id) => atoms.push(id),
                other => {
                    if !atoms.is_empty() {
                        cursor +=
                            self.atom_rows(block, &std::mem::take(&mut atoms), x, cursor, width);
                    }
                    match other {
                        Piece::Text(job) => cursor += self.text(block, job, x, cursor, width),
                        Piece::Blank(height) => cursor += height,
                        Piece::Atom(_) => {}
                    }
                }
            }
        }
        if !atoms.is_empty() {
            cursor += self.atom_rows(block, &atoms, x, cursor, width);
        }
        cursor - y
    }

    fn new_job(&self, block: &Style, width: f32) -> LayoutJob {
        LayoutJob {
            wrap: TextWrapping {
                max_width: if block.nowrap {
                    f32::INFINITY
                } else {
                    width.max(1.0)
                },
                ..Default::default()
            },
            halign: match block.text_align {
                Align::Center => egui::Align::Center,
                Align::Right => egui::Align::RIGHT,
                _ => egui::Align::LEFT,
            },
            justify: block.text_align == Align::Justify,
            break_on_newline: true,
            ..Default::default()
        }
    }

    fn finish_job(&self, mut job: LayoutJob, pieces: &mut Vec<Piece>, block: &Style) {
        // A trailing space or line break adds nothing.
        let had_text = !job.text.is_empty();
        while job.text.ends_with(' ') || job.text.ends_with('\n') {
            if job.text.ends_with('\n') && job.text.trim_end_matches(' ').len() == job.text.len() {
                // Keep the break if the text is only breaks (an empty paragraph: `<p><br></p>`).
                if job.text.trim_matches('\n').is_empty() {
                    break;
                }
            }
            job.text.pop();
        }
        if let Some(last) = job.sections.last_mut() {
            last.byte_range.end = egui::text::ByteIndex(last.byte_range.end.0.min(job.text.len()));
        }
        job.sections
            .retain(|s| s.byte_range.start < s.byte_range.end);
        if job.text.is_empty() || job.sections.is_empty() {
            return;
        }
        if job.text.chars().all(|c| c == '\n') {
            let lines = job.text.len().max(1) as f32;
            pieces.push(Piece::Blank(block.line_px() * lines));
            return;
        }
        let _ = had_text;
        pieces.push(Piece::Text(job));
    }

    /// Walk inline content, appending text to `job` and splitting it at atoms.
    #[allow(clippy::too_many_arguments)]
    fn collect(
        &mut self,
        id: NodeId,
        background: Option<Color32>,
        job: &mut LayoutJob,
        pieces: &mut Vec<Piece>,
        block: &Style,
        width: f32,
        first_text: &mut bool,
    ) {
        match self.display(id) {
            Display::None => return,
            Display::Inline => {}
            _ => {
                let finished = std::mem::replace(job, self.new_job(block, width));
                self.finish_job(finished, pieces, block);
                pieces.push(Piece::Atom(id));
                return;
            }
        }
        match &self.doc.node(id).kind {
            Kind::Text(text) => {
                let style = self.style(id).clone();
                let text = if style.pre {
                    text.replace("\r\n", "\n").replace('\t', "    ")
                } else {
                    collapse_whitespace(
                        text,
                        job.text.is_empty() || job.text.ends_with([' ', '\n']),
                    )
                };
                if text.is_empty() {
                    return;
                }
                let start = job.text.len();
                job.text.push_str(&text);
                let indent = if *first_text {
                    *first_text = false;
                    block.text_indent.or_zero(width)
                } else {
                    0.0
                };
                job.sections.push(LayoutSection {
                    leading_space: indent,
                    byte_range: egui::text::ByteIndex(start)..egui::text::ByteIndex(job.text.len()),
                    format: self.format(&style, background),
                });
            }
            Kind::Element { name, .. } => {
                if name == "br" {
                    trim_trailing_spaces(job);
                    let style = self.style(id).clone();
                    let start = job.text.len();
                    job.text.push('\n');
                    job.sections.push(LayoutSection {
                        leading_space: 0.0,
                        byte_range: egui::text::ByteIndex(start)
                            ..egui::text::ByteIndex(job.text.len()),
                        format: self.format(&style, None),
                    });
                    return;
                }
                let background = self.style(id).background.or(background);
                for &child in self.doc.children(id).to_vec().iter() {
                    self.collect(child, background, job, pieces, block, width, first_text);
                }
            }
            _ => {}
        }
    }

    fn format(&self, style: &Style, background: Option<Color32>) -> TextFormat {
        let family = if style.mono {
            self.families.mono.clone()
        } else if style.bold {
            self.families.bold.clone()
        } else {
            self.families.regular.clone()
        };
        let line = style.line_px();
        TextFormat {
            font_id: FontId::new(style.font_size, family),
            extra_letter_spacing: style.letter_spacing,
            line_height: Some(line.max(style.font_size)),
            color: style.color,
            background: background.unwrap_or(Color32::TRANSPARENT),
            italics: style.italic,
            underline: if style.underline {
                Stroke::new(1.0, style.color)
            } else {
                Stroke::NONE
            },
            strikethrough: if style.strike {
                Stroke::new(1.0, style.color)
            } else {
                Stroke::NONE
            },
            valign: match style.vertical_align {
                VAlign::Super => egui::Align::TOP,
                VAlign::Sub => egui::Align::BOTTOM,
                VAlign::Baseline => egui::Align::BOTTOM,
            },
            ..Default::default()
        }
    }

    fn text(&mut self, block: NodeId, job: LayoutJob, x: f32, y: f32, width: f32) -> f32 {
        let halign = job.halign;
        let galley = self.ctx.fonts_mut(|fonts| fonts.layout_job(job));
        let anchor = match halign {
            egui::Align::Center => x + width / 2.0,
            egui::Align::Max => x + width,
            egui::Align::Min => x,
        };
        let height = galley.rect.height();
        self.items.push(Item::Text {
            node: block,
            pos: pos2(anchor, y),
            galley,
        });
        height
    }

    /// Pictures and inline blocks flowed in rows. Returns the height used.
    fn atom_rows(&mut self, block: NodeId, atoms: &[NodeId], x: f32, y: f32, width: f32) -> f32 {
        let align = self.style(block).text_align;
        let mut cursor_y = y;
        let mut row: Vec<(usize, usize, f32, f32)> = Vec::new(); // item range, width, height
        let mut row_width = 0.0f32;
        let flush = |cx: &mut Self,
                     row: &mut Vec<(usize, usize, f32, f32)>,
                     row_width: &mut f32,
                     cursor_y: &mut f32| {
            if row.is_empty() {
                return;
            }
            let height = row.iter().map(|r| r.3).fold(0.0, f32::max);
            let offset = match align {
                Align::Center => (width - *row_width).max(0.0) / 2.0,
                Align::Right => (width - *row_width).max(0.0),
                _ => 0.0,
            };
            let mut at = 0.0;
            for &(start, end, w, h) in row.iter() {
                // Bottom-align atoms on the row, like inline content on a baseline.
                let delta = vec2(x + offset + at, *cursor_y + height - h);
                for item in &mut cx.items[start..end] {
                    item.translate(delta);
                }
                at += w;
            }
            *cursor_y += height;
            row.clear();
            *row_width = 0.0;
        };
        for &atom in atoms {
            let start = self.items.len();
            let (w, h) = self.atom(atom, width);
            let end = self.items.len();
            if row_width + w > width + 0.5 && !row.is_empty() {
                flush(self, &mut row, &mut row_width, &mut cursor_y);
            }
            row.push((start, end, w, h));
            row_width += w;
        }
        flush(self, &mut row, &mut row_width, &mut cursor_y);
        cursor_y - y
    }

    /// Lay out an atom at the origin. Returns its margin-box size.
    fn atom(&mut self, id: NodeId, avail: f32) -> (f32, f32) {
        if self.doc.name(id) == Some("img") {
            return self.image(id, avail);
        }
        let laid = self.block(id, 0.0, 0.0, avail, true);
        let [mt, mr, mb, ml] = laid.margin;
        (ml + laid.width + mr, mt + laid.height + mb)
    }

    fn image(&mut self, id: NodeId, avail: f32) -> (f32, f32) {
        let style = self.style(id).clone();
        let src = self.doc.attr(id, "src").unwrap_or("").to_string();
        let alt = self.doc.attr(id, "alt").unwrap_or("").to_string();
        let natural = self.images.image_size(&src);
        let ml = style.margin[3].or_zero(avail);
        let mr = style.margin[1].or_zero(avail);
        let mt = style.margin[0].or_zero(avail);
        let mb = style.margin[2].or_zero(avail);
        let room = (avail - ml - mr).max(1.0);
        let width = style.width.resolve(room);
        let height = style
            .height
            .resolve(0.0)
            .filter(|_| matches!(style.height, Length::Px(_)));
        let base = natural.unwrap_or(PLACEHOLDER);
        let aspect = if base.x > 0.0 {
            base.y / base.x
        } else {
            0.5625
        };
        let (mut w, mut h) = match (width, height) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * aspect),
            (None, Some(h)) => (if aspect > 0.0 { h / aspect } else { h }, h),
            (None, None) => (base.x, base.y),
        };
        let max = style.max_width.resolve(room).unwrap_or(room).min(room);
        if w > max {
            h *= max / w;
            w = max;
        }
        let deco_rect = Rect::from_min_size(pos2(ml, mt), vec2(w, h));
        self.items.push(Item::Box {
            node: id,
            rect: deco_rect,
            fill: style.background,
            border: style.border,
            radius: style.radius,
        });
        self.items.push(Item::Image {
            node: id,
            rect: deco_rect,
            src,
            alt,
            radius: style.radius.min(w / 2.0).min(h / 2.0),
        });
        (ml + w + mr, mt + h + mb)
    }

    fn marker(&mut self, item: NodeId, x: f32, y: f32) {
        let style = self.style(item).clone();
        let index = self
            .doc
            .node(item)
            .parent
            .map(|parent| {
                let start = self
                    .doc
                    .attr(parent, "start")
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .unwrap_or(1);
                let position = self
                    .doc
                    .children(parent)
                    .iter()
                    .filter(|&&c| self.display(c) == Display::ListItem)
                    .position(|&c| c == item)
                    .unwrap_or(0) as i64;
                start + position
            })
            .unwrap_or(1);
        let text = match style.list_style {
            ListStyle::None => return,
            ListStyle::Disc => "•".to_string(),
            ListStyle::Circle => "◦".to_string(),
            ListStyle::Square => "▪".to_string(),
            ListStyle::Decimal => format!("{index}."),
            ListStyle::LowerAlpha => format!("{}.", alpha(index, false)),
            ListStyle::UpperAlpha => format!("{}.", alpha(index, true)),
            ListStyle::LowerRoman => format!("{}.", roman(index).to_lowercase()),
            ListStyle::UpperRoman => format!("{}.", roman(index)),
        };
        let mut plain = style.clone();
        plain.underline = false;
        plain.strike = false;
        plain.bold = false;
        let format = self.format(&plain, None);
        let job = LayoutJob::single_section(text, format);
        let galley = self.ctx.fonts_mut(|fonts| fonts.layout_job(job));
        let pos = pos2(x - galley.rect.width() - 6.0, y);
        self.items.push(Item::Text {
            node: item,
            pos,
            galley,
        });
    }

    /// Rows of cells; columns share the table's width. Returns the content height.
    fn table(&mut self, table: NodeId, x: f32, y: f32, width: f32) -> f32 {
        let mut rows: Vec<NodeId> = Vec::new();
        let mut caption: Vec<NodeId> = Vec::new();
        for &child in self.doc.children(table) {
            match self.display(child) {
                Display::TableRow => rows.push(child),
                Display::TableRowGroup => rows.extend(
                    self.doc
                        .children(child)
                        .iter()
                        .copied()
                        .filter(|&r| self.display(r) == Display::TableRow),
                ),
                Display::None | Display::Inline => {}
                _ => caption.push(child),
            }
        }
        let cells_of = |cx: &Self, row: NodeId| -> Vec<NodeId> {
            cx.doc
                .children(row)
                .iter()
                .copied()
                .filter(|&c| !matches!(cx.display(c), Display::None | Display::Inline))
                .collect()
        };
        let span = |cx: &Self, cell: NodeId| -> usize {
            cx.doc
                .attr(cell, "colspan")
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 64)
        };
        let columns = rows
            .iter()
            .map(|&row| {
                cells_of(self, row)
                    .iter()
                    .map(|&c| span(self, c))
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0);
        let mut cursor = y;
        for id in caption {
            let laid = self.block(id, x, cursor, width, false);
            cursor += laid.margin[0] + laid.height + laid.margin[2];
        }
        if columns == 0 {
            return cursor - y;
        }
        // Column widths: a width given on a cell of the first row that has one, else an
        // equal share of what is left.
        let mut fixed: Vec<Option<f32>> = vec![None; columns];
        for &row in &rows {
            let mut col = 0;
            for cell in cells_of(self, row) {
                let n = span(self, cell);
                if n == 1
                    && col < columns
                    && fixed[col].is_none()
                    && let Some(w) = self.style(cell).width.resolve(width)
                {
                    fixed[col] = Some(w);
                }
                col += n;
            }
        }
        let taken: f32 = fixed.iter().flatten().sum();
        let free = fixed.iter().filter(|f| f.is_none()).count();
        let share = if free > 0 {
            ((width - taken) / free as f32).max(24.0)
        } else {
            0.0
        };
        let widths: Vec<f32> = fixed.iter().map(|f| f.unwrap_or(share)).collect();
        for row in rows {
            let row_start = self.items.len();
            let row_style = self.style(row).clone();
            let row_deco = self.items.len();
            self.items.push(Item::Box {
                node: row,
                rect: Rect::NOTHING,
                fill: row_style.background,
                border: [Border::NONE; 4],
                radius: 0.0,
            });
            let _ = row_start;
            let mut col = 0;
            let mut laid_cells: Vec<(Laid, usize, usize, f32)> = Vec::new();
            for cell in cells_of(self, row) {
                let n = span(self, cell);
                let cell_width: f32 = widths[col.min(columns)..(col + n).min(columns)]
                    .iter()
                    .sum();
                let cell_x = x + widths[..col.min(columns)].iter().sum::<f32>();
                let start = self.items.len();
                let mut laid =
                    self.block_with(cell, cell_x, cursor, cell_width, false, Some(cell_width));
                laid.margin = [0.0; 4];
                laid_cells.push((laid, start, self.items.len(), cell_x));
                col += n;
            }
            let height = laid_cells.iter().map(|c| c.0.height).fold(0.0, f32::max);
            for (laid, start, end, cell_x) in laid_cells {
                let cell = match &self.items[laid.deco] {
                    Item::Box { node, .. } => *node,
                    _ => continue,
                };
                if self.style(cell).align_center && laid.height < height {
                    let delta = vec2(0.0, (height - laid.height) / 2.0);
                    for item in &mut self.items[start + 1..end] {
                        item.translate(delta);
                    }
                }
                if let Item::Box { rect, .. } = &mut self.items[laid.deco] {
                    *rect = Rect::from_min_size(pos2(cell_x, cursor), vec2(laid.width, height));
                }
            }
            if let Item::Box { rect, .. } = &mut self.items[row_deco] {
                *rect = Rect::from_min_size(pos2(x, cursor), vec2(width, height));
            }
            cursor += height;
        }
        cursor - y
    }

    /// `display: flex` in a row. Returns the content height.
    fn flex(&mut self, parent: NodeId, x: f32, y: f32, width: f32) -> f32 {
        let style = self.style(parent).clone();
        let children: Vec<NodeId> = self
            .doc
            .children(parent)
            .iter()
            .copied()
            .filter(|&c| match &self.doc.node(c).kind {
                Kind::Text(t) => !t.trim().is_empty(),
                Kind::Element { .. } => self.display(c) != Display::None,
                _ => false,
            })
            .collect();
        if children.is_empty() {
            return 0.0;
        }
        let gaps = style.gap * (children.len() - 1) as f32;
        let room = (width - gaps).max(0.0);
        // Each child's base width: its own width, else its content's.
        let mut bases: Vec<f32> = Vec::new();
        for &child in &children {
            let base = if matches!(self.doc.node(child).kind, Kind::Text(_)) {
                self.text_natural(parent, child, room)
            } else if self.doc.name(child) == Some("img") {
                let mark = self.items.len();
                let (w, _) = self.image(child, room);
                self.items.truncate(mark);
                w
            } else {
                let s = self.style(child).clone();
                let margins = s.margin[1].or_zero(room) + s.margin[3].or_zero(room);
                match s.width.resolve(room) {
                    Some(w) => w + margins,
                    None => {
                        let mark = self.items.len();
                        let laid = self.block(child, 0.0, 0.0, room, true);
                        self.items.truncate(mark);
                        laid.width + margins
                    }
                }
            };
            bases.push(base);
        }
        let total: f32 = bases.iter().sum();
        let grow: f32 = children.iter().map(|&c| self.style(c).flex_grow).sum();
        let mut widths = bases.clone();
        if total > room && total > 0.0 {
            for w in &mut widths {
                *w *= room / total;
            }
        } else if grow > 0.0 {
            let extra = room - total;
            for (w, &c) in widths.iter_mut().zip(&children) {
                *w += extra * self.style(c).flex_grow / grow;
            }
        }
        let used: f32 = widths.iter().sum::<f32>() + gaps;
        let spare = (width - used).max(0.0);
        let n = children.len() as f32;
        let (mut at, between) = match style.justify {
            Justify::Start => (0.0, style.gap),
            Justify::Center => (spare / 2.0, style.gap),
            Justify::End => (spare, style.gap),
            Justify::SpaceBetween if n > 1.0 => (0.0, style.gap + spare / (n - 1.0)),
            Justify::SpaceBetween => (0.0, style.gap),
            Justify::SpaceAround => (spare / n / 2.0, style.gap + spare / n),
        };
        let mut placed: Vec<(usize, usize, f32, Option<usize>)> = Vec::new();
        for (&child, &w) in children.iter().zip(&widths) {
            let start = self.items.len();
            let (height, deco) = if matches!(self.doc.node(child).kind, Kind::Text(_)) {
                (self.inline(parent, &[child], x + at, y, w), None)
            } else if self.doc.name(child) == Some("img") {
                let (_, h) = self.image(child, w);
                for item in &mut self.items[start..] {
                    item.translate(vec2(x + at, y));
                }
                (h, None)
            } else {
                let s = self.style(child).clone();
                let ml = s.margin[3].or_zero(room);
                let mr = s.margin[1].or_zero(room);
                let inner = (w - ml - mr).max(0.0);
                let laid = self.block_with(child, x + at, y, room, false, Some(inner));
                (
                    laid.margin[0] + laid.height + laid.margin[2],
                    Some(laid.deco),
                )
            };
            placed.push((start, self.items.len(), height, deco));
            at += w + between;
        }
        let height = placed.iter().map(|p| p.2).fold(0.0, f32::max);
        for (start, end, h, deco) in placed {
            if style.align_center && h < height {
                let delta = vec2(0.0, (height - h) / 2.0);
                for item in &mut self.items[start..end] {
                    item.translate(delta);
                }
            } else if !style.align_center
                && let Some(deco) = deco
                && let Item::Box { rect, .. } = &mut self.items[deco]
                && rect.is_finite()
            {
                // Stretch: the box fills the row's height.
                rect.max.y = rect.min.y + (height - (rect.min.y - y)).max(rect.height());
            }
        }
        height
    }

    fn text_natural(&mut self, parent: NodeId, text: NodeId, room: f32) -> f32 {
        let mark = self.items.len();
        self.inline(parent, &[text], 0.0, 0.0, room);
        let width = self.items[mark..]
            .iter()
            .map(|i| i.rect().width())
            .fold(0.0, f32::max);
        self.items.truncate(mark);
        width.ceil() + 1.0
    }
}

/// Drop spaces at the end of the job's text (a line's end), fixing the sections.
fn trim_trailing_spaces(job: &mut LayoutJob) {
    let trimmed = job.text.trim_end_matches(' ').len();
    if trimmed == job.text.len() {
        return;
    }
    job.text.truncate(trimmed);
    for section in &mut job.sections {
        section.byte_range.end = egui::text::ByteIndex(section.byte_range.end.0.min(trimmed));
        section.byte_range.start = egui::text::ByteIndex(section.byte_range.start.0.min(trimmed));
    }
    job.sections
        .retain(|s| s.byte_range.start < s.byte_range.end);
}

/// Collapse runs of HTML whitespace to one space (non-breaking spaces stay).
fn collapse_whitespace(text: &str, at_line_start: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = at_line_start;
    for c in text.chars() {
        if matches!(c, ' ' | '\n' | '\t' | '\r' | '\u{c}') {
            if !space {
                out.push(' ');
                space = true;
            }
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

fn alpha(mut n: i64, upper: bool) -> String {
    if n < 1 {
        return n.to_string();
    }
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        let base = if upper { b'A' } else { b'a' };
        out.push((base + (n % 26) as u8) as char);
        n /= 26;
    }
    out.iter().rev().collect()
}

fn roman(mut n: i64) -> String {
    if !(1..4000).contains(&n) {
        return n.to_string();
    }
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, letters) in table {
        while n >= value {
            out.push_str(letters);
            n -= value;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(html: &str, width: f32) -> Page {
        let ctx = egui::Context::default();
        let mut result = None;
        let doc = Document::parse(html);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            result = Some(layout(&doc, width, ui.ctx(), &Families::default(), &mut ()));
        });
        output.textures_delta.clear();
        result.unwrap()
    }

    fn boxes(page: &Page) -> Vec<Rect> {
        page.items
            .iter()
            .filter_map(|i| match i {
                Item::Box {
                    rect,
                    fill: Some(_),
                    ..
                } => Some(*rect),
                _ => None,
            })
            .collect()
    }

    fn texts(page: &Page) -> Vec<(Rect, String)> {
        page.items
            .iter()
            .filter_map(|i| match i {
                Item::Text { galley, .. } => Some((i.rect(), galley.job.text.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn blocks_stack_and_margins_collapse() {
        let page = page(
            "<p style=\"margin: 10px 0\">a</p><p style=\"margin: 20px 0\">b</p>",
            300.0,
        );
        let t = texts(&page);
        assert_eq!(t.len(), 2);
        let gap = t[1].0.min.y - t[0].0.max.y;
        assert!(
            (gap - 20.0).abs() < 0.5,
            "collapsed gap is the larger margin, got {gap}"
        );
    }

    #[test]
    fn padding_border_background_and_auto_margins() {
        let page = page(
            "<section style=\"width: 100px; margin: 0 auto; padding: 10px; border: 2px solid #000; background: #eee\">x</section>",
            300.0,
        );
        let rect = boxes(&page)[0];
        assert_eq!(rect.width(), 124.0);
        assert_eq!(rect.min.x, 88.0);
        let (text, _) = &texts(&page)[0];
        assert!((text.min.x - 100.0).abs() < 0.5);
        assert!((text.min.y - 12.0).abs() < 0.5);
    }

    #[test]
    fn whitespace_collapses_and_br_breaks() {
        let page = page("<p>  a \n\n b <br>c  </p>", 300.0);
        let t = texts(&page);
        assert_eq!(t[0].1, "a b\nc");
    }

    #[test]
    fn centered_text_is_centered() {
        let page = page("<p style=\"text-align:center\">abc</p>", 300.0);
        let (rect, _) = &texts(&page)[0];
        let middle = (rect.min.x + rect.max.x) / 2.0;
        assert!((middle - 150.0).abs() < 1.0, "middle at {middle}");
    }

    #[test]
    fn inline_block_badge_shrinks_and_centers() {
        let page = page(
            "<h2 style=\"text-align:center\"><span style=\"display:inline-block; background:#f60; padding: 4px 10px\">标题</span></h2>",
            300.0,
        );
        let rect = boxes(&page)[0];
        assert!(
            rect.width() < 120.0,
            "badge shrinks to its text, got {}",
            rect.width()
        );
        let middle = (rect.min.x + rect.max.x) / 2.0;
        assert!((middle - 150.0).abs() < 1.0);
    }

    #[test]
    fn images_scale_to_fit() {
        struct Sizes;
        impl ImageSizes for Sizes {
            fn image_size(&mut self, _src: &str) -> Option<Vec2> {
                Some(vec2(1000.0, 500.0))
            }
        }
        let ctx = egui::Context::default();
        let doc = Document::parse("<p><img src=\"a.png\"></p>");
        let mut page = None;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            page = Some(layout(
                &doc,
                300.0,
                ui.ctx(),
                &Families::default(),
                &mut Sizes,
            ));
        });
        output.textures_delta.clear();
        let image = page
            .unwrap()
            .items
            .iter()
            .find_map(|i| match i {
                Item::Image { rect, .. } => Some(*rect),
                _ => None,
            })
            .unwrap();
        assert_eq!(image.size(), vec2(300.0, 150.0));
    }

    #[test]
    fn list_markers() {
        let page = page(
            "<ol start=\"3\"><li>a</li><li>b</li></ol><ul style=\"list-style:none\"><li>c</li></ul>",
            300.0,
        );
        let t: Vec<String> = texts(&page).into_iter().map(|t| t.1).collect();
        assert_eq!(t, ["a", "3.", "b", "4.", "c"]);
    }

    #[test]
    fn table_columns_share_width() {
        let page = page(
            "<table style=\"width:100%\"><tr><td style=\"background:#eee\">a</td><td style=\"background:#ddd\">b</td></tr></table>",
            200.0,
        );
        let cells = boxes(&page);
        assert_eq!(cells.len(), 2);
        assert!((cells[0].width() - 100.0).abs() < 0.5);
        let narrow = super::tests::page(
            "<table style=\"width:100%\"><tr><td style=\"width:30%; background:#eee\">a</td><td style=\"background:#ddd\">b</td></tr></table>",
            200.0,
        );
        let cells = boxes(&narrow);
        assert!(
            (cells[0].width() - 60.0).abs() < 0.5,
            "width {}",
            cells[0].width()
        );
        assert!((cells[1].width() - 140.0).abs() < 0.5);
        assert!((cells[1].min.x - 60.0).abs() < 0.5);
        assert_eq!(cells[0].height(), cells[1].height());
    }

    #[test]
    fn flex_row_places_children_side_by_side() {
        let page = page(
            "<section style=\"display:flex\"><section style=\"width:50%; background:#eee\">a</section><section style=\"width:50%; background:#ddd\">b<br>c</section></section>",
            200.0,
        );
        let cells = boxes(&page);
        assert_eq!(cells.len(), 2);
        assert!((cells[1].min.x - 100.0).abs() < 0.5);
        assert!(
            (cells[0].width() - 100.0).abs() < 0.5,
            "width {}",
            cells[0].width()
        );
        assert_eq!(cells[0].min.y, cells[1].min.y);
        assert_eq!(cells[0].height(), cells[1].height(), "stretched to the row");
    }

    #[test]
    fn hidden_and_head_content_is_not_drawn() {
        let page = page(
            "<html><head><title>t</title><style>p{}</style></head><body style=\"margin:0\"><p style=\"display:none\">x</p><p>y</p></body></html>",
            200.0,
        );
        let t: Vec<String> = texts(&page).into_iter().map(|t| t.1).collect();
        assert_eq!(t, ["y"]);
    }

    #[test]
    fn numbering() {
        assert_eq!(alpha(1, false), "a");
        assert_eq!(alpha(28, true), "AB");
        assert_eq!(roman(1994), "MCMXCIV");
    }
}
