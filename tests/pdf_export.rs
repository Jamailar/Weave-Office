use std::sync::Arc;

use serde_json::json;
use weave_office::{
    editor::{Clipboard, Editor},
    output::{PdfPurpose, pdf_bytes},
};
use weave_workspace::DocumentKind;

fn editor(kind: DocumentKind) -> Editor {
    Editor::create(kind, None, &Clipboard::default()).expect("create hosted editor")
}

fn reopen(bytes: Vec<u8>) -> pdfcraft_engine::Session {
    let mut reader = pdfcraft_engine::Session::new();
    reader
        .open("snapshot.pdf", None, Arc::new(bytes), None)
        .expect("reopen PDF snapshot");
    reader
}

fn source_path(editor: &Editor) -> Option<String> {
    match editor {
        Editor::Word(app) => app
            .session
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        Editor::Sheet(app) => app.session.active().and_then(|doc| doc.path.clone()),
        Editor::Slides(app) => app.session.active().and_then(|doc| doc.path.clone()),
        Editor::Pdf(app) => app
            .active_ids()
            .and_then(|(_, id)| app.session.get(id))
            .and_then(|doc| doc.path.clone()),
    }
}

#[test]
fn snapshots_include_unsaved_content_without_acknowledging_it() {
    let marker = "PDF snapshot marker";
    for kind in [
        DocumentKind::Text,
        DocumentKind::Spreadsheet,
        DocumentKind::Presentation,
    ] {
        let mut editor = editor(kind);
        match &mut editor {
            Editor::Word(app) => {
                app.hosted_command("text.insert", json!({"text": marker}))
                    .expect("edit Word");
            }
            Editor::Sheet(app) => {
                app.hosted_command("cell.set", json!({"cell":"A1", "input":marker}))
                    .expect("edit grid");
            }
            Editor::Slides(app) => {
                let shape = app.session.active().expect("presentation").doc.slides[0]
                    .shapes
                    .iter()
                    .find(|shape| shape.text.is_some())
                    .expect("title")
                    .id
                    .0;
                app.hosted_command("text.set", json!({"id":shape, "text":marker}))
                    .expect("edit slide");
            }
            Editor::Pdf(_) => unreachable!(),
        }
        let dirty = editor.dirty();
        let history = editor.history_available();
        let path = source_path(&editor);
        assert!(dirty, "{kind:?} should have unsaved edits");
        for purpose in [PdfPurpose::Export, PdfPurpose::Print] {
            let bytes = pdf_bytes(&mut editor, purpose).expect("make PDF snapshot");
            assert!(bytes.starts_with(b"%PDF-"));
            let reader = reopen(bytes);
            let doc = reader.docs().first().expect("reopened document");
            assert!(!doc.info.pages.is_empty());
            let text = pdfcraft_engine::export::Exporter::new(doc)
                .text(0)
                .expect("read PDF text");
            assert!(
                text.contains(marker),
                "{kind:?} {purpose:?} PDF text: {text:?}"
            );
            assert_eq!(editor.dirty(), dirty, "snapshot changed dirty state");
            assert_eq!(
                editor.history_available(),
                history,
                "snapshot changed undo history"
            );
            assert_eq!(source_path(&editor), path, "snapshot changed source path");
        }
    }
}

#[test]
fn pdf_snapshot_preserves_unsaved_state_and_page_count() {
    let mut editor = editor(DocumentKind::Pdf);
    editor
        .command("page.rotate", json!({}))
        .expect("rotate source page");
    let dirty = editor.dirty();
    let history = editor.history_available();
    let path = source_path(&editor);
    assert!(dirty);
    for purpose in [PdfPurpose::Export, PdfPurpose::Print] {
        let bytes = pdf_bytes(&mut editor, purpose).expect("make PDF snapshot");
        assert!(bytes.starts_with(b"%PDF-"));
        let reader = reopen(bytes);
        assert_eq!(reader.docs()[0].info.pages.len(), 1);
        assert_eq!(editor.dirty(), dirty);
        assert_eq!(editor.history_available(), history);
        assert_eq!(source_path(&editor), path);
    }
}

#[test]
fn export_keeps_the_source_unsaved_and_refuses_every_open_destination() {
    use weave_office::host::Host;
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::default();
    let word = host.new_document(DocumentKind::Text).unwrap();
    host.documents
        .get_mut(&word)
        .unwrap()
        .editor
        .command("text.insert", json!({"text":"Protected export marker"}))
        .unwrap();
    let pdf = host.new_document(DocumentKind::Pdf).unwrap();
    let opened = directory.path().join("open.pdf");
    host.save_as(pdf, &opened).unwrap();
    let original = std::fs::read(&opened).unwrap();
    assert!(host.export_pdf_as(word, &opened).is_err());
    assert!(
        host.export_pdf_as(pdf, &opened).is_err(),
        "PDF export must not silently act as Save"
    );
    #[cfg(unix)]
    {
        let alias = directory.path().join("alias.pdf");
        std::os::unix::fs::symlink(&opened, &alias).unwrap();
        assert!(host.export_pdf_as(word, &alias).is_err());
    }
    assert_eq!(std::fs::read(&opened).unwrap(), original);
    let copy = directory.path().join("export.pdf");
    host.export_pdf_as(word, &copy).unwrap();
    let reader = reopen(std::fs::read(&copy).unwrap());
    assert!(
        pdfcraft_engine::export::Exporter::new(&reader.docs()[0])
            .text(0)
            .unwrap()
            .contains("Protected export marker")
    );
    assert!(host.dirty(word));
    assert!(host.path(word).is_none());
    let invalid = directory.path().join("not-a-file.pdf");
    std::fs::create_dir(&invalid).unwrap();
    assert!(host.export_pdf_as(word, &invalid).is_err());
    assert!(host.dirty(word));
    assert!(host.path(word).is_none());
}

#[test]
fn a_background_writer_observes_later_open_save_as_and_close_operations() {
    use weave_office::host::Host;
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::default();
    // A worker may retain this service while the user changes which paths are open.
    let writer = host.export_writer();
    let pdf = host.new_document(DocumentKind::Pdf).unwrap();
    let first = directory.path().join("first.pdf");
    host.save_as(pdf, &first).unwrap();
    let original = std::fs::read(&first).unwrap();
    assert!(writer(&first, b"must not overwrite").is_err());
    assert_eq!(std::fs::read(&first).unwrap(), original);
    let second = directory.path().join("second.pdf");
    host.save_as(pdf, &second).unwrap();
    assert!(writer(&second, b"must not overwrite").is_err());
    writer(&first, &original).expect("old save-as path is no longer an open identity");
    host.close(pdf, false);
    writer(&second, &original).expect("closed path can be an explicit export destination");
    let reopened = host.open(&first).unwrap();
    assert!(writer(&first, b"must not overwrite").is_err());
    assert!(host.path(reopened).is_some());
}

#[cfg(unix)]
#[test]
fn exported_copies_respect_a_read_only_destination_and_keep_its_bytes() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("read-only.pdf");
    std::fs::write(&path, b"keep read-only content").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    let host = weave_office::host::Host::default();
    assert!(host.export_writer()(&path, b"replacement").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"keep read-only content");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
}
