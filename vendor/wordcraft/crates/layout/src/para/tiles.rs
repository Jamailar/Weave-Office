//! Intern repeated runs of wrapped text and paginate their line metrics using
//! the regular page fitter. Only pages actually read need glyph/cluster arrays.
use super::{Arc, Line, LineEnd, ParaEnv, ParaLayout, Paragraph, ParagraphGeometry, layout_para};
use std::{collections::HashMap, ops::Range};
const WINDOW: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub(super) struct Tile {
    first_line: usize,
    byte: usize,
    layout: Arc<ParaLayout>,
}

#[derive(Default)]
pub(crate) struct TileCache {
    settings: String,
    windows: HashMap<Box<str>, Arc<ParaLayout>>,
}

/// The same run formatting and paragraph settings apply to every window. Restart
/// only at an actual soft wrap on which both shaped windows allow concatenation.
pub(crate) fn tiled_paragraph(p: &Paragraph, env: &ParaEnv<'_>, previous: &mut TileCache, document_env: u64) -> Option<ParaLayout> {
    if p.len() <= super::MAX_PROOF_BYTES
        || env.grid.line.is_some()
        || env.grid.chars.is_some()
        || env.doc.settings.auto_hyphenation
        || !p.objects.is_empty()
        || env.label.is_some()
        || !env.exclusions.is_empty()
        || !p.text.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
    {
        return None;
    }
    let [run] = p.runs.as_slice() else { return None };
    // Avoid allocating extra windows for ordinary prose that doesn't repeat.
    let repeats = |at: usize| p.text.get(at..at + 256).zip(p.text.get(at + 1..at + 2 * WINDOW)).is_some_and(|(needle, rest)| rest.contains(needle));
    if !repeats(0) && !repeats(WINDOW / 2) {
        return None;
    }
    let settings = serde_json::to_string(&(document_env, env.width.to_bits(), &p.props, &p.mark, &run.props, &env.doc.settings)).ok()?;
    if settings != previous.settings {
        previous.windows.clear();
        previous.settings = settings;
    }
    let env = ParaEnv { proofing: None, label: None, ..*env };
    let mut cache: HashMap<&str, Arc<ParaLayout>> = HashMap::new();
    let mut geometry = ParagraphGeometry::default();
    let mut safe = Vec::new();
    let mut top = 0.0;
    let mut byte = 0;
    while byte < p.len() {
        let end = if p.len() - byte <= WINDOW + WINDOW / 2 { p.len() } else { byte + WINDOW };
        let text = p.text.get(byte..end)?;
        let tile = if let Some(saved) = cache.get(text) {
            saved.clone()
        } else {
            // A new revision with little repetition uses the general shaper.
            if cache.len() >= 32 {
                return None;
            }
            if let Some(old) = previous.windows.get(text) {
                cache.insert(text, old.clone());
                old.clone()
            } else {
                let mut fragment = Paragraph::with_text(text, run.props.clone());
                fragment.props = p.props.clone();
                fragment.mark = p.mark.clone();
                let tile = Arc::new(layout_para(&fragment, &env));
                if tile.safe_lines.first() != Some(&0)
                    || tile.rp.indent_first != 0.0
                    || tile.rp.bidi
                    || tile.rp.align != wordcraft_doc::props::Align::Left
                    || tile.rp.frame.is_some()
                    || tile.rp.borders.is_some()
                    || tile.rp.drop_cap != 0
                    || !tile.hyph_after.is_empty()
                    || !tile.left_out.is_empty()
                {
                    return None;
                }
                cache.insert(text, tile.clone());
                tile
            }
        };
        if let Some(first) = geometry.tiles.first() {
            let other = &first.layout.styles;
            if other.len() != tile.styles.len() || !other.iter().zip(&tile.styles).all(|(a, b)| a.face.id() == b.face.id() && a.rc == b.rc) {
                return None;
            }
        }
        let cut = if end == p.len() {
            text.len()
        } else {
            let n = tile.safe_lines.partition_point(|&b| b <= WINDOW - WINDOW / 4);
            // Restart inside ordinary Latin text or after a space. Earlier
            // punctuation can influence numeric and punctuation break rules.
            *tile.safe_lines.get(..n)?.iter().rev().find(|&&off| {
                text.as_bytes().get(off.saturating_sub(1)..off.saturating_add(2)).is_some_and(|s| {
                    matches!(s, [prev, first, next] if (prev.is_ascii_alphabetic() || *prev == b' ') && first.is_ascii_alphabetic() && next.is_ascii_alphabetic())
                })
            })?
        };
        if cut < WINDOW / 4 && end != p.len() {
            return None;
        }
        let first = tile.lines.first()?;
        if first.top != 0.0 || !first.height.is_finite() || first.height <= 0.0 {
            return None;
        }
        let first_line = geometry.lines.len();
        for l in tile.lines.iter().take_while(|l| l.start < cut) {
            if (l.height, l.text_asc, l.text_desc) != (first.height, first.text_asc, first.text_desc)
                || l.baseline != l.top + first.baseline
                || l.beside
                || !l.vis.is_empty()
                || !l.leaders.is_empty()
                || l.hyphen.is_some()
            {
                return None;
            }
            // Pagination reads only the line metrics. Page placements recover all
            // drawing and hit-testing data from this tile via page_geometry.
            geometry.lines.push(Line {
                top,
                height: l.height,
                baseline: top + first.baseline,
                text_asc: l.text_asc,
                text_desc: l.text_desc,
                start: byte + l.start,
                stop: byte + l.stop,
                left: l.left,
                right: l.right,
                end: l.end,
                c0: 0,
                c1: 0,
                xs: Vec::new(),
                leaders: Vec::new(),
                hyphen: None,
                beside: false,
                vis: Vec::new(),
                rtl: false,
            });
            top += l.height;
        }
        safe.extend(tile.safe_lines.iter().take_while(|&&s| s < cut).map(|s| byte + s));
        geometry.tiles.push(Tile { byte, first_line, layout: tile });
        byte += cut;
    }
    if geometry.lines.last()?.end != LineEnd::Para {
        return None;
    }
    let mut paragraph = (*geometry.tiles.first()?.layout).clone();
    paragraph.plain_reflow = geometry.tiles.iter().all(|tile| tile.layout.plain_reflow);
    paragraph.geometry = Arc::new(geometry);
    paragraph.safe_lines = Arc::new(safe);
    paragraph.height = top;
    paragraph.text_len = p.len();
    previous.windows = cache.into_iter().map(|(s, tile)| (Box::from(s), tile)).collect();
    Some(paragraph)
}

pub(super) fn page_geometry(base: &ParagraphGeometry, range: Range<usize>, delta: isize) -> ParagraphGeometry {
    let mut out = ParagraphGeometry::default();
    for i in range {
        let Some(metrics) = base.lines.get(i) else { break };
        let n = base.tiles.partition_point(|t| t.first_line <= i);
        let Some(tile) = n.checked_sub(1).and_then(|n| base.tiles.get(n)) else { break };
        let source = &tile.layout;
        let Some(line) = source.lines.get(i - tile.first_line) else { break };
        let offset = |byte: usize| (byte + tile.byte).saturating_add_signed(delta);
        let mut l = line.clone();
        l.top = metrics.top;
        l.baseline = metrics.baseline;
        l.start = offset(l.start);
        l.stop = offset(l.stop);
        l.c0 = out.clusters.len();
        for c in source.clusters.get(line.c0..line.c1).into_iter().flatten() {
            let mut target = c.clone();
            target.start = offset(c.start);
            target.end = offset(c.end);
            target.g0 = out.glyphs.len() as u32;
            out.glyphs.extend_from_slice(source.glyphs.get(c.g0 as usize..c.g1 as usize).unwrap_or_default());
            target.g1 = out.glyphs.len() as u32;
            out.clusters.push(target);
        }
        l.c1 = out.clusters.len();
        out.lines.push(l);
    }
    out
}
