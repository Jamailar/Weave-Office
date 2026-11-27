//! A new document can be edited in the running app: clicks reach the editing surface and the
//! ribbon commands, and typing lands in the document. These drive the whole `eframe::App`
//! (raw input hook, logic and ui, with the host's interface fonts), as the window does.
use super::tests::test_app;
use super::*;

const SHEET_BOLD: &str = if cfg!(target_os = "macos") {
    "粗体 (⌘B)"
} else {
    "粗体 (Ctrl+B)"
};

struct Running {
    app: WeaveApp,
    ctx: egui::Context,
    frame: eframe::Frame,
    output: egui::FullOutput,
    time: f64,
    texture_bytes: usize,
}

impl Running {
    fn new(kind: DocumentKind) -> (Self, TabId) {
        Self::after(&[], kind)
    }

    /// Open `before` first, then make `kind` from the running app, as the New button does.
    fn after(before: &[DocumentKind], kind: DocumentKind) -> (Self, TabId) {
        let mut host = Host::default();
        for &earlier in before {
            host.new_document(earlier).expect("earlier document");
        }
        let mut app = test_app(host);
        app.mounted = None;
        app.ui_font = Some(crate::ui_fonts::UiFonts::load());
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut running = Self {
            app,
            ctx,
            frame: eframe::Frame::_new_kittest(),
            output: egui::FullOutput::default(),
            time: 0.0,
            texture_bytes: 0,
        };
        for _ in 0..20 {
            running.pass(Vec::new());
        }
        running.app.actions.push_back(Action::New(kind));
        // Fonts, the first layout, thumbnails: settle before the first click.
        for _ in 0..40 {
            running.pass(Vec::new());
        }
        let id = running
            .app
            .host
            .workspace
            .active()
            .expect("the new document is open");
        assert_eq!(running.app.mounted, Some(id));
        (running, id)
    }

    /// One window frame, as eframe runs it.
    fn pass(&mut self, events: Vec<egui::Event>) -> usize {
        self.pass_at_rate(events, 30.0)
    }

    fn pass_at_rate(&mut self, events: Vec<egui::Event>, hz: f64) -> usize {
        self.time += 1.0 / hz;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 900.0),
            )),
            time: Some(self.time),
            focused: true,
            events,
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut self.app, &self.ctx, &mut input);
        let (app, frame) = (&mut self.app, &mut self.frame);
        let mut output = self.ctx.run_ui(input, |ui| {
            eframe::App::logic(app, ui.ctx(), frame);
            eframe::App::ui(app, ui, frame);
        });
        self.texture_bytes = output
            .textures_delta
            .set
            .iter()
            .flat_map(|(_, deltas)| deltas.iter())
            .map(|delta| delta.image.size().iter().product::<usize>() * 4)
            .sum();
        output.textures_delta.clear();
        let passes = output.platform_output.num_completed_passes;
        self.output = output;
        passes
    }

    /// Settle, then report how many passes the last frames needed.
    fn idle(&mut self, frames: usize) -> usize {
        (0..frames)
            .map(|_| self.pass(Vec::new()))
            .max()
            .unwrap_or(0)
    }

    fn click(&mut self, pos: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.pass(vec![egui::Event::PointerMoved(pos)]);
        self.pass(vec![button(true)]);
        self.pass(vec![button(false)]);
        self.idle(3);
    }

    fn type_text(&mut self, text: &str) {
        self.pass(vec![egui::Event::Text(text.into())]);
        self.idle(3);
    }

    fn widget(&self, label: &str) -> Option<egui::Rect> {
        let update = self.output.platform_output.accesskit_update.as_ref()?;
        let bounds = update
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))?
            .1
            .bounds()?;
        Some(egui::Rect::from_min_max(
            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
        ))
    }

    fn labels(&self) -> Vec<String> {
        self.output
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|u| {
                u.nodes
                    .iter()
                    .filter_map(|(_, n)| n.label().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn editor(&mut self, id: TabId) -> &mut crate::editor::Editor {
        &mut self.app.host.documents.get_mut(&id).unwrap().editor
    }
}

fn pdf(running: &mut Running, id: TabId) -> &mut pdfcraft_ui_egui::PdfCraftApp {
    let crate::editor::Editor::Pdf(app) = running.editor(id) else {
        panic!("PDF did not open");
    };
    app
}

fn settle_pdf(running: &mut Running, id: TabId) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    // Advance past the zoom holds before treating an empty queue as settled.
    running.idle(13);
    loop {
        running.pass(Vec::new());
        if !pdf(running, id).render_pending() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "PDF raster did not settle");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(pdf(running, id).views[0].page_errors().is_empty());
}

#[test]
fn pdf_rapid_zoom_reuses_the_page_then_sharpens_at_the_final_scale() {
    let (mut running, id) = Running::new(DocumentKind::Pdf);
    pdf(&mut running, id).views[0].set_zoom(0.9);
    settle_pdf(&mut running, id);
    let page = pdf(&mut running, id).views[0].page_screen_rect(0).unwrap();
    let pos = page.intersect(egui::Rect::from_min_max(egui::pos2(250.0, 220.0), egui::pos2(1390.0, 840.0))).center();
    let manager = running.ctx.tex_manager();
    let raster = || {
        manager.read().allocated().find(|(_, meta)| meta.name.starts_with("page-")).map(|(id, meta)| (*id, meta.size)).unwrap()
    };
    let zoom = || vec![egui::Event::PointerMoved(pos), egui::Event::Zoom(1.05)];
    let before = raster();
    // The first notch can submit work; all later transient scales must leave the texture
    // alone. The warm cache is important: a page without pixels must still be able to render.
    running.pass_at_rate(zoom(), 60.0);
    let during = raster();
    for _ in 1..8 {
        running.pass_at_rate(zoom(), 60.0);
        assert_eq!(raster(), during);
        assert!(!pdf(&mut running, id).render_pending(), "no transient raster should be queued");
    }
    assert!(pdf(&mut running, id).views[0].zoom > 1.3);
    settle_pdf(&mut running, id);
    let after = raster();
    assert_ne!(after.0, during.0);
    assert!(after.1[0] > before.1[0]);
    let page = pdf(&mut running, id).views[0].page_screen_rect(0).unwrap();
    assert!((after.1[0] as f32 - page.width() * running.ctx.pixels_per_point()).abs() <= 2.0);
}

/// Replays sustained PDF zoom at a 60 Hz input cadence, with workers but without a GPU.
/// Use the same PDF and run alone for comparable p95 CPU time and texture upload demand.
#[test]
#[ignore = "set WEAVE_ZOOM_PDF to a local test PDF and run with --nocapture"]
fn pdf_zoom_replay() {
    use std::time::{Duration, Instant};

    let path = std::env::var_os("WEAVE_ZOOM_PDF").expect("WEAVE_ZOOM_PDF");
    let (mut running, blank_id) = Running::new(DocumentKind::Pdf);
    running.app.actions.push_back(Action::Open(path.into()));
    running.idle(4);
    let id = running.app.mounted.unwrap();
    assert_ne!(id, blank_id, "the requested benchmark PDF did not open");
    let at = ["夜间阅读", "Night reading"].into_iter()
        .find_map(|label| running.widget(label)).expect("night button").center();
    for night in [false, true, true, false] {
        let app = pdf(&mut running, id);
        let (_, doc) = app.active_ids().unwrap();
        if app.session.reading_colors(doc).unwrap().params.is_some() != night {
            running.click(at);
        }
        pdf(&mut running, id).views[0].set_zoom(1.0);
        settle_pdf(&mut running, id);
        let pos = pdf(&mut running, id).views[0].page_screen_rect(0).unwrap().intersect(
            egui::Rect::from_min_max(egui::pos2(250.0, 220.0), egui::pos2(1390.0, 840.0))
        ).center();
        let mut times = Vec::new();
        let mut bytes = 0;
        let started = Instant::now();
        for k in 0..120 {
            let frame = Instant::now();
            let factor = if k % 60 < 30 { 1.045 } else { 1.0 / 1.045 };
            running.pass_at_rate(vec![egui::Event::PointerMoved(pos), egui::Event::Zoom(factor)], 60.0);
            times.push(frame.elapsed().as_secs_f64() * 1000.0);
            bytes += running.texture_bytes;
            if k == 29 {
                assert!(pdf(&mut running, id).views[0].zoom > 3.0, "gesture reached the page");
            }
            let next = started + Duration::from_secs_f64((k + 1) as f64 / 60.0);
            if let Some(remaining) = next.checked_duration_since(Instant::now()) {
                std::thread::sleep(remaining);
            }
        }
        let ended = started.elapsed();
        times.sort_by(f64::total_cmp);
        eprintln!("night={night} p50={:.2}ms p95={:.2}ms max={:.2}ms over16.7ms={} rgba={:.1}MiB elapsed={ended:.2?}",
            times[59], times[113], times[119], times.iter().filter(|&&t| t > 1000.0 / 60.0).count(), bytes as f64 / 1048576.0);
    }
}

#[test]
fn night_reading_buttons_toggle_only_the_active_document_view() {
    use crate::editor::Editor;
    use crate::output::{PdfPurpose, pdf_bytes};

    fn print_image(editor: &mut Editor) -> Vec<u8> {
        let bytes = pdf_bytes(editor, PdfPurpose::Print).unwrap();
        let mut session = pdfcraft_engine::Session::new();
        let id = session.open("print.pdf", None, std::sync::Arc::new(bytes), None).unwrap();
        pdfcraft_engine::export::Exporter::new(session.get(id).unwrap()).png(0, 72.0).unwrap()
    }

    fn exported_image(editor: &Editor) -> Option<Vec<u8>> {
        let Editor::Pdf(app) = editor else { return None };
        let (_, doc) = app.active_ids().unwrap();
        Some(pdfcraft_engine::export::Exporter::new(app.session.get(doc).unwrap()).png(0, 72.0).unwrap())
    }

    fn has_fill(shapes: &[egui::epaint::ClippedShape], color: egui::Color32) -> bool {
        fn filled(shape: &egui::Shape, color: egui::Color32) -> bool {
            match shape {
                egui::Shape::Rect(rect) => rect.fill == color && rect.rect.area() > 20_000.0,
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| filled(shape, color)),
                _ => false,
            }
        }
        shapes.iter().any(|shape| filled(&shape.shape, color))
    }

    let button = |running: &Running| {
        ["夜间阅读", "Night reading"]
            .into_iter()
            .find_map(|label| running.widget(label))
    };
    for kind in [DocumentKind::Text, DocumentKind::Spreadsheet, DocumentKind::Pdf, DocumentKind::RichText] {
        let (mut running, id) = Running::after(&[kind], kind);
        let other = running.app.host.workspace.tabs().iter().find(|tab| tab.id != id).unwrap().id;
        let original = running.editor(id).snapshot().unwrap()().unwrap();
        let dirty = running.editor(id).dirty();
        let history = running.editor(id).history_available();
        let printed = print_image(running.editor(id));
        let image = exported_image(running.editor(id));
        let night = egui::Color32::from_rgb(28, 30, 34);
        let at = button(&running).unwrap_or_else(|| panic!("no night button in {kind:?}")).center();
        for on in [true, false] {
            running.click(at);
            match running.editor(id) {
                Editor::Pdf(app) => {
                    let (_, doc) = app.active_ids().unwrap();
                    assert_eq!(app.session.reading_colors(doc).unwrap().params.map(|p| p.background), on.then_some([28, 30, 34]));
                }
                Editor::Word(app) => assert_eq!(app.session.view.dark_mode, on),
                Editor::Sheet(_) | Editor::Rich(_) => assert_eq!(has_fill(&running.output.shapes, night), on, "{kind:?}"),
                _ => unreachable!(),
            }
            assert_eq!(running.editor(id).snapshot().unwrap()().unwrap(), original, "serialized {kind:?}");
            assert_eq!(running.editor(id).dirty(), dirty);
            assert_eq!(running.editor(id).history_available(), history);
            if on {
                assert_eq!(print_image(running.editor(id)), printed, "print rendering for {kind:?}");
                assert_eq!(exported_image(running.editor(id)), image, "PDF image export");
                running.app.actions.push_back(Action::Activate(other));
                running.idle(8);
                assert_eq!(running.app.mounted, Some(other));
                match running.editor(other) {
                    Editor::Pdf(app) => assert!(app.session.reading_colors(app.active_ids().unwrap().1).unwrap().params.is_none()),
                    Editor::Word(app) => assert!(!app.session.view.dark_mode),
                    Editor::Sheet(_) | Editor::Rich(_) => assert!(!has_fill(&running.output.shapes, night), "other {kind:?}"),
                    _ => unreachable!(),
                }
                running.app.actions.push_back(Action::Activate(id));
                running.idle(8);
                assert_eq!(running.app.mounted, Some(id));
            }
        }
    }

    let (running, _) = Running::new(DocumentKind::Presentation);
    assert!(button(&running).is_none());
}

/// Settled frames need one pass: a font set that the host and an editor keep replacing
/// makes every frame a discarded pass, and editors drop input on discarded passes.
#[test]
fn every_new_document_settles_to_single_pass_frames() {
    let kinds = [
        DocumentKind::Text,
        DocumentKind::Spreadsheet,
        DocumentKind::Presentation,
        DocumentKind::Pdf,
    ];
    let mut replayed = Vec::new();
    for before in std::iter::once(None).chain(kinds.map(Some)) {
        for kind in kinds {
            let before: Vec<_> = before.into_iter().collect();
            let (mut running, _) = Running::after(&before, kind);
            let passes = running.idle(10);
            if passes != 1 {
                replayed.push((before, kind, passes));
            }
        }
    }
    assert!(
        replayed.is_empty(),
        "frames keep being replayed: {replayed:?}"
    );
}

#[test]
fn a_new_presentation_takes_a_click_and_typing_in_its_title() {
    let (mut running, id) = Running::new(DocumentKind::Presentation);
    let crate::editor::Editor::Slides(slides) = running.editor(id) else {
        panic!("presentation");
    };
    let slide = slides.slide_rect.expect("the slide was drawn");
    // The title placeholder of the title layout sits in the upper half of the slide.
    let title = egui::pos2(slide.center().x, slide.top() + slide.height() * 0.4);
    running.click(title);
    running.click(title);
    running.type_text("Weave");
    let crate::editor::Editor::Slides(slides) = running.editor(id) else {
        panic!()
    };
    let document = slides.session.active().unwrap();
    let text = serde_json::to_string(&document.doc.slides[0]).unwrap();
    assert!(
        text.contains("Weave"),
        "typing reached the title placeholder; selection {:?}",
        document.selection
    );
}

#[test]
fn a_new_presentations_ribbon_buttons_take_clicks() {
    let (mut running, id) = Running::new(DocumentKind::Presentation);
    let labels = running.labels();
    let button = ["新建 幻灯片", "新建幻灯片", "New Slide"]
        .into_iter()
        .find_map(|label| running.widget(label))
        .unwrap_or_else(|| panic!("no New Slide button among {labels:?}"));
    running.click(button.center());
    let crate::editor::Editor::Slides(slides) = running.editor(id) else {
        panic!()
    };
    assert_eq!(slides.session.active().unwrap().doc.slides.len(), 2);
}

/// As in the window: a presentation made with the New button while a PDF (or a spreadsheet,
/// whose copies of shared faces the host keeps) is open.
#[test]
fn a_presentation_made_next_to_another_document_takes_clicks_and_typing() {
    for before in [DocumentKind::Pdf, DocumentKind::Spreadsheet] {
        let (mut running, id) = Running::after(&[before], DocumentKind::Presentation);
        assert_eq!(running.idle(5), 1, "frames keep being replayed");
        let crate::editor::Editor::Slides(slides) = running.editor(id) else {
            panic!("presentation");
        };
        let slide = slides.slide_rect.expect("the slide was drawn");
        let title = egui::pos2(slide.center().x, slide.top() + slide.height() * 0.4);
        running.click(title);
        running.click(title);
        running.type_text("Weave");
        let crate::editor::Editor::Slides(slides) = running.editor(id) else {
            panic!()
        };
        let document = slides.session.active().unwrap();
        let text = serde_json::to_string(&document.doc.slides[0]).unwrap();
        assert!(
            text.contains("Weave"),
            "{before:?}: selection {:?}",
            document.selection
        );
        let button = running.widget("新建 幻灯片").expect("New Slide button");
        running.click(button.center());
        let crate::editor::Editor::Slides(slides) = running.editor(id) else {
            panic!()
        };
        assert_eq!(slides.session.active().unwrap().doc.slides.len(), 2);
    }
}

/// The ribbon commands under the host's tab row take clicks in every editor, also next to
/// another editor's open document.
#[test]
fn every_editors_ribbon_buttons_take_clicks_next_to_a_presentation() {
    for (kind, label) in [
        (DocumentKind::Text, "居中"),
        (DocumentKind::Spreadsheet, SHEET_BOLD),
    ] {
        let (mut running, id) = Running::after(&[DocumentKind::Presentation], kind);
        assert_eq!(running.editor(id).history_available(), (false, false));
        let button = running
            .widget(label)
            .unwrap_or_else(|| panic!("{kind:?} shows {label}"));
        running.click(button.center());
        assert!(
            running.editor(id).history_available().0,
            "{kind:?}: a click on {label} changed the document"
        );
    }
    let (mut running, id) = Running::after(&[DocumentKind::Presentation], DocumentKind::Pdf);
    let rotate_label = if cfg!(target_os = "macos") {
        "顺时针旋转视图 (⇧⌘+)"
    } else {
        "顺时针旋转视图 (Ctrl+Shift++)"
    };
    let button = running
        .widget(rotate_label)
        .expect("PDF shows Rotate View");
    running.click(button.center());
    let crate::editor::Editor::Pdf(pdf) = running.editor(id) else {
        panic!("PDF")
    };
    assert_eq!(pdf.views[pdf.active.unwrap()].rotation, 90);
}

/// Typing reaches a new document's editing surface.
#[test]
fn a_new_text_document_and_spreadsheet_take_typing() {
    for kind in [DocumentKind::Text, DocumentKind::Spreadsheet] {
        let (mut running, id) = Running::after(&[DocumentKind::Presentation], kind);
        running.type_text("Weave");
        running.pass(vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        running.idle(3);
        assert!(
            running.editor(id).history_available().0,
            "{kind:?}: typing changed the document"
        );
    }
}

/// Back on a spreadsheet after making a presentation: the sheet's ribbon still takes clicks.
#[test]
fn a_spreadsheets_ribbon_takes_clicks_after_switching_back_from_a_new_presentation() {
    let (mut running, slides) =
        Running::after(&[DocumentKind::Spreadsheet], DocumentKind::Presentation);
    let sheet = *running
        .app
        .host
        .workspace
        .tabs()
        .iter()
        .map(|tab| &tab.id)
        .find(|id| **id != slides)
        .unwrap();
    running.app.actions.push_back(Action::Activate(sheet));
    running.idle(20);
    assert_eq!(running.app.mounted, Some(sheet));
    assert_eq!(running.idle(5), 1, "frames keep being replayed");
    let button = running.widget(SHEET_BOLD).expect("Bold");
    running.click(button.center());
    assert!(running.editor(sheet).history_available().0);
}
