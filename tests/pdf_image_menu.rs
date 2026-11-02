//! Real hosted PDF canvas and AccessKit menu routing, without a native window.
use eframe::egui;
use egui::accesskit::{Node, NodeId};
use std::collections::{HashMap, HashSet};
use weave_office::{editor::Editor, host::Host};

fn source_pdf() -> Vec<u8> {
    fn stream(dict: &str, bytes: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {dict} /Length {} >>\nstream\n", bytes.len()).into_bytes();
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\nendstream");
        out
    }
    let first = b"q 80 0 0 40 100 100 cm /Im0 Do Q\nBT /F1 16 Tf 20 235 Td (Weave source invoice 123.45) Tj ET\n";
    let second = b"BT /F1 16 Tf 20 235 Td (Second page) Tj ET\n";
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 5 0 R >> /XObject << /Im0 8 0 R >> >> /Contents 6 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 5 0 R >> >> /Contents 7 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
        stream("", first),
        stream("", second),
        stream("/Type /XObject /Subtype /Image /Width 3 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8", &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255]),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

#[derive(Default)]
struct AccessibleTree {
    root: Option<NodeId>,
    nodes: HashMap<NodeId, Node>,
}

impl AccessibleTree {
    fn apply(&mut self, update: egui::accesskit::TreeUpdate) {
        if let Some(tree) = update.tree {
            self.root = Some(tree.root);
        }
        self.nodes.extend(update.nodes);
    }

    fn live_nodes(&self) -> Vec<&Node> {
        let mut live = Vec::new();
        let mut seen = HashSet::new();
        let mut pending: Vec<_> = self.root.into_iter().collect();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(node) = self.nodes.get(&id) {
                pending.extend(node.children().iter().copied());
                live.push(node);
            }
        }
        live
    }

    fn labelled(&self, label: &str) -> Vec<&Node> {
        self.live_nodes()
            .into_iter()
            .filter(|node| node.label() == Some(label))
            .collect()
    }

    fn centre(&self, label: &str) -> egui::Pos2 {
        let matches = self.labelled(label);
        assert_eq!(
            matches.len(),
            1,
            "expected one live {label:?} control, got {}",
            matches.len()
        );
        let bounds = matches[0].bounds().expect("menu control has screen bounds");
        egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.0) as f32,
            ((bounds.y0 + bounds.y1) / 2.0) as f32,
        )
    }
}

struct Ui {
    ctx: egui::Context,
    host: Host,
    tree: AccessibleTree,
    frame: u64,
}

impl Ui {
    fn new(path: &std::path::Path) -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut host = Host::default();
        host.open(path)
            .expect("open original PDF through Weave host");
        match &mut host.active_mut().expect("PDF tab").editor {
            Editor::Pdf(app) => {
                app.set_option("language", "en").expect("English UI");
                app.views[0].continuous = true;
                app.views[0].set_zoom(0.65);
            }
            _ => panic!("fixture should open as PDF"),
        }
        let mut ui = Self {
            ctx,
            host,
            tree: AccessibleTree::default(),
            frame: 0,
        };
        for _ in 0..80 {
            ui.step(Vec::new());
            if ui.pdf().views[0].glyph_screen_pos(0, 0).is_some()
                && ui.pdf().views[0].page_screen_rect(1).is_some()
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            ui.pdf().views[0].glyph_screen_pos(0, 0).is_some(),
            "the source text layer must be ready before selecting it"
        );
        ui
    }

    fn step(&mut self, events: Vec<egui::Event>) {
        self.frame += 1;
        let mut input = egui::RawInput {
            focused: true,
            time: Some(self.frame as f64 / 60.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 860.0),
            )),
            events,
            ..Default::default()
        };
        let document = self.host.active_mut().expect("active PDF view");
        document.editor.raw_input(&self.ctx, &mut input);
        let output = self.ctx.run_ui(input, |ui| {
            document.editor.logic(&self.ctx);
            document.editor.ui(ui);
        });
        if let Some(update) = output.platform_output.accesskit_update.clone() {
            self.tree.apply(update);
        }
        output.drop_without_applying_deltas();
    }

    fn hover(&mut self, at: egui::Pos2) {
        self.step(vec![egui::Event::PointerMoved(at)]);
    }

    fn click(&mut self, at: egui::Pos2, button: egui::PointerButton) {
        self.hover(at);
        for pressed in [true, false] {
            self.step(vec![egui::Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers: Default::default(),
            }]);
        }
        self.step(Vec::new());
    }

    fn pdf(&self) -> &pdfcraft_ui_egui::PdfCraftApp {
        match &self.host.active().expect("active PDF").editor {
            Editor::Pdf(app) => app,
            _ => panic!("active editor should be PDF"),
        }
    }

    fn picture_centre(&self) -> egui::Pos2 {
        let page = self.pdf().views[0]
            .page_screen_rect(0)
            .expect("first page visible");
        egui::pos2(
            page.left() + page.width() * 140.0 / 300.0,
            page.top() + page.height() * 180.0 / 300.0,
        )
    }

    fn select_all_text(&mut self) {
        for pressed in [true, false] {
            self.step(vec![egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }]);
        }
        let selected = self.pdf().views[0]
            .selected_text()
            .expect("Cmd+A selects the loaded page text");
        assert!(
            selected.contains("Weave source invoice 123.45"),
            "Cmd+A selected the source PDF text: {selected:?}"
        );
    }
}

#[test]
fn hosted_image_menu_excludes_page_menu_after_read_selection_and_routes_rotate() {
    let directory = tempfile::tempdir().expect("temporary fixture directory");
    let source = directory.path().join("source.pdf");
    std::fs::write(&source, source_pdf()).expect("write synthetic PDF");
    let mut ui = Ui::new(&source);
    let second = ui.pdf().views[0]
        .page_screen_rect(1)
        .expect("second page laid out");
    assert!(
        second.intersects(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 860.0)
        )),
        "the second page should be visible alongside the first"
    );

    // Reproduce the retained Read selection using the real keyboard shortcut.
    ui.select_all_text();
    let edit = ui.tree.centre("Edit");
    ui.click(edit, egui::PointerButton::Primary);
    let picture = ui.picture_centre();
    ui.click(picture, egui::PointerButton::Primary);
    assert!(
        ui.pdf().views[0].image_selection.is_some(),
        "the original image is selected"
    );
    let before = ui
        .pdf()
        .session
        .get(ui.pdf().active_ids().expect("PDF id").1)
        .expect("document")
        .page_images(0)[0]
        .rect;

    ui.click(picture, egui::PointerButton::Secondary);
    for label in ["Rotate Clockwise", "Flip Horizontal", "Replace Image…"] {
        assert_eq!(
            ui.tree.labelled(label).len(),
            1,
            "only the image menu should contain {label:?}"
        );
        let at = ui.tree.centre(label);
        assert!(
            at.is_finite(),
            "{label} has a real clickable screen position"
        );
    }
    assert!(
        ui.tree.labelled("Comments panel").is_empty(),
        "generic annotation menu must not coexist with image menu"
    );

    // The open menu survives leaving the image to reach its controls, and a real click edits the PDF.
    let rotate = ui.tree.centre("Rotate Clockwise");
    ui.hover(rotate);
    assert_eq!(ui.tree.labelled("Replace Image…").len(), 1);
    ui.click(rotate, egui::PointerButton::Primary);
    let doc = ui
        .pdf()
        .session
        .get(ui.pdf().active_ids().expect("PDF id").1)
        .expect("document");
    assert_ne!(
        doc.page_images(0)[0].rect,
        before,
        "the menu click rotates the original placement"
    );
    assert_eq!(
        doc.can_undo(),
        Some("Rotate image"),
        "rotation must be an actual undoable edit"
    );
}

#[test]
fn hosted_read_selection_and_edit_blank_keep_the_generic_menu() {
    let directory = tempfile::tempdir().expect("temporary fixture directory");
    let source = directory.path().join("source.pdf");
    std::fs::write(&source, source_pdf()).expect("write synthetic PDF");
    let mut ui = Ui::new(&source);
    ui.select_all_text();
    let page = ui.pdf().views[0]
        .page_screen_rect(0)
        .expect("first page visible");
    let text = egui::pos2(
        page.left() + page.width() * 80.0 / 300.0,
        page.top() + page.height() * 65.0 / 300.0,
    );
    ui.click(text, egui::PointerButton::Secondary);
    assert_eq!(
        ui.tree.labelled("Comments panel").len(),
        1,
        "Read's page menu stays available"
    );
    assert!(
        ui.tree
            .live_nodes()
            .into_iter()
            .any(|node| node.label().is_some_and(|label| label == "Copy"
                || label.starts_with("Copy ") && !label.starts_with("Copy Image"))),
        "Read's selected text can still be copied"
    );

    ui.step(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    ui.step(Vec::new());
    let edit = ui.tree.centre("Edit");
    ui.click(edit, egui::PointerButton::Primary);
    let page = ui.pdf().views[0]
        .page_screen_rect(0)
        .expect("first page visible in Edit");
    let blank = egui::pos2(
        page.left() + page.width() * 250.0 / 300.0,
        page.top() + page.height() * 200.0 / 300.0,
    );
    ui.click(blank, egui::PointerButton::Secondary);
    assert_eq!(
        ui.tree.labelled("Comments panel").len(),
        1,
        "a fresh Edit right click on blank page owns the generic menu"
    );
    assert!(
        ui.tree.labelled("Replace Image…").is_empty(),
        "the old image menu cannot follow a blank right click"
    );
}
