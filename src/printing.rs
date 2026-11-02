//! One print dialog and system spooler for every document type.
use crate::language::tr;
use crate::{
    dialog::{self, ButtonKind},
    host::Host,
    output::{self, PdfPurpose},
};
use eframe::egui;
use pdfcraft_print::spool::{self, Duplex, Job, Printer};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use weave_workspace::TabId;

pub struct PrintDialog {
    source: TabId,
    title: String,
    discovering: Option<Receiver<Vec<Printer>>>,
    printers: Vec<Printer>,
    selected: usize,
    copies: u32,
    duplex: bool,
    pending: Option<Receiver<Result<String, String>>>,
    result: Option<Result<String, String>>,
}
impl PrintDialog {
    pub fn new(source: TabId, title: String, ctx: &egui::Context) -> Self {
        let (sender, receiver) = mpsc::channel();
        let wake = ctx.clone();
        let result = std::thread::Builder::new()
            .name("weave-printers".into())
            .spawn(move || {
                let _ = sender.send(spool::printers());
                wake.request_repaint();
            });
        Self {
            source,
            title,
            discovering: result.as_ref().ok().map(|_| receiver),
            printers: Vec::new(),
            selected: 0,
            copies: 1,
            duplex: false,
            pending: None,
            result: result.err().map(|e| Err(e.to_string())),
        }
    }
    /// Returns true when the dialog can close. Printer discovery and submission stay off the UI thread.
    pub fn show(&mut self, ctx: &egui::Context, host: &mut Host) -> bool {
        if let Some(receiver) = &self.discovering {
            match receiver.try_recv() {
                Ok(printers) => {
                    self.selected = printers.iter().position(|p| p.default).unwrap_or(0);
                    self.printers = printers;
                    self.discovering = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.discovering = None;
                    self.result = Some(Err(tr("无法读取系统打印机").into()));
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok(result) => {
                    self.result = Some(result);
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.result = Some(Err(tr("打印任务未返回结果").into()));
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut close = false;
        let mut submit = false;
        egui::Modal::new(egui::Id::unique("weave-print"))
            .frame(dialog::frame(ctx))
            .show(ctx, |ui| {
                dialog::prepare(ui);
                dialog::header(ui, tr("打印"));
                dialog::body(ui, &self.title);
                if self.discovering.is_some() {
                    ui.spinner();
                    ui.label(tr("正在读取打印机…"));
                } else if self.printers.is_empty() {
                    ui.label(tr("暂无可用打印机"));
                } else {
                    ui.add_enabled_ui(self.pending.is_none(), |ui| {
                        egui::ComboBox::from_id_salt("weave-printer")
                            .selected_text(
                                self.printers
                                    .get(self.selected)
                                    .map(printer_label)
                                    .unwrap_or_default(),
                            )
                            .show_ui(ui, |ui| {
                                for (index, printer) in self.printers.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut self.selected,
                                        index,
                                        printer_label(printer),
                                    );
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.label(tr("份数"));
                            ui.add(egui::DragValue::new(&mut self.copies).range(1..=999));
                        });
                        ui.checkbox(&mut self.duplex, tr("双面打印（长边翻转）"));
                    });
                }
                if self.pending.is_some() {
                    ui.spinner();
                    ui.label(tr("正在提交打印任务…"));
                }
                if let Some(result) = &self.result {
                    match result {
                        Ok(_) => {
                            ui.label(tr("已提交到打印队列"));
                        }
                        Err(error) => {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                    }
                }
                dialog::footer(ui, |ui| {
                    if !matches!(self.result, Some(Ok(_))) {
                        submit = ui
                            .add_enabled_ui(
                                !self.printers.is_empty() && self.pending.is_none(),
                                |ui| dialog::button(ui, tr("打印"), ButtonKind::Primary),
                            )
                            .inner
                            .clicked();
                    }
                    let close_kind = if matches!(self.result, Some(Ok(_))) {
                        ButtonKind::Primary
                    } else {
                        ButtonKind::Secondary
                    };
                    close = ui
                        .add_enabled_ui(self.pending.is_none(), |ui| {
                            dialog::button(ui, tr("关闭"), close_kind)
                        })
                        .inner
                        .clicked();
                });
            });
        if submit {
            let result = host
                .documents
                .get_mut(&self.source)
                .ok_or_else(|| "文件已关闭".to_string())
                .and_then(|document| output::pdf_bytes(&mut document.editor, PdfPurpose::Print));
            match result {
                Err(error) => self.result = Some(Err(error)),
                Ok(bytes) => {
                    let job = Job {
                        printer: self
                            .printers
                            .get(self.selected)
                            .map(|printer| printer.name.clone()),
                        title: self.title.clone(),
                        copies: self.copies.clamp(1, 999),
                        duplex: Some(if self.duplex {
                            Duplex::LongEdge
                        } else {
                            Duplex::Off
                        }),
                        ..Default::default()
                    };
                    let (sender, receiver) = mpsc::channel();
                    let wake = ctx.clone();
                    match std::thread::Builder::new()
                        .name("weave-print".into())
                        .spawn(move || {
                            let _ =
                                sender.send(spool::submit(&bytes, &job).map_err(|e| e.to_string()));
                            wake.request_repaint();
                        }) {
                        Ok(_) => {
                            self.pending = Some(receiver);
                            self.result = None;
                        }
                        Err(error) => self.result = Some(Err(error.to_string())),
                    }
                }
            }
        }
        close
    }
    pub fn can_close(&self) -> bool {
        self.pending.is_none()
    }
}
fn printer_label(printer: &Printer) -> &str {
    if printer.description.is_empty() {
        &printer.name
    } else {
        &printer.description
    }
}
