//! Generated PDFs cross the hosted-editor boundary as owned bytes, never hidden tabs.
//! All documents are synthetic and in memory; these tests do not touch user files.
use pdfcraft_engine::Edit;
use pdfcraft_ui_egui::{Clip, HostedDocument, PdfCraftApp, fill_sign};

fn source() -> PdfCraftApp {
    let mut app = PdfCraftApp::new();
    app.enable_hosted();
    let bytes = app.session.create_blank(300.0, 400.0, 3).unwrap();
    app.hosted_open_bytes("Source.pdf", (*bytes).clone())
        .unwrap();
    assert!(app.apply_edit(Edit::RotatePages {
        pages: vec![1],
        degrees: 90
    }));
    assert!(app.apply_edit(Edit::RotatePages {
        pages: vec![2],
        degrees: 180
    }));
    app
}

fn rotations(app: &PdfCraftApp) -> Vec<u16> {
    let (_, id) = app.active_ids().unwrap();
    app.session
        .get(id)
        .unwrap()
        .info
        .pages
        .iter()
        .map(|page| page.rotation)
        .collect()
}

fn receive(result: HostedDocument) -> (u64, PdfCraftApp) {
    let mut app = PdfCraftApp::new();
    app.enable_hosted();
    app.hosted_open_bytes_with_password(&result.name, result.bytes, result.password.as_deref())
        .unwrap();
    assert_eq!(app.views.len(), 1);
    assert_eq!(app.session.docs().len(), 1);
    assert!(app.hosted_dirty());
    (result.id, app)
}

fn extract(app: &mut PdfCraftApp) -> HostedDocument {
    app.extract_draft.pages = "2".into();
    app.extract_draft.name = "Middle page".into();
    app.extract_draft.separate = false;
    app.extract_draft.delete = true;
    app.extract_selection();
    let mut results = app.take_host_documents();
    assert_eq!(results.len(), 1);
    results.remove(0)
}

#[test]
fn generated_pdf_workflows_keep_one_source_and_open_independent_editors() {
    let mut app = source();
    let source_id = app.active_ids();
    let bytes = (*app.session.create_blank(200.0, 300.0, 1).unwrap()).clone();
    app.open_bytes("Imported.pdf", None, bytes).unwrap();
    app.create_from_clip(Clip::Text("Contributor-original generated text".into()))
        .unwrap();
    app.hosted_command("create.blank", serde_json::Value::Null)
        .unwrap();
    assert!(app.take_host_commands().is_empty());
    app.open_revision(1);
    let original_len = app.session.get(source_id.unwrap().1).unwrap().bytes.len();
    app.view_signed_version(original_len, 1);
    app.summarize_comments();
    let results = app.take_host_documents();
    assert_eq!(results.len(), 6);
    assert!(
        results
            .iter()
            .any(|result| result.name == "Source (revision 1).pdf")
    );
    assert!(
        results
            .iter()
            .any(|result| result.name == "Source (signed version 1).pdf")
    );
    assert!(app.take_host_documents().is_empty());
    for result in results {
        let (request, received) = receive(result);
        assert!(!rotations(&received).is_empty());
        app.hosted_document_result(request, Ok(())).unwrap();
    }
    assert_eq!(app.active_ids(), source_id);
    assert_eq!(app.views.len(), 1);
    assert_eq!(app.session.docs().len(), 1);
    assert_eq!(rotations(&app), [0, 90, 180]);
    app.open_combine_tab();
    assert!(app.combine_showing());
    assert_eq!(app.active_ids(), source_id);
    app.close_combine_tab();
    assert_eq!(app.active_ids(), source_id);
}

#[test]
fn extraction_requires_host_success_before_deleting_only_the_requested_pages() {
    let mut app = source();
    let original = app.active_ids();
    let result = extract(&mut app);
    assert_eq!(result.name, "Middle page.pdf");
    assert_eq!(rotations(&app), [0, 90, 180]);
    let (request, received) = receive(result);
    assert_eq!(rotations(&received), [90]);
    // Even a successful independent parse is insufficient until the host confirms its tab.
    assert_eq!(rotations(&app), [0, 90, 180]);
    app.hosted_document_result(request, Ok(())).unwrap();
    assert_eq!(rotations(&app), [0, 180]);
    assert_eq!(app.active_ids(), original);
    assert_eq!(app.views.len(), 1);
    assert_eq!(app.session.docs().len(), 1);
    assert!(app.hosted_document_result(request, Ok(())).is_err());
    assert_eq!(rotations(&app), [0, 180]);
}

#[test]
fn failed_or_cancelled_host_open_keeps_source_pages_and_reports_error() {
    for reason in ["Host rejected the PDF", "Opening cancelled"] {
        let mut app = source();
        let result = extract(&mut app);
        assert!(
            app.hosted_document_result(result.id, Err(reason.into()))
                .is_err()
        );
        assert_eq!(rotations(&app), [0, 90, 180]);
        assert!(
            app.toast
                .as_ref()
                .is_some_and(|(message, _)| message.contains(reason))
        );
        assert!(app.hosted_dirty());
    }
}

#[test]
fn delayed_acknowledgement_preserves_new_source_edits_and_comment_drafts() {
    let mut app = source();
    let (request, _) = receive(extract(&mut app));
    assert!(app.apply_edit(Edit::RotatePages {
        pages: vec![0],
        degrees: 90
    }));
    assert!(app.hosted_document_result(request, Ok(())).is_err());
    assert_eq!(rotations(&app), [90, 90, 180]);

    let (request, _) = receive(extract(&mut app));
    app.views[0].comments.add_box = "Unposted comment must survive".into();
    assert!(app.hosted_document_result(request, Ok(())).is_err());
    assert_eq!(rotations(&app), [90, 90, 180]);
    assert_eq!(
        app.views[0].comments.add_box,
        "Unposted comment must survive"
    );
    // An existing comment draft also blocks starting a destructive extraction.
    app.extract_selection();
    assert!(app.take_host_documents().is_empty());
    assert_eq!(
        app.views[0].comments.add_box,
        "Unposted comment must survive"
    );
}

#[test]
fn extraction_includes_page_drafts_and_failed_open_retains_the_committed_text() {
    let mut app = source();
    app.views[0].fill_text = Some(fill_sign::TypeBox {
        page: 1,
        at: [30.0, 100.0],
        text: "Draft retained".into(),
        size: 12.0,
        color: [0.0; 3],
        focus: false,
    });
    let (request, received) = receive(extract(&mut app));
    let (_, id) = received.active_ids().unwrap();
    assert!(
        received
            .session
            .get(id)
            .unwrap()
            .info
            .annotations
            .iter()
            .any(|note| note.page == 0 && note.contents.as_deref() == Some("Draft retained"))
    );
    app.hosted_document_result(request, Err("Cancelled".into()))
        .unwrap_err();
    assert_eq!(rotations(&app), [0, 90, 180]);
    let (_, id) = app.active_ids().unwrap();
    assert!(
        app.session
            .get(id)
            .unwrap()
            .info
            .annotations
            .iter()
            .any(|note| note.page == 1 && note.contents.as_deref() == Some("Draft retained"))
    );
}

#[test]
fn generated_byte_entry_never_replaces_an_existing_editor_document() {
    let mut app = source();
    let original = app.active_ids();
    let bytes = (*app.session.create_blank(200.0, 300.0, 1).unwrap()).clone();
    assert!(app.hosted_open_bytes("Replacement.pdf", bytes).is_err());
    assert_eq!(app.active_ids(), original);
    assert_eq!(rotations(&app), [0, 90, 180]);
    assert_eq!(app.session.docs().len(), 1);
}

#[test]
fn host_installs_the_extracted_file_before_acknowledging_source_deletion() {
    use weave_office::{editor::Editor, host::Host};

    let fixture = source();
    let (_, document) = fixture.active_ids().unwrap();
    let bytes = fixture.session.save_bytes(document).unwrap();
    let mut host = Host::default();
    let source_tab = host
        .open_generated_pdf("Source.pdf", (*bytes).clone(), None)
        .unwrap();
    {
        let Editor::Pdf(app) = &mut host.documents.get_mut(&source_tab).unwrap().editor else {
            panic!("PDF source")
        };
        app.extract_draft.pages = "2".into();
        app.extract_draft.name = "Middle page".into();
        app.extract_draft.delete = true;
        app.extract_draft.separate = false;
        app.extract_selection();
        assert_eq!(rotations(app), [0, 90, 180]);
    }
    assert_eq!(host.workspace.tabs().len(), 1);
    assert!(host.receive_generated_pdfs().is_empty());
    assert_eq!(host.workspace.tabs().len(), 2);
    assert_eq!(host.documents.len(), 2);
    let generated_tab = host.workspace.active().unwrap();
    assert_ne!(generated_tab, source_tab);
    assert!(host.workspace.tab(generated_tab).unwrap().path.is_none());
    let Editor::Pdf(generated) = &host.documents.get(&generated_tab).unwrap().editor else {
        panic!("PDF result")
    };
    assert_eq!(rotations(generated), [90]);
    assert!(generated.hosted_dirty());
    let Editor::Pdf(source) = &host.documents.get(&source_tab).unwrap().editor else {
        panic!("PDF source retained")
    };
    assert_eq!(rotations(source), [0, 180]);
    assert_eq!(source.views.len(), 1);
    assert_eq!(source.session.docs().len(), 1);
    assert!(host.receive_generated_pdfs().is_empty());
    assert_eq!(host.workspace.tabs().len(), 2);
}
