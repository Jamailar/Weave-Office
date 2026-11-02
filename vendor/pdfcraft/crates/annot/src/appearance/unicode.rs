//! Unicode FreeText uses the existing approved-font shaper and CID font writer.
use pdfcraft_cos::{Dict, Document, Object};
use pdfcraft_fonts::{CraftFont, embedding, source_shaping};
use std::{borrow::Cow, collections::HashMap};

const MAX_TEXT_BYTES: usize = 262_144;
const MAX_GLYPHS: usize = 65_534;

/// Only build-embedded, approved faces are candidates. Prefer Simplified Chinese coverage;
/// font validation also enforces editable embedding and bounded font programs.
pub(super) fn face(text: &str) -> Option<&'static CraftFont> {
    if text.len() > MAX_TEXT_BYTES {
        return None;
    }
    let text = normalized(text);
    pdfcraft_fonts::ui_cjk_fonts(true).into_iter().find(|font| embedding::covers(font.bytes, &text) && embedding::validate(font.bytes).is_ok())
}

/// Match the established WinAnsi appearance's TAB-as-space policy without changing /Contents.
fn normalized(text: &str) -> Cow<'_, str> {
    if text.contains('\t') { Cow::Owned(text.replace('\t', " ")) } else { Cow::Borrowed(text) }
}

pub(super) fn width(face: &CraftFont, text: &str, size: f64) -> Option<f64> {
    source_shaping::shape(face.bytes, &normalized(text)).ok().map(|run| run.advance * size)
}

/// Choose a face for the whole annotation, including its Latin-only rows. The initial linear
/// breaker measures words and spaces with that face, then checks each complete shaped row:
/// contextual shaping/kerning must never make a supposedly fitting row exceed the clip box.
pub(super) fn wrap(face: &CraftFont, text: &str, size: f64, limit: f64) -> Option<Vec<String>> {
    let text = normalized(text);
    let candidates = pdfcraft_fonts::wrap_with(&text, size, limit, |part, size| width(face, part, size).unwrap_or(f64::INFINITY));
    let mut lines = Vec::new();
    for candidate in candidates {
        let mut rest = candidate.as_str();
        while width(face, rest, size)? > limit && rest.chars().count() > 1 {
            // The initial breaker already bounded this row. Search its scalar boundaries for
            // a fitting prefix; an indivisible glyph wider than the box occupies a row alone.
            let ends: Vec<_> = rest.char_indices().skip(1).map(|(i, _)| i).chain(std::iter::once(rest.len())).collect();
            let (mut lo, mut hi) = (1, ends.len());
            while lo < hi {
                let middle = lo + (hi - lo).div_ceil(2);
                if width(face, &rest[..ends[middle - 1]], size)? <= limit {
                    lo = middle;
                } else {
                    hi = middle - 1;
                }
            }
            let end = ends[lo - 1];
            lines.push(rest[..end].to_owned());
            rest = &rest[end..];
        }
        lines.push(rest.to_owned());
    }
    Some(lines)
}

pub(super) struct Row {
    pub resource: String,
    pub glyphs: Vec<(u16, [f64; 2])>,
    pub advance: f64,
}

pub(super) fn rows(doc: &mut Document, annotation: &Dict, text: &str, size: f64, width: f64) -> Option<(Dict, Vec<Row>)> {
    let face = face(text)?;
    let lines = wrap(face, text, size, width)?;
    let shaped = lines.iter().map(|line| source_shaping::shape(face.bytes, line)).collect::<Result<Vec<_>, _>>().ok()?;
    if shaped.iter().map(|run| run.glyphs.len()).sum::<usize>() > MAX_GLYPHS {
        return None;
    }
    let mut keys = HashMap::new();
    let mut mappings = Vec::new();
    let mut rows = Vec::new();
    for run in shaped {
        let mut glyphs = Vec::new();
        for glyph in run.glyphs {
            let key = (glyph.text.clone(), glyph.id.to_u32());
            let code = match keys.get(&key) {
                Some(code) => *code,
                None => {
                    let code = u16::try_from(mappings.len().checked_add(1)?).ok()?;
                    mappings.push((glyph.text, glyph.id));
                    keys.insert(key, code);
                    code
                }
            };
            glyphs.push((code, glyph.position));
        }
        rows.push(Row { resource: String::new(), glyphs, advance: run.advance });
    }
    // Re-editing can reuse the previous font resource; the embedding library also reuses
    // matching full programs elsewhere in the document, so each comment need not copy a font.
    let mut fonts = annotation
        .get(b"AP")
        .and_then(|ap| doc.dict(ap))
        .and_then(|ap| ap.get(b"N").cloned())
        .map(|normal| doc.resolve(&normal))
        .and_then(|normal| match normal.as_ref() {
            Object::Stream(stream) => stream.dict.get(b"Resources").and_then(|r| doc.dict(r)),
            _ => None,
        })
        .and_then(|resources| resources.get(b"Font").and_then(|f| doc.dict(f)))
        .unwrap_or_default();
    let resource = embedding::embed_mapping(doc, &mut fonts, face.bytes, face.family, &mappings).ok()?;
    for row in &mut rows {
        row.resource.clone_from(&resource);
    }
    // Retain only the font this appearance actually uses; old resources stay available to
    // earlier incremental revisions, without growing every replacement appearance's dictionary.
    let current = fonts.get(resource.as_bytes())?.clone();
    let mut used = Dict::new();
    used.set(resource.into_bytes(), current);
    Some((used, rows))
}
