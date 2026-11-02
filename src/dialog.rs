//! Shared surfaces and action hierarchy for application dialogs.
//! File and folder panels remain owned by the operating system.
use eframe::egui::{self, Color32, Response, RichText, Stroke};

#[derive(Clone, Copy)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Destructive,
}

pub fn frame(ctx: &egui::Context) -> egui::Frame {
    crate::theme::surface_frame(ctx)
        .inner_margin(20)
        .corner_radius(8)
        .shadow(ctx.global_style().visuals.window_shadow)
}

pub fn prepare(ui: &mut egui::Ui) {
    ui.set_style(crate::theme::style(ui.ctx()));
    let width = (ui.ctx().content_rect().width() - 80.0).clamp(260.0, 420.0);
    ui.set_width(width);
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
    ui.spacing_mut().interact_size.y = 32.0;
    ui.visuals_mut().button_frame = true;
}

pub fn header(ui: &mut egui::Ui, title: &str) {
    ui.label(
        RichText::new(crate::language::tr(title))
            .size(18.0)
            .strong(),
    );
    ui.add_space(4.0);
}

pub fn body(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(13.0)
                .color(ui.visuals().weak_text_color()),
        )
        .wrap(),
    );
}

fn footer_space(ui: &mut egui::Ui) {
    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
}

/// The first action is drawn at the right edge, followed by its secondary action.
pub fn footer<R>(ui: &mut egui::Ui, actions: impl FnOnce(&mut egui::Ui) -> R) -> R {
    footer_space(ui);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions)
            .inner
    })
    .inner
}

/// Isolate a destructive choice at the left, away from the affirmative action.
pub fn split_footer<L, R>(
    ui: &mut egui::Ui,
    left: impl FnOnce(&mut egui::Ui) -> L,
    right: impl FnOnce(&mut egui::Ui) -> R,
) -> (L, R) {
    footer_space(ui);
    ui.horizontal(|ui| {
        let left = left(ui);
        let right = ui
            .with_layout(egui::Layout::right_to_left(egui::Align::Center), right)
            .inner;
        (left, right)
    })
    .inner
}

pub fn button(ui: &mut egui::Ui, label: &str, kind: ButtonKind) -> Response {
    let label = crate::language::tr(label);
    let dark = ui.visuals().dark_mode;
    let accent = if dark {
        Color32::from_rgb(89, 149, 225)
    } else {
        Color32::from_rgb(35, 103, 184)
    };
    let control = match kind {
        ButtonKind::Primary => egui::Button::new(RichText::new(label).color(Color32::WHITE))
            .fill(accent)
            .stroke(Stroke::new(1.0, accent)),
        ButtonKind::Secondary => egui::Button::new(label)
            .fill(ui.visuals().window_fill())
            .stroke(ui.visuals().widgets.inactive.bg_stroke),
        ButtonKind::Destructive => egui::Button::new(RichText::new(label).color(if dark {
            Color32::from_rgb(242, 139, 145)
        } else {
            Color32::from_rgb(174, 52, 62)
        }))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE),
    };
    ui.add(
        control
            .frame(true)
            .corner_radius(4)
            .min_size(egui::vec2(84.0, 32.0)),
    )
}
