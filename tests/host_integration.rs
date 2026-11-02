//! Integration acceptance for real hosted editors and their native Office/PDF codecs.
//! These tests exercise document ownership and persistence without native windows or clipboard use.
use serde_json::{Value, json};
use std::{fs, path::Path};
use weave_office::{
    editor::Editor,
    host::{Host, extension},
};
use weave_workspace::{CloseResult, DocumentKind, TabId};

const KINDS: [DocumentKind; 6] = [
    DocumentKind::Text,
    DocumentKind::Spreadsheet,
    DocumentKind::Presentation,
    DocumentKind::Pdf,
    DocumentKind::Markdown,
    DocumentKind::PlainText,
];

fn command(host: &mut Host, id: TabId, command: &str, params: Value) -> Value {
    host.documents
        .get_mut(&id)
        .expect("editor exists")
        .editor
        .command(command, params)
        .unwrap_or_else(|error| panic!("{command} failed: {error}"))
}

/// One undoable content change in each real editor. PDF rotation changes its page dictionary,
/// rather than merely the viewer's rotation, and is therefore observable after reopening.
fn edit(host: &mut Host, id: TabId, marker: &str) {
    let editor = &mut host.documents.get_mut(&id).expect("editor exists").editor;
    match editor {
        Editor::Word(app) => {
            app.hosted_command("select.all", json!({}))
                .expect("select Word content");
            app.hosted_command("text.insert", json!({"text": marker}))
                .expect("insert Word content");
        }
        Editor::Sheet(app) => {
            app.hosted_command("cell.set", json!({"cell": "A1", "input": marker}))
                .expect("edit cell");
        }
        Editor::Slides(app) => {
            app.hosted_command("slide.go", json!({"index": 0}))
                .expect("select first slide");
            let shape = app
                .session
                .active()
                .expect("presentation exists")
                .doc
                .slides
                .first()
                .expect("initial slide exists")
                .shapes
                .iter()
                .find(|shape| shape.text.is_some())
                .expect("initial title shape exists")
                .id
                .0;
            app.hosted_command("text.set", json!({"id": shape, "text": marker}))
                .expect("edit slide title");
        }
        Editor::Pdf(app) => {
            app.hosted_command("page.rotate", json!({}))
                .expect("rotate PDF page content");
        }
        Editor::TextFile(app) => {
            let end = app.text().len();
            app.apply(weave_text_editor::edit::Edit {
                range: 0..end,
                text: marker.into(),
                selection: weave_text_editor::edit::Selection::caret(marker.len()),
            });
        }
    }
    host.sync_dirty();
}

/// Read document content through command/model APIs; do not infer success from a dirty flag.
fn content(host: &mut Host, id: TabId) -> Value {
    match &mut host.documents.get_mut(&id).expect("editor exists").editor {
        Editor::Word(app) => {
            let result = app
                .hosted_command("document.text", json!({}))
                .expect("read Word content");
            json!(
                result["text"]
                    .as_str()
                    .expect("Word text is a string")
                    .trim_end_matches(['\r', '\n'])
            )
        }
        Editor::Sheet(app) => app
            .hosted_command("cell.get", json!({"cell": "A1"}))
            .expect("read cell")["value"]
            .clone(),
        Editor::Slides(app) => {
            json!(
                app.session
                    .active()
                    .expect("presentation exists")
                    .doc
                    .slides
                    .first()
                    .expect("slide exists")
                    .title()
            )
        }
        Editor::TextFile(app) => json!(app.text()),
        Editor::Pdf(app) => {
            let (_, doc_id) = app.active_ids().expect("PDF view exists");
            let doc = app.session.get(doc_id).expect("PDF exists");
            json!(
                doc.info
                    .pages
                    .iter()
                    .map(|page| page.rotation)
                    .collect::<Vec<_>>()
            )
        }
    }
}

fn assert_dirty(host: &mut Host, id: TabId, expected: bool) {
    host.sync_dirty();
    assert_eq!(host.dirty(id), expected, "editor dirty state for {id:?}");
    assert_eq!(
        host.workspace.tab(id).expect("tab exists").is_dirty(),
        expected,
        "tab must show its editor's dirty state"
    );
}

fn save(host: &mut Host, id: TabId, path: &Path) {
    host.save_as(id, path)
        .unwrap_or_else(|error| panic!("saving {}: {error}", path.display()));
    assert_dirty(host, id, false);
    assert_eq!(
        host.path(id),
        Some(path.canonicalize().expect("saved path exists").as_path())
    );
}

#[test]
fn four_formats_edit_save_close_and_reopen_with_their_content() {
    let directory = tempfile::tempdir().expect("test directory");
    let mut host = Host::default();
    for kind in KINDS {
        let id = host.new_document(kind).expect("create real editor");
        let baseline = content(&mut host, id);
        edit(&mut host, id, "Weave 集成验收 123");
        let edited = content(&mut host, id);
        assert_ne!(edited, baseline, "{kind:?} content changed");
        if kind != DocumentKind::Pdf {
            assert_eq!(edited, json!("Weave 集成验收 123"));
        } else {
            assert_eq!(edited, json!([90]));
        }
        assert_dirty(&mut host, id, true);
        assert_eq!(host.close(id, false), CloseResult::NeedsSave);
        assert_eq!(
            content(&mut host, id),
            edited,
            "close refusal preserves edits"
        );
        let path = directory
            .path()
            .join(format!("roundtrip.{}", extension(kind)));
        save(&mut host, id, &path);
        assert!(!fs::read(&path).expect("saved file").is_empty());
        assert_eq!(host.close(id, false), CloseResult::Closed);
        assert!(!host.documents.contains_key(&id));
        let reopened = host.open(&path).expect("reopen saved file");
        assert_ne!(reopened, id, "reopening creates a fresh editor");
        assert_eq!(
            content(&mut host, reopened),
            edited,
            "{kind:?} codec round-trip"
        );
        assert_dirty(&mut host, reopened, false);
        assert_eq!(host.close(reopened, false), CloseResult::Closed);
    }
    assert!(host.documents.is_empty());
    assert!(host.workspace.tabs().is_empty());
}

#[test]
fn opening_the_same_path_or_symlink_keeps_the_existing_unsaved_editor() {
    let directory = tempfile::tempdir().expect("test directory");
    for kind in KINDS {
        let mut host = Host::default();
        let id = host.new_document(kind).expect("new editor");
        let path = directory
            .path()
            .join(format!("original.{}", extension(kind)));
        save(&mut host, id, &path);
        edit(&mut host, id, "Unsaved content must survive duplicate open");
        let unsaved = content(&mut host, id);
        let other = host.new_document(kind).expect("another active tab");
        assert_eq!(host.workspace.active(), Some(other));
        assert_eq!(host.open(&path).expect("duplicate open"), id);
        assert_eq!(host.workspace.active(), Some(id));
        assert_eq!(content(&mut host, id), unsaved);
        assert_dirty(&mut host, id, true);
        #[cfg(unix)]
        {
            let alias = directory.path().join(format!("alias.{}", extension(kind)));
            std::os::unix::fs::symlink(&path, &alias).expect("symlink to saved file");
            assert!(host.workspace.activate(other));
            assert_eq!(host.open(&alias).expect("open symlink"), id);
            assert_eq!(host.workspace.active(), Some(id));
            assert_eq!(content(&mut host, id), unsaved);
            assert_dirty(&mut host, id, true);
        }
        assert_eq!(
            host.documents.len(),
            2,
            "duplicates must not allocate another editor"
        );
        assert_eq!(host.workspace.tabs().len(), 2);
    }
}

#[test]
fn same_type_tabs_keep_independent_content_undo_redo_and_saved_state() {
    let directory = tempfile::tempdir().expect("test directory");
    for kind in KINDS {
        let mut host = Host::default();
        let first = host.new_document(kind).expect("first editor");
        let second = host.new_document(kind).expect("second editor");
        let first_path = directory.path().join(format!("first.{}", extension(kind)));
        let second_path = directory.path().join(format!("second.{}", extension(kind)));
        save(&mut host, first, &first_path);
        save(&mut host, second, &second_path);
        let first_baseline = content(&mut host, first);
        let second_baseline = content(&mut host, second);
        edit(&mut host, first, "First independent edit");
        edit(&mut host, second, "Second independent edit");
        let first_edited = content(&mut host, first);
        let second_edited = content(&mut host, second);
        assert_dirty(&mut host, first, true);
        assert_dirty(&mut host, second, true);

        assert!(host.workspace.activate(first));
        command(&mut host, first, "edit.undo", json!({}));
        assert_eq!(
            content(&mut host, first),
            first_baseline,
            "{kind:?} undo first"
        );
        assert_eq!(
            content(&mut host, second),
            second_edited,
            "undo cannot touch another tab"
        );
        assert_dirty(&mut host, first, false);
        assert_dirty(&mut host, second, true);
        command(&mut host, first, "edit.redo", json!({}));
        assert_eq!(content(&mut host, first), first_edited);
        save(&mut host, first, &first_path);
        assert_dirty(&mut host, second, true);

        assert!(host.workspace.activate(second));
        command(&mut host, second, "edit.undo", json!({}));
        assert_eq!(
            content(&mut host, second),
            second_baseline,
            "{kind:?} undo second"
        );
        assert_eq!(content(&mut host, first), first_edited);
        assert_dirty(&mut host, first, false);
        assert_dirty(&mut host, second, false);
        assert_eq!(host.close(first, false), CloseResult::Closed);
        let reopened = host
            .open(&first_path)
            .expect("reopen first while second remains open");
        assert_eq!(content(&mut host, reopened), first_edited);
        assert_eq!(content(&mut host, second), second_baseline);
        assert_dirty(&mut host, second, false);
    }
}

#[test]
fn an_actual_writer_failure_preserves_content_identity_and_dirty_state() {
    let directory = tempfile::tempdir().expect("test directory");
    for kind in KINDS {
        let mut host = Host::default();
        let id = host.new_document(kind).expect("editor");
        edit(&mut host, id, "Persisted original");
        let original = directory.path().join(format!("saved.{}", extension(kind)));
        save(&mut host, id, &original);
        let original_bytes = fs::read(&original).expect("original bytes");
        let original_title = host.workspace.tab(id).expect("tab").title.clone();
        edit(&mut host, id, "New unsaved content");
        let modified = content(&mut host, id);
        // Existing parent and correct suffix get past destination validation. A directory at
        // the destination forces a real filesystem writer failure in every editor backend.
        let blocked = directory
            .path()
            .join(format!("blocked.{}", extension(kind)));
        fs::create_dir(&blocked).expect("directory blocks destination");
        assert!(
            host.save_as(id, &blocked).is_err(),
            "{kind:?} cannot overwrite a directory"
        );
        assert!(blocked.is_dir());
        assert_eq!(
            host.path(id),
            Some(original.canonicalize().expect("original exists").as_path())
        );
        assert_eq!(host.workspace.tab(id).expect("tab").title, original_title);
        assert_eq!(content(&mut host, id), modified);
        assert_dirty(&mut host, id, true);
        assert_eq!(host.close(id, false), CloseResult::NeedsSave);
        assert_eq!(
            fs::read(&original).expect("original bytes remain"),
            original_bytes
        );
        save(&mut host, id, &original);
        assert_eq!(host.close(id, false), CloseResult::Closed);
        let reopened = host.open(&original).expect("reopen recovered save");
        assert_eq!(content(&mut host, reopened), modified);
    }
}

#[test]
fn save_as_cannot_overwrite_another_open_tab_even_through_a_symlink() {
    let directory = tempfile::tempdir().expect("test directory");
    for kind in KINDS {
        let mut host = Host::default();
        let source = host.new_document(kind).expect("source editor");
        let target = host.new_document(kind).expect("target editor");
        let source_path = directory.path().join(format!("source.{}", extension(kind)));
        let target_path = directory.path().join(format!("target.{}", extension(kind)));
        save(&mut host, source, &source_path);
        save(&mut host, target, &target_path);
        let source_bytes = fs::read(&source_path).expect("saved source");
        let target_bytes = fs::read(&target_path).expect("saved target");
        edit(&mut host, source, "Source pending edit");
        edit(&mut host, target, "Target pending edit");
        let source_content = content(&mut host, source);
        let target_content = content(&mut host, target);
        assert!(
            host.save_as(source, &target_path).is_err(),
            "another tab owns the destination"
        );
        #[cfg(unix)]
        {
            let alias = directory
                .path()
                .join(format!("target-alias.{}", extension(kind)));
            std::os::unix::fs::symlink(&target_path, &alias).expect("target symlink");
            assert!(
                host.save_as(source, &alias).is_err(),
                "aliases cannot bypass tab ownership"
            );
        }
        assert_eq!(content(&mut host, source), source_content);
        assert_eq!(content(&mut host, target), target_content);
        assert_dirty(&mut host, source, true);
        assert_dirty(&mut host, target, true);
        assert_eq!(
            host.path(source),
            Some(source_path.canonicalize().expect("source exists").as_path())
        );
        assert_eq!(
            host.path(target),
            Some(target_path.canonicalize().expect("target exists").as_path())
        );
        assert_eq!(
            fs::read(&source_path).expect("source still readable"),
            source_bytes
        );
        assert_eq!(
            fs::read(&target_path).expect("target still readable"),
            target_bytes
        );
        assert_eq!(host.documents.len(), 2);
    }
}

#[test]
fn new_code_files_take_the_language_and_extension_of_their_tile() {
    let mut host = Host::default();
    for (extension, language) in [("json", "JSON"), ("py", "Python"), ("sql", "SQL")] {
        let id = host.new_text_file(extension).unwrap();
        let tab = host.workspace.tab(id).unwrap();
        assert_eq!(tab.kind, DocumentKind::PlainText);
        assert!(
            tab.title.ends_with(&format!(".{extension}")),
            "{}",
            tab.title
        );
        match &host.documents[&id].editor {
            weave_office::editor::Editor::TextFile(app) => {
                assert_eq!(app.language_name(), language)
            }
            _ => unreachable!(),
        }
    }
    assert!(host.new_text_file("../x").is_err());
    for (_, entries) in weave_office::new_document::groups() {
        for entry in entries {
            match entry.extension {
                Some(extension) => assert!(host.new_text_file(extension).is_ok()),
                None => assert!(host.new_document(entry.kind).is_ok()),
            }
        }
    }
}
