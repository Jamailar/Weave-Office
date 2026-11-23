//! Share immutable text geometry between revisions. After wrapping converges,
//! unchanged continuation pages need only a byte translation. Materialize the
//! translated clusters when a consumer actually reads that page.
use super::{Arc, Cluster, Glyph, Line, ParaLayout};
use std::{
    ops::{Deref, DerefMut, Range},
    sync::OnceLock,
};

#[derive(Clone, Debug, Default)]
pub struct ParagraphGeometry {
    pub glyphs: Vec<Glyph>,
    pub clusters: Vec<Cluster>,
    pub lines: Vec<Line>,
    pub(super) tiles: Vec<super::tiles::Tile>,
}

#[derive(Debug)]
pub(super) struct PageSlice {
    pub(super) lines: Range<usize>,
    pub(super) delta: isize,
    cached: OnceLock<Arc<ParagraphGeometry>>,
}

impl PageSlice {
    fn get<'a>(&'a self, base: &ParagraphGeometry) -> &'a Arc<ParagraphGeometry> {
        self.cached.get_or_init(|| {
            if !base.tiles.is_empty() {
                return Arc::new(super::tiles::page_geometry(base, self.lines.clone(), self.delta));
            }
            let mut lines = base.lines.get(self.lines.clone()).unwrap_or_default().to_vec();
            let c0 = lines.first().map_or(0, |l| l.c0);
            let c1 = lines.last().map_or(c0, |l| l.c1);
            let mut clusters = base.clusters.get(c0..c1).unwrap_or_default().to_vec();
            let g0 = clusters.first().map_or(0, |c| c.g0);
            let g1 = clusters.last().map_or(g0, |c| c.g1);
            let glyphs = base.glyphs.get(g0 as usize..g1 as usize).unwrap_or_default().to_vec();
            for c in &mut clusters {
                c.start = c.start.saturating_add_signed(self.delta);
                c.end = c.end.saturating_add_signed(self.delta);
                c.g0 -= g0;
                c.g1 -= g0;
            }
            for l in &mut lines {
                l.start = l.start.saturating_add_signed(self.delta);
                l.stop = l.stop.saturating_add_signed(self.delta);
                l.c0 -= c0;
                l.c1 -= c0;
            }
            Arc::new(ParagraphGeometry { lines, clusters, glyphs, ..Default::default() })
        })
    }
}

impl Deref for ParaLayout {
    type Target = ParagraphGeometry;
    fn deref(&self) -> &ParagraphGeometry {
        self.shifted.as_ref().map_or(&self.geometry, |s| s.get(&self.geometry))
    }
}

impl DerefMut for ParaLayout {
    fn deref_mut(&mut self) -> &mut ParagraphGeometry {
        if let Some(s) = self.shifted.take() {
            self.geometry = s.get(&self.geometry).clone();
        }
        Arc::make_mut(&mut self.geometry)
    }
}

impl ParaLayout {
    pub(crate) fn page_piece(self: &Arc<Self>, l0: usize, l1: usize) -> (Arc<Self>, usize, usize) {
        if !self.geometry.tiles.is_empty()
            && let Some((p, a, b)) = self.translate_piece(l0, l1, 0, self.text_len)
        {
            return (Arc::new(p), a, b);
        }
        (self.clone(), l0, l1)
    }

    /// Include one neighbouring line at both ends for caret affinity and the
    /// paragraph mark. Collapse repeated translations onto the same source.
    pub(crate) fn translate_piece(&self, l0: usize, l1: usize, delta: isize, len: usize) -> Option<(Self, usize, usize)> {
        let offset = self.shifted.as_ref().map_or(0, |s| s.lines.start);
        let a = l0.checked_add(offset)?;
        let b = l1.checked_add(offset)?;
        let lines = a.saturating_sub(1)..b.saturating_add(1).min(self.geometry.lines.len());
        let previous = self.shifted.as_ref().map_or(0, |s| s.delta);
        let shift = previous.checked_add(delta)?;
        let first = self.geometry.lines.get(lines.start)?.start.saturating_add_signed(previous);
        let end = self.geometry.lines.get(lines.end.checked_sub(1)?)?.stop.saturating_add_signed(previous);
        let safe = &self.safe_lines;
        let safe = &safe[safe.partition_point(|s| *s < first)..safe.partition_point(|s| *s <= end)];
        let mut copy = self.clone();
        copy.safe_lines = Arc::new(safe.iter().map(|s| s.saturating_add_signed(delta)).collect());
        copy.text_len = len;
        copy.first_line = self.first_line.checked_add(l0)?.checked_sub(a - lines.start)?;
        let local = (a - lines.start, b - lines.start);
        copy.shifted = Some(Arc::new(PageSlice { lines, delta: shift, cached: OnceLock::new() }));
        Some((copy, local.0, local.1))
    }
}
