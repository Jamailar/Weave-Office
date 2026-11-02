//! Border strokes occupy the bands already reserved by table layout.

use super::*;
use crate::table::{CellPart, RowLayout, split_row};
use wordcraft_doc::props::{HeightRule, LineSpacing, VMerge};

fn paragraph(text: &str) -> Arc<Block> {
    let mut p = wordcraft_doc::Paragraph::with_text(text, Default::default());
    p.props = ParaProps {
        line_spacing: Some(LineSpacing::Exactly(10.0)),
        space_before: Some(0.0),
        space_after: Some(0.0),
        widow_control: Some(false),
        ..Default::default()
    };
    wordcraft_doc::para_block(p)
}

fn table(rows: usize, columns: usize, width: f32) -> Table {
    let mut t = Table::new(rows, columns, 200.0);
    t.props.style = None;
    t.props.fixed = true;
    t.props.cell_margins = Some([2.0; 4]);
    let b = Some(Border::single(width));
    t.props.borders = Some(Borders { top: b, bottom: b, left: b, right: b, between: b, inside_v: b });
    for r in &mut t.rows {
        for c in &mut r.cells {
            c.blocks = vec![paragraph("Cell")];
        }
    }
    t
}

fn document(t: Table) -> Document {
    let mut doc = Document::new();
    doc.body = vec![Arc::new(Block::Table(t))];
    doc
}

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.002, "{a} != {b}");
}

fn horizontal(items: &[Placed]) -> Vec<(f32, f32)> {
    items
        .iter()
        .filter_map(|it| match it {
            Placed::Rule { y0, y1, border, .. } if y0 == y1 => Some((*y0, border.width)),
            _ => None,
        })
        .collect()
}

#[test]
fn table_bands_preserve_flow_and_place_strokes_inside_reserved_bands() {
    for width in [0.5, 0.75, 3.0] {
        for rows in [1, 3] {
            for rule in [HeightRule::Auto, HeightRule::AtLeast, HeightRule::Exact] {
                let mut t = table(rows, 1, width);
                for r in &mut t.rows {
                    r.props.height_rule = rule;
                    r.props.height = (rule != HeightRule::Auto).then_some(20.0);
                }
                let laid = lay(&document(t));
                assert_eq!(laid.pages.len(), 1);
                let items = &laid.pages[0].items;
                let cells: Vec<_> = items
                    .iter()
                    .filter_map(|it| match it {
                        Placed::Cell { rect, .. } => Some(*rect),
                        _ => None,
                    })
                    .collect();
                let text: Vec<_> = items
                    .iter()
                    .filter_map(|it| match it {
                        Placed::Lines { y, .. } => Some(*y),
                        _ => None,
                    })
                    .collect();
                let rules = horizontal(items);
                assert_eq!(rules.len(), rows * 2);
                let mut next = 72.0;
                for (i, cell) in cells.iter().enumerate() {
                    let last = i + 1 == rows;
                    let h = match rule {
                        HeightRule::Exact => 22.0,
                        HeightRule::AtLeast => 24.0 + width + if last { width } else { 0.0 },
                        HeightRule::Auto => 14.0 + width + if last { width } else { 0.0 },
                    };
                    close(cell.y, next);
                    close(cell.h, h);
                    close(text[i], cell.y + 2.0 + width);
                    close(rules[i * 2].0 - width / 2.0, cell.y);
                    close(rules[i * 2 + 1].0, cell.bottom() + if last { -width / 2.0 } else { width / 2.0 });
                    next += h;
                }
                for it in items {
                    if let Placed::Rule { x0, x1, y1, .. } = it {
                        if x0 == x1 {
                            assert!(*y1 <= next - width + 0.002);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn table_bands_single_sided_shared_edge_keeps_text_clear() {
    for hidden in [None, Some(Border { style: BorderStyle::None, ..Border::single(0.0) })] {
        let mut t = table(2, 1, 2.0);
        t.props.borders = Some(Borders::default());
        let b = Some(Border::single(2.0));
        t.rows[0].cells[0].props.borders = Some(Borders { top: b, bottom: b, ..Default::default() });
        t.rows[1].cells[0].props.borders = Some(Borders { top: hidden, bottom: b, ..Default::default() });
        let laid = lay(&document(t));
        let items = &laid.pages[0].items;
        let tops: Vec<_> = items
            .iter()
            .filter_map(|it| match it {
                Placed::Lines { y, .. } => Some(*y),
                _ => None,
            })
            .collect();
        assert_eq!(tops, [76.0, 92.0]);
        assert_eq!(horizontal(items), [(73.0, 2.0), (89.0, 2.0), (105.0, 2.0)]);
    }
}

#[test]
fn table_bands_borrowed_top_inset_survives_continuation() {
    let mut t = table(2, 1, 2.0);
    t.props.borders = Some(Borders::default());
    let b = Some(Border::single(2.0));
    t.rows[0].cells[0].props.borders = Some(Borders { top: b, bottom: b, ..Default::default() });
    t.rows[1].cells[0].props.borders = Some(Borders { bottom: b, ..Default::default() });
    t.rows[1].cells[0].blocks = (0..20).map(|_| paragraph("Line")).collect();
    let mut doc = document(t);
    doc.last_section.page_h = 90.0;
    doc.last_section.margin_top = 10.0;
    doc.last_section.margin_bottom = 10.0;
    let laid = lay(&doc);
    assert!(laid.pages.len() >= 3);
    for page in laid.pages.iter().skip(1) {
        let cell = page
            .items
            .iter()
            .find_map(|it| match it {
                Placed::Cell { rect, row: 1, .. } => Some(*rect),
                _ => None,
            })
            .unwrap();
        let first_text = page
            .items
            .iter()
            .find_map(|it| match it {
                Placed::Lines { y, .. } => Some(*y),
                _ => None,
            })
            .unwrap();
        close(first_text, cell.y + 4.0); // 2pt margin plus the preceding cell's 2pt band.
    }
}

#[test]
fn table_bands_merge_suppresses_internal_edges_and_closes_terminal_band() {
    let mut t = table(3, 2, 3.0);
    t.rows[0].cells[0].props.vmerge = VMerge::Restart;
    t.rows[1].cells[0].props.vmerge = VMerge::Continue;
    t.rows[2].cells[0].props.vmerge = VMerge::Continue;
    let laid = lay(&document(t));
    let items = &laid.pages[0].items;
    let first = items
        .iter()
        .find_map(|it| match it {
            Placed::Cell { rect, row: 0, cell: 0, .. } => Some(*rect),
            _ => None,
        })
        .unwrap();
    let left_rules: Vec<_> = items
        .iter()
        .filter_map(|it| match it {
            Placed::Rule { x0, x1, y0, y1, .. } if y0 == y1 && *x0 < first.x && *x1 < first.right() + 2.0 => Some(*y0),
            _ => None,
        })
        .collect();
    assert_eq!(left_rules.len(), 2, "merged cell has only top and bottom: {left_rules:?}");
    close(left_rules[0], first.y + 1.5);
    close(left_rules[1], first.bottom() - 1.5);
}

#[test]
fn table_bands_survive_repeated_page_splits() {
    for width in [0.5, 3.0] {
        let mut t = table(1, 1, width);
        t.rows[0].cells[0].blocks = (0..20).map(|_| paragraph("Line")).collect();
        let mut doc = document(t);
        doc.last_section.page_h = 90.0;
        doc.last_section.margin_top = 10.0;
        doc.last_section.margin_bottom = 10.0;
        let laid = lay(&doc);
        assert!(laid.pages.len() >= 3);
        let mut lines = 0;
        for page in &laid.pages {
            let cell = page
                .items
                .iter()
                .find_map(|it| match it {
                    Placed::Cell { rect, .. } => Some(*rect),
                    _ => None,
                })
                .unwrap();
            let edges = horizontal(&page.items);
            assert_eq!(edges.len(), 2, "each fragment closes on both sides");
            close(edges[0].0, cell.y + width / 2.0);
            close(edges[1].0, cell.bottom() - width / 2.0);
            for it in &page.items {
                match it {
                    Placed::Rule { x0, x1, y0, y1, .. } if x0 == x1 => {
                        close(*y0, cell.y);
                        close(*y1, cell.bottom() - width);
                    }
                    Placed::Lines { .. } => lines += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(lines, 20, "pagination preserves every paragraph");
    }
}

#[test]
fn table_bands_split_only_repeats_the_owning_cells_edges() {
    let laid = lay(&Document::from_text("Line"));
    let mut line = laid.pages[0].items.iter().find(|it| matches!(it, Placed::Lines { .. })).unwrap().clone();
    if let Placed::Lines { y, .. } = &mut line {
        *y = 5.0;
    }
    let mut later = line.clone();
    later.translate(0.0, 145.0);
    let edge = |y, width| Placed::Rule { x0: 0.0, y0: y, x1: 100.0, y1: y, border: Border::single(width) };
    let row = RowLayout {
        height: 200.0,
        bottom: 4.0,
        edge: 0.0,
        items: vec![line, later, edge(0.0, 0.5), edge(1.0, 2.0), edge(199.0, 2.0)],
        cells: vec![CellPart { top: 4.0, items: 0..5, end: Some(170.0), top_rule: Some(3), bottom_rule: Some(4), ..Default::default() }],
    };
    let (first, rest) = split_row(&row, 100.0).unwrap();
    assert_eq!(horizontal(&first.items), [(0.0, 0.5), (1.0, 2.0), (99.0, 2.0)]);
    let continuation = horizontal(&rest.items);
    assert_eq!(continuation.len(), 2, "nested content rule must not become a repeated cell edge");
    close(continuation[0].0, 1.0);
    close(continuation[1].0, rest.height - 1.0);
}

#[test]
fn table_bands_nested_table_keeps_its_own_geometry_when_parent_splits() {
    let mut outer = table(1, 1, 3.0);
    let inner = table(1, 1, 0.5);
    outer.rows[0].cells[0].blocks = vec![paragraph("Before"), Arc::new(Block::Table(inner))];
    outer.rows[0].cells[0].blocks.extend((0..20).map(|_| paragraph("After")));
    let mut doc = document(outer);
    doc.last_section.page_h = 100.0;
    doc.last_section.margin_top = 10.0;
    doc.last_section.margin_bottom = 10.0;
    let laid = lay(&doc);
    assert!(laid.pages.len() >= 3);
    let nested = laid.pages[0]
        .items
        .iter()
        .find_map(|it| match it {
            Placed::Cell { rect, table, .. } if table.0.len() > 1 => Some(*rect),
            _ => None,
        })
        .unwrap();
    let rules: Vec<_> = horizontal(&laid.pages[0].items).into_iter().filter(|(_, w)| *w == 0.5).collect();
    assert_eq!(rules.len(), 2);
    close(rules[0].0, nested.y + 0.25);
    close(rules[1].0, nested.bottom() - 0.25);
    for page in laid.pages.iter().skip(1) {
        assert!(horizontal(&page.items).iter().all(|(_, w)| *w == 3.0), "nested edges cannot be repeated as outer edges");
    }
}

#[test]
fn table_bands_keep_unequal_shared_widths_coincident_until_conflict_resolution() {
    let mut t = table(2, 1, 1.0);
    t.rows[0].cells[0].props.borders = Some(Borders { bottom: Some(Border::single(3.0)), ..Default::default() });
    let laid = lay(&document(t));
    let rules = horizontal(&laid.pages[0].items);
    close(rules[1].0, rules[2].0);
    assert_eq!((rules[1].1, rules[2].1), (3.0, 1.0));
}
