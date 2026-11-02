use super::*;
use crate::core::{CellRef, Value};
use crate::model::{CellPicture, Comment, Hyperlink, Validation};

fn session() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session
}

fn set(session: &mut Session, cell: &str, input: &str) {
    session.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

fn value(session: &Session, cell: &str) -> Value {
    session.doc().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

fn cut(session: &mut Session, range: &str) -> Clipboard {
    session.execute("edit.cut", json!({"range": range})).unwrap();
    session.clipboard.clone().unwrap()
}

// Debug includes Calc's dependency/cache state and all DocState members, including dirty/saved
// identity, history, revision, selections, trust and unlocked ranges. Pointer identity is also
// checked separately because equal workbook contents do not imply an unchanged dirty flag.
fn state(session: &Session) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        session.docs, session.windows, session.clipboard, session.journal, session.ui_requests, session.mode, session.active, session.active_window
    )
}

fn refused_unchanged(source: &mut Session, target: &mut Session, clipboard: &Clipboard, params: Json) {
    let before_source = state(source);
    let before_target = state(target);
    let source_wb = source.active().map(|d| d.wb.clone());
    let target_wb = target.active().map(|d| d.wb.clone());
    assert!(Session::move_cut_between(source, target, clipboard, &params).is_err(), "{params}");
    assert_eq!(state(source), before_source, "source changed on failure: {params}");
    assert_eq!(state(target), before_target, "target changed on failure: {params}");
    if let Some(wb) = source_wb {
        assert!(Arc::ptr_eq(&wb, &source.doc().unwrap().wb));
    }
    if let Some(wb) = target_wb {
        assert!(Arc::ptr_eq(&wb, &target.doc().unwrap().wb));
    }
}

#[test]
fn moves_cells_styles_formulas_pictures_notes_links_validation_and_merges() {
    let mut source = session();
    let mut target = session();
    source.doc_mut().unwrap().trust = crate::Trust::FromFile;
    set(&mut source, "A1", "10");
    set(&mut source, "A2", "=A1*2");
    set(&mut source, "A3", "merged");
    source.execute("home.bold", json!({"range": "A1"})).unwrap();
    source.execute("home.mergeCells", json!({"range": "A3:B3"})).unwrap();
    let a1 = CellRef::parse("A1").unwrap();
    let b1 = CellRef::parse("B1").unwrap();
    {
        let wb = Arc::make_mut(&mut source.doc_mut().unwrap().wb);
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_picture(b1, Arc::new(CellPicture { data: vec![1, 2, 3], mime: "image/png".into(), alt: "sample".into() }));
        sh.comments.insert(
            a1,
            Comment {
                author: "Author".into(),
                text: "Note".into(),
                replies: vec![],
                threaded: false,
                resolved: false,
                visible: false,
                dates: vec![],
                note_box: None,
            },
        );
        sh.hyperlinks.insert(a1, Hyperlink { target: "https://example.com/".into(), tooltip: None, external: true });
        sh.validations.push(Validation { ranges: vec![RangeRef::cell(a1)], ..Validation::default() });
    }
    let clip = cut(&mut source, "A1:B3");
    target.clipboard = Some(clip.clone());
    target.execute("selection.set", json!({"range": "D5:G10"})).unwrap();
    let source_before = source.doc().unwrap().clone();
    let target_before = target.doc().unwrap().clone();
    let source_window = source.active_window();
    let target_window = target.active_window();
    Session::move_cut_between(&mut source, &mut target, &clip, &json!({})).unwrap();
    let from = source.doc().unwrap();
    let to = target.doc().unwrap();
    let src = from.wb.active().unwrap();
    let dst = to.wb.active().unwrap();
    assert_eq!(value(&source, "A1"), Value::Empty);
    assert_eq!(value(&source, "A2"), Value::Empty);
    assert!(src.cell(a1).is_none());
    assert!(src.cell_pictures.is_empty());
    assert!(src.comments.is_empty());
    assert!(src.hyperlinks.is_empty());
    assert!(src.validations.is_empty());
    assert!(src.merges.is_empty());
    assert_eq!(value(&target, "D5"), Value::Number(10.0));
    assert_eq!(value(&target, "D6"), Value::Number(20.0));
    assert_eq!(dst.cell(CellRef::parse("D6").unwrap()).unwrap().formula.as_ref().unwrap().text, "D5*2");
    assert!(dst.cell_pictures.contains_key(&CellRef::parse("E5").unwrap()));
    assert_eq!(dst.comments[&CellRef::parse("D5").unwrap()].text, "Note");
    assert!(dst.hyperlinks.contains_key(&CellRef::parse("D5").unwrap()));
    assert_eq!(dst.validations[0].ranges, vec![RangeRef::parse("D5").unwrap()]);
    assert_eq!(dst.merges, vec![RangeRef::parse("D7:E7").unwrap()]);
    assert_eq!(to.wb.styles.get(dst.style_id(CellRef::parse("D5").unwrap())), clip.wb.styles.get(clip.wb.active().unwrap().style_id(a1)));
    assert_eq!(value(&target, "F5"), Value::Empty, "a move never tiles the larger selection");
    assert_eq!(from.undo.len(), source_before.undo.len() + 1);
    assert_eq!(to.undo.len(), target_before.undo.len() + 1);
    assert_eq!(from.wb.sheets.len(), source_before.wb.sheets.len());
    assert_eq!(src.cols, source_before.wb.active().unwrap().cols);
    assert_eq!(from.selection, source_before.selection);
    assert_eq!(source.active_window(), source_window);
    assert_eq!(target.active_window(), target_window);
    assert!(source.clipboard.is_none());
    assert!(target.clipboard.is_none());
    assert_eq!(to.selection.a1(), "D5:E7");
    assert_eq!(to.trust, crate::Trust::FromFile);
}

#[test]
fn full_legacy_array_moves_with_its_range_and_partial_array_is_refused() {
    let mut source = session();
    let mut target = session();
    source.execute("selection.set", json!({"range": "A1:A2"})).unwrap();
    source.execute("cell.set", json!({"input": "=1", "array": true})).unwrap();
    let partial = cut(&mut source, "A2");
    refused_unchanged(&mut source, &mut target, &partial, json!({"at": "C3"}));
    let clip = cut(&mut source, "A1:A2");
    Session::move_cut_between(&mut source, &mut target, &clip, &json!({"at": "C3"})).unwrap();
    assert_eq!(value(&source, "A1"), Value::Empty);
    assert_eq!(value(&source, "A2"), Value::Empty);
    assert_eq!(value(&target, "C3"), Value::Number(1.0));
    assert_eq!(value(&target, "C4"), Value::Number(1.0));
    assert_eq!(
        target.doc().unwrap().wb.active().unwrap().cell(CellRef::parse("C3").unwrap()).unwrap().formula.as_ref().unwrap().array,
        RangeRef::parse("C3:C4")
    );
}

#[test]
fn histories_are_independent_and_each_side_undoes_and_redoes_one_move() {
    let mut source = session();
    let mut target = session();
    set(&mut source, "A1", "7");
    set(&mut source, "Z1", "source history");
    set(&mut target, "C3", "old target");
    let clip = cut(&mut source, "A1");
    let source_before = source.doc().unwrap().wb.clone();
    let target_before = target.doc().unwrap().wb.clone();
    Session::move_cut_between(&mut source, &mut target, &clip, &json!({"at": "C3"})).unwrap();
    let source_after = source.doc().unwrap().wb.clone();
    let target_after = target.doc().unwrap().wb.clone();
    source.execute("edit.undo", json!({})).unwrap();
    assert!(Arc::ptr_eq(&source.doc().unwrap().wb, &source_before));
    assert!(Arc::ptr_eq(&target.doc().unwrap().wb, &target_after));
    source.execute("edit.redo", json!({})).unwrap();
    assert!(Arc::ptr_eq(&source.doc().unwrap().wb, &source_after));
    target.execute("edit.undo", json!({})).unwrap();
    assert!(Arc::ptr_eq(&target.doc().unwrap().wb, &target_before));
    assert!(Arc::ptr_eq(&source.doc().unwrap().wb, &source_after));
    target.execute("edit.redo", json!({})).unwrap();
    assert!(Arc::ptr_eq(&target.doc().unwrap().wb, &target_after));
}

#[test]
fn rejected_options_and_overflow_preserve_both_redo_stacks_and_clipboards() {
    let mut source = session();
    let mut target = session();
    set(&mut source, "A1", "7");
    set(&mut source, "Z1", "undo me");
    source.execute("edit.undo", json!({})).unwrap();
    set(&mut target, "C3", "undo me too");
    target.execute("edit.undo", json!({})).unwrap();
    let clip = cut(&mut source, "A1:B2");
    target.clipboard = Some(clip.clone());
    for params in [
        json!({"at": "XFD1048576"}),
        json!({"at": "bad cell"}),
        json!({"at": 42}),
        json!({"sheet": 9}),
        json!({"sheet": false}),
        json!({"what": "values"}),
        json!({"transpose": true}),
        json!({"skipBlanks": true}),
        json!({"link": true}),
        json!({"operation": "add"}),
        json!({"transpose": "false"}),
        json!({"text": "different"}),
        json!([]),
        json!({"unsupported": true}),
    ] {
        refused_unchanged(&mut source, &mut target, &clip, params);
    }
    assert_eq!(source.doc().unwrap().redo.len(), 1);
    assert_eq!(target.doc().unwrap().redo.len(), 1);
}

#[test]
fn protected_source_or_target_keeps_both_documents_unchanged() {
    for protect_source in [true, false] {
        let mut source = session();
        let mut target = session();
        set(&mut source, "A1", "7");
        if protect_source {
            source.execute("review.protectSheet", json!({})).unwrap();
        } else {
            target.execute("review.protectSheet", json!({})).unwrap();
        }
        let clip = cut(&mut source, "A1");
        refused_unchanged(&mut source, &mut target, &clip, json!({"at": "C3"}));
    }
}

#[test]
fn target_array_rejection_during_staging_leaves_both_live_sessions_untouched() {
    let mut source = session();
    let mut target = session();
    set(&mut source, "A1", "7");
    target.execute("selection.set", json!({"range": "C3:C4"})).unwrap();
    target.execute("cell.set", json!({"input": "=1", "array": true})).unwrap();
    let clip = cut(&mut source, "A1");
    refused_unchanged(&mut source, &mut target, &clip, json!({"at": "C4"}));
}

#[test]
fn stale_snapshot_wrong_active_document_and_closed_document_are_refused() {
    let mut source = session();
    let mut target = session();
    set(&mut source, "A1", "7");
    let clip = cut(&mut source, "A1");
    set(&mut source, "Z1", "any workbook edit invalidates the snapshot");
    refused_unchanged(&mut source, &mut target, &clip, json!({}));
    let clip = cut(&mut source, "A1");
    source.new_workbook();
    refused_unchanged(&mut source, &mut target, &clip, json!({}));
    source.close_document(1);
    source.close_document(0);
    refused_unchanged(&mut source, &mut target, &clip, json!({}));
    let mut source = session();
    set(&mut source, "A1", "7");
    let clip = cut(&mut source, "A1");
    target.close_document(0);
    refused_unchanged(&mut source, &mut target, &clip, json!({}));
}

#[test]
fn existing_group_cannot_absorb_the_move_and_partial_merges_are_refused() {
    let mut source = session();
    let mut target = session();
    set(&mut source, "A1", "7");
    let clip = cut(&mut source, "A1");
    let group = source.group_begin("Other action").unwrap();
    refused_unchanged(&mut source, &mut target, &clip, json!({}));
    source.group_end(group).unwrap();
    source.execute("home.mergeCells", json!({"range": "A1:B1"})).unwrap();
    let clip = cut(&mut source, "A1");
    refused_unchanged(&mut source, &mut target, &clip, json!({}));
}

#[test]
fn ordinary_paste_never_degrades_a_cross_workbook_cut_to_copy() {
    let mut source = session();
    let mut target = session();
    set(&mut source, "A1", "7");
    target.clipboard = Some(cut(&mut source, "A1"));
    let before = state(&target);
    assert!(target.execute("edit.paste", json!({"at": "B2"})).is_err());
    assert_eq!(state(&target), before);
    assert_eq!(value(&source, "A1"), Value::Number(7.0));
}

#[test]
fn formula_links_across_cut_boundary_are_refused_without_mutation() {
    for (cell, formula) in [("Z1", "=SUM(A1:A2)"), ("A2", "=Z1+1"), ("A2", "=INDIRECT(\"A1\")"), ("A2", "=OFFSET(A1,0,0)")] {
        let mut source = session();
        let mut target = session();
        set(&mut source, "A1", "7");
        set(&mut source, cell, formula);
        let clipboard = cut(&mut source, "A1:A2");
        refused_unchanged(&mut source, &mut target, &clipboard, json!({"at":"C3"}));
    }
}

#[test]
fn internal_absolute_and_qualified_references_follow_the_moved_block() {
    let mut source = session();
    let mut target = session();
    set(&mut source, "A1", "7");
    let name = source.doc().unwrap().wb.active().unwrap().name.clone();
    set(&mut source, "A2", &format!("=$A$1+A$1+$A1+'{name}'!A1"));
    let clipboard = cut(&mut source, "A1:A2");
    Session::move_cut_between(&mut source, &mut target, &clipboard, &json!({"at":"C3"})).unwrap();
    assert_eq!(value(&target, "C4"), Value::Number(28.0));
    let formula = target.doc().unwrap().wb.active().unwrap().cell(CellRef::parse("C4").unwrap()).unwrap().formula.as_ref().unwrap();
    assert_eq!(formula.text, "$C$3+C$3+$C3+C3");
}
