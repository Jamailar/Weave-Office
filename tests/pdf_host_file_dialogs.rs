//! Hosted PDF tools request Weave-owned file dialogs; callbacks stay in the source editor.
//! Fixtures are synthetic, and no native picker or desktop window is needed.
use pdfcraft_engine::{Edit, compare::OfficeFormat};
use pdfcraft_ui_egui::{HostFileKind, HostFileRequest, PdfCraftApp};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn source() -> PdfCraftApp {
    let mut app = PdfCraftApp::new();
    app.enable_hosted();
    let bytes = app.session.create_blank(300.0, 400.0, 3).unwrap();
    app.hosted_open_bytes("Source.pdf", (*bytes).clone())
        .unwrap();
    app
}

fn request(app: &mut PdfCraftApp) -> HostFileRequest {
    let mut requests = app.take_host_file_requests();
    assert_eq!(requests.len(), 1);
    requests.remove(0)
}

fn pages(app: &PdfCraftApp) -> usize {
    app.session
        .get(app.active_ids().unwrap().1)
        .unwrap()
        .info
        .pages
        .len()
}

#[test]
fn save_description_round_trips_and_writes_only_after_the_host_answers() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = source();
    app.hosted_save(&directory.path().join("Source.pdf"))
        .unwrap();
    let received = Arc::new(Mutex::new(Vec::<(PathBuf, Vec<u8>)>::new()));
    let writes = received.clone();
    app.set_host_export_writer(Arc::new(move |path, bytes| {
        writes
            .lock()
            .unwrap()
            .push((path.to_owned(), bytes.to_vec()));
        Ok(())
    }));
    app.export_office_dialog(OfficeFormat::Html);
    let asked = request(&mut app);
    assert_eq!(asked.kind, HostFileKind::Save);
    assert_eq!(asked.file_name.as_deref(), Some("Source.html"));
    assert_eq!(asked.directory.as_deref(), Some(directory.path()));
    assert!(asked.title.is_some());
    assert_eq!(asked.filters.len(), 1);
    assert_eq!(asked.filters[0].extensions, ["html"]);
    let serialized = serde_json::to_string(&asked).unwrap();
    assert_eq!(
        serde_json::from_str::<HostFileRequest>(&serialized).unwrap(),
        asked
    );
    assert!(received.lock().unwrap().is_empty());
    let output = directory.path().join("Chosen.html");
    app.hosted_file_result(asked.id, Some(vec![output.clone()]))
        .unwrap();
    let written = received.lock().unwrap();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].0, output);
    assert!(!written[0].1.is_empty());
    assert!(!output.exists());
    assert_eq!(pages(&app), 3);
}

#[test]
fn cancelling_separate_extraction_never_writes_or_deletes_source_pages() {
    let mut app = source();
    let written = Arc::new(Mutex::new(0));
    let calls = written.clone();
    app.set_host_export_writer(Arc::new(move |_, _| {
        *calls.lock().unwrap() += 1;
        Ok(())
    }));
    app.extract_draft.pages = "2".into();
    app.extract_draft.separate = true;
    app.extract_draft.delete = true;
    app.extract_selection();
    let asked = request(&mut app);
    assert_eq!(asked.kind, HostFileKind::Folder);
    app.hosted_file_result(asked.id, None).unwrap();
    assert_eq!(*written.lock().unwrap(), 0);
    assert_eq!(pages(&app), 3);
    assert!(
        app.hosted_file_result(asked.id, Some(vec![PathBuf::from("unused")]))
            .is_err()
    );
    assert_eq!(*written.lock().unwrap(), 0);
}

#[test]
fn file_and_multiple_selection_callbacks_keep_target_and_closed_source_checks() {
    let directory = tempfile::tempdir().unwrap();
    let attachment = directory.path().join("Original.txt");
    std::fs::write(&attachment, "Contributor-original attachment").unwrap();
    let mut app = source();
    app.attach_file_dialog();
    let asked = request(&mut app);
    assert_eq!(asked.kind, HostFileKind::File);
    app.hosted_file_result(asked.id, Some(vec![attachment]))
        .unwrap();
    let (_, document) = app.active_ids().unwrap();
    assert_eq!(app.session.get(document).unwrap().info.attachments.len(), 1);

    let input = directory.path().join("Insert.pdf");
    std::fs::write(&input, &*app.session.create_blank(200.0, 300.0, 1).unwrap()).unwrap();
    app.insert_from_file_dialog();
    let asked = request(&mut app);
    assert_eq!(asked.kind, HostFileKind::Files);
    assert!(app.apply_edit(Edit::RotatePages {
        pages: vec![0],
        degrees: 90
    }));
    assert!(
        app.hosted_file_result(asked.id, Some(vec![input.clone()]))
            .is_err()
    );
    assert_eq!(pages(&app), 3);

    // Even target-less exports must not execute after their originating document closes.
    app.export_office_dialog(OfficeFormat::Html);
    let asked = request(&mut app);
    app.session.close(document);
    assert!(
        app.hosted_file_result(
            asked.id,
            Some(vec![directory.path().join("Forbidden.html")])
        )
        .is_err()
    );
    assert!(!directory.path().join("Forbidden.html").exists());
}

#[test]
fn chained_multiple_create_then_folder_selection_remains_host_owned() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("Original.txt");
    std::fs::write(
        &input,
        "Contributor-original text converted by the host dialog flow.",
    )
    .unwrap();
    let mut app = source();
    let received = Arc::new(Mutex::new(Vec::<(PathBuf, Vec<u8>)>::new()));
    let writes = received.clone();
    app.set_host_export_writer(Arc::new(move |path, bytes| {
        writes
            .lock()
            .unwrap()
            .push((path.to_owned(), bytes.to_vec()));
        Ok(())
    }));
    app.create_multiple_separate_dialog();
    let first = request(&mut app);
    assert_eq!(first.kind, HostFileKind::Files);
    assert!(
        first
            .filters
            .iter()
            .any(|filter| filter.extensions.iter().any(|ext| ext == "txt"))
    );
    app.hosted_file_result(first.id, Some(vec![input])).unwrap();
    let second = request(&mut app);
    assert_eq!(second.kind, HostFileKind::Folder);
    assert_ne!(first.id, second.id);
    assert!(received.lock().unwrap().is_empty());
    app.hosted_file_result(second.id, Some(vec![directory.path().to_owned()]))
        .unwrap();
    let results = received.lock().unwrap();
    assert_eq!(results.len(), 1);
    let mut generated = PdfCraftApp::new();
    generated.enable_hosted();
    generated
        .hosted_open_bytes("Original.pdf", results[0].1.clone())
        .unwrap();
    assert_eq!(pages(&generated), 1);
    assert_eq!(pages(&app), 3);
}

#[test]
fn cancelling_one_host_dialog_advances_the_next_without_running_cancelled_work() {
    let mut app = source();
    app.attach_file_dialog();
    let first = request(&mut app);
    app.insert_from_file_dialog();
    assert!(app.take_host_file_requests().is_empty());
    app.hosted_file_result(first.id, None).unwrap();
    let next = request(&mut app);
    assert_eq!(next.kind, HostFileKind::Files);
    app.hosted_file_result(next.id, None).unwrap();
    assert!(app.take_host_file_requests().is_empty());
    assert_eq!(pages(&app), 3);
    assert!(
        app.session
            .get(app.active_ids().unwrap().1)
            .unwrap()
            .info
            .attachments
            .is_empty()
    );
}
