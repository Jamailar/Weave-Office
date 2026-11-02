//! The title-bar New button and its anchored format picker.
use crate::{icons, language::tr};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, vec2};
use weave_workspace::DocumentKind;

pub type Anchor = (Rect, egui::LayerId, egui::Id);

pub fn button(ui: &mut egui::Ui, open: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(42.0, 28.0), egui::Sense::click());
    let visuals = ui.visuals();
    let fill = if open || response.hovered() || response.has_focus() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    let ink = visuals.text_color();
    let stroke = Stroke::new(1.0, visuals.window_stroke.color);
    ui.painter().rect_filled(rect.shrink(0.5), 5, fill);
    ui.painter()
        .rect_stroke(rect.shrink(0.5), 5, stroke, egui::StrokeKind::Inside);
    let center = Pos2::new(rect.left() + 14.0, rect.center().y);
    ui.painter().line_segment(
        [center - vec2(6.0, 0.0), center + vec2(6.0, 0.0)],
        Stroke::new(1.4, ink),
    );
    ui.painter().line_segment(
        [center - vec2(0.0, 6.0), center + vec2(0.0, 6.0)],
        Stroke::new(1.4, ink),
    );
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + 28.0, rect.top() + 1.0),
            Pos2::new(rect.left() + 28.0, rect.bottom() - 1.0),
        ],
        stroke,
    );
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.right() - 7.0, rect.center().y),
            vec2(10.0, 10.0),
        ),
        icons::Icon::ChevronDown,
        ink,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tr("新建文件"))
    });
    response.on_hover_text(tr("新建文件"))
}

pub fn show(ctx: &egui::Context, anchor: Anchor, open: &mut bool) -> Option<DocumentKind> {
    let mut selected = None;
    let visuals = &crate::theme::style(ctx).visuals;
    let frame = egui::Frame::new()
        .fill(if visuals.dark_mode {
            visuals.window_fill
        } else {
            Color32::from_rgb(241, 243, 244)
        })
        .stroke(visuals.window_stroke)
        .corner_radius(10)
        .inner_margin(24)
        .shadow(egui::epaint::Shadow {
            offset: [0, 4],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(35),
        });
    egui::Popup::new(
        egui::Id::unique("weave-new-formats"),
        ctx.clone(),
        anchor.0,
        anchor.1,
    )
    .anchor_widget(anchor.2)
    .accessible_name(tr("新建"))
    .gap(7.0)
    .open_bool(open)
    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
    .frame(frame)
    .show(|ui| {
        ui.set_style(crate::theme::style(ctx));
        ui.set_width(312.0);
        ui.label(egui::RichText::new(tr("新建")).size(22.0).strong());
        ui.add_space(20.0);
        ui.label(egui::RichText::new(tr("Office 文档")).size(13.0).strong());
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (kind, label) in [
                (DocumentKind::Text, tr("文字")),
                (DocumentKind::Presentation, tr("演示")),
                (DocumentKind::Spreadsheet, tr("表格")),
                (DocumentKind::Pdf, "PDF"),
            ] {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(73.5, 82.0), egui::Sense::click());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
                if response.hovered() || response.has_focus() {
                    ui.painter()
                        .rect_filled(rect, 6, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        rect,
                        6,
                        Stroke::new(1.0, ui.visuals().selection.bg_fill),
                        egui::StrokeKind::Inside,
                    );
                }
                icons::document(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(rect.center().x, rect.top() + 26.0),
                        vec2(40.0, 40.0),
                    ),
                    kind,
                );
                ui.painter().text(
                    Pos2::new(rect.center().x, rect.top() + 64.0),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(12.5),
                    ui.visuals().text_color(),
                );
                if response.clicked() {
                    selected = Some(kind);
                    ui.close();
                }
            }
        });
    });
    selected
}
