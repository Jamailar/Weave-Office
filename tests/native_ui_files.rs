//! Opt-in, read-only verification of files saved through the packaged application's GUI.
//! These files are external acceptance evidence, not fixtures created by this test.
//!
//! Run with an explicitly chosen directory:
//! WEAVE_ACCEPTANCE_DIR=/path/to/acceptance cargo test --test native_ui_files -- --ignored
use serde_json::json;
use std::{fs, path::PathBuf};
use weave_office::{editor::Editor, host::Host};
use weave_workspace::DocumentKind;

#[test]
#[ignore = "requires actual GUI-saved files and an explicit WEAVE_ACCEPTANCE_DIR"]
fn fresh_host_reads_the_four_files_saved_through_the_native_ui() {
    let directory = PathBuf::from(std::env::var_os("WEAVE_ACCEPTANCE_DIR").filter(|value| !value.is_empty()).expect(
        "Set WEAVE_ACCEPTANCE_DIR explicitly to the directory containing the four GUI-saved weave-* files; this test never generates or overwrites them",
    ));
    assert!(
        directory.is_dir(),
        "WEAVE_ACCEPTANCE_DIR is not a directory: {}",
        directory.display()
    );
    let files = [
        ("weave-word.docx", DocumentKind::Text),
        ("weave-sheet.xlsx", DocumentKind::Spreadsheet),
        ("weave-slides.pptx", DocumentKind::Presentation),
        ("weave-pdf.pdf", DocumentKind::Pdf),
    ];
    // Capture the evidence before any reader starts. No saves, edits, or draft flushing below.
    let originals: Vec<_> = files
        .iter()
        .map(|(name, _)| {
            let path = directory.join(name);
            let bytes = fs::read(&path).unwrap_or_else(|error| {
                panic!(
                    "Missing or unreadable GUI evidence {}: {error}",
                    path.display()
                )
            });
            assert!(!bytes.is_empty(), "Empty GUI evidence: {}", path.display());
            let modified = fs::metadata(&path)
                .expect("evidence metadata")
                .modified()
                .expect("evidence modification time");
            (path, bytes, modified)
        })
        .collect();

    let mut host = Host::default();
    for ((name, expected_kind), (path, _, _)) in files.iter().zip(&originals) {
        let id = host
            .open(path)
            .unwrap_or_else(|error| panic!("Cannot reopen GUI-saved {name}: {error}"));
        assert_eq!(
            host.workspace.tab(id).expect("new host tab").kind,
            *expected_kind
        );
        assert!(
            !host.dirty(id),
            "opening saved {name} must not create unsaved work"
        );
        let editor = &mut host.documents.get_mut(&id).expect("fresh editor").editor;
        match editor {
            Editor::Word(app) => {
                let result = app
                    .hosted_command("document.text", json!({}))
                    .expect("read parsed DOCX text");
                let text = result["text"].as_str().expect("DOCX text string");
                assert_eq!(
                    text.trim_end_matches(['\r', '\n']).replace("\r\n", "\n"),
                    "Weave 统一工作区验收\n文字、表格、演示和 PDF 共享文件标签。\n这份内容用于验证保存后重开。",
                    "all three paragraphs must survive the GUI save"
                );
            }
            Editor::Sheet(app) => {
                for (cell, expected) in [
                    ("A1", "项目"),
                    ("B1", "数量"),
                    ("C1", "单价"),
                    ("D1", "金额"),
                    ("A2", "样例 A"),
                    ("A3", "样例 B"),
                ] {
                    let result = app
                        .hosted_command("cell.get", json!({"cell": cell}))
                        .expect("read XLSX label");
                    assert_eq!(result["value"], json!(expected), "XLSX {cell}");
                }
                for (cell, expected) in [("B2", 2.0), ("C2", 15.0), ("B3", 3.0), ("C3", 20.0)] {
                    let result = app
                        .hosted_command("cell.get", json!({"cell": cell}))
                        .expect("read XLSX number");
                    assert_eq!(result["value"].as_f64(), Some(expected), "XLSX {cell}");
                }
                for (cell, formula, expected) in [("D2", "=B2*C2", 30.0), ("D3", "=B3*C3", 60.0)] {
                    let result = app
                        .hosted_command("cell.get", json!({"cell": cell}))
                        .expect("read saved formula and value");
                    assert_eq!(
                        result["formula"],
                        json!(formula),
                        "XLSX {cell} must retain its formula"
                    );
                    assert_eq!(
                        result["value"].as_f64(),
                        Some(expected),
                        "XLSX {cell} computed value"
                    );
                    // Evaluate the formula read from disk against reopened cells. This query
                    // verifies the actual calculation independently of the stored cell value.
                    let evaluated = app
                        .hosted_command(
                            "formulas.evaluate",
                            json!({"cell": cell, "formula": result["formula"]}),
                        )
                        .expect("read-only formula evaluation");
                    assert_eq!(
                        evaluated.as_f64(),
                        Some(expected),
                        "XLSX {cell} fresh formula evaluation"
                    );
                }
            }
            Editor::Slides(app) => {
                let presentation = &app.session.active().expect("parsed presentation").doc;
                assert_eq!(
                    presentation.slides.len(),
                    1,
                    "GUI acceptance presentation has one slide"
                );
                assert_eq!(presentation.slides[0].title(), "Weave Office 统一办公");
            }
            Editor::Pdf(app) => {
                let (_, document_id) = app.active_ids().expect("reopened PDF view");
                let document = app.session.get(document_id).expect("parsed PDF");
                assert_eq!(document.info.pages.len(), 1);
                let notes: Vec<_> = document
                    .info
                    .annotations
                    .iter()
                    .filter(|annotation| annotation.subtype == "Text")
                    .collect();
                assert_eq!(
                    notes.len(),
                    1,
                    "one real sticky-note annotation, not only a UI draft"
                );
                let note = notes[0];
                assert_eq!(note.page, 0);
                assert_eq!(note.author.as_deref(), Some("Jam"));
                assert_eq!(note.contents.as_deref(), Some("Reviewed amount: 123.45"));
                assert!(note.rect[2] > note.rect[0] && note.rect[3] > note.rect[1]);
            }
        }
        host.sync_dirty();
        assert!(
            !host.dirty(id),
            "read-only inspection must leave {name} clean"
        );
        assert!(!host.workspace.tab(id).expect("tab").is_dirty());
    }
    assert_eq!(host.documents.len(), 4);
    assert_eq!(host.workspace.tabs().len(), 4);
    drop(host);

    for (path, bytes, modified) in originals {
        assert_eq!(
            fs::read(&path).expect("evidence remains readable"),
            bytes,
            "inspection changed {}",
            path.display()
        );
        assert_eq!(
            fs::metadata(&path)
                .expect("evidence metadata")
                .modified()
                .expect("modification time"),
            modified,
            "inspection rewrote {}",
            path.display()
        );
    }
}

#[test]
#[ignore = "requires the PDF body/image files saved through the native UI"]
fn fresh_host_reads_pdf_body_and_added_image_saved_through_the_native_ui() {
    let directory =
        PathBuf::from(std::env::var_os("WEAVE_ACCEPTANCE_DIR").expect("Set WEAVE_ACCEPTANCE_DIR"));
    let original = directory.join("weave-content-source.pdf");
    let edited = directory.join("weave-content-edited.pdf");
    let before: Vec<_> = [&original, &edited]
        .into_iter()
        .map(|path| {
            (
                path.clone(),
                fs::read(path).unwrap(),
                fs::metadata(path).unwrap().modified().unwrap(),
            )
        })
        .collect();
    let mut host = Host::default();
    let source = host
        .open(&original)
        .expect("read contributor-original source");
    let Editor::Pdf(source_pdf) = &host.documents[&source].editor else {
        panic!("PDF")
    };
    let (_, source_id) = source_pdf.active_ids().unwrap();
    let document = source_pdf.session.get(source_id).unwrap();
    assert_eq!(
        document
            .text_blocks(0)
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>(),
        ["Weave source invoice 123.45"]
    );
    assert_eq!(document.page_images(0).len(), 1);
    let source_image = document.page_image_file(0, 0).unwrap().1;
    let changed = host.open(&edited).expect("read GUI-edited PDF");
    let Editor::Pdf(pdf) = &host.documents[&changed].editor else {
        panic!("PDF")
    };
    let (_, id) = pdf.active_ids().unwrap();
    let document = pdf.session.get(id).unwrap();
    assert_eq!(document.info.pages.len(), 1);
    assert!(
        document.info.annotations.is_empty(),
        "edits must be original page content, not annotations"
    );
    assert_eq!(
        document
            .text_blocks(0)
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>(),
        ["Weave edited invoice 678.90"]
    );
    let images = document.page_images(0);
    assert_eq!(
        images.len(),
        2,
        "the original and newly added image must both persist"
    );
    assert!(
        images
            .iter()
            .all(|image| image.width == 3 && image.height == 2)
    );
    assert_eq!(
        document.page_image_file(0, 0).unwrap().1,
        source_image,
        "existing image remains intact"
    );
    assert_ne!(
        document.page_image_file(0, 1).unwrap().1,
        source_image,
        "added image has distinct pixels"
    );
    assert!(!host.dirty(source));
    assert!(!host.dirty(changed));
    drop(host);
    for (path, bytes, modified) in before {
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }
}

#[test]
#[ignore = "requires the image files saved through native chooser and WEAVE_ACCEPTANCE_DIR"]
fn fresh_host_reads_native_image_inserts_without_changing_evidence() {
    let directory = PathBuf::from(
        std::env::var_os("WEAVE_ACCEPTANCE_DIR")
            .filter(|value| !value.is_empty())
            .expect("Set WEAVE_ACCEPTANCE_DIR to the existing native image acceptance files"),
    );
    assert!(directory.is_dir(), "acceptance directory is missing");
    let names = [
        "weave-content-replacement.png",
        "weave-image-word.docx",
        "weave-image-sheet.xlsx",
        "weave-image-slides.pptx",
    ];
    let before: Vec<_> = names
        .iter()
        .map(|name| {
            let path = directory.join(name);
            let bytes = fs::read(&path)
                .unwrap_or_else(|error| panic!("Cannot read {}: {error}", path.display()));
            let modified = fs::metadata(&path).unwrap().modified().unwrap();
            (path, bytes, modified)
        })
        .collect();
    let original_image = &before[0].1;
    assert_eq!(
        original_image.len(),
        78,
        "expected the native chooser's 3×2 PNG source"
    );

    let mut host = Host::default();
    for (path, _, _) in before.iter().skip(1) {
        let id = host
            .open(path)
            .unwrap_or_else(|error| panic!("Cannot reopen {}: {error}", path.display()));
        assert!(!host.dirty(id), "{} opened dirty", path.display());
        let (matches, total) = match &host.documents[&id].editor {
            Editor::Word(app) => (
                app.session
                    .doc
                    .media
                    .values()
                    .filter(|bytes| bytes.as_slice() == original_image.as_slice())
                    .count(),
                app.session.doc.media.len(),
            ),
            Editor::Sheet(app) => {
                let sheet = app
                    .session
                    .active()
                    .expect("workbook")
                    .wb
                    .active()
                    .expect("sheet");
                (
                    sheet
                        .images
                        .iter()
                        .filter(|image| image.data.as_slice() == original_image.as_slice())
                        .count()
                        + sheet
                            .cell_pictures
                            .values()
                            .filter(|picture| picture.data.as_slice() == original_image.as_slice())
                            .count(),
                    sheet.images.len() + sheet.cell_pictures.len(),
                )
            }
            Editor::Slides(app) => {
                let media = &app.session.active().expect("presentation").doc.media;
                (
                    media
                        .iter()
                        .filter(|item| item.data.as_slice() == original_image.as_slice())
                        .count(),
                    media.len(),
                )
            }
            Editor::Pdf(_) => panic!("expected an Office document"),
        };
        assert_eq!(
            total,
            1,
            "{} must contain exactly one image",
            path.display()
        );
        assert_eq!(
            matches,
            1,
            "{} must contain the selected PNG bytes once",
            path.display()
        );
        assert!(
            !host.dirty(id),
            "reading {} changed its document",
            path.display()
        );
    }
    drop(host);
    for (path, bytes, modified) in before {
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "inspection changed {}",
            path.display()
        );
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            modified,
            "inspection rewrote {}",
            path.display()
        );
    }
}

#[test]
#[ignore = "requires the PDF image replacement saved through native UI and WEAVE_ACCEPTANCE_DIR"]
fn fresh_host_reads_replaced_and_transformed_existing_pdf_image() {
    let directory = PathBuf::from(
        std::env::var_os("WEAVE_ACCEPTANCE_DIR")
            .filter(|value| !value.is_empty())
            .expect("Set WEAVE_ACCEPTANCE_DIR to the existing native PDF image acceptance files"),
    );
    assert!(directory.is_dir(), "acceptance directory is missing");
    let paths = [
        directory.join("weave-content-source.pdf"),
        directory.join("weave-image-edited.pdf"),
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|path| {
            let bytes = fs::read(path)
                .unwrap_or_else(|error| panic!("Cannot read {}: {error}", path.display()));
            let modified = fs::metadata(path)
                .expect("PDF evidence metadata")
                .modified()
                .expect("PDF evidence modification time");
            (path.clone(), bytes, modified)
        })
        .collect();

    let mut host = Host::default();
    let mut source_pixels = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let id = host
            .open(path)
            .unwrap_or_else(|error| panic!("Cannot reopen {}: {error}", path.display()));
        assert!(!host.dirty(id), "{} opened dirty", path.display());
        let Editor::Pdf(app) = &host.documents[&id].editor else {
            panic!("{} must open in the PDF editor", path.display());
        };
        let (_, document_id) = app.active_ids().expect("one active PDF view");
        let document = app.session.get(document_id).expect("parsed PDF");
        assert_eq!(
            document.info.pages.len(),
            1,
            "{} has one page",
            path.display()
        );
        assert!(
            document.info.annotations.is_empty(),
            "{} has no annotations",
            path.display()
        );
        assert_eq!(
            document
                .text_blocks(0)
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["Weave source invoice 123.45"],
            "the existing body must remain unchanged in {}",
            path.display(),
        );
        let images = document.page_images(0);
        assert_eq!(
            images.len(),
            1,
            "one actually drawn image in {}",
            path.display()
        );
        let image = &images[0];
        assert!(!image.is_form, "the drawn artwork is a raster image");
        assert_eq!((image.width, image.height), (3, 2));
        let (_, _, pixels) = document
            .page_image_rgba(0, 0)
            .expect("decode drawn image pixels");
        if index == 0 {
            assert_eq!(image.matrix, [120.0, 0.0, 0.0, 80.0, 36.0, 80.0]);
            source_pixels = pixels;
        } else {
            assert_eq!(
                image.matrix,
                [0.0, -120.0, 80.0, 0.0, 66.0, 170.0],
                "saved rotation and two 10pt nudges"
            );
            assert_eq!(
                pixels,
                [
                    45, 180, 100, 255, 45, 180, 100, 255, 250, 200, 30, 255, 250, 200, 30, 255, 45,
                    180, 100, 255, 45, 180, 100, 255,
                ],
                "saved drawn image uses the selected replacement pixels"
            );
            assert_ne!(pixels, source_pixels, "the original image was replaced");
        }
        assert!(!host.dirty(id), "read-only PDF inspection left a dirty tab");
    }
    drop(host);

    for (path, bytes, modified) in before {
        assert_eq!(
            fs::read(&path).expect("PDF evidence remains readable"),
            bytes,
            "inspection changed {}",
            path.display()
        );
        assert_eq!(
            fs::metadata(&path)
                .expect("PDF evidence metadata")
                .modified()
                .expect("PDF evidence modification time"),
            modified,
            "inspection rewrote {}",
            path.display()
        );
    }
}

#[test]
#[ignore = "requires the PDF page edits saved through native UI and WEAVE_ACCEPTANCE_DIR"]
fn fresh_host_reads_rotated_inserted_and_undo_restored_pdf_pages() {
    let directory = PathBuf::from(
        std::env::var_os("WEAVE_ACCEPTANCE_DIR")
            .filter(|value| !value.is_empty())
            .expect("Set WEAVE_ACCEPTANCE_DIR to the existing native PDF page acceptance files"),
    );
    assert!(directory.is_dir(), "acceptance directory is missing");
    let paths = [
        directory.join("weave-content-source.pdf"),
        directory.join("weave-image-edited.pdf"),
        directory.join("weave-pages-edited.pdf"),
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|path| {
            let bytes = fs::read(path)
                .unwrap_or_else(|error| panic!("Cannot read {}: {error}", path.display()));
            let modified = fs::metadata(path)
                .expect("PDF evidence metadata")
                .modified()
                .expect("PDF evidence modification time");
            (path.clone(), bytes, modified)
        })
        .collect();

    let mut host = Host::default();
    let mut reference_pixels = Vec::new();
    for path in paths.iter().take(2) {
        let id = host
            .open(path)
            .unwrap_or_else(|error| panic!("Cannot reopen {}: {error}", path.display()));
        assert!(!host.dirty(id), "{} opened dirty", path.display());
        let Editor::Pdf(app) = &host.documents[&id].editor else {
            panic!("{} must open in the PDF editor", path.display());
        };
        let (_, document_id) = app.active_ids().expect("one active PDF view");
        let document = app.session.get(document_id).expect("parsed reference PDF");
        let images = document.page_images(0);
        assert_eq!(images.len(), 1, "one drawn reference image");
        reference_pixels.push(
            document
                .page_image_rgba(0, 0)
                .expect("decode reference image")
                .2,
        );
    }
    assert_ne!(
        reference_pixels[0], reference_pixels[1],
        "the two reference images differ"
    );

    let path = &paths[2];
    let id = host
        .open(path)
        .unwrap_or_else(|error| panic!("Cannot reopen {}: {error}", path.display()));
    assert!(!host.dirty(id), "page-edited PDF opened dirty");
    let Editor::Pdf(app) = &host.documents[&id].editor else {
        panic!("page-edited file must open in the PDF editor");
    };
    let (_, document_id) = app.active_ids().expect("one active PDF view");
    let document = app
        .session
        .get(document_id)
        .expect("parsed page-edited PDF");
    assert_eq!(
        document.info.pages.len(),
        2,
        "Undo restored the deleted second page"
    );
    assert_eq!(
        document
            .info
            .pages
            .iter()
            .map(|page| page.rotation)
            .collect::<Vec<_>>(),
        [90, 0]
    );
    assert!(
        document.info.annotations.is_empty(),
        "both pages contain real page content, not annotations"
    );
    for (page, expected_matrix) in [
        [120.0, 0.0, 0.0, 80.0, 36.0, 80.0],
        [0.0, -120.0, 80.0, 0.0, 66.0, 170.0],
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            document
                .text_blocks(page)
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["Weave source invoice 123.45"],
            "page {} retains the source body",
            page + 1,
        );
        let images = document.page_images(page);
        assert_eq!(images.len(), 1, "page {} draws exactly one image", page + 1);
        assert!(!images[0].is_form);
        assert_eq!((images[0].width, images[0].height), (3, 2));
        assert_eq!(
            images[0].matrix,
            expected_matrix,
            "page {} image placement",
            page + 1
        );
        assert_eq!(
            document
                .page_image_rgba(page, 0)
                .expect("decode saved page image")
                .2,
            reference_pixels[page],
            "page {} contains the matching reference image pixels",
            page + 1,
        );
    }
    assert!(
        !host.dirty(id),
        "read-only inspection left the page-edited PDF dirty"
    );
    drop(host);

    for (path, bytes, modified) in before {
        assert_eq!(
            fs::read(&path).expect("PDF evidence remains readable"),
            bytes,
            "inspection changed {}",
            path.display()
        );
        assert_eq!(
            fs::metadata(&path)
                .expect("PDF evidence metadata")
                .modified()
                .expect("PDF evidence modification time"),
            modified,
            "inspection rewrote {}",
            path.display()
        );
    }
}
