//! Every advanced PDF file write uses the host's current export policy.
//! Fixtures and destinations are synthetic and confined to disposable test directories.
use pdfcraft_engine::{SplitBy, actions::Action, compare::OfficeFormat};
use pdfcraft_ui_egui::{PdfCraftApp, SplitPlan};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

fn source() -> PdfCraftApp {
    let mut app = PdfCraftApp::new();
    app.enable_hosted();
    let bytes = app.session.create_blank(300.0, 400.0, 3).unwrap();
    app.hosted_open_bytes("Source.pdf", (*bytes).clone())
        .unwrap();
    app
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
fn rejected_separate_extraction_preserves_every_source_page_and_destination() {
    let directory = tempfile::tempdir().unwrap();
    let protected = directory.path().join("Only.pdf");
    std::fs::write(&protected, b"existing open document").unwrap();
    let mut app = source();
    let called = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
    let calls = called.clone();
    app.set_host_export_writer(Arc::new(move |path, _| {
        calls.lock().unwrap().push(path.to_owned());
        Err("Destination is open in another host tab".into())
    }));
    app.export_dir_override = Some(directory.path().to_string_lossy().into_owned());
    app.extract_draft.name = "Only".into();
    app.extract_draft.pages = "2".into();
    app.extract_draft.separate = true;
    app.extract_draft.delete = true;
    app.extract_selection();
    assert_eq!(called.lock().unwrap().as_slice(), [protected.clone()]);
    assert_eq!(
        std::fs::read(&protected).unwrap(),
        b"existing open document"
    );
    assert_eq!(pages(&app), 3);
    assert!(
        app.toast
            .as_ref()
            .unwrap()
            .0
            .contains("Destination is open")
    );
    assert!(app.take_host_documents().is_empty());
}

#[test]
fn split_and_office_export_deliver_owned_output_to_the_injected_writer() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = source();
    let output = Arc::new(Mutex::new(Vec::<(PathBuf, Vec<u8>)>::new()));
    let writes = output.clone();
    app.set_host_export_writer(Arc::new(move |path, bytes| {
        writes
            .lock()
            .unwrap()
            .push((path.to_owned(), bytes.to_vec()));
        Ok(())
    }));
    app.export_dir_override = Some(directory.path().to_string_lossy().into_owned());
    app.split_active(&SplitPlan::By(SplitBy::PageCount(1)));
    let office = directory.path().join("Source.html");
    app.save_override = Some(office.to_string_lossy().into_owned());
    app.export_office_dialog(OfficeFormat::Html);
    let results = output.lock().unwrap();
    assert_eq!(results.len(), 4);
    for (path, bytes) in results.iter().take(3) {
        assert_eq!(path.parent(), Some(directory.path()));
        let mut accepted = PdfCraftApp::new();
        accepted.enable_hosted();
        accepted
            .hosted_open_bytes("Split.pdf", bytes.clone())
            .unwrap();
        assert_eq!(pages(&accepted), 1);
    }
    assert_eq!(results[3].0, office);
    assert!(!results[3].1.is_empty());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    assert_eq!(pages(&app), 3);
}

#[test]
fn trusted_host_save_does_not_use_the_export_copy_policy() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Source.pdf");
    let mut app = source();
    app.set_host_export_writer(Arc::new(|_, _| Err("All exports denied".into())));
    app.hosted_save(&path).unwrap();
    assert!(!app.hosted_dirty());
    let mut reopened = PdfCraftApp::new();
    reopened.enable_hosted();
    reopened.hosted_open(&path).unwrap();
    assert_eq!(pages(&reopened), 3);
}

#[test]
fn an_existing_worker_observes_a_destination_policy_changed_after_launch() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = source();
    app.run_inline = false;
    app.export_dir_override = Some(directory.path().to_string_lossy().into_owned());
    let permitted = Arc::new(AtomicBool::new(true));
    let current_policy = permitted.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let (decision_tx, decision_rx) = mpsc::channel();
    app.set_host_export_writer(Arc::new(move |path, bytes| {
        started_tx
            .send(path.to_owned())
            .map_err(|e| e.to_string())?;
        release_rx
            .lock()
            .map_err(|e| e.to_string())?
            .recv_timeout(Duration::from_secs(10))
            .map_err(|e| e.to_string())?;
        let allowed = current_policy.load(Ordering::SeqCst);
        decision_tx
            .send((allowed, !bytes.is_empty()))
            .map_err(|e| e.to_string())?;
        if allowed {
            Ok(())
        } else {
            Err("Destination was opened while the worker was running".into())
        }
    }));
    let bytes = app.session.create_blank(200.0, 300.0, 1).unwrap();
    let action = Action {
        name: "Copy synthetic PDF".into(),
        description: String::new(),
        steps: Vec::new(),
        builtin: false,
    };
    app.run_action_on(action, vec![("Worker.pdf".into(), (*bytes).clone())]);
    let destination = started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(destination, directory.path().join("Worker.pdf"));
    // A different host tab claims this path after the worker was created.
    permitted.store(false, Ordering::SeqCst);
    release_tx.send(()).unwrap();
    assert_eq!(
        decision_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        (false, true)
    );
    assert!(!destination.exists());
    assert_eq!(pages(&app), 3);
}
