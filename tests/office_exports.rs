//! Real Deck export commands use per-origin native selections and protected host writes.
use deckcraft_ui_egui::{HostExportFile, HostExportRequest, SlideApp};
use serde_json::{Value, json};
use std::{fs, path::Path};
use weave_office::{
    editor::Editor, file_dialog::FileDialogMode, host::Host, office_files::OfficeFileRequest,
};
use weave_workspace::{CloseResult, DocumentKind, TabId};

fn deck(host: &mut Host, id: TabId) -> &mut SlideApp {
    let Editor::Slides(app) = &mut host.documents.get_mut(&id).unwrap().editor else {
        panic!("expected presentation")
    };
    app
}

fn title(host: &mut Host, id: TabId, title: &str) {
    let app = deck(host, id);
    let shape = app.session.active().unwrap().doc.slides[0]
        .shapes
        .iter()
        .find(|shape| shape.text.is_some())
        .unwrap()
        .id
        .0;
    app.hosted_command("text.set", json!({"id": shape, "text": title}))
        .unwrap();
}

fn request(host: &mut Host, id: TabId, format: &str, extra: Value) -> OfficeFileRequest {
    assert!(deck(host, id).export_as(format, extra));
    let mut requests = host.documents[&id].file_services.take_requests();
    assert_eq!(requests.len(), 1);
    requests.pop().unwrap()
}

fn finish(host: &Host, id: TabId, request: OfficeFileRequest, path: &Path) {
    host.documents[&id]
        .file_services
        .complete(request.id, Some(vec![path.into()]))
        .unwrap();
}

#[test]
fn pptx_and_outline_export_snapshots_preserve_source_path_dirty_and_history() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.pptx");
    let copy = directory.path().join("copy.pptx");
    let outline = directory.path().join("outline.txt");
    let mut host = Host::default();
    let id = host.new_document(DocumentKind::Presentation).unwrap();
    title(&mut host, id, "Saved original");
    host.save_as(id, &source).unwrap();
    let source_bytes = fs::read(&source).unwrap();
    title(&mut host, id, "Export snapshot");
    let pptx = request(&mut host, id, "pptx", json!({}));
    assert_eq!(pptx.mode, FileDialogMode::Save);
    assert!(pptx.options.file_name.ends_with(".pptx"));
    assert_eq!(pptx.options.filters[0].1, ["pptx"]);
    let txt = request(&mut host, id, "txt", json!({}));
    title(&mut host, id, "Newer edit while selecting");
    let history = deck(&mut host, id)
        .session
        .active()
        .unwrap()
        .history
        .undo
        .len();
    finish(&host, id, pptx, &copy);
    finish(&host, id, txt, &outline);
    assert_eq!(
        fs::read_to_string(outline)
            .unwrap()
            .contains("Export snapshot"),
        true
    );
    assert!(host.dirty(id));
    assert_eq!(
        host.path(id),
        Some(source.canonicalize().unwrap().as_path())
    );
    assert_eq!(
        deck(&mut host, id)
            .session
            .active()
            .unwrap()
            .history
            .undo
            .len(),
        history
    );
    assert_eq!(fs::read(source).unwrap(), source_bytes);
    let mut reopened = Host::default();
    let copy_id = reopened.open(&copy).unwrap();
    assert_eq!(
        deck(&mut reopened, copy_id)
            .session
            .active()
            .unwrap()
            .doc
            .slides[0]
            .title(),
        "Export snapshot"
    );
}

#[test]
fn requests_with_same_id_stay_per_origin_and_cancel_or_close_cannot_write() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::default();
    let first = host.new_document(DocumentKind::Presentation).unwrap();
    let second = host.new_document(DocumentKind::Presentation).unwrap();
    title(&mut host, first, "First source");
    title(&mut host, second, "Second source");
    let first_request = request(&mut host, first, "txt", json!({}));
    let second_request = request(&mut host, second, "txt", json!({}));
    assert_eq!(first_request.id, second_request.id);
    host.documents[&first]
        .file_services
        .complete(first_request.id, None)
        .unwrap();
    assert!(
        host.documents[&first]
            .file_services
            .complete(
                first_request.id,
                Some(vec![directory.path().join("cancelled.txt")])
            )
            .is_err()
    );
    let second_path = directory.path().join("second.txt");
    finish(&host, second, second_request, &second_path);
    assert!(
        fs::read_to_string(&second_path)
            .unwrap()
            .contains("Second source")
    );
    let late = request(&mut host, first, "txt", json!({}));
    let broker = host.documents[&first].file_services.clone();
    assert_eq!(host.close(first, true), CloseResult::Closed);
    assert!(
        broker
            .complete(late.id, Some(vec![directory.path().join("late.txt")]))
            .is_err()
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn save_export_rejects_source_and_other_live_open_destinations() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.pptx");
    let other = directory.path().join("other.pptx");
    let mut host = Host::default();
    let id = host.new_document(DocumentKind::Presentation).unwrap();
    host.save_as(id, &source).unwrap();
    let source_bytes = fs::read(&source).unwrap();
    fs::write(&other, &source_bytes).unwrap();
    let writer = host.office_export_writer();
    // The policy must use the live registry, even for a writer obtained earlier.
    host.open(&other).unwrap();
    for path in [&source, &other] {
        let req = request(&mut host, id, "pptx", json!({}));
        assert!(
            host.documents[&id]
                .file_services
                .complete(req.id, Some(vec![path.clone()]))
                .is_err()
        );
        assert_eq!(fs::read(path).unwrap(), source_bytes);
    }
    assert!(writer(&[(other.clone(), b"must not write".to_vec())], true).is_err());
    #[cfg(unix)]
    {
        let alias = directory.path().join("alias.pptx");
        std::os::unix::fs::symlink(&other, &alias).unwrap();
        assert!(writer(&[(alias, b"must not write".to_vec())], true).is_err());
        assert_eq!(fs::read(other).unwrap(), source_bytes);
    }
    let external = directory.path().join("external.txt");
    fs::write(&external, b"previous external file").unwrap();
    let req = request(&mut host, id, "txt", json!({}));
    finish(&host, id, req, &external);
    assert_ne!(fs::read(external).unwrap(), b"previous external file");
}

#[test]
fn all_slides_png_uses_folder_even_for_one_slide_and_never_overwrites() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::default();
    let id = host.new_document(DocumentKind::Presentation).unwrap();
    assert_eq!(
        deck(&mut host, id)
            .session
            .active()
            .unwrap()
            .doc
            .slides
            .len(),
        1
    );
    let req = request(&mut host, id, "pngAll", json!({}));
    assert_eq!(req.mode, FileDialogMode::Folder);
    finish(&host, id, req, directory.path());
    let entries = fs::read_dir(directory.path())
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(entries.len(), 1);
    let output = entries[0].path();
    assert!(fs::read(&output).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
    fs::write(&output, b"external replacement").unwrap();
    let req = request(&mut host, id, "pngAll", json!({}));
    assert!(
        host.documents[&id]
            .file_services
            .complete(req.id, Some(vec![directory.path().into()]))
            .is_err()
    );
    assert_eq!(fs::read(output).unwrap(), b"external replacement");
}

#[test]
fn folder_batch_preflights_collisions_before_writing_any_file() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("first.png");
    let existing = directory.path().join("second.png");
    fs::write(&existing, b"existing").unwrap();
    let host = Host::default();
    let writer = host.office_export_writer();
    assert!(
        writer(
            &[(missing.clone(), vec![1]), (existing.clone(), vec![2])],
            false
        )
        .is_err()
    );
    assert!(!missing.exists());
    assert_eq!(fs::read(existing).unwrap(), b"existing");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn malformed_export_names_and_target_counts_never_open_a_panel() {
    let mut host = Host::default();
    let id = host.new_document(DocumentKind::Presentation).unwrap();
    for (names, directory) in [
        (vec![], true),
        (vec![""], false),
        (vec!["../escape.png"], true),
        (vec!["/absolute.png"], true),
        (vec!["a\\b.png"], true),
        (vec!["."], true),
        (vec!["a.png", "a.png"], true),
        (vec!["a.png", "b.png"], false),
    ] {
        let request = HostExportRequest {
            files: names
                .into_iter()
                .map(|name| HostExportFile {
                    name: name.into(),
                    bytes: vec![1],
                })
                .collect(),
            directory,
        };
        assert!(deck(&mut host, id).services.export_files.as_mut().unwrap()(request).is_err());
        assert!(host.documents[&id].file_services.take_requests().is_empty());
    }
}

#[test]
fn pdf_export_preserves_hidden_slide_and_handout_options_through_native_save() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::default();
    let id = host.new_document(DocumentKind::Presentation).unwrap();
    for _ in 0..2 {
        deck(&mut host, id)
            .hosted_command("slide.new", json!({"layout":"blank"}))
            .unwrap();
    }
    deck(&mut host, id)
        .hosted_command("slide.hide", json!({"index":1,"hidden":true}))
        .unwrap();
    for (name, extra, pages) in [
        ("visible", json!({}), 2),
        ("all", json!({"includeHidden":true}), 3),
        (
            "handouts",
            json!({"includeHidden":true,"layout":"handouts","perPage":2}),
            2,
        ),
    ] {
        let output = directory.path().join(format!("{name}.pdf"));
        let req = request(&mut host, id, "pdf", extra);
        finish(&host, id, req, &output);
        assert!(fs::read(&output).unwrap().starts_with(b"%PDF-"));
        let mut reopened = Host::default();
        let pdf = reopened.open(&output).unwrap();
        let Editor::Pdf(app) = &reopened.documents[&pdf].editor else {
            panic!("PDF")
        };
        let (_, document) = app.active_ids().unwrap();
        assert_eq!(app.session.get(document).unwrap().info.pages.len(), pages);
    }
}

#[test]
fn failed_export_retries_same_id_with_original_snapshot_and_consumes_success() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.pptx");
    let copy = directory.path().join("copy.pptx");
    let mut host = Host::default();
    let id = host.new_document(DocumentKind::Presentation).unwrap();
    title(&mut host, id, "Saved source");
    host.save_as(id, &source).unwrap();
    let source_bytes = fs::read(&source).unwrap();
    title(&mut host, id, "Original export snapshot");
    let req = request(&mut host, id, "pptx", json!({}));
    let broker = host.documents[&id].file_services.clone();
    assert!(broker.complete(req.id, Some(vec![source.clone()])).is_err());
    assert!(broker.can_retry(req.id));
    assert!(
        broker.take_requests().is_empty(),
        "retry must be explicit, not queued automatically"
    );
    title(&mut host, id, "Edited after failed selection");
    let history = deck(&mut host, id)
        .session
        .active()
        .unwrap()
        .history
        .undo
        .len();
    broker.complete(req.id, Some(vec![copy.clone()])).unwrap();
    assert!(!broker.can_retry(req.id));
    assert!(
        broker
            .complete(req.id, Some(vec![directory.path().join("late.pptx")]))
            .is_err()
    );
    assert!(!directory.path().join("late.pptx").exists());
    assert_eq!(fs::read(&source).unwrap(), source_bytes);
    assert_eq!(
        host.path(id),
        Some(source.canonicalize().unwrap().as_path())
    );
    assert!(host.dirty(id));
    assert_eq!(
        deck(&mut host, id)
            .session
            .active()
            .unwrap()
            .history
            .undo
            .len(),
        history
    );
    let mut reopened = Host::default();
    let copy_id = reopened.open(&copy).unwrap();
    assert_eq!(
        deck(&mut reopened, copy_id)
            .session
            .active()
            .unwrap()
            .doc
            .slides[0]
            .title(),
        "Original export snapshot"
    );
}

#[test]
fn cancelled_or_closed_failed_exports_cannot_be_revived_by_retry() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::default();
    let id = host.new_document(DocumentKind::Presentation).unwrap();
    let broker = host.documents[&id].file_services.clone();
    let req = request(&mut host, id, "txt", json!({}));
    assert!(
        broker
            .complete(req.id, Some(vec![directory.path().join("wrong.pdf")]))
            .is_err()
    );
    assert!(broker.can_retry(req.id));
    broker.complete(req.id, None).unwrap();
    assert!(!broker.can_retry(req.id));
    assert!(
        broker
            .complete(req.id, Some(vec![directory.path().join("cancelled.txt")]))
            .is_err()
    );
    let req = request(&mut host, id, "txt", json!({}));
    assert!(
        broker
            .complete(req.id, Some(vec![directory.path().join("wrong.pdf")]))
            .is_err()
    );
    assert!(broker.can_retry(req.id));
    assert_eq!(host.close(id, true), CloseResult::Closed);
    assert!(!broker.can_retry(req.id));
    assert!(
        broker
            .complete(req.id, Some(vec![directory.path().join("closed.txt")]))
            .is_err()
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn wrong_or_missing_extension_is_not_written_and_same_export_can_retry() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::default();
    let id = host.new_document(DocumentKind::Presentation).unwrap();
    let req = request(&mut host, id, "pptx", json!({}));
    let broker = host.documents[&id].file_services.clone();
    let wrong = directory.path().join("existing.pdf");
    fs::write(&wrong, b"original pdf bytes").unwrap();
    assert!(broker.complete(req.id, Some(vec![wrong.clone()])).is_err());
    assert_eq!(fs::read(wrong).unwrap(), b"original pdf bytes");
    assert!(broker.can_retry(req.id));
    let no_extension = directory.path().join("no-extension");
    assert!(
        broker
            .complete(req.id, Some(vec![no_extension.clone()]))
            .is_err()
    );
    assert!(!no_extension.exists());
    assert!(!directory.path().join("no-extension.pptx").exists());
    let correct = directory.path().join("correct.PPTX");
    broker
        .complete(req.id, Some(vec![correct.clone()]))
        .unwrap();
    assert!(!broker.can_retry(req.id));
    assert!(fs::read(&correct).unwrap().starts_with(b"PK"));
    Host::default().open(&correct).unwrap();
}
