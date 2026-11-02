//! Every submitted editor frame must contain its canvas, including initialization.
//! No host background is drawn here: it must not conceal an editor that skipped painting.
use eframe::egui::{self, epaint::Shape};
use serde_json::json;
use weave_office::{editor::Editor, font_set::FontSet, host::Host};
use weave_workspace::{DocumentKind, TabId};

struct PaintRun {
    ctx: egui::Context,
    host: Host,
    mounted: Option<TabId>,
    fonts: FontSet,
    frame: u64,
}
impl PaintRun {
    fn new(passes: usize) -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx.options_mut(|options| options.max_passes = passes.try_into().unwrap());
        Self {
            ctx,
            host: Host::default(),
            mounted: None,
            fonts: FontSet::new(egui::FontDefinitions::default()),
            frame: 0,
        }
    }
    fn activate(&mut self, id: TabId) {
        if self.mounted == Some(id) {
            return;
        }
        self.fonts
            .capture_tab(&self.ctx, &mut self.host, self.mounted);
        if let Some(old) = self.mounted {
            let doc = self.host.documents.get_mut(&old).unwrap();
            doc.editor.deactivate(&self.ctx).unwrap();
            doc.memory = self.ctx.memory(Clone::clone);
        }
        let options = self.ctx.options(Clone::clone);
        let doc = &self.host.documents[&id];
        self.ctx.memory_mut(|memory| {
            *memory = doc.memory.clone();
            memory.options = options;
        });
        self.host.workspace.activate(id);
        self.mounted = Some(id);
    }
    fn draw(&mut self, reason: &str) {
        self.frame += 1;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            time: Some(self.frame as f64 / 60.0),
            focused: true,
            ..Default::default()
        };
        let (host, fonts, mounted) = (&mut self.host, &mut self.fonts, self.mounted);
        let output = self.ctx.run_ui(input, |ui| {
            // Match eframe's per-pass order. Font preparation must precede all shell text.
            fonts.sync(ui.ctx(), host, mounted, None);
            let editor = &mut host.active_mut().unwrap().editor;
            editor.logic(ui.ctx());
            weave_office::ui_fonts::UiFonts::load().install(ui.ctx());
            ui.ctx()
                .set_global_style(weave_office::theme::style(ui.ctx()));
            egui::Panel::top("test-host-title")
                .exact_size(38.0)
                .show(ui, |ui| {
                    ui.label("Weave Office file tabs");
                });
            editor.ui(ui);
        });
        let probe = egui::Rect::from_center_size(egui::pos2(600.0, 400.0), egui::vec2(60.0, 60.0));
        let canvas = output.shapes.iter().any(|shape| {
            shape.clip_rect.contains_rect(probe) && opaque_canvas(&shape.shape, probe)
        });
        assert!(
            canvas,
            "{reason}: submitted frame {} has no editor canvas ({} shapes, {} passes)",
            self.frame,
            output.shapes.len(),
            output.platform_output.num_completed_passes
        );
        let controls = output
            .platform_output
            .accesskit_update
            .as_ref()
            .map_or(0, |update| {
                update
                    .nodes
                    .iter()
                    .filter(|(_, node)| node.role() == egui::accesskit::Role::Button)
                    .count()
            });
        assert!(
            controls >= 10,
            "{reason}: frame {} painted only a fallback surface ({controls} editor buttons)",
            self.frame
        );
        output.drop_without_applying_deltas();
    }
}
fn opaque_canvas(shape: &Shape, probe: egui::Rect) -> bool {
    match shape {
        Shape::Rect(rect) => rect.fill.a() == 255 && rect.rect.contains_rect(probe),
        Shape::Vec(shapes) => shapes.iter().any(|shape| opaque_canvas(shape, probe)),
        _ => false,
    }
}
#[test]
fn every_first_and_restored_mixed_editor_frame_has_a_real_canvas() {
    for passes in [1, 2] {
        let mut run = PaintRun::new(passes);
        let ids = [
            DocumentKind::Text,
            DocumentKind::Spreadsheet,
            DocumentKind::Presentation,
            DocumentKind::Pdf,
        ]
        .map(|kind| run.host.new_document(kind).unwrap());
        for id in ids.into_iter().chain(ids.into_iter().rev()) {
            run.activate(id);
            for _ in 0..3 {
                run.draw(&format!("tab {id:?}, pass budget {passes}"));
            }
        }
    }
}
#[test]
fn formatting_and_font_changes_do_not_submit_an_empty_spreadsheet() {
    for passes in [1, 2] {
        let mut run = PaintRun::new(passes);
        let id = run.host.new_document(DocumentKind::Spreadsheet).unwrap();
        run.activate(id);
        run.draw("initial workbook");
        for (command, params) in [
            ("cell.set", json!({"cell":"A1","input":"中文与字体"})),
            ("home.fillColor", json!({"range":"A1","color":"#FFFF00"})),
            ("home.borders", json!({"range":"A1","preset":"all"})),
            ("home.fontName", json!({"range":"A1","name":"Arial"})),
            ("home.fontName", json!({"range":"A1","name":"宋体"})),
        ] {
            run.host
                .active_mut()
                .unwrap()
                .editor
                .command(command, params)
                .unwrap();
            for _ in 0..3 {
                run.draw(command);
            }
        }
        let Editor::Sheet(sheet) = &mut run.host.active_mut().unwrap().editor else {
            unreachable!()
        };
        let value = sheet
            .hosted_command("cell.get", json!({"cell":"A1"}))
            .unwrap();
        assert_eq!(value["value"], "中文与字体");
    }
}
