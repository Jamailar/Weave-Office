//! Reuse bounded windows of long Latin runs. The overlap provides context;
//! harfrust must certify a cluster boundary in both windows before we splice.
use super::{Direction, Feature, FontFace, PlanFace, ShapedGlyph, UnicodeBuffer, shape_plan};
use std::{cell::RefCell, rc::Rc};

const BLOCK: usize = 2048;
const OVERLAP: usize = 32;

struct Window {
    text: Box<str>,
    glyphs: Vec<ShapedGlyph>,
    safe: Vec<bool>,
}

impl Window {
    fn boundary(&self, byte: usize) -> Option<usize> {
        let index = self.glyphs.partition_point(|g| g.cluster < byte);
        (self.glyphs.get(index)?.cluster == byte && *self.safe.get(index)?).then_some(index)
    }
}

#[derive(Default)]
struct Cache {
    face: Option<PlanFace>,
    features: Vec<Feature>,
    windows: std::collections::VecDeque<Rc<Window>>,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::default();
}

impl Cache {
    fn window(&mut self, face: &FontFace, text: &str, features: &[Feature]) -> Option<Rc<Window>> {
        if let Some(w) = self.windows.iter().find(|w| w.text.as_ref() == text) {
            return Some(w.clone());
        }
        let hb = face.hb()?;
        let shaper = face.shaper.shaper(&hb).instance(face.instance.as_ref()).build();
        let mut buf = UnicodeBuffer::new();
        for (i, c) in text.char_indices() {
            buf.add(c, i as u32);
        }
        buf.set_script(harfrust::script::LATIN);
        buf.set_direction(Direction::LeftToRight);
        buf.set_flags(harfrust::BufferFlags::PRODUCE_UNSAFE_TO_CONCAT);
        buf.guess_segment_properties();
        let plan = shape_plan(face, &shaper, &buf, features);
        let gb = shaper.shape(buf, harfrust::ShapeOptions::new().plan(plan.as_deref()).features(features));
        let mut glyphs = Vec::with_capacity(gb.len());
        let mut safe = Vec::with_capacity(gb.len());
        for (info, p) in gb.glyph_infos().iter().zip(gb.glyph_positions()) {
            glyphs.push(ShapedGlyph {
                gid: info.glyph_id,
                cluster: info.cluster as usize,
                x_advance: p.x_advance,
                x_offset: p.x_offset,
                y_offset: p.y_offset,
            });
            safe.push(!info.unsafe_to_concat());
        }
        // All glyphs in the cluster must allow concatenation. Only its first
        // glyph can denote a boundary, even for one-to-many substitutions.
        let mut remaining = safe.as_mut_slice();
        for cluster in glyphs.chunk_by(|a, b| a.cluster == b.cluster) {
            let (flags, rest) = remaining.split_at_mut(cluster.len());
            if let Some((first, others)) = flags.split_first_mut() {
                *first &= others.iter().all(|flag| *flag);
                others.fill(false);
            }
            remaining = rest;
        }
        let w = Rc::new(Window { text: text.into(), glyphs, safe });
        if self.windows.len() == 32 {
            self.windows.pop_front();
        }
        self.windows.push_back(w.clone());
        Some(w)
    }
}

/// Glyphs and the corresponding cluster starts at which the shaper permits splicing.
pub struct LatinRun {
    pub glyphs: Vec<ShapedGlyph>,
    pub safe_starts: Vec<bool>,
}

/// Shape an untransformed Latin run using reusable windows, or decline when the
/// script, feature ranges, or contextual shaping require an uninterrupted run.
pub fn shape_long_latin(face: &FontFace, text: &str, features: &[Feature]) -> Option<LatinRun> {
    if text.len() < BLOCK * 4
        || !text.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
        || !text.bytes().any(|b| b.is_ascii_alphabetic())
        || !features.iter().all(|f| f.start == 0 && f.end == u32::MAX)
    {
        return None;
    }
    CACHE.with(|slot| {
        let mut cache = slot.try_borrow_mut().ok()?;
        let id = Some(PlanFace::of(face));
        if cache.face != id || cache.features != features {
            cache.windows.clear();
            cache.face = id;
            cache.features = features.to_vec();
        }
        let mut base = 0;
        let mut first = 0;
        let mut current = cache.window(face, text.get(..BLOCK + OVERLAP)?, features)?;
        let mut out = Vec::with_capacity(text.len());
        let mut safe_starts = vec![false; text.len()];
        let mut append = |out: &mut Vec<ShapedGlyph>, w: &Window, range: std::ops::Range<usize>, base: usize| {
            let glyphs = &w.glyphs[range.clone()];
            for (g, safe) in glyphs.iter().zip(&w.safe[range]) {
                if *safe && let Some(slot) = safe_starts.get_mut(base + g.cluster) {
                    *slot = true;
                }
            }
            out.extend(glyphs.iter().map(|g| ShapedGlyph { cluster: g.cluster + base, ..*g }));
        };
        for split in (BLOCK..text.len()).step_by(BLOCK) {
            if base + current.text.len() == text.len() {
                break;
            }
            let next_base = split - OVERLAP;
            let end = (split + BLOCK + OVERLAP).min(text.len());
            let next = cache.window(face, text.get(next_base..end)?, features)?;
            let (a, b) = (split..(split + OVERLAP).min(text.len()))
                .find_map(|byte| Some((current.boundary(byte - base)?, next.boundary(byte - next_base)?)))?;
            append(&mut out, &current, first..a, base);
            current = next;
            first = b;
            base = next_base;
        }
        append(&mut out, &current, first..current.glyphs.len(), base);
        Some(LatinRun { glyphs: out, safe_starts })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windowed_latin_matches_uninterrupted_shaping() {
        let face = crate::word::resolve("Source Serif 4", false, false).face;
        let mut text = "Office Affinity AV To fl ffi !!! ".repeat(800);
        for insertion in [0, BLOCK - 1, BLOCK + 1, 4 * BLOCK, text.len()] {
            text.insert_str(insertion, "ffil");
            for features in [vec![], vec![crate::feature_value(b"kern", 0)], vec![crate::feature_value(b"liga", 0)]] {
                let run = shape_long_latin(&face, &text, &features).unwrap();
                assert_eq!(run.glyphs, crate::shape(&face, &text, &features, |c| c));
            }
        }
    }
}
