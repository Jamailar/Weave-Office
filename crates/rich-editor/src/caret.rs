//! Between document positions and the laid-out page: where a position shows, which position a
//! point or a galley cursor is, and how far a selection reaches on each line.

use crate::dom::Document;
use crate::edit::{self, Pos};
use crate::layout::{Item, Page, Seg, Target};
use egui::text::CCursor;
use egui::{Pos2, Rect, pos2};
use std::cmp::Ordering;

/// A caret on the page: a text item and a cursor in its galley.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub item: usize,
    pub cursor: CCursor,
}

fn map(page: &Page, item: usize) -> Option<(&[Seg], Pos2, &egui::Galley)> {
    match page.items.get(item)? {
        Item::Text {
            map, pos, galley, ..
        } if !map.is_empty() => Some((map, *pos, galley)),
        _ => None,
    }
}

/// Text items that hold editable text, in page order.
pub fn text_items(page: &Page) -> impl Iterator<Item = usize> + '_ {
    (0..page.items.len()).filter(|&i| map(page, i).is_some())
}

/// The document position a galley cursor stands for. At a boundary between two stretches the
/// caret belongs to the text before it, so typing continues that text's formatting.
pub fn pos_of(page: &Page, spot: Spot) -> Option<Pos> {
    let (segs, _, _) = map(page, spot.item)?;
    let c = spot.cursor.index.0;
    let at = |seg: &Seg, c: usize| match &seg.target {
        Target::Text { node, bytes } => {
            Pos::new(*node, bytes[(c - seg.chars.start).min(bytes.len() - 1)])
        }
        Target::Break { parent, index } => Pos::new(*parent, *index),
    };
    if let Some(seg) = segs
        .iter()
        .find(|s| matches!(s.target, Target::Text { .. }) && s.chars.start < c && c <= s.chars.end)
    {
        return Some(at(seg, c));
    }
    if let Some(seg) = segs
        .iter()
        .find(|s| s.chars.start <= c && (c < s.chars.end || s.chars.is_empty()))
    {
        return Some(at(seg, c));
    }
    let last = segs.last()?;
    Some(match &last.target {
        Target::Text { node, bytes } => Pos::new(*node, *bytes.last()?),
        Target::Break { parent, index } => Pos::new(*parent, *index),
    })
}

/// Where a document position shows.
pub fn spot_of(page: &Page, doc: &Document, pos: Pos) -> Option<Spot> {
    let found = |item: usize, c: usize| Spot {
        item,
        cursor: CCursor::new(c),
    };
    if doc.text(pos.node).is_some() {
        for item in text_items(page) {
            let (segs, _, _) = map(page, item)?;
            for seg in segs {
                if let Target::Text { node, bytes } = &seg.target
                    && *node == pos.node
                {
                    let k = match bytes.binary_search(&pos.offset) {
                        Ok(k) => k,
                        Err(k) => k.saturating_sub(1),
                    };
                    let k = k.min(seg.chars.len());
                    return Some(found(item, seg.chars.start + k));
                }
            }
        }
    } else {
        for item in text_items(page) {
            let (segs, _, _) = map(page, item)?;
            for seg in segs {
                if seg.target
                    == (Target::Break {
                        parent: pos.node,
                        index: pos.offset,
                    })
                {
                    return Some(found(item, seg.chars.start));
                }
            }
        }
    }
    // A position the page does not show (collapsed whitespace, an element boundary): the first
    // shown position after it, else the last before it.
    let mut best_after: Option<(Pos, Spot)> = None;
    let mut best_before: Option<(Pos, Spot)> = None;
    for item in text_items(page) {
        let (segs, _, _) = map(page, item)?;
        for seg in segs {
            let (start, end) = match &seg.target {
                Target::Text { node, bytes } => (
                    Pos::new(*node, bytes[0]),
                    Pos::new(*node, *bytes.last().unwrap_or(&0)),
                ),
                Target::Break { parent, index } => {
                    (Pos::new(*parent, *index), Pos::new(*parent, *index))
                }
            };
            if edit::compare(doc, start, pos) != Ordering::Less {
                if best_after.is_none_or(|(p, _)| edit::compare(doc, start, p) == Ordering::Less) {
                    best_after = Some((start, found(item, seg.chars.start)));
                }
            } else if edit::compare(doc, end, pos) != Ordering::Greater
                && best_before.is_none_or(|(p, _)| edit::compare(doc, end, p) == Ordering::Greater)
            {
                let end_char = match seg.target {
                    Target::Break { .. } => seg.chars.start,
                    _ => seg.chars.end,
                };
                best_before = Some((end, found(item, end_char)));
            }
        }
    }
    best_after.or(best_before).map(|(_, spot)| spot)
}

/// The caret rectangle of a spot, in page coordinates.
pub fn caret_rect(page: &Page, spot: Spot) -> Option<Rect> {
    let (_, pos, galley) = map(page, spot.item)?;
    Some(galley.pos_from_cursor(spot.cursor).translate(pos.to_vec2()))
}

/// The spot nearest a point on the page.
pub fn hit(page: &Page, point: Pos2) -> Option<Spot> {
    let mut best: Option<(f32, f32, usize)> = None;
    for item in text_items(page) {
        let rect = page.items[item].rect();
        let dy = if point.y < rect.min.y {
            rect.min.y - point.y
        } else if point.y > rect.max.y {
            point.y - rect.max.y
        } else {
            0.0
        };
        let dx = if point.x < rect.min.x {
            rect.min.x - point.x
        } else if point.x > rect.max.x {
            point.x - rect.max.x
        } else {
            0.0
        };
        if best.is_none_or(|(by, bx, _)| (dy, dx) < (by, bx)) {
            best = Some((dy, dx, item));
        }
    }
    let (_, _, item) = best?;
    let (_, pos, galley) = map(page, item)?;
    Some(Spot {
        item,
        cursor: galley.cursor_from_pos(point - pos),
    })
}

/// One character left or right, crossing into the neighbouring text item at the ends.
pub fn step(page: &Page, spot: Spot, forward: bool) -> Spot {
    let Some((_, _, galley)) = map(page, spot.item) else {
        return spot;
    };
    let c = spot.cursor.index.0;
    let len = galley.end().index.0;
    if forward && c < len {
        return Spot {
            item: spot.item,
            cursor: CCursor::new(c + 1),
        };
    }
    if !forward && c > 0 {
        return Spot {
            item: spot.item,
            cursor: CCursor::new(c - 1),
        };
    }
    let items: Vec<usize> = text_items(page).collect();
    let at = items.iter().position(|&i| i == spot.item).unwrap_or(0);
    let next = if forward {
        items.get(at + 1)
    } else {
        at.checked_sub(1).and_then(|i| items.get(i))
    };
    match next {
        Some(&item) => {
            let (_, _, galley) = map(page, item).expect("text item");
            Spot {
                item,
                cursor: if forward {
                    galley.begin()
                } else {
                    galley.end()
                },
            }
        }
        None => spot,
    }
}

/// One line up or down at horizontal position `x` (page coordinates).
pub fn line(page: &Page, spot: Spot, down: bool, x: f32) -> Spot {
    let Some((_, pos, galley)) = map(page, spot.item) else {
        return spot;
    };
    let rect = galley.pos_from_cursor(spot.cursor).translate(pos.to_vec2());
    let local = x - pos.x;
    let (moved, _) = if down {
        galley.cursor_down_one_row(&spot.cursor, Some(local))
    } else {
        galley.cursor_up_one_row(&spot.cursor, Some(local))
    };
    let moved_rect = galley.pos_from_cursor(moved).translate(pos.to_vec2());
    if (moved_rect.center().y - rect.center().y).abs() > 1.0 {
        return Spot {
            item: spot.item,
            cursor: moved,
        };
    }
    // The first or last line of this item: the nearest item above or below.
    let mut best: Option<(f32, f32, usize)> = None;
    for item in text_items(page) {
        if item == spot.item {
            continue;
        }
        let r = page.items[item].rect();
        let gap = if down {
            r.min.y - rect.max.y
        } else {
            rect.min.y - r.max.y
        };
        if gap < -1.0 {
            continue;
        }
        let dx = if x < r.min.x {
            r.min.x - x
        } else if x > r.max.x {
            x - r.max.x
        } else {
            0.0
        };
        if best.is_none_or(|(bg, bx, _)| (gap.max(0.0), dx) < (bg, bx)) {
            best = Some((gap.max(0.0), dx, item));
        }
    }
    let Some((_, _, item)) = best else {
        return Spot {
            item: spot.item,
            cursor: if down { galley.end() } else { galley.begin() },
        };
    };
    let (_, pos, galley) = map(page, item).expect("text item");
    let y = if down {
        galley.rect.min.y + 1.0
    } else {
        galley.rect.max.y - 1.0
    };
    Spot {
        item,
        cursor: galley.cursor_from_pos(pos2(x, pos.y + y) - pos),
    }
}

/// The start or end of the spot's visual line.
pub fn line_edge(page: &Page, spot: Spot, end: bool) -> Spot {
    let Some((_, _, galley)) = map(page, spot.item) else {
        return spot;
    };
    Spot {
        item: spot.item,
        cursor: if end {
            galley.cursor_end_of_row(&spot.cursor)
        } else {
            galley.cursor_begin_of_row(&spot.cursor)
        },
    }
}

/// The characters of each text item a selection covers, as `(item, start..end)`.
pub fn selected(
    page: &Page,
    doc: &Document,
    a: Pos,
    b: Pos,
) -> Vec<(usize, std::ops::Range<usize>)> {
    let (start, end) = edit::ordered(doc, a, b);
    if start == end {
        return Vec::new();
    }
    let mut out = Vec::new();
    for item in text_items(page) {
        let (segs, _, _) = map(page, item).expect("text item");
        let mut range: Option<std::ops::Range<usize>> = None;
        for seg in segs {
            let mut add = |from: usize, to: usize| {
                if from < to {
                    range = Some(match range.take() {
                        Some(r) => r.start.min(from)..r.end.max(to),
                        None => from..to,
                    });
                }
            };
            match &seg.target {
                Target::Text { node, bytes } => {
                    for k in 0..seg.chars.len() {
                        let before = Pos::new(*node, bytes[k]);
                        let after = Pos::new(*node, bytes[k + 1]);
                        if edit::compare(doc, before, start) != Ordering::Less
                            && edit::compare(doc, after, end) != Ordering::Greater
                        {
                            add(seg.chars.start + k, seg.chars.start + k + 1);
                        }
                    }
                }
                Target::Break { parent, index } => {
                    let before = Pos::new(*parent, *index);
                    let after = Pos::new(*parent, index + 1);
                    if edit::compare(doc, before, start) != Ordering::Less
                        && edit::compare(doc, after, end) != Ordering::Greater
                    {
                        add(seg.chars.start, seg.chars.end);
                    }
                }
            }
        }
        if let Some(range) = range {
            out.push((item, range));
        }
    }
    out
}

/// Rectangles (page coordinates) covering characters `range` of a text item, one per row.
pub fn range_rects(page: &Page, item: usize, range: std::ops::Range<usize>) -> Vec<Rect> {
    let Some((_, pos, galley)) = map(page, item) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut row_start = 0;
    for row in &galley.rows {
        let count = row.char_count_including_newline().0;
        let row_end = row_start + count;
        let from = range.start.max(row_start);
        let to = range.end.min(row_end);
        if from < to || (count == 0 && range.start <= row_start && row_start < range.end) {
            let left = galley.pos_from_cursor(CCursor {
                index: from.into(),
                prefer_next_row: true,
            });
            let newline = row.ends_with_newline && to == row_end;
            let right_index = if newline { to - 1 } else { to };
            let right = galley.pos_from_cursor(CCursor {
                index: right_index.into(),
                prefer_next_row: false,
            });
            let mut x1 = right.max.x;
            if newline || count == 0 {
                x1 += 4.0;
            }
            let rect = Rect::from_min_max(
                pos2(left.min.x.min(x1), row.min_y()),
                pos2(x1.max(left.min.x), row.max_y()),
            );
            out.push(rect.translate(pos.to_vec2()));
        }
        row_start = row_end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Families, layout};

    fn page(doc: &Document, width: f32) -> Page {
        let ctx = egui::Context::default();
        let mut page = None;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            page = Some(layout(doc, width, ui.ctx(), &Families::default(), &mut ()));
        });
        output.textures_delta.clear();
        page.unwrap()
    }

    fn text_node(doc: &Document, needle: &str) -> usize {
        doc.descendants(crate::dom::ROOT)
            .into_iter()
            .find(|&n| doc.text(n).is_some_and(|t| t.contains(needle)))
            .unwrap()
    }

    #[test]
    fn positions_round_trip_through_the_page() {
        let doc = Document::parse("<p>a  b<b>cd</b></p><p><br></p><p>x<br><br></p>");
        let page = page(&doc, 300.0);
        let ab = text_node(&doc, "a  b");
        let cd = text_node(&doc, "cd");
        // "a  b" shows as "a b": the second space has no character of its own.
        let spot = spot_of(&page, &doc, Pos::new(ab, 3)).unwrap();
        assert_eq!(spot.cursor.index.0, 2);
        assert_eq!(pos_of(&page, spot), Some(Pos::new(ab, 1 + 2)));
        // Between "b" and "cd" the caret stays with the text before it.
        let spot = spot_of(&page, &doc, Pos::new(cd, 0)).unwrap();
        assert_eq!(spot.cursor.index.0, 3);
        assert_eq!(pos_of(&page, spot), Some(Pos::new(ab, 4)));
        assert_eq!(
            pos_of(&page, step(&page, spot, true)),
            Some(Pos::new(cd, 1))
        );
        // An empty paragraph's caret stands before its <br>.
        let p2 = doc.children(doc.body()).to_vec()[1];
        let spot = spot_of(&page, &doc, Pos::new(p2, 0)).unwrap();
        assert_eq!(pos_of(&page, spot), Some(Pos::new(p2, 0)));
        // The second line of "x<br><br>" is a caret stop before the last <br>.
        let p3 = doc.children(doc.body()).to_vec()[2];
        let spot = spot_of(&page, &doc, Pos::new(p3, 2)).unwrap();
        assert_eq!(pos_of(&page, spot), Some(Pos::new(p3, 2)));
        assert_eq!(spot.cursor.index.0, 2);
    }

    #[test]
    fn moving_between_paragraphs() {
        let doc = Document::parse("<p>ab</p><p>cd</p>");
        let page = page(&doc, 300.0);
        let ab = text_node(&doc, "ab");
        let cd = text_node(&doc, "cd");
        let end = spot_of(&page, &doc, Pos::new(ab, 2)).unwrap();
        let next = step(&page, end, true);
        assert_eq!(pos_of(&page, next), Some(Pos::new(cd, 0)));
        assert_eq!(
            pos_of(&page, step(&page, next, false)),
            Some(Pos::new(ab, 2))
        );
        let x = caret_rect(&page, end).unwrap().center().x;
        let down = line(&page, end, true, x);
        assert_eq!(pos_of(&page, down), Some(Pos::new(cd, 2)));
        let up = line(&page, down, false, x);
        assert_eq!(pos_of(&page, up), Some(Pos::new(ab, 2)));
        // A click far below the text lands on the last line.
        let hit = hit(&page, pos2(1.0, 500.0)).unwrap();
        assert_eq!(hit.item, down.item);
    }

    #[test]
    fn selection_covers_characters() {
        let doc = Document::parse("<p>ab</p><p>cd</p>");
        let page = page(&doc, 300.0);
        let ab = text_node(&doc, "ab");
        let cd = text_node(&doc, "cd");
        let parts = selected(&page, &doc, Pos::new(cd, 1), Pos::new(ab, 1));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].1, 1..2);
        assert_eq!(parts[1].1, 0..1);
        assert_eq!(range_rects(&page, parts[0].0, parts[0].1.clone()).len(), 1);
    }
}
