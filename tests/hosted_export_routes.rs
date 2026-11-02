//! Hosted editors hand file exports to the shared workspace before any destination is written.
use serde_json::{Value, json};
use std::path::Path;
use weave_office::{
    commands::{HostRequest, classify},
    editor::Editor,
    host::Host,
};
use weave_workspace::DocumentKind;

fn queued_command(editor: &mut Editor, id: &str, params: Value, path: &Path) -> HostRequest {
    assert!(!path.exists(), "test destination already exists");
    let result = editor
        .command(id, params.clone())
        .unwrap_or_else(|error| panic!("{id} failed: {error}"));
    assert_eq!(result["hosted"], true, "{id} did not defer to the host");
    assert!(
        !path.exists(),
        "{id} wrote the destination before host dispatch"
    );
    let requests = editor.host_commands();
    assert_eq!(requests, vec![(id.to_owned(), params)]);
    let (queued_id, queued_params) = &requests[0];
    classify(queued_id, queued_params)
}

#[test]
fn grid_explicit_export_paths_stay_in_the_host_queue() {
    let directory = tempfile::tempdir().expect("test directory");
    let mut host = Host::default();
    let tab = host
        .new_document(DocumentKind::Spreadsheet)
        .expect("new workbook");
    let editor = &mut host.documents.get_mut(&tab).expect("workbook tab").editor;

    for (id, extension) in [
        ("file.exportPdf", "pdf"),
        ("file.exportCsv", "csv"),
        ("file.exportHtml", "html"),
    ] {
        let path = directory.path().join(format!("workbook.{extension}"));
        let params = json!({"path": path.to_string_lossy()});
        let request = queued_command(editor, id, params.clone(), &path);
        if extension == "pdf" {
            assert_eq!(request, HostRequest::ExportPdf { path: Some(path) });
        } else {
            assert_eq!(
                request,
                HostRequest::Unsupported {
                    id: id.into(),
                    params,
                }
            );
        }
    }
}

#[test]
fn deck_explicit_export_paths_stay_in_the_host_queue() {
    let directory = tempfile::tempdir().expect("test directory");
    let mut host = Host::default();
    let tab = host
        .new_document(DocumentKind::Presentation)
        .expect("new presentation");
    let editor = &mut host
        .documents
        .get_mut(&tab)
        .expect("presentation tab")
        .editor;

    let pdf_path = directory.path().join("presentation.pdf");
    assert_eq!(
        queued_command(
            editor,
            "file.export",
            json!({"path": pdf_path.to_string_lossy(), "format": "pdf"}),
            &pdf_path,
        ),
        HostRequest::ExportPdf {
            path: Some(pdf_path)
        }
    );

    for (id, format, extension) in [
        ("file.export", "png", "png"),
        ("file.saveTemplate", "potx", "potx"),
    ] {
        let path = directory.path().join(format!("presentation.{extension}"));
        let params = json!({"path": path.to_string_lossy(), "format": format});
        assert_eq!(
            queued_command(editor, id, params.clone(), &path),
            HostRequest::Unsupported {
                id: id.into(),
                params,
            }
        );
    }

    let ambiguous = json!({});
    assert_eq!(
        editor
            .command("app.exportDialog", ambiguous.clone())
            .expect("defer export dialog")["hosted"],
        true
    );
    assert_eq!(
        editor.host_commands(),
        vec![("app.exportDialog".into(), ambiguous.clone())]
    );
    assert_eq!(
        classify("app.exportDialog", &ambiguous),
        HostRequest::Unsupported {
            id: "app.exportDialog".into(),
            params: ambiguous,
        }
    );
}
