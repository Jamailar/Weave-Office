//! Image picks belong to their originating Office tab and survive an immediate save.
use serde_json::json;
use std::{fs, path::Path};
use weave_office::{editor::Editor, host::Host};
use weave_workspace::{CloseResult, DocumentKind, TabId};

// 1×1 RGB PNG fixture, kept inline so the test has no external file dependency.
const PNG: [u8; 69] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

fn image_file(directory: &Path) -> std::path::PathBuf {
    let path = directory.join("picked.png");
    fs::write(&path, PNG).expect("write image fixture");
    path
}

fn word_has_png(host: &Host, id: TabId) -> bool {
    let Editor::Word(app) = &host.documents.get(&id).expect("Word tab").editor else {
        panic!("expected Word editor");
    };
    app.session
        .doc
        .media
        .values()
        .any(|bytes| bytes.as_slice() == PNG.as_slice())
}

fn deck_has_png(host: &Host, id: TabId) -> bool {
    let Editor::Slides(app) = &host.documents.get(&id).expect("Deck tab").editor else {
        panic!("expected Deck editor");
    };
    app.session
        .active()
        .expect("presentation")
        .doc
        .media
        .iter()
        .any(|item| item.data.as_slice() == PNG.as_slice())
}

fn grid_has_png(host: &Host, id: TabId) -> bool {
    let Editor::Sheet(app) = &host.documents.get(&id).expect("Grid tab").editor else {
        panic!("expected Grid editor");
    };
    app.session
        .active()
        .expect("workbook")
        .wb
        .active()
        .expect("sheet")
        .images
        .iter()
        .any(|image| image.data.as_slice() == PNG.as_slice())
}

#[test]
fn word_picture_pick_is_committed_by_flush_before_save() {
    let directory = tempfile::tempdir().expect("test directory");
    let picture = image_file(directory.path());
    let destination = directory.path().join("writing.docx");
    let mut host = Host::default();
    let id = host.new_document(DocumentKind::Text).expect("new document");
    let document = host.documents.get_mut(&id).expect("Word tab");

    // This is the actual Insert > Picture command, with no injected image data or path.
    document
        .editor
        .command("insert.picture", json!({}))
        .expect("request picture");
    let requests = document.file_services.take_requests();
    assert_eq!(requests.len(), 1);
    document
        .file_services
        .complete(requests[0].id, Some(vec![picture]))
        .expect("complete picture pick");
    assert!(
        !word_has_png(&host, id),
        "the answer is pending until the editor flushes"
    );
    host.documents
        .get_mut(&id)
        .unwrap()
        .editor
        .flush()
        .expect("commit picked picture");
    assert!(word_has_png(&host, id));
    host.save_as(id, &destination).expect("save Word document");
    assert_eq!(host.close(id, false), CloseResult::Closed);
    let reopened = host.open(&destination).expect("reopen Word document");
    assert!(
        word_has_png(&host, reopened),
        "reopened document lost the image bytes"
    );
}

#[test]
fn deck_picture_answer_stays_with_source_tab_after_switch_and_immediate_save() {
    let directory = tempfile::tempdir().expect("test directory");
    let picture = image_file(directory.path());
    let destination = directory.path().join("slides.pptx");
    let mut host = Host::default();
    let source = host
        .new_document(DocumentKind::Presentation)
        .expect("source deck");
    let other = host
        .new_document(DocumentKind::Presentation)
        .expect("other deck");
    let source_document = host.documents.get_mut(&source).unwrap();
    source_document
        .editor
        .command("app.insertPictureDialog", json!({}))
        .expect("request picture");
    let requests = source_document.file_services.take_requests();
    assert_eq!(requests.len(), 1);

    assert!(host.workspace.activate(other));
    host.documents
        .get(&source)
        .unwrap()
        .file_services
        .complete(requests[0].id, Some(vec![picture]))
        .expect("complete source pick");
    assert!(!deck_has_png(&host, other));
    host.documents
        .get_mut(&source)
        .unwrap()
        .editor
        .flush()
        .expect("commit source inbox");
    assert!(deck_has_png(&host, source));
    assert!(!deck_has_png(&host, other));
    host.save_as(source, &destination)
        .expect("save source deck");
    assert_eq!(host.close(source, false), CloseResult::Closed);
    let reopened = host.open(&destination).expect("reopen source deck");
    assert!(
        deck_has_png(&host, reopened),
        "reopened deck lost the image bytes"
    );
    assert!(
        !deck_has_png(&host, other),
        "other deck received the source image"
    );
}

#[test]
fn cancelled_or_late_picture_answers_do_not_reach_another_deck() {
    let directory = tempfile::tempdir().expect("test directory");
    let picture = image_file(directory.path());
    let mut host = Host::default();
    let cancelled = host
        .new_document(DocumentKind::Presentation)
        .expect("cancelled deck");
    let other = host
        .new_document(DocumentKind::Presentation)
        .expect("other deck");
    let document = host.documents.get_mut(&cancelled).unwrap();
    document
        .editor
        .command("app.insertPictureDialog", json!({}))
        .unwrap();
    let request = document
        .file_services
        .take_requests()
        .pop()
        .expect("picture request");
    document
        .file_services
        .complete(request.id, None)
        .expect("cancel pick");
    document.editor.flush().expect("drain cancellation");
    assert!(!deck_has_png(&host, cancelled));
    assert!(!deck_has_png(&host, other));

    let document = host.documents.get_mut(&cancelled).unwrap();
    document
        .editor
        .command("app.insertPictureDialog", json!({}))
        .unwrap();
    let request = document
        .file_services
        .take_requests()
        .pop()
        .expect("late picture request");
    let broker = document.file_services.clone();
    assert_eq!(host.close(cancelled, true), CloseResult::Closed);
    // A completion after the originating tab closes must not be delivered to the other tab.
    let _ = broker.complete(request.id, Some(vec![picture]));
    host.documents
        .get_mut(&other)
        .unwrap()
        .editor
        .flush()
        .expect("drain other deck");
    assert!(!deck_has_png(&host, other));
}

#[test]
fn grid_picture_service_keeps_original_bytes_through_insert_and_reopen() {
    let directory = tempfile::tempdir().expect("test directory");
    let picture = image_file(directory.path());
    let destination = directory.path().join("workbook.xlsx");
    let mut host = Host::default();
    let id = host
        .new_document(DocumentKind::Spreadsheet)
        .expect("new workbook");
    let document = host.documents.get_mut(&id).unwrap();
    let inbox = gridcraft_ui_egui::Inbox::default();
    document
        .editor
        .command("insert.picture", json!({}))
        .expect("open insert picture dialog");
    let Editor::Sheet(app) = &mut document.editor else {
        panic!("expected Grid editor")
    };
    app.services
        .pick_picture_async
        .as_ref()
        .expect("host picture service")(inbox.clone());
    let request = document
        .file_services
        .take_requests()
        .pop()
        .expect("picture request");
    document
        .file_services
        .complete(request.id, Some(vec![picture]))
        .expect("complete Grid picture pick");
    let delivered = inbox.lock().unwrap().pop().expect("delivered image bytes");
    assert_eq!(
        delivered.1.as_slice(),
        PNG.as_slice(),
        "Grid service changed the original bytes"
    );

    // The dialog normally turns the delivered bytes into this same insert.picture command.
    app.hosted_command(
        "insert.picture",
        json!({"base64": gridcraft_engine::io::base64_encode(&delivered.1), "placement": "overCells"}),
    )
    .expect("insert picture");
    assert!(grid_has_png(&host, id));
    host.save_as(id, &destination).expect("save workbook");
    assert_eq!(host.close(id, false), CloseResult::Closed);
    let reopened = host.open(&destination).expect("reopen workbook");
    assert!(
        grid_has_png(&host, reopened),
        "reopened workbook lost the image bytes"
    );
}
