use super::{Counters, Document, LevelSuffix, NumFormat, Path, Pos, StoryRef, Stroke, Tokens, Ui, WordApp, json, regular, semibold, vec2};

/// Draws the Navigation pane: search box and tabs, page list, find results and heading outline.
pub(super) fn nav(app: &mut WordApp, ui: &mut Ui) {
    let qid = app.window_id("nav_query");
    let mut q = ui.data(|d| d.get_temp::<String>(qid)).unwrap_or_default();
    // Ctrl+F with text selected searches for it, as Word does (a selection within one line).
    let focus = std::mem::take(&mut app.nav_search_focus);
    let picked = focus.then(|| selected_query(&app.session)).flatten();
    if let Some(t) = &picked {
        q = t.clone();
    }
    let mut out = egui::TextEdit::singleline(&mut q).hint_text(tl!("Search document")).desired_width(f32::INFINITY).show(ui);
    let r = out.response.clone();
    if focus {
        // Focused with its text selected, so typing replaces it.
        r.request_focus();
        let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(q.chars().count()));
        out.state.cursor.set_char_range(Some(all));
        out.state.store(ui.ctx(), r.id);
    }
    if r.changed() || picked.is_some() {
        app.ui.nav_tab = "results".into();
        ui.data_mut(|d| d.insert_temp(qid, q.clone()));
        if !q.is_empty() {
            let _ = app.session.run("edit.find", &json!({"text": q}));
        } else {
            app.session.find.results.clear();
        }
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for (k, l) in [("headings", "Headings"), ("pages", "Pages"), ("results", "Results")] {
            if ui.selectable_label(app.ui.nav_tab == k, tl!(l)).clicked() {
                app.ui.nav_tab = k.into();
            }
        }
    });
    ui.separator();
    match app.ui.nav_tab.as_str() {
        "pages" => {
            let layout = app.session.layout();
            egui::ScrollArea::vertical().id_salt(("nav_pages", app.session.document_id())).show_rows(
                ui,
                ui.spacing().interact_size.y,
                layout.pages.len(),
                |ui, rows| {
                    for i in rows {
                        let label = crate::i18n::fmt(tl!("Page {page}"), &[("page", &layout.pages[i].number.to_string())]);
                        if ui.selectable_label(app.session.page_hint == i, label).clicked() {
                            let _ = app.run("edit.goto", json!({"page": i + 1}));
                        }
                    }
                },
            );
        }
        "results" => {
            let results = &app.session.find.results;
            ui.label(egui::RichText::new(format!("{} results", results.len())).small().weak());
            let mut picked = None;
            let navigation = &mut app.canvas.navigation;
            let stamp = (app.session.document_id(), app.session.rev());
            if navigation.result_stamp != Some(stamp) || navigation.result_query != app.session.find.query {
                navigation.result_stamp = Some(stamp);
                navigation.result_query.clone_from(&app.session.find.query);
                navigation.results = RowHeights { next_scroll: Some(0.0), ..Default::default() };
            }
            navigation.results.show(ui, "nav_results", results.len().min(500), |ui, i| {
                let (a, b) = &results[i];
                let Some(p) = app.session.doc.para_at(a) else { return };
                let from = p.clamp(a.off.saturating_sub(30));
                let to = p.clamp(b.off.saturating_add(40).min(p.len()));
                let pre = p.text.get(from..a.off).unwrap_or("");
                let hit = p.text.get(a.off..b.off).unwrap_or("");
                let post = p.text.get(b.off..to).unwrap_or("");
                let mut job = egui::text::LayoutJob::default();
                let fmt = |c| egui::TextFormat { font_id: regular(12.0), color: c, ..Default::default() };
                job.append(pre, 0.0, fmt(ui.visuals().text_color()));
                // The match is marked the way Find marks it on the page, per theme (#656).
                let mark = Tokens::get(ui.ctx()).find_highlight;
                job.append(
                    hit,
                    0.0,
                    egui::TextFormat { font_id: semibold(12.0), background: mark, color: ui.visuals().text_color(), ..Default::default() },
                );
                job.append(post, 0.0, fmt(ui.visuals().text_color()));
                job.wrap.max_width = ui.available_width();
                if ui.add(egui::Button::new(job).frame(false)).clicked() {
                    picked = Some((a.clone(), b.clone()));
                }
                ui.separator();
            });
            if let Some((a, b)) = picked {
                let _ = app.run("select.range", json!({"anchor": a, "focus": b}));
            }
        }
        _ => headings(app, ui),
    }
}

/// Ctrl+F accepts a single line of at most 255 characters. Avoid copying whole
/// paragraphs or constructing a clipboard document to reject a large selection.
fn selected_query(session: &wordcraft_engine::Session) -> Option<String> {
    let (a, b) = match session.column_segments() {
        Some([(a, b)]) => (a.min(b), a.max(b)),
        Some(_) => return None,
        None => ((&session.sel.anchor).min(&session.sel.focus), (&session.sel.anchor).max(&session.sel.focus)),
    };
    if a == b || a.story != b.story || a.path.0.first() != b.path.0.first() || (a.path != b.path && a.path.parent() == b.path.parent()) {
        return None;
    }
    let eligible = |s: &str| !s.is_empty() && s.len() <= 255 * 4 && !s.contains(['\n', '\r']) && s.chars().take(256).count() <= 255;
    if a.path == b.path {
        let para = session.doc.para_at(a)?;
        let text = para.text.get(a.off..b.off)?;
        if para.objects.is_empty() || !text.contains(wordcraft_doc::para::OBJ) {
            return eligible(text).then(|| text.to_owned());
        }
    }
    // Preserve the document model's inline object text and whole-table cell separators.
    let text = session.selected_text();
    eligible(&text).then_some(text)
}

/// Cached per window so navigating another view cannot change its collapse or scroll state.
#[derive(Default)]
pub(crate) struct Navigation {
    stamp: Option<(u64, u64)>,
    items: Vec<Heading>,
    visible: Vec<usize>,
    rows: RowHeights,
    results: RowHeights,
    result_query: String,
    result_stamp: Option<(u64, u64)>,
}

pub(super) struct Heading {
    path: Path,
    level: u8,
    pub(super) label: String,
    prefix_bytes: usize,
}

impl Navigation {
    fn refresh(&mut self, session: &wordcraft_engine::Session, collapsed: &std::collections::HashSet<Path>) {
        let stamp = (session.document_id(), session.rev());
        if self.stamp == Some(stamp) {
            return;
        }
        if let Some(edit) = session.text_edit()
            && self.stamp == Some((stamp.0, edit.previous))
            && let Some(p) = session.doc.para(StoryRef::Body, &edit.path)
        {
            match self.items.binary_search_by(|h| h.path.cmp(&edit.path)) {
                Ok(i) if !p.text.trim().is_empty() => {
                    let heading = &mut self.items[i];
                    heading.label.truncate(heading.prefix_bytes);
                    heading.label.push_str(p.text.trim());
                    self.stamp = Some(stamp);
                    return;
                }
                Err(_) if session.doc.styles.resolve_para(&p.props).outline_level.is_none() || p.text.trim().is_empty() => {
                    self.stamp = Some(stamp);
                    return;
                }
                _ => {}
            }
        }
        if self.stamp.is_none_or(|(id, _)| id != stamp.0) {
            self.rows = RowHeights { next_scroll: Some(0.0), ..Default::default() };
        }
        self.stamp = Some(stamp);
        self.rebuild_visible(collapsed, Some(nav_headings(&session.doc)));
    }

    fn has_children(&self, i: usize) -> bool {
        self.items.get(i + 1).is_some_and(|next| next.level > self.items[i].level)
    }

    fn rebuild_visible(&mut self, collapsed: &std::collections::HashSet<Path>, new_items: Option<Vec<Heading>>) {
        let first = self.rows.offsets.partition_point(|&y| y <= self.rows.scroll_y).saturating_sub(1);
        let anchor =
            self.visible.get(first).zip(self.rows.offsets.get(first)).map(|(&i, top)| (self.items[i].path.clone(), self.rows.scroll_y - top));
        let measured: std::collections::HashMap<_, _> =
            self.visible.iter().zip(&self.rows.heights).map(|(&i, &h)| (self.items[i].path.clone(), h)).collect();
        if let Some(items) = new_items {
            self.items = items;
        }
        let levels: Vec<_> = self.items.iter().map(|h| h.level).collect();
        let flags: Vec<_> = self.items.iter().enumerate().map(|(i, h)| self.has_children(i) && collapsed.contains(&h.path)).collect();
        self.visible = visible_headings(&levels, &flags).into_iter().enumerate().filter_map(|(i, shown)| shown.then_some(i)).collect();
        if !measured.is_empty() {
            self.rows.heights = self.visible.iter().map(|&i| measured.get(&self.items[i].path).copied().unwrap_or(self.rows.estimate)).collect();
            self.rows.offsets = vec![0.0; self.visible.len() + 1];
            self.rows.update_offsets(0);
            if let Some((path, within)) = anchor {
                let i = self.visible.partition_point(|&i| self.items[i].path <= path).saturating_sub(1);
                self.rows.next_scroll = self.rows.heights.get(i).map(|&height| self.rows.offsets[i] + within.min(height));
            }
        }
    }
}

fn headings(app: &mut WordApp, ui: &mut Ui) {
    let outline = &mut app.canvas.navigation;
    outline.refresh(&app.session, &app.ui.nav_collapsed);
    if outline.items.is_empty() {
        ui.label(egui::RichText::new(tl!("Create an interactive outline of your document.\n\nIt's a great way to keep track of where you are or quickly move your content around.\n\nTo get started, go to the Home tab and apply Heading styles to the headings in your document.")).weak());
        return;
    }
    let current = outline.items.partition_point(|h| h.path <= app.session.sel.focus.path).checked_sub(1);
    let mut toggle = None;
    let mut jump = None;
    outline.rows.show(ui, "nav_headings", outline.visible.len(), |ui, row| {
        let i = outline.visible[row];
        let h = &outline.items[i];
        let children = outline.items.get(i + 1).is_some_and(|next| next.level > h.level);
        let collapsed = children.then(|| app.ui.nav_collapsed.contains(&h.path));
        let (clicked, response) = nav_heading(ui, h.level, &h.label, current == Some(i), collapsed);
        if clicked {
            toggle = Some(h.path.clone());
        } else if response.clicked() {
            jump = Some(h.path.clone());
        }
    });
    if let Some(path) = toggle {
        if !app.ui.nav_collapsed.remove(&path) {
            app.ui.nav_collapsed.insert(path);
        }
        outline.rebuild_visible(&app.ui.nav_collapsed, None);
    }
    if let Some(path) = jump {
        let _ = app.run("caret.set", json!({"pos": Pos { story: StoryRef::Body, path, off: 0 }}));
        app.canvas.nav_to_caret = true;
        app.canvas.scroll_to_caret = false;
        app.canvas.want_focus = true;
    }
}

/// Wrapped rows keep their measured heights. Unvisited rows start at a single line.
#[derive(Default)]
struct RowHeights {
    estimate: f32,
    scroll_y: f32,
    next_scroll: Option<f32>,
    heights: Vec<f32>,
    offsets: Vec<f32>,
}

impl RowHeights {
    fn update_offsets(&mut self, start: usize) {
        for i in start..self.heights.len() {
            self.offsets[i + 1] = self.offsets[i] + self.heights[i];
        }
    }

    fn show(&mut self, ui: &mut Ui, id: &'static str, count: usize, mut row: impl FnMut(&mut Ui, usize)) {
        let mut scroll = egui::ScrollArea::vertical().id_salt(id);
        if let Some(y) = self.next_scroll.take() {
            scroll = scroll.vertical_scroll_offset(y);
        }
        scroll.show_viewport(ui, |ui, viewport| {
            self.scroll_y = viewport.min.y;
            let spacing = ui.spacing().item_spacing.y;
            let estimate = (ui.spacing().interact_size.y + spacing).max(1.0);
            self.estimate = estimate;
            if self.offsets.len() != count + 1 {
                self.heights = vec![estimate; count];
                self.offsets = (0..=count).map(|i| i as f32 * estimate).collect();
            }
            ui.set_height((self.offsets[count] - spacing).max(0.0));
            let first = self.offsets.partition_point(|&y| y <= viewport.min.y).saturating_sub(1).min(count);
            let end = self.offsets.partition_point(|&y| y <= viewport.max.y).saturating_add(1).min(count);
            let mut rect = ui.max_rect();
            rect.min.y += self.offsets[first];
            let mut dirty = None;
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                for i in first..end {
                    let y = ui.cursor().top();
                    ui.push_id(i, |ui| row(ui, i));
                    let height = (ui.cursor().top() - y).max(1.0);
                    if (height - self.heights[i]).abs() > 0.1 {
                        self.heights[i] = height;
                        dirty.get_or_insert(i);
                    }
                }
            });
            if let Some(start) = dirty {
                self.update_offsets(start);
                ui.ctx().request_repaint();
            }
        });
    }
}

/// Which headings the pane shows: those under a collapsed heading (every following heading
/// of a deeper level) are hidden (#1997).
pub(super) fn visible_headings(levels: &[u8], collapsed: &[bool]) -> Vec<bool> {
    let mut hide_below: Option<u8> = None;
    levels
        .iter()
        .zip(collapsed)
        .map(|(&lv, &c)| {
            if hide_below.is_some_and(|h| lv > h) {
                return false;
            }
            hide_below = c.then_some(lv);
            true
        })
        .collect()
}

/// The body's headings (paragraphs with an outline level) in document order, as (path, level,
/// text). A numbered heading starts with the number it shows (`2.1 Method`), as in Word.
pub(super) fn nav_headings(doc: &Document) -> Vec<Heading> {
    let mut counters = Counters::default();
    let mut items = Vec::new();
    let mut styles = std::collections::HashMap::new();
    let body = StoryRef::Body;
    doc.visit_paras_between(&doc.start_of(body), &doc.end_of(body), |path, p| {
        // Only these two inherited properties affect the outline. Resolve each style once.
        let style = p.props.style.as_deref().unwrap_or("Normal");
        let (outline, numbering) = if let Some(&cached) = styles.get(style) {
            cached
        } else {
            let base = doc.styles.resolve_para(&wordcraft_doc::ParaProps { style: p.props.style.clone(), ..Default::default() });
            let cached = (base.outline_level, base.numbering);
            styles.insert(style.to_owned(), cached);
            cached
        };
        let number = p
            .props
            .numbering
            .or(numbering)
            .filter(|n| n.num != 0)
            .and_then(|n| counters.next_label(&doc.numbering, n.num, n.level))
            .filter(|(label, def)| !label.is_empty() && !matches!(def.format, NumFormat::Bullet | NumFormat::None));
        if let Some(level) = p.props.outline_level.or(outline).filter(|l| *l < 9)
            && !style.starts_with("TOC")
        {
            let plain = p.plain_text();
            let text = plain.trim();
            if !text.is_empty() {
                let mut label = String::new();
                if let Some((number, def)) = number {
                    label.push_str(&number);
                    if def.suffix != LevelSuffix::Nothing {
                        label.push(' ');
                    }
                }
                let prefix_bytes = label.len();
                label.push_str(text);
                items.push(Heading { path: Path(path.to_vec()), level, label, prefix_bytes });
            }
        }
        true
    });
    items
}

/// Constrain each heading to the available pane width so long document text cannot
/// enlarge the panel. A heading too long for the width wraps onto further lines — a word
/// wider than the pane is broken too — and the row grows with it, so the label reads whole
/// instead of ending in an ellipsis (#3015).
pub(super) fn nav_heading(ui: &mut Ui, level: u8, text: &str, selected: bool, collapsed: Option<bool>) -> (bool, egui::Response) {
    ui.horizontal(|ui| {
        ui.add_space(level as f32 * 14.0);
        // The triangle is drawn in code; headings without children keep an empty slot so text aligns.
        let mut toggled = false;
        let (rect, tri) = ui.allocate_exact_size(vec2(14.0, ui.spacing().interact_size.y), egui::Sense::click());
        if let Some(c) = collapsed {
            let ctr = rect.center();
            let col = ui.visuals().text_color();
            let pts = if c {
                vec![ctr + vec2(-2.5, -4.0), ctr + vec2(-2.5, 4.0), ctr + vec2(3.0, 0.0)]
            } else {
                vec![ctr + vec2(-4.0, -2.5), ctr + vec2(4.0, -2.5), ctr + vec2(0.0, 3.0)]
            };
            ui.painter().add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
            toggled = tri.clicked();
        }
        let label = egui::RichText::new(text).font(if level == 0 { semibold(12.5) } else { regular(12.5) });
        let width = ui.available_width();
        let r = ui.add(
            // The trailing grow atom takes the spare width, so the text starts flush left
            // under its parent instead of centred, as in Word (#1199). A minimum height
            // keeps short headings on one row; a wrapped one becomes as tall as it needs.
            egui::Button::selectable(selected, (label, egui::Atom::grow())).wrap().min_size(vec2(width, ui.spacing().interact_size.y)),
        );
        (toggled, r)
    })
    .inner
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use wordcraft_doc::{CharProps, ListKind, Paragraph, para_block, props::NumRef};

    #[test]
    fn wrapped_lists_measure_only_viewport_rows() {
        let ctx = egui::Context::default();
        let mut rows = RowHeights::default();
        let mut draws = 0;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(200.0, 300.0));
        for _ in 0..3 {
            draws = 0;
            ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                rows.show(ui, "wrapped_test", 10000, |ui, i| {
                    draws += 1;
                    ui.allocate_exact_size(vec2(60.0, if i == 0 { 90.0 } else { 20.0 }), egui::Sense::hover());
                });
            })
            .drop_without_applying_deltas();
        }
        assert!((2..30).contains(&draws));
        assert!(rows.offsets[1] >= 90.0);
        assert!(rows.offsets.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn outline_tracks_text_edits_and_structural_changes() {
        let mut doc = Document::new();
        let mut h = Paragraph::with_text("Heading", CharProps::default()).styled("Heading1");
        h.props.numbering = Some(NumRef { num: doc.numbering.add_list(ListKind::Legal), level: 0 });
        doc.body = vec![para_block(h), para_block(Paragraph::with_text("Body", CharProps::default()))];
        let mut s = wordcraft_engine::Session::new(doc);
        let mut index = Navigation::default();
        let collapsed = Default::default();
        index.refresh(&s, &collapsed);
        assert_eq!(index.items[0].label, "1. Heading");
        s.run("caret.set", &json!({"pos": Pos::new(StoryRef::Body, Path::top(1), 4)})).unwrap();
        s.run("text.insert", &json!({"text": " text"})).unwrap();
        let old = index.items.as_ptr();
        index.refresh(&s, &collapsed);
        assert_eq!(old, index.items.as_ptr(), "typing in body text keeps the outline");
        s.run("caret.set", &json!({"pos": Pos::new(StoryRef::Body, Path::top(0), 7)})).unwrap();
        s.run("text.insert", &json!({"text": " title"})).unwrap();
        index.refresh(&s, &collapsed);
        assert_eq!(index.items[0].label, "1. Heading title");
        s.run("edit.undo", &json!({})).unwrap();
        index.refresh(&s, &collapsed);
        assert_eq!(index.items[0].label, "1. Heading");
        let other = wordcraft_engine::Session::new(Document::from_text("No headings"));
        index.refresh(&other, &collapsed);
        assert!(index.items.is_empty());
    }
}
