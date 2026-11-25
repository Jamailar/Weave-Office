//! Selection summaries used by the status bar, ribbon and floating toolbar while the page scrolls.
use crate::WordApp;
use serde_json::Value;
use std::cell::OnceCell;
use std::sync::Arc;
use wordcraft_doc::Pos;
use wordcraft_engine::{Selection, Session};

#[derive(Default)]
pub(crate) struct SelectionInfo {
    id: u64,
    rev: u64,
    selection: Selection,
    segments: Option<Arc<[(Pos, Pos)]>>,
    cells: bool,
    words: OnceCell<usize>,
    format: OnceCell<Value>,
    objects: OnceCell<SelectionObjects>,
}

#[derive(Default, Copy, Clone)]
pub(crate) struct SelectionObjects {
    pub picture: bool,
    pub shape: bool,
    pub smart_art: bool,
    pub chart: bool,
}

impl SelectionObjects {
    pub(crate) fn of(s: &Session) -> Self {
        use wordcraft_doc::InlineObject;
        match wordcraft_engine::cmd::objects::selected(s) {
            Some((_, InlineObject::Image { .. })) => Self { picture: true, ..Default::default() },
            Some((_, InlineObject::Graphic { graphic, .. })) => {
                Self { smart_art: graphic.smart_art.is_some(), chart: graphic.chart.is_some(), ..Default::default() }
            }
            Some((_, o)) => Self { shape: matches!(o, InlineObject::Shape { .. }) || o.is_canvas(), ..Default::default() },
            None => Self::default(),
        }
    }
}

impl WordApp {
    fn with_selection_info<T>(&self, read: impl FnOnce(&SelectionInfo, &Session) -> T) -> T {
        let s = &self.session;
        let segments = s.shared_column_segments();
        let cells = s.cell_range().is_some();
        let mut cached = self.canvas.selection_info.borrow_mut();
        if cached.as_ref().is_some_and(|c| {
            c.id != s.document_id()
                || c.rev != s.rev()
                || c.selection != s.sel
                || c.cells != cells
                || match (&c.segments, segments) {
                    (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
                    (None, None) => false,
                    _ => true,
                }
        }) {
            *cached = None;
        }
        let info = cached.get_or_insert_with(|| SelectionInfo {
            id: s.document_id(),
            rev: s.rev(),
            selection: s.sel.clone(),
            segments: segments.cloned(),
            cells,
            ..Default::default()
        });
        read(info, s)
    }

    pub(crate) fn selected_word_count(&self) -> usize {
        self.with_selection_info(|c, s| *c.words.get_or_init(|| s.selected_word_count()))
    }

    pub(crate) fn displayed_format(&self) -> Value {
        // A collapsed caret can have pending formatting without an edit.
        if self.session.sel.is_collapsed() {
            return wordcraft_engine::cmd::format::state(&self.session);
        }
        self.with_selection_info(|c, s| c.format.get_or_init(|| wordcraft_engine::cmd::format::state(s)).clone())
    }

    pub(crate) fn selection_objects(&self) -> SelectionObjects {
        self.with_selection_info(|c, s| *c.objects.get_or_init(|| SelectionObjects::of(s)))
    }
}
