//! The same pages written through the PDF engine the other editors share (`craft-pdf`), with
//! the fonts embedded: Liberation Sans, Serif and Mono stand in for Helvetica, Times and
//! Courier (they have the same advance widths, so [`text_width`](crate::text_width) still
//! measures what is drawn), and Noto Sans CJK SC for the characters WinAnsiEncoding lacks.
//! A reader without those fonts installed sees the same glyphs, which the standard-14 output
//! cannot promise for Chinese text.
//!
//! Only an embedding host turns this on (`shared-pdf`); it brings in the shared font bytes.

use std::collections::HashMap;
use std::sync::OnceLock;

use craft_pdf::write::{Metadata, Writer, backend as krilla};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::destination::XyzDestination;
use krilla::geom::{PathBuilder, Point as KPoint, Rect as KRect, Size, Transform};
use krilla::image::Image;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LineJoin, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::text::{Font as KFont, GlyphId, KrillaGlyph};
use skrifa::MetadataProvider;

use crate::{CID_WIDTH, Font, LinkTarget, Op, PdfDoc, Rgb, needs_cid};

/// The faces drawn with, by [`Font`] slot; the last is the CJK face.
fn face_bytes(slot: usize) -> &'static [u8] {
    use weave_font_assets::font;
    match slot {
        0 => font!("craft/fonts/liberation-sans/LiberationSans-Regular.ttf"),
        1 => font!("craft/fonts/liberation-sans/LiberationSans-Bold.ttf"),
        2 => font!("craft/fonts/liberation-sans/LiberationSans-Italic.ttf"),
        3 => font!("craft/fonts/liberation-sans/LiberationSans-BoldItalic.ttf"),
        4 => font!("craft/fonts/liberation-serif/LiberationSerif-Regular.ttf"),
        5 => font!("craft/fonts/liberation-serif/LiberationSerif-Bold.ttf"),
        6 => font!("craft/fonts/liberation-serif/LiberationSerif-Italic.ttf"),
        7 => font!("craft/fonts/liberation-serif/LiberationSerif-BoldItalic.ttf"),
        8 => font!("craft/fonts/liberation-mono/LiberationMono-Regular.ttf"),
        9 => font!("craft/fonts/liberation-mono/LiberationMono-Bold.ttf"),
        10 => font!("craft/fonts/liberation-mono/LiberationMono-Italic.ttf"),
        11 => font!("craft/fonts/liberation-mono/LiberationMono-BoldItalic.ttf"),
        _ => font!("craft/fonts/noto-sans-cjk-sc/NotoSansCJKsc-Regular.otf"),
    }
}

/// The CJK face's slot.
const CJK: usize = Font::ALL.len();

/// One face, prepared once per process: the bytes are static, so only the embedding handle
/// and the character map lookup are built here.
struct Face {
    font: KFont,
    bytes: &'static [u8],
}

fn face(slot: usize) -> Option<&'static Face> {
    static FACES: OnceLock<Vec<Option<Face>>> = OnceLock::new();
    FACES
        .get_or_init(|| {
            (0..=CJK)
                .map(|slot| {
                    let bytes = face_bytes(slot);
                    craft_pdf::write::font(bytes, 0, None).ok().map(|font| Face { font, bytes })
                })
                .collect()
        })
        .get(slot)?
        .as_ref()
}

fn glyph(face: &Face, c: char, cache: &mut HashMap<(usize, char), u32>, slot: usize) -> u32 {
    *cache.entry((slot, c)).or_insert_with(|| skrifa::FontRef::new(face.bytes).ok().and_then(|f| f.charmap().map(c)).map_or(0, |g| g.to_u32()))
}

fn paint(c: Rgb) -> krilla::paint::Paint {
    krilla::color::rgb::Color::new(c[0], c[1], c[2]).into()
}

fn fill(c: Rgb) -> Fill {
    Fill { paint: paint(c), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }
}

fn stroke(c: Rgb, width: f32, dash: &[f32], round: bool) -> Stroke {
    let array: Vec<f32> = dash.iter().take(8).map(|v| v.max(0.0)).collect();
    let dash =
        (!array.is_empty() && array.iter().all(|v| v.is_finite()) && array.iter().any(|v| *v > 0.0)).then_some(StrokeDash { array, offset: 0.0 });
    Stroke {
        paint: paint(c),
        width: crate::stroke_width(width),
        line_join: if round { LineJoin::Round } else { LineJoin::Miter },
        dash,
        ..Default::default()
    }
}

fn polyline(points: &[[f32; 2]], close: bool) -> Option<krilla::geom::Path> {
    let mut path = PathBuilder::new();
    for (i, p) in points.iter().enumerate() {
        if i == 0 {
            path.move_to(p[0], p[1]);
        } else {
            path.line_to(p[0], p[1]);
        }
    }
    if close {
        path.close();
    }
    path.finish()
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Option<krilla::geom::Path> {
    let rect = KRect::from_xywh(x, y, w.max(0.0), h.max(0.0))?;
    let mut path = PathBuilder::new();
    path.push_rect(rect);
    path.finish()
}

fn circle_path(cx: f32, cy: f32, r: f32) -> Option<krilla::geom::Path> {
    const K: f32 = 0.552_284_8;
    let k = r * K;
    let mut p = PathBuilder::new();
    p.move_to(cx + r, cy);
    p.cubic_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
    p.cubic_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
    p.cubic_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
    p.cubic_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
    p.close();
    p.finish()
}

/// Draw `text` with its baseline at the origin: runs WinAnsi can encode in `font`'s stand-in,
/// the rest in the CJK face, each glyph advancing by the width [`Font::char_width`] measures.
fn text(s: &mut Surface<'_>, font: Font, size: f32, color: Rgb, text: &str, cache: &mut HashMap<(usize, char), u32>) {
    s.set_fill(Some(fill(color)));
    s.set_stroke(None);
    let mut x = 0.0f32;
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        let cjk = needs_cid(first);
        let end = rest.char_indices().find(|(_, c)| needs_cid(*c) != cjk).map_or(rest.len(), |(i, _)| i);
        let (run, tail) = rest.split_at(end);
        let slot = if cjk { CJK } else { font.index() };
        let Some(face) = face(slot) else { return };
        let mut glyphs = Vec::new();
        let mut width = 0.0f32;
        for (i, c) in run.char_indices() {
            let advance = f32::from(if cjk { CID_WIDTH } else { font.char_width(c) }) / 1000.0;
            let range = i..i + c.len_utf8();
            glyphs.push(KrillaGlyph::new(GlyphId::new(glyph(face, c, cache, slot)), advance, 0.0, 0.0, 0.0, range, None));
            width += advance;
        }
        s.draw_glyphs(KPoint::from_xy(x, 0.0), &glyphs, face.font.clone(), run, size, false);
        x += width * size;
        rest = tail;
    }
    s.set_fill(None);
}

/// Serialize `doc` through the shared engine; `None` if that fails (the caller then writes it
/// the self-contained way).
pub(crate) fn finish(doc: &PdfDoc) -> Option<Vec<u8>> {
    let creator = if doc.info.creator.is_empty() { "GridCraft".to_string() } else { doc.info.creator.clone() };
    let metadata = Metadata {
        creator,
        producer: "GridCraft PDF".into(),
        title: (!doc.info.title.is_empty()).then(|| doc.info.title.clone()),
        authors: if doc.info.author.is_empty() { Vec::new() } else { vec![doc.info.author.clone()] },
        ..Default::default()
    };
    let result = craft_pdf::write::guard(|| -> Result<Vec<u8>, craft_pdf::write::WriteError> {
        let mut writer = Writer::new(Default::default(), metadata);
        let pdf = writer.document_mut();
        let mut glyphs = HashMap::new();
        let count = doc.pages.len();
        for page in &doc.pages {
            let Some(size) = Size::from_wh(page.width, page.height) else { continue };
            let mut out = pdf.start_page_with(PageSettings::new(size));
            {
                let mut s = out.surface();
                // Pushes made since each save, so a restore pops exactly those.
                let mut levels: Vec<usize> = vec![0];
                for op in &page.ops {
                    match op {
                        Op::Save => levels.push(0),
                        Op::Restore => {
                            if levels.len() > 1 {
                                for _ in 0..levels.pop().unwrap_or(0) {
                                    s.pop();
                                }
                            }
                        }
                        Op::Clip(x, y, w, h) => {
                            if let Some(path) = rect_path(*x, *y, *w, *h) {
                                s.push_clip_path(&path, &FillRule::NonZero);
                                if let Some(level) = levels.last_mut() {
                                    *level += 1;
                                }
                            }
                        }
                        Op::FillRect(x, y, w, h, c) => {
                            if let Some(path) = rect_path(*x, *y, *w, *h) {
                                s.set_stroke(None);
                                s.set_fill(Some(fill(*c)));
                                s.draw_path(&path);
                            }
                        }
                        Op::StrokeRect(x, y, w, h, width, c) => {
                            if let Some(path) = rect_path(*x, *y, *w, *h) {
                                s.set_fill(None);
                                s.set_stroke(Some(stroke(*c, *width, &[], false)));
                                s.draw_path(&path);
                            }
                        }
                        Op::Line { points, width, color, dash, round } => {
                            if let Some(path) = polyline(points, false) {
                                s.set_fill(None);
                                s.set_stroke(Some(stroke(*color, *width, dash, *round)));
                                s.draw_path(&path);
                            }
                        }
                        Op::Polygon { points, fill: f, stroke: st } => {
                            if let Some(path) = polyline(points, true) {
                                s.set_fill(f.map(fill));
                                s.set_stroke(st.map(|(c, w)| stroke(c, w, &[], true)));
                                s.draw_path(&path);
                            }
                        }
                        Op::Circle { cx, cy, r, fill: f, stroke: st } => {
                            if let Some(path) = circle_path(*cx, *cy, *r) {
                                s.set_fill(f.map(fill));
                                s.set_stroke(st.map(|(c, w)| stroke(c, w, &[], false)));
                                s.draw_path(&path);
                            }
                        }
                        Op::Text { x, y, size, font, color, text: t, angle } => {
                            let (sn, cs) = if angle.is_finite() { angle.sin_cos() } else { (0.0, 1.0) };
                            s.push_transform(&Transform::from_row(cs, sn, -sn, cs, *x, *y));
                            text(&mut s, *font, *size, *color, t, &mut glyphs);
                            s.pop();
                        }
                        Op::Image { x, y, w, h, image } => {
                            let Some(data) = page.images.get(*image) else { continue };
                            let mut rgba = Vec::with_capacity(data.rgb.len() / 3 * 4);
                            for (i, px) in data.rgb.chunks_exact(3).enumerate() {
                                rgba.extend_from_slice(px);
                                rgba.push(data.alpha.as_ref().and_then(|a| a.get(i).copied()).unwrap_or(255));
                            }
                            let Some(size) = Size::from_wh(*w, *h) else { continue };
                            let image = Image::from_rgba8(rgba, data.width, data.height);
                            s.push_transform(&Transform::from_translate(*x, *y));
                            s.draw_image(image, size);
                            s.pop();
                        }
                    }
                }
                while let Some(level) = levels.pop() {
                    for _ in 0..level {
                        s.pop();
                    }
                }
                s.finish();
            }
            for link in &page.links {
                let [x0, y0, x1, y1] = link.rect;
                // Link rectangles are kept in PDF space (origin bottom left).
                let Some(rect) = KRect::from_ltrb(x0, page.height - y1, x1, page.height - y0) else { continue };
                let target = match &link.target {
                    LinkTarget::Uri(uri) => craft_pdf::write::uri_target(uri),
                    LinkTarget::Page { index, x, y } => {
                        (*index < count).then(|| Target::Destination(XyzDestination::new(*index, KPoint::from_xy(*x, *y)).into()))
                    }
                };
                if let Some(target) = target {
                    out.add_annotation(Annotation::new_link(LinkAnnotation::new(rect, target), None));
                }
            }
            out.finish();
        }
        writer.finish()
    });
    result.ok()
}
