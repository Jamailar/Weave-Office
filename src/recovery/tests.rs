use super::*;
use weave_workspace::Workspace;

fn tabs(n: usize) -> Vec<TabId> {
    let mut workspace = Workspace::default();
    (0..n)
        .map(|i| {
            workspace
                .add(DocumentKind::Text, format!("t{i}"), None)
                .unwrap()
        })
        .collect()
}

fn bytes(content: &'static [u8]) -> Snapshot {
    Box::new(move || Ok(content.to_vec()))
}

#[test]
fn a_crashed_run_s_copies_are_found_by_the_next_run_and_a_clean_exit_leaves_none() {
    let root = tempfile::tempdir().unwrap();
    let ids = tabs(2);
    let now = Instant::now();
    let mut first = Recovery::open(root.path()).unwrap();
    first.copy(
        ids[0],
        DocumentKind::Text,
        "报告.docx",
        Some(Path::new("/docs/报告.docx")),
        bytes(b"one"),
        now,
    );
    first.copy(
        ids[1],
        DocumentKind::Spreadsheet,
        "表格1",
        None,
        bytes(b"two"),
        now,
    );
    first.flush();
    // While the first run is alive, another instance must not offer or delete its copies.
    let second = Recovery::open(root.path()).unwrap();
    assert!(second.found().is_empty());
    drop(second);
    first.crash();
    let mut next = Recovery::open(root.path()).unwrap();
    let mut found: Vec<_> = next
        .found()
        .iter()
        .map(|f| (f.manifest.title.clone(), std::fs::read(&f.file).unwrap()))
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            ("报告.docx".to_string(), b"one".to_vec()),
            ("表格1".to_string(), b"two".to_vec())
        ]
    );
    let docx = next
        .found()
        .iter()
        .find(|f| f.manifest.title == "报告.docx")
        .unwrap()
        .clone();
    assert_eq!(docx.manifest.document_kind(), Some(DocumentKind::Text));
    assert_eq!(
        docx.manifest.origin.as_deref(),
        Some(Path::new("/docs/报告.docx"))
    );
    assert_eq!(docx.file.extension().unwrap(), "docx");
    next.clear_found();
    let dir = next.dir().to_path_buf();
    drop(next);
    assert!(!dir.exists());
    // Only this run's folders ever existed; all are gone.
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn a_saved_or_closed_document_s_copy_is_removed() {
    let root = tempfile::tempdir().unwrap();
    let ids = tabs(2);
    let now = Instant::now();
    let mut run = Recovery::open(root.path()).unwrap();
    run.copy(ids[0], DocumentKind::Text, "a", None, bytes(b"a"), now);
    run.copy(ids[1], DocumentKind::Text, "b", None, bytes(b"b"), now);
    // Saved: no longer dirty.
    assert_eq!(run.due(ids[0], false, now, now), Err(None));
    // Closed: no longer open.
    run.retain(&[]);
    run.flush();
    let left: Vec<_> = std::fs::read_dir(run.dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(left, [std::ffi::OsString::from(LOCK)]);
}

#[test]
fn copies_wait_for_a_pause_and_an_edit_and_are_spaced_out() {
    let root = tempfile::tempdir().unwrap();
    let id = tabs(1)[0];
    let mut run = Recovery::open(root.path()).unwrap();
    let start = Instant::now();
    // Never copied: due as soon as the person pauses.
    assert_eq!(run.due(id, true, start, start), Err(Some(IDLE)));
    let later = start + IDLE;
    assert_eq!(run.due(id, true, later, start), Ok(()));
    run.copy(id, DocumentKind::Text, "a", None, bytes(b"a"), later);
    // Not edited since: nothing to do, however long.
    assert_eq!(run.due(id, true, later + INTERVAL * 3, start), Err(None));
    // Edited: waits out the interval since the last copy.
    run.touch(id);
    assert_eq!(
        run.due(id, true, later + IDLE, later),
        Err(Some(INTERVAL - IDLE))
    );
    assert_eq!(run.due(id, true, later + INTERVAL, later), Ok(()));
    // Clean documents are never copied.
    assert_eq!(run.due(id, false, later + INTERVAL, later), Err(None));
}

#[test]
fn an_unchanged_copy_is_not_written_again_and_failures_are_reported() {
    let root = tempfile::tempdir().unwrap();
    let id = tabs(1)[0];
    let now = Instant::now();
    let mut run = Recovery::open(root.path()).unwrap();
    run.copy(id, DocumentKind::Pdf, "a.pdf", None, bytes(b"same"), now);
    run.copy(
        id,
        DocumentKind::Pdf,
        "a.pdf",
        None,
        Box::new(|| Err("boom".into())),
        now,
    );
    // flush() restarts the worker, which forgets what it wrote; check before it.
    std::thread::sleep(Duration::from_millis(200));
    let manifest = run.dir().join(format!("{}.json", id.get()));
    let written = std::fs::metadata(&manifest).unwrap().modified().unwrap();
    run.copy(id, DocumentKind::Pdf, "a.pdf", None, bytes(b"same"), now);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        std::fs::metadata(&manifest).unwrap().modified().unwrap(),
        written
    );
    assert_eq!(run.take_errors(), ["a.pdf: boom"]);
}

#[test]
fn a_folder_without_a_complete_copy_offers_nothing() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("run-1-1");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("7.json"),
        br#"{"kind":"text","title":"x","origin":null,"written_ms":1}"#,
    )
    .unwrap();
    std::fs::write(dir.join("8.json"), b"not json").unwrap();
    let mut run = Recovery::open(root.path()).unwrap();
    assert!(run.found().is_empty());
    run.clear_found();
    assert!(!dir.exists());
}
