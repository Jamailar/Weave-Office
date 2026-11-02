use deckcraft_ui_egui::SlideApp;
use eframe::egui;
use gridcraft_ui_egui::SheetApp;
use wordcraft_ui_egui::WordApp;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Word,
    Sheet,
    Slide,
}
struct Weave {
    current: Kind,
    word: WordApp,
    sheet: SheetApp,
    slide: SlideApp,
    allow_close: bool,
    chinese_font: Option<egui::FontData>,
    styled_kind: Option<Kind>,
}
impl eframe::App for Weave {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            let dirty = self.word.session.documents().iter().any(|d| d.dirty)
                || self.sheet.session.documents().iter().any(|d| d.is_dirty())
                || self.slide.session.documents().iter().any(|d| d.is_dirty());
            if dirty {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                let answer = rfd::MessageDialog::new()
                    .set_title("存在未保存的更改")
                    .set_description("文字、表格或演示中有未保存的内容。选择取消以返回并逐一保存；选择确定将放弃所有未保存的更改。")
                    .set_buttons(rfd::MessageButtons::OkCancel)
                    .show();
                if answer == rfd::MessageDialogResult::Ok {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                return;
            }
        }
        match self.current {
            Kind::Word => self.word.logic(ctx),
            Kind::Sheet => self.sheet.logic(ctx),
            Kind::Slide => self.slide.logic(ctx),
        }
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        match self.current {
            Kind::Word => self.word.raw_input_hook(ctx, raw),
            Kind::Sheet => self.sheet.raw_input_hook(ctx, raw),
            Kind::Slide => self.slide.raw_input_hook(ctx, raw),
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if self.styled_kind != Some(self.current) {
            match self.current {
                Kind::Word => {
                    wordcraft_ui_egui::theme::install_fonts(ui.ctx());
                    wordcraft_ui_egui::theme::apply(
                        ui.ctx(),
                        wordcraft_ui_egui::theme::Appearance::Light,
                    );
                }
                Kind::Sheet => SheetApp::setup_context(ui.ctx(), false),
                Kind::Slide => {
                    deckcraft_ui_egui::theme::install_fonts(ui.ctx());
                    deckcraft_ui_egui::theme::apply(
                        ui.ctx(),
                        &deckcraft_ui_egui::theme::Tokens::light(),
                    );
                }
            }
            self.styled_kind = Some(self.current);
            ui.ctx().request_repaint();
        }
        if let Some(font) = &self.chinese_font {
            ui.ctx().add_font(egui::FontInsert {
                name: "weave-ui-chinese".into(),
                data: font.clone(),
                families: vec![egui::epaint::text::InsertFontFamily {
                    family: egui::FontFamily::Proportional,
                    priority: egui::epaint::text::FontPriority::Lowest,
                }],
            });
        }
        egui::Panel::top("weave-tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current, Kind::Word, "文字");
                ui.selectable_value(&mut self.current, Kind::Sheet, "表格");
                ui.selectable_value(&mut self.current, Kind::Slide, "演示");
            });
        });
        match self.current {
            Kind::Word => self.word.ui(ui),
            Kind::Sheet => self.sheet.ui(ui),
            Kind::Slide => self.slide.ui(ui),
        }
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title("Weave Office".into()));
    }
}
fn main() -> eframe::Result {
    eframe::run_native(
        "Weave Office",
        eframe::NativeOptions::default(),
        Box::new(|_| {
            let word_services = wordcraft_ui_egui::Services {
                pick_open: Some(Box::new(|_| {
                    rfd::FileDialog::new()
                        .pick_file()
                        .map(|p| p.to_string_lossy().into_owned())
                })),
                pick_save: Some(Box::new(|name| {
                    rfd::FileDialog::new()
                        .set_file_name(name)
                        .save_file()
                        .map(|p| p.to_string_lossy().into_owned())
                })),
                ..Default::default()
            };
            let word = WordApp::new(
                wordcraft_engine::Session::new(wordcraft_doc::Document::new()),
                word_services,
            );
            let mut session = gridcraft_engine::Session::new();
            session.new_workbook();
            let sheet_services = gridcraft_ui_egui::Services {
                pick_open: Some(Box::new(|| {
                    rfd::FileDialog::new()
                        .add_filter("表格", &["xlsx", "xlsm", "csv"])
                        .pick_file()
                        .map(|p| p.to_string_lossy().into_owned())
                })),
                pick_save: Some(Box::new(|name| {
                    rfd::FileDialog::new()
                        .set_file_name(name)
                        .save_file()
                        .map(|p| p.to_string_lossy().into_owned())
                })),
                ..Default::default()
            };
            let sheet = SheetApp::new(session, sheet_services);
            let slide_services = deckcraft_ui_egui::Services {
                pick_open: Some(Box::new(|_| {
                    rfd::FileDialog::new()
                        .pick_file()
                        .map(|p| p.to_string_lossy().into_owned())
                })),
                pick_save: Some(Box::new(|name| {
                    rfd::FileDialog::new()
                        .set_file_name(name)
                        .save_file()
                        .map(|p| p.to_string_lossy().into_owned())
                })),
                read: Some(Box::new(|path| {
                    std::fs::read(path).map_err(|e| e.to_string())
                })),
                write: Some(Box::new(|path, bytes| {
                    std::fs::write(path, bytes).map_err(|e| e.to_string())
                })),
                ..Default::default()
            };
            let slide = SlideApp::new(deckcraft_engine::Session::with_new(), slide_services);
            Ok(Box::new(Weave {
                current: Kind::Word,
                word,
                sheet,
                slide,
                allow_close: false,
                styled_kind: None,
                chinese_font: std::fs::read("/System/Library/Fonts/PingFang.ttc")
                    .ok()
                    .map(egui::FontData::from_owned),
            }))
        }),
    )
}
