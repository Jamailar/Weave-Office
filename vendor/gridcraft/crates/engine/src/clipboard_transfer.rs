//! Atomic cell-block moves between independently owned sessions.

use std::sync::Arc;

use gridcraft_core::{MAX_COLS, MAX_ROWS, RangeRef};
use serde_json::{Value as Json, json};

use crate::{Clipboard, DocState, EngineError, Result, Session, cmd, push_history};

const COMMAND: &str = "edit.moveCutBetween";

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: COMMAND.into(), msg: message.into() }
}

fn idle(session: &Session) -> Result<()> {
    let d = session.doc()?;
    if session.depth != 0
        || session.in_group(d.uid)
        || session.recorder.is_some()
        || session.recording.is_some()
        || session.macro_recording.is_some()
        || session.macro_debug.is_some()
        || d.touched.is_some()
        || d.edits.is_some()
    {
        return Err(EngineError::Other("Finish the current action or recording before moving cells between workbooks.".into()));
    }
    Ok(())
}

fn valid_range(range: RangeRef) -> bool {
    range.start.row <= range.end.row && range.start.col <= range.end.col && range.end.row < MAX_ROWS && range.end.col < MAX_COLS
}

fn staged(session: &Session) -> Result<Session> {
    Ok(Session { docs: vec![session.doc()?.clone()], prefs: session.prefs.clone(), text_width: session.text_width.clone(), ..Session::default() })
}

fn one_step(before: &DocState, after: &mut DocState) {
    push_history(&mut after.undo, "Move Cut Cells".into(), before.wb.clone(), &after.wb, before.selection.clone());
    after.redo.clear();
    after.revision = before.revision.saturating_add(1);
}

// External workbook references cannot yet be authored. A move is safe only when
// every cell formula dependency stays on its side of the cut boundary. Dynamic
// references and workbook-scoped names cannot be proven local, so refuse them.
fn check_formula_boundary(clipboard: &Clipboard) -> Result<()> {
    use gridcraft_formula::{Expr, SheetSel};
    let wb = &clipboard.wb;
    let source_name = &wb.sheets[clipboard.sheet].name;
    if !wb.names.is_empty() {
        return Err(invalid("Moving cells from a workbook with defined names is not supported yet; the cut was kept."));
    }
    for (si, sheet) in wb.sheets.iter().enumerate() {
        if !sheet.cond_formats.is_empty()
            || !sheet.tables.is_empty()
            || !sheet.charts.is_empty()
            || !sheet.sparklines.is_empty()
            || !sheet.pivots.is_empty()
        {
            return Err(invalid(
                "Moving cells from a workbook with conditional formats, tables, charts, sparklines or pivots is not supported yet; the cut was kept.",
            ));
        }
        // Literal validation rules travel; formula-based rules need a separate dependency rewrite.
        for rule in &sheet.validations {
            for text in std::iter::once(&rule.f1).chain(rule.f2.iter()) {
                if text.is_empty() {
                    continue;
                }
                let expr = gridcraft_formula::parse(text.trim_start_matches('='))
                    .map_err(|_| invalid("Cannot verify a validation formula; the cut was kept."))?;
                let mut depends = false;
                expr.walk(&mut |node| {
                    if matches!(node, Expr::Ref(_) | Expr::Name(_) | Expr::Struct(_) | Expr::Call(_, _) | Expr::Invoke(_, _)) {
                        depends = true;
                    }
                });
                if depends {
                    return Err(invalid("Formula-based validation is not supported for cross-workbook moves yet; the cut was kept."));
                }
            }
        }
        for (cell, content) in sheet.cells.iter() {
            let Some(formula) = &content.formula else { continue };
            let expr = formula.expr().ok_or_else(|| invalid("Cannot verify an unparsed formula; the cut was kept."))?;
            let inside = si == clipboard.sheet && clipboard.range.contains(cell);
            let mut unsafe_reference = false;
            expr.walk(&mut |node| match node {
                Expr::Name(_) | Expr::Struct(_) | Expr::Invoke(_, _) => unsafe_reference = true,
                Expr::Call(name, _) if matches!(name.as_str(), "INDIRECT" | "OFFSET" | "CELL" | "EVALUATE") => unsafe_reference = true,
                Expr::Ref(reference) => {
                    let same_sheet = match &reference.sheet {
                        SheetSel::Current => si == clipboard.sheet,
                        SheetSel::Named(name) => name.eq_ignore_ascii_case(source_name),
                        SheetSel::Span(_, _) => {
                            unsafe_reference = true;
                            false
                        }
                    };
                    let range = reference.range();
                    if inside {
                        unsafe_reference |= !same_sheet || !clipboard.range.contains_range(&range);
                    } else {
                        unsafe_reference |= same_sheet && clipboard.range.intersects(&range);
                    }
                }
                _ => {}
            });
            if unsafe_reference {
                return Err(invalid(
                    "This move requires external workbook formula links or an unverified dependency; both workbooks and the cut were kept.",
                ));
            }
        }
    }
    Ok(())
}

// The ordinary copy path shifts relative anchors. Pre-shift absolute anchors
// and localize qualified references so an internal block keeps move semantics.
fn prepared_copy(clipboard: &Clipboard, at: gridcraft_core::CellRef) -> Clipboard {
    use gridcraft_formula::{Expr, RefKind, SheetSel};
    let mut copy = clipboard.clone();
    copy.cut = false;
    let dr = i64::from(at.row) - i64::from(clipboard.range.start.row);
    let dc = i64::from(at.col) - i64::from(clipboard.range.start.col);
    let sheet = Arc::make_mut(&mut copy.wb).sheet_mut(clipboard.sheet).unwrap();
    let cells: Vec<_> =
        sheet.cells.iter_range(clipboard.range).filter(|(_, cell)| cell.formula.is_some()).map(|(at, cell)| (at, cell.clone())).collect();
    for (at, mut cell) in cells {
        let old = cell.formula.as_ref().unwrap();
        let expr = old.expr().unwrap().map(&mut |node| match node {
            Expr::Ref(mut reference) => {
                reference.sheet = SheetSel::Current;
                let shift = |anchor: &mut gridcraft_formula::Anchor| {
                    if anchor.row_abs {
                        anchor.row = (i64::from(anchor.row) + dr) as u32;
                    }
                    if anchor.col_abs {
                        anchor.col = (i64::from(anchor.col) + dc) as u32;
                    }
                };
                match &mut reference.kind {
                    RefKind::Cell(anchor) => shift(anchor),
                    RefKind::Range(a, b) => {
                        shift(a);
                        shift(b);
                    }
                    RefKind::Rows(a, aa, b, ba) => {
                        if *aa {
                            *a = (i64::from(*a) + dr) as u32;
                        }
                        if *ba {
                            *b = (i64::from(*b) + dr) as u32;
                        }
                    }
                    RefKind::Cols(a, aa, b, ba) => {
                        if *aa {
                            *a = (i64::from(*a) + dc) as u32;
                        }
                        if *ba {
                            *b = (i64::from(*b) + dc) as u32;
                        }
                    }
                }
                Expr::Ref(reference)
            }
            other => other,
        });
        let mut formula = crate::model::Formula::from_expr(expr);
        formula.array = old.array;
        formula.dynamic = old.dynamic;
        cell.formula = Some(Arc::new(formula));
        sheet.set_cell(at, cell);
    }
    copy
}

impl Session {
    /// Move one complete cut block between two independent sessions, with one undo step in
    /// each document. The source must still be active and hold the exact workbook snapshot
    /// captured by the cut; editing either the source cells or anything else in its workbook
    /// invalidates the cut. Missing documents and non-idle sessions are refused.
    ///
    /// Accepts `at`, `sheet` and ordinary full-content paste parameters only: `what: "all"`,
    /// `operation: "none"`, `transpose/skipBlanks/link: false`. The destination selection's
    /// first cell is the default anchor; a larger selection never tiles a move. External text
    /// must match the internal clipboard. Unsupported options and malformed fields are errors.
    ///
    /// Complete internal formula dependencies move, including absolute anchors. Formula links
    /// across the cut boundary, dynamic references and unsupported dependency features are
    /// refused until external workbook links can be authored. Otherwise,
    /// styles are re-interned, and cells, cell pictures, notes, hyperlinks, validation and fully
    /// enclosed merged areas travel. Row/column structure, floating objects, conditional formats,
    /// workbook names and macros stay in their workbook. This does not create external workbook
    /// links or redirect formulas elsewhere in the source workbook to the destination workbook.
    /// Protected sheets and partially selected merged areas are refused conservatively.
    ///
    /// All preparation, calculation and history updates happen on COW document clones. No live
    /// document, window, selection, dirty flag, history, calculation state or clipboard changes
    /// on error (including an escaped panic). Success replaces only the two active documents
    /// and consumes their matching cut clipboards. Their undo/redo stacks remain independent;
    /// undoing one side never automatically undoes the other. The caller owns any host clipboard.
    pub fn move_cut_between(source: &mut Session, target: &mut Session, clipboard: &Clipboard, params: &Json) -> Result<Json> {
        idle(source)?;
        idle(target)?;
        let source_doc = source.doc()?;
        let target_doc = target.doc()?;
        if !clipboard.cut || !clipboard.areas.is_empty() {
            return Err(invalid("A single cut block is required."));
        }
        if source_doc.uid != clipboard.doc_uid || source_doc.uid == target_doc.uid {
            return Err(invalid("The cut source must be the active source workbook and differ from the destination."));
        }
        if !Arc::ptr_eq(&source_doc.wb, &clipboard.wb) {
            return Err(EngineError::Other("The source workbook changed after Cut. Cut the cells again before moving them.".into()));
        }
        if !valid_range(clipboard.range) || clipboard.range.count() > 4_000_000 {
            return Err(invalid("The cut block is invalid or exceeds 4,000,000 cells."));
        }
        let src_sheet = source_doc.wb.sheet(clipboard.sheet).ok_or_else(|| invalid("The cut sheet no longer exists."))?;
        if src_sheet.is_protected() {
            return Err(EngineError::Other(cmd::PROTECTED.into()));
        }
        if src_sheet.merges.iter().any(|m| m.intersects(&clipboard.range) && !clipboard.range.contains_range(m)) {
            return Err(invalid("A move must include each merged source area in full."));
        }
        if src_sheet.spill_ranges.values().any(|r| r.intersects(&clipboard.range) && !clipboard.range.contains_range(r)) {
            return Err(invalid("A move must include each source array or spill in full."));
        }

        let mut paste = if params.is_null() { json!({}) } else { params.clone() };
        let fields = paste.as_object_mut().ok_or_else(|| invalid("Paste parameters must be an object."))?;
        for (key, value) in fields.iter() {
            let accepted = match key.as_str() {
                "at" => value.is_null() || value.is_string(),
                "sheet" => value.is_string() || value.as_u64().is_some(),
                "what" => value.as_str() == Some("all"),
                "operation" => value.as_str() == Some("none"),
                "transpose" | "skipBlanks" | "link" => value.as_bool() == Some(false),
                "text" => value.as_str().is_some_and(|text| text.trim_end_matches(['\r', '\n']) == clipboard.text.trim_end_matches(['\r', '\n'])),
                "html" => value.is_string(),
                _ => false,
            };
            if !accepted {
                return Err(invalid(format!("Unsupported full-content move parameter `{key}`; the cut was kept.")));
            }
        }
        let dest_sheet = cmd::target_sheet(target, &paste)?;
        let dest = target_doc.wb.sheet(dest_sheet).ok_or_else(|| invalid("The destination sheet no longer exists."))?;
        if dest.is_protected() {
            return Err(EngineError::Other(cmd::PROTECTED.into()));
        }
        let at = cmd::cell_param_on(target, &paste, "at", dest_sheet)?.unwrap_or(target_doc.selection.current().start);
        let end = at
            .offset(i64::from(clipboard.range.height()) - 1, i64::from(clipboard.range.width()) - 1)
            .ok_or_else(|| invalid("The cut area does not fit at this position."))?;
        let footprint = RangeRef::new(at, end);
        if !valid_range(footprint) {
            return Err(invalid("The destination is outside the worksheet."));
        }
        if dest.merges.iter().any(|m| m.intersects(&footprint) && !footprint.contains_range(m)) {
            return Err(invalid("A move must include each merged destination area in full."));
        }
        check_formula_boundary(clipboard)?;
        paste["at"] = Json::String(at.a1()); // An explicit anchor prevents copy-style tiling.
        paste["what"] = Json::String("all".into());

        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(DocState, DocState, Json)> {
            let mut next_target = staged(target)?;
            next_target.clipboard = Some(prepared_copy(clipboard, at));
            // Prepare the target first. A target refusal never even attempts source cleanup.
            let result = next_target.execute("edit.pasteSpecial", paste)?;
            let mut next_source = staged(source)?;
            cmd::edit(&mut next_source, |cx| {
                cmd::edit::clear_cut_source(cx, clipboard.sheet, clipboard.range)?;
                Ok(())
            })?;
            let mut source_after = next_source.docs.pop().ok_or(EngineError::NoDocument)?;
            let mut target_after = next_target.docs.pop().ok_or(EngineError::NoDocument)?;
            one_step(source_doc, &mut source_after);
            // execute already made exactly one target step; preserve its selection and label.
            if let Some(step) = target_after.undo.last_mut() {
                step.label = "Move Cut Cells".into();
            }
            Ok((source_after, target_after, result))
        }));
        let (source_after, target_after, result) = match prepared {
            Ok(result) => result?,
            Err(_) => return Err(EngineError::Internal(COMMAND.into(), "The move failed; both workbooks were kept as they were.".into())),
        };
        let source_slot = source.docs.get_mut(source.active).ok_or(EngineError::NoDocument)?;
        let target_slot = target.docs.get_mut(target.active).ok_or(EngineError::NoDocument)?;
        *source_slot = source_after;
        *target_slot = target_after;
        for session in [source, target] {
            if session.clipboard.as_ref().is_some_and(|c| {
                c.cut
                    && c.doc_uid == clipboard.doc_uid
                    && c.sheet == clipboard.sheet
                    && c.range == clipboard.range
                    && Arc::ptr_eq(&c.wb, &clipboard.wb)
            }) {
                session.clipboard = None;
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
