//! A new document can be edited in the running app: clicks reach the editing surface and the
//! ribbon commands, and typing lands in the document. These drive the whole `eframe::App`
//! (raw input hook, logic and ui, with the host's interface fonts), as the window does.
use super::tests::test_app;
use super::*;

struct Running {
    app: WeaveApp,
    ctx: egui::Context,
    frame: eframe::Frame,
    output: egui::FullOutput,
    time: f64,
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
        self.time += 1.0 / 30.0;
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
        (DocumentKind::Spreadsheet, "粗体 (Ctrl+B)"),
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
    let button = running
        .widget("顺时针旋转视图 (Ctrl+Shift++)")
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
    let button = running.widget("粗体 (Ctrl+B)").expect("Bold");
    running.click(button.center());
    assert!(running.editor(sheet).history_available().0);
}
