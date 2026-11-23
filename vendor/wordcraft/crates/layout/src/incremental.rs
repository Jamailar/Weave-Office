//! Re-shape a locally edited paragraph when the existing pagination still fits.
use super::{Arc, DocLayout, Document, FieldCtx, LayoutCache, LayoutOptions, Path, Placed, StoryRef, ViewMode, cache, para, styleref};

#[derive(Clone, Debug)]
pub(crate) struct ParagraphMeasure {
    pub width: f32,
    pub grid: para::Grid,
    pub cell: Option<Arc<para::CellText>>,
}

impl DocLayout {
    /// The caller verified that only the text/runs at this path changed and that the previous
    /// layout has the same view settings. AutoFit, frames, split pieces and text-dependent
    /// headers need the full pagination pass when a paragraph changes.
    pub fn reflow_text(&self, doc: &Document, cache: &LayoutCache, opts: &LayoutOptions, path: &Path) -> Option<Self> {
        self.reflow_text_after(doc, cache, opts, path, None)
    }

    /// As reflow_text, with the changed byte range in the new paragraph. The caller verifies both unchanged spans and run formatting.
    pub fn reflow_text_after(
        &self,
        doc: &Document,
        cache: &LayoutCache,
        opts: &LayoutOptions,
        path: &Path,
        changed: Option<std::ops::Range<usize>>,
    ) -> Option<Self> {
        let started = super::now_ms();
        if opts.view != ViewMode::Print || opts.comment_balloons || cache.env != cache::env_hash(doc, opts) || styleref::headers_use_styleref(doc) {
            return None;
        }
        let story = StoryRef::Body;
        let p = doc.para(story, path)?;
        let pieces = self.index.get(&(story, path.clone()))?;
        let (pi, ii) = pieces.last()?;
        let page = self.pages.get(*pi)?;
        let Placed::Lines { para: old, l0, l1, turn, .. } = page.items.get(*ii)? else { return None };
        let measure = old.reflow.as_ref()?;
        if *l1 != old.lines.len()
            || turn.is_turned()
            || old.rp.frame.is_some()
            || old.rp.drop_cap != 0
            || old.text_len == 0
            || p.text.is_empty()
            || !p.objects.is_empty()
            || p.section.is_some()
        {
            return None;
        }
        let mut blocks = &doc.body;
        for step in path.parent().chunks_exact(3) {
            let [b, r, c] = step else { return None };
            let table = blocks.get(*b as usize)?.as_table()?;
            let cell = table.rows.get(*r as usize)?.cells.get(*c as usize)?;
            if !table.props.fixed || cell.props.text_direction.is_turned() {
                return None;
            }
            blocks = &cell.blocks;
        }
        let fields = FieldCtx::default();
        let env = para::ParaEnv {
            doc,
            width: measure.width,
            grid: measure.grid,
            table: measure.cell.as_deref(),
            fields: &fields,
            label: None,
            show_hidden: opts.show_hidden,
            field_codes: opts.field_codes,
            hide_deleted: opts.hides_deleted(story),
            proofing: opts.proofing.then_some(opts.proof),
            typing_word: opts.typing_word.filter(|w| w.rev == p.rev).map(|w| (w.start, w.end)),
            numeral: opts.numeral,
            exclusions: &[],
            eq_number: 0,
            read_line_spacing: None,
            fit_objects: false,
            fitted: &[],
        };
        if pieces.len() > 1 {
            if path.0.len() != 1
                || measure.cell.is_some()
                || !doc.parts.is_empty()
                || !doc.body.iter().all(|b| b.as_para().is_some_and(|p| p.objects.is_empty() && p.section.is_none()))
            {
                return None;
            }
            let mut updated = reflow_window(self, pieces, p, &env, changed?, old.text_len)?;
            updated.ms = super::now_ms() - started;
            return Some(updated);
        }
        if *l0 != 0 {
            return None;
        }
        let mut new = para::layout_para(p, &env);
        // Widths within a line may change. Everything pagination, row splitting and
        // neighbouring blocks use to position text must still agree.
        let metrics = |l: &para::Line| (l.top, l.height, l.baseline, l.text_asc, l.text_desc, l.end, l.beside, l.left, l.right);
        if old.rp != new.rp
            || old.height != new.height
            || old.lines.len() != new.lines.len()
            || !old.lines.iter().zip(&new.lines).all(|(a, b)| metrics(a) == metrics(b))
        {
            return None;
        }
        new.reflow = Some(measure.clone());
        let mut updated = self.clone();
        let page = Arc::make_mut(updated.pages.get_mut(*pi)?);
        let Placed::Lines { para, .. } = page.items.get_mut(*ii)? else { return None };
        *para = Arc::new(new);
        updated.ms = super::now_ms() - started;
        Some(updated)
    }
}

/// Rewrap from a certified line before the edit. Once a later page starts on
/// the same shaped text with the same geometry, its remaining pages only move
/// in byte space. A disagreement at either seam invokes normal pagination.
fn reflow_window(
    layout: &DocLayout,
    pieces: &[(usize, usize)],
    p: &wordcraft_doc::Paragraph,
    env: &para::ParaEnv<'_>,
    changed: std::ops::Range<usize>,
    old_len: usize,
) -> Option<DocLayout> {
    let piece = |i: usize| {
        let &(pi, ii) = pieces.get(i)?;
        let Placed::Lines { para, l0, l1, .. } = layout.pages.get(pi)?.items.get(ii)? else { return None };
        Some((para, *l0, *l1))
    };
    let (last, _, _) = piece(pieces.len().checked_sub(1)?)?;
    if p.len() <= para::MAX_PROOF_BYTES
        || last.safe_lines.is_empty()
        || last.rp.bidi
        || last.rp.indent_first != 0.0
        || last.rp.align != wordcraft_doc::props::Align::Left
        || env.doc.settings.auto_hyphenation
        || last.label.is_some()
        || last.drop_cap.is_some()
        || last.rp.borders.is_some()
        || !last.issues.is_empty()
        || !last.hyph_after.is_empty()
        || !last.plain_reflow
        || !p.text.get(changed.clone())?.bytes().all(|b| b.is_ascii_alphanumeric() || b == b' ')
    {
        return None;
    }
    let [run] = p.runs.as_slice() else { return None };
    let page_at = |off: usize| {
        let mut lo = 0;
        let mut hi = pieces.len() - 1;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let (para, _, l1) = piece(mid)?;
            if para.line_of(off) < l1 { hi = mid } else { lo = mid + 1 }
        }
        Some(lo)
    };
    let delta = isize::try_from(p.len()).ok()?.checked_sub(isize::try_from(old_len).ok()?)?;
    let old_suffix = changed.end.checked_add_signed(-delta)?;
    let first = page_at(changed.start.saturating_sub(1))?;
    let (focus, focus_l0, _) = piece(first)?;
    let first_byte = focus.lines.get(focus_l0)?.start;
    // Two overlapping shaping windows are required even on short final pages.
    let before = first_byte.saturating_sub(8192);
    let mut saved = page_at(before)?;
    let (start, anchor_line) = loop {
        let (part, l0, l1) = piece(saved)?;
        let min = part.lines.get(l0)?.start;
        let max = part.lines.get(l1.checked_sub(1)?)?.start.min(before);
        if let Some(off) =
            part.safe_lines.partition_point(|&s| s <= max).checked_sub(1).and_then(|i| part.safe_lines.get(i)).copied().filter(|&s| s >= min)
        {
            break (off, part.first_line + part.line_of(off));
        }
        if saved == 0 {
            break (0, 0);
        }
        saved -= 1;
    };
    let resume = (page_at(old_suffix.saturating_add(8192))? + 1).min(pieces.len());
    let end = if resume + 2 < pieces.len() {
        let (part, l0, _) = piece(resume + 2)?;
        part.lines.get(l0)?.start.checked_add_signed(delta)?
    } else {
        p.len()
    };
    if end.checked_sub(start)? > 65536 || start > changed.start {
        return None;
    }
    let mut fragment = wordcraft_doc::Paragraph::with_text(p.text.get(start..end)?, run.props.clone());
    fragment.props = p.props.clone();
    fragment.mark = p.mark.clone();
    let mut new = para::layout_para(&fragment, &para::ParaEnv { proofing: None, label: None, ..*env });
    if new.rp != last.rp || (start != 0 && new.safe_lines.first() != Some(&0)) {
        return None;
    }
    for line in &mut new.lines {
        line.start += start;
        line.stop += start;
    }
    for c in &mut new.clusters {
        c.start += start;
        c.end += start;
    }
    for byte in Arc::make_mut(&mut new.safe_lines) {
        *byte += start;
    }
    new.text_len = p.len();
    new.height = last.height;
    new.first_line = anchor_line;
    new.reflow = last.reflow.clone();
    let mut placements = Vec::new();
    for i in saved..resume {
        let (part, l0, l1) = piece(i)?;
        let from = (part.first_line + l0).max(anchor_line);
        let to = part.first_line + l1;
        for absolute in from..to {
            let was = part.lines.get(absolute - part.first_line)?;
            let now = new.lines.get_mut(absolute - anchor_line)?;
            if (now.height, now.text_asc, now.text_desc, now.end, now.left, now.right)
                != (was.height, was.text_asc, was.text_desc, was.end, was.left, was.right)
                || (i < first && (now.start, now.stop, &now.xs) != (was.start, was.stop, &was.xs))
            {
                return None;
            }
            now.top = was.top;
            now.baseline = was.baseline;
        }
        if i >= first {
            placements.push((from - anchor_line, to - anchor_line));
        }
    }
    if new.lines.get(placements.first()?.0)?.start != first_byte {
        return None;
    }
    if resume == pieces.len() {
        if placements.last()?.1 != new.lines.len() {
            return None;
        }
    } else {
        // Certify the seam *before* the first reused page. Equal advances alone
        // would not detect a context-dependent glyph substitution of equal width.
        let (next, l0, _) = piece(resume)?;
        let next_line = next.lines.get(l0)?;
        let first_reused = new.lines.get((next.first_line + l0).checked_sub(anchor_line)?)?;
        if next_line.start.checked_add_signed(delta)? != first_reused.start || next_line.xs != first_reused.xs {
            return None;
        }
        let (prev, l0, l1) = piece(resume.checked_sub(1)?)?;
        let lower = prev.lines.get(l0)?.start.max(old_suffix);
        let upper = prev.lines.get(l1.checked_sub(1)?)?.start;
        let safe = &prev.safe_lines;
        let certified = lower <= upper
            && safe
                .get(safe.partition_point(|s| *s < lower)..safe.partition_point(|s| *s <= upper))?
                .iter()
                .any(|s| s.checked_add_signed(delta).is_some_and(|byte| new.safe_lines.binary_search(&byte).is_ok()));
        if !certified {
            return None;
        }
    }
    let new = Arc::new(new);
    let mut updated = layout.clone();
    for (i, &(pi, ii)) in pieces.iter().enumerate().skip(first) {
        let (para, a, b) = if let Some(&(a, b)) = placements.get(i - first) {
            (new.clone(), a, b)
        } else {
            let (part, l0, l1) = piece(i)?;
            let (copy, a, b) = part.translate_piece(l0, l1, delta, p.len())?;
            (Arc::new(copy), a, b)
        };
        let page = Arc::make_mut(updated.pages.get_mut(pi)?);
        let Placed::Lines { para: value, l0, l1, .. } = page.items.get_mut(ii)? else { return None };
        *value = para;
        *l0 = a;
        *l1 = b;
    }
    Some(updated)
}
