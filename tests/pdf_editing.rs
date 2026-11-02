//! PDF content acceptance through the actual hosted editor and its native codecs.
//! Fixtures are contributor-original PDF syntax, generated only in a temporary directory.
//! No private document, native window, or system font is required. The Chinese test requires
//! approved craft-fonts at build time and verifies its font is embedded in the saved PDF.
use pdfcraft_ui_egui::{PdfCraftApp, fill_sign};
use serde_json::json;
use std::fs;
use weave_office::{editor::Editor, host::Host};
use weave_workspace::{CloseResult, TabId};

const SOURCE_TEXT: &str = "Weave source invoice 123.45";
const NOTE_TEXT: &str = "Reviewed amount: 123.45";
const AUTHOR: &str = "Weave acceptance";

/// A real, nonempty, one-page PDF using a standard-14 font name and no font program.
fn source_pdf() -> Vec<u8> {
    let stream = format!("BT /F1 16 Tf 36 240 Td ({SOURCE_TEXT}) Tj ET\n");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

fn pdf_app(host: &mut Host, id: TabId) -> &mut PdfCraftApp {
    match &mut host.documents.get_mut(&id).expect("PDF tab exists").editor {
        Editor::Pdf(app) => app,
        _ => panic!("PDF fixture must open in PDF editor"),
    }
}

fn assert_content(app: &PdfCraftApp, expected_note: Option<&str>) {
    let (_, id) = app.active_ids().expect("active PDF view");
    let document = app.session.get(id).expect("real PDF document");
    assert_eq!(document.info.pages.len(), 1);
    assert_eq!(
        document.info.pages[0].rotation, 0,
        "this is not a rotation test"
    );
    assert_eq!(
        document
            .text_blocks(0)
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>(),
        [SOURCE_TEXT],
        "adding a comment must preserve the original page text"
    );
    let notes = &document.info.annotations;
    if let Some(text) = expected_note {
        assert_eq!(notes.len(), 1, "exactly one persisted annotation");
        let note = &notes[0];
        assert_eq!(note.page, 0);
        assert_eq!(note.subtype, "FreeText");
        assert_eq!(note.contents.as_deref(), Some(text));
        assert_eq!(note.author.as_deref(), Some(AUTHOR));
        assert!(note.fill_sign);
        assert!(note.rect[2] > note.rect[0] && note.rect[3] > note.rect[1]);
        assert!(
            document.edit_notices.is_empty(),
            "annotation appearance must not substitute missing glyphs: {:?}",
            document.edit_notices
        );
    } else {
        assert!(notes.is_empty(), "undo must remove the annotation itself");
    }
}

/// Render only the persisted annotation appearance, synchronously and without system fonts.
/// The transparent page contains neither source text nor UI chrome, so nonzero alpha is
/// evidence from the PDF appearance itself, not from its /Contents metadata or a text widget.
fn annotation_pixels(app: &PdfCraftApp) -> (u32, u32, Vec<u8>) {
    let (_, id) = app.active_ids().expect("active PDF");
    assert!(
        !app.session
            .render_config(id)
            .expect("render configuration")
            .system_fonts,
        "acceptance rendering must never opt in to installed system fonts"
    );
    let doc = app.session.get(id).expect("PDF document");
    let index = doc
        .info
        .annotations
        .first()
        .expect("annotation to render")
        .index;
    let layer = doc
        .comment_layer(0, &[index])
        .expect("isolate annotation appearance")
        .expect("drawable appearance");
    let raster = layer
        .render(2.0)
        .expect("render persisted annotation without system fonts");
    assert!(
        raster.error.is_none(),
        "annotation raster error: {:?}",
        raster.error
    );
    assert!(
        raster.warnings.is_empty(),
        "partial annotation raster: {:?}",
        raster.warnings
    );
    assert_eq!(
        raster.rgba.len(),
        raster.width as usize * raster.height as usize * 4
    );
    (raster.width, raster.height, raster.rgba.to_vec())
}

#[test]
fn chinese_pdf_annotation_persists_real_glyphs_without_system_fonts() {
    const CHINESE: &str = "金额已核对，共计123.45元";
    let directory = tempfile::tempdir().expect("fixture directory");
    let original = directory.path().join("chinese-source.pdf");
    let saved = directory.path().join("chinese-reviewed.pdf");
    let source = source_pdf();
    fs::write(&original, &source).expect("write contributor-original PDF");
    let mut host = Host::default();
    let id = host.open(&original).expect("open source PDF");
    let app = pdf_app(&mut host, id);
    assert!(
        app.apply_edit(fill_sign::typed(0, [36.0, 190.0], CHINESE, 12.0, 0, AUTHOR)),
        "Chinese FreeText requires a build with approved craft-fonts (CRAFT_FONTS_DIR and CRAFT_FONTS_REQUIRED=1) and a Unicode appearance writer; edit error: {:?}",
        app.toast
    );
    let (_, document_id) = app.active_ids().expect("PDF id");
    let notices = app
        .session
        .get(document_id)
        .expect("edited PDF")
        .edit_notices
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    app.hosted_command("edit.undo", json!({}))
        .expect("undo Chinese annotation");
    assert_content(app, None);
    app.hosted_command("edit.redo", json!({}))
        .expect("redo Chinese annotation");
    let (_, document_id) = app.active_ids().expect("PDF id after redo");
    assert_eq!(
        app.session
            .get(document_id)
            .expect("redone PDF")
            .info
            .annotations[0]
            .contents
            .as_deref(),
        Some(CHINESE)
    );
    assert_dirty(&mut host, id, true);
    host.save_as(id, &saved)
        .expect("persist Chinese annotation");
    assert_dirty(&mut host, id, false);
    assert_eq!(fs::read(&original).expect("original bytes"), source);
    drop(host);

    let mut reopened_host = Host::default();
    let reopened = reopened_host
        .open(&saved)
        .expect("parse Chinese annotation in a new session");
    let app = pdf_app(&mut reopened_host, reopened);
    assert_content(app, Some(CHINESE));
    let actual = annotation_pixels(app);
    assert!(actual.0 > 0 && actual.1 > 0);
    assert!(
        actual
            .2
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 32)
            .count()
            > 200,
        "the saved annotation must contain visible glyph pixels"
    );

    // A file may preserve Chinese /Contents while drawing only '?' in /AP. Render that
    // exact replacement string separately; metadata or placeholder-only support must fail.
    let placeholders: String = CHINESE
        .chars()
        .map(|ch| if ch.is_ascii() { ch } else { '?' })
        .collect();
    let mut fallback_host = Host::default();
    let fallback_id = fallback_host
        .open(&original)
        .expect("independent placeholder control");
    let fallback_app = pdf_app(&mut fallback_host, fallback_id);
    assert!(fallback_app.apply_edit(fill_sign::typed(
        0,
        [36.0, 190.0],
        &placeholders,
        12.0,
        0,
        AUTHOR
    )));
    let fallback = annotation_pixels(fallback_app);
    assert_eq!((actual.0, actual.1), (fallback.0, fallback.1));
    let changed_pixels = actual
        .2
        .chunks_exact(4)
        .zip(fallback.2.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        notices.is_empty(),
        "Chinese glyphs were substituted: {notices:?}. Build with approved craft-fonts via CRAFT_FONTS_DIR and CRAFT_FONTS_REQUIRED=1; a WinAnsi-only FreeText appearance is not Chinese support. Raster pixels different from '?' control: {changed_pixels}"
    );
    assert!(
        changed_pixels > 100,
        "saved Chinese appearance is indistinguishable from missing-glyph '?' replacements ({changed_pixels} pixels)"
    );
    assert_dirty(&mut reopened_host, reopened, false);
}

fn assert_dirty(host: &mut Host, id: TabId, dirty: bool) {
    host.sync_dirty();
    assert_eq!(host.dirty(id), dirty);
    assert_eq!(host.workspace.tab(id).expect("PDF tab").is_dirty(), dirty);
}

#[test]
fn chinese_pdf_tab_keeps_its_contents_and_renders_like_a_space_after_reopen() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let original = directory.path().join("tab-source.pdf");
    let source = source_pdf();
    fs::write(&original, &source).expect("original PDF");
    let mut pixels = Vec::new();
    for (index, text) in ["金额\t123", "金额 123"].into_iter().enumerate() {
        let saved = directory.path().join(format!("tab-{index}.pdf"));
        let mut host = Host::default();
        let id = host.open(&original).expect("fresh PDF session");
        let app = pdf_app(&mut host, id);
        assert!(app.apply_edit(fill_sign::typed(0, [36.0, 190.0], text, 12.0, 0, AUTHOR)));
        assert_content(app, Some(text));
        host.save_as(id, &saved)
            .expect("persist TAB or space appearance");
        drop(host);
        let mut reopened = Host::default();
        let id = reopened.open(&saved).expect("reopen saved appearance");
        let app = pdf_app(&mut reopened, id);
        assert_content(app, Some(text));
        pixels.push(annotation_pixels(app));
    }
    assert!(
        pixels[0]
            .2
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 32)
            .count()
            > 200
    );
    assert_eq!(
        pixels[0], pixels[1],
        "TAB normalizes only for appearance; it must render exactly as a space"
    );
    assert_eq!(fs::read(original).expect("original unchanged"), source);
}

#[test]
fn pdf_text_annotation_undo_redo_save_and_reopen_preserve_source_content() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let original = directory.path().join("invoice.pdf");
    let edited = directory.path().join("invoice-reviewed.pdf");
    let source = source_pdf();
    fs::write(&original, &source).expect("write original fixture");
    let mut host = Host::default();
    let id = host
        .open(&original)
        .expect("open nonempty PDF through host");
    assert_content(pdf_app(&mut host, id), None);
    assert_dirty(&mut host, id, false);

    // Same Edit used by Fill & Sign typing; this reaches the actual engine history and writer.
    let app = pdf_app(&mut host, id);
    assert!(app.apply_edit(fill_sign::typed(
        0,
        [36.0, 190.0],
        NOTE_TEXT,
        12.0,
        0,
        AUTHOR
    )));
    assert_content(app, Some(NOTE_TEXT));
    assert_dirty(&mut host, id, true);
    assert_eq!(host.close(id, false), CloseResult::NeedsSave);

    pdf_app(&mut host, id)
        .hosted_command("edit.undo", json!({}))
        .expect("undo annotation");
    assert_content(pdf_app(&mut host, id), None);
    assert_dirty(&mut host, id, false);
    pdf_app(&mut host, id)
        .hosted_command("edit.redo", json!({}))
        .expect("redo annotation");
    assert_content(pdf_app(&mut host, id), Some(NOTE_TEXT));
    assert_dirty(&mut host, id, true);

    host.save_as(id, &edited)
        .expect("save annotated PDF through host");
    assert_dirty(&mut host, id, false);
    assert_ne!(fs::read(&edited).expect("saved bytes"), source);
    assert_eq!(
        fs::read(&original).expect("source bytes"),
        source,
        "save-as preserves original"
    );
    assert_eq!(host.close(id, false), CloseResult::Closed);
    drop(host);

    // A new host/session must parse disk bytes, with no access to the former editor's history.
    let mut reopened_host = Host::default();
    let reopened = reopened_host
        .open(&edited)
        .expect("reopen persisted annotation");
    assert_content(pdf_app(&mut reopened_host, reopened), Some(NOTE_TEXT));
    assert_dirty(&mut reopened_host, reopened, false);
}

#[test]
fn pdf_save_commits_an_unfinished_typewriter_draft_before_writing() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("typed-draft.pdf");
    fs::write(&path, source_pdf()).expect("write original fixture");
    let mut host = Host::default();
    let id = host.open(&path).expect("open source");
    let app = pdf_app(&mut host, id);
    app.comment_prefs.author = AUTHOR.into();
    let (view, _) = app.active_ids().expect("active view");
    // This is the actual page input's draft. Saving must flush it without a click-away first.
    app.views[view].fill_text = Some(fill_sign::TypeBox {
        page: 0,
        at: [36.0, 190.0],
        text: NOTE_TEXT.into(),
        size: 12.0,
        color: [0.0; 3],
        focus: false,
    });
    assert_content(app, None);
    assert_dirty(&mut host, id, true);
    host.save_as(id, &path)
        .expect("save flushes the pending text draft");
    assert_content(pdf_app(&mut host, id), Some(NOTE_TEXT));
    assert!(pdf_app(&mut host, id).views[view].fill_text.is_none());
    assert_dirty(&mut host, id, false);
    assert_eq!(host.close(id, false), CloseResult::Closed);
    let reopened = host.open(&path).expect("reopen committed typewriter text");
    assert_content(pdf_app(&mut host, reopened), Some(NOTE_TEXT));
    assert_dirty(&mut host, reopened, false);
}

#[test]
fn unfinished_comments_survive_tab_switches_and_block_saving_without_losing_the_draft() {
    use pdfcraft_ui_egui::comments::{Composer, ComposerKind};

    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("comment-draft.pdf");
    let original = source_pdf();
    fs::write(&path, &original).expect("source fixture");
    let mut host = Host::default();
    let id = host.open(&path).expect("source PDF");
    let other = host
        .new_document(weave_workspace::DocumentKind::Text)
        .expect("other host tab");
    assert!(host.workspace.activate(id));
    let ctx = eframe::egui::Context::default();
    // Every user-facing comment input must count as unsaved, including an edit to empty text.
    for kind in 0..5 {
        let app = pdf_app(&mut host, id);
        let (index, _) = app.active_ids().expect("view");
        let comments = &mut app.views[index].comments;
        match kind {
            0 => {
                comments.composer = Some(Composer {
                    page: 0,
                    at: [36.0, 190.0],
                    kind: ComposerKind::Note,
                    text: "批注草稿 must survive".into(),
                    focus: false,
                })
            }
            1 => comments.reply = "Pending reply".into(),
            2 => comments.add_box = "Pending comment".into(),
            3 => comments.editing = Some((0, 0, String::new())),
            _ => {
                comments.composer = Some(Composer {
                    page: 0,
                    at: [36.0, 190.0],
                    kind: ComposerKind::Edit(0),
                    text: String::new(),
                    focus: false,
                })
            }
        }
        assert!(
            app.hosted_dirty(),
            "the comment alone must count as unsaved"
        );
        // A concurrent typewriter draft must not be committed while doing so could clear
        // an inline comment being edited to empty text via the engine's comment_added hook.
        app.views[index].fill_text = Some(fill_sign::TypeBox {
            page: 0,
            at: [36.0, 190.0],
            text: NOTE_TEXT.into(),
            size: 12.0,
            color: [0.0; 3],
            focus: false,
        });
        let before = app.views[index].comments.clone();
        let typewriter_before = app.views[index].fill_text.clone();
        assert_dirty(&mut host, id, true);
        assert_eq!(host.close(id, false), CloseResult::NeedsSave);
        let error = host
            .save_as(id, &path)
            .expect_err("unposted comment must not appear saved");
        assert!(
            error.contains("Post or cancel"),
            "actionable error: {error}"
        );
        let app = pdf_app(&mut host, id);
        app.hosted_deactivate(&ctx)
            .expect("unposted comments must allow switching tabs");
        assert!(host.workspace.activate(other));
        assert_dirty(&mut host, id, true);
        assert!(host.workspace.activate(id));
        let app = pdf_app(&mut host, id);
        let after = &app.views[index].comments;
        assert_eq!(after.composer, before.composer);
        assert_eq!(after.reply, before.reply);
        assert_eq!(after.add_box, before.add_box);
        assert_eq!(after.editing, before.editing);
        assert_eq!(app.views[index].fill_text, typewriter_before);
        assert_content(app, None);
        assert_eq!(fs::read(&path).expect("source remains intact"), original);
        assert_dirty(&mut host, id, true);
        // Explicit user cancellation clears just this draft, allowing the next scenario.
        let comments = &mut pdf_app(&mut host, id).views[index].comments;
        comments.composer = None;
        comments.reply.clear();
        comments.add_box.clear();
        comments.editing = None;
        pdf_app(&mut host, id).views[index].fill_text = None;
        assert_dirty(&mut host, id, false);
    }
}

#[test]
fn hosted_pdf_cannot_hide_another_document_inside_its_host_tab() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("single-document.pdf");
    fs::write(&path, source_pdf()).expect("source fixture");
    let mut host = Host::default();
    let id = host.open(&path).expect("source PDF");
    let app = pdf_app(&mut host, id);
    assert!(app.apply_edit(fill_sign::typed(
        0,
        [36.0, 190.0],
        NOTE_TEXT,
        12.0,
        0,
        AUTHOR
    )));
    let active = app.active_ids();
    app.open_bytes("hidden.pdf", None, source_pdf())
        .expect("queue a host-owned file");
    assert_eq!(app.views.len(), 1);
    assert_eq!(app.session.docs().len(), 1);
    assert_eq!(app.active_ids(), active);
    // These used to open a second view without allocating a host tab.
    app.open_revision(1);
    app.view_signed_version(source_pdf().len(), 1);
    app.summarize_comments();
    app.open_combine_tab();
    app.close_combine_tab();
    app.extract_draft.separate = false;
    app.extract_draft.delete = true;
    app.extract_selection();
    let generated = app.take_host_documents();
    assert!(
        generated
            .iter()
            .any(|document| document.name == "hidden.pdf")
    );
    for document in generated {
        assert!(
            app.hosted_document_result(document.id, Err("Cancelled by the test host".into()))
                .is_err()
        );
    }
    assert_eq!(app.views.len(), 1);
    assert_eq!(app.session.docs().len(), 1);
    assert_eq!(app.active_ids(), active);
    assert_content(app, Some(NOTE_TEXT));
    app.set_full_screen(&eframe::egui::Context::default(), true);
    assert!(
        !app.full_screen,
        "embedded PDF must not take over the host window"
    );
    assert_eq!(
        app.take_host_commands(),
        [("view.full_screen".into(), json!({"enabled": true}))]
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        let another = directory.path().join("another.pdf");
        assert!(matches!(
            app.open_path(another.to_str().expect("UTF-8 path")),
            pdfcraft_ui_egui::OpenOutcome::Pending
        ));
        assert_eq!(app.views.len(), 1);
        assert_eq!(app.active_ids(), active);
        assert_eq!(
            app.take_host_commands(),
            [(
                "file.open".into(),
                json!({"path": another.to_str().unwrap()})
            )]
        );
    }
    assert_dirty(&mut host, id, true);
}
