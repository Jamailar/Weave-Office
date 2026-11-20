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
        let started = super::now_ms();
        if opts.view != ViewMode::Print || opts.comment_balloons || cache.env != cache::env_hash(doc, opts) || styleref::headers_use_styleref(doc) {
            return None;
        }
        let story = StoryRef::Body;
        let p = doc.para(story, path)?;
        let [(pi, ii)] = self.index.get(&(story, path.clone()))?.as_slice() else { return None };
        let page = self.pages.get(*pi)?;
        let Placed::Lines { para: old, l0, l1, turn, .. } = page.items.get(*ii)? else { return None };
        let measure = old.reflow.as_ref()?;
        if *l0 != 0
            || *l1 != old.lines.len()
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
