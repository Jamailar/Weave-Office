//! The ribbon tab row shared by every Weave Office editor.
//!
//! Each editor describes its tabs ([`HostedRibbon::ribbon_tabs`]) and reacts to clicks on them
//! ([`HostedRibbon::ribbon_event`]); the host draws the row once, with its own file controls in
//! front, so the four editors' tab rows look and behave the same. The editor still draws the
//! active tab's commands below it.

use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

/// One tab of a ribbon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    /// The editor's own name for the tab, passed back in [`Event`]s.
    pub id: String,
    /// What the row shows, already translated.
    pub label: String,
    /// Shown only for the current selection (a table, a picture): drawn in the accent color.
    pub contextual: bool,
}

impl Tab {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            contextual: false,
        }
    }

    pub fn contextual(mut self, contextual: bool) -> Self {
        self.contextual = contextual;
        self
    }
}

/// A ribbon's tabs and the one that is open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tabs {
    pub tabs: Vec<Tab>,
    /// The open tab's id. `None` when no tab is open (a read-only mode, a full-window page).
    pub active: Option<String>,
}

/// What the person did on the row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A tab was clicked: open it, or fold the ribbon when it is already open.
    Clicked(String),
    /// A tab was double-clicked: fold or unfold the ribbon.
    DoubleClicked(String),
}

/// An editor whose ribbon tab row the host draws.
pub trait HostedRibbon {
    /// The tabs to show, in order.
    fn ribbon_tabs(&self, ctx: &egui::Context) -> Tabs;
    /// React to a click on the row.
    fn ribbon_event(&mut self, event: Event);
    /// Whether undo and redo are available, for the host's quick controls.
    fn undo_redo(&self) -> (bool, bool);
    /// Draw the editor's own controls at the right end of the row (right to left).
    fn ribbon_trailing(&mut self, _ui: &mut Ui) {}
    /// Where each shown tab was drawn this frame, for key tips over them.
    fn ribbon_tab_rects(&mut self, _rects: Vec<(String, Rect)>) {}
}

/// What [`strip`] drew and what was clicked.
#[derive(Debug, Default)]
pub struct Response {
    pub event: Option<Event>,
    /// Each tab shown in the row and its rectangle. Tabs moved into the overflow menu are left out.
    pub rects: Vec<(String, Rect)>,
}

/// Horizontal padding around a tab's label.
pub const TAB_PADDING: f32 = 20.0;
/// Width of the overflow menu button.
pub const MORE_WIDTH: f32 = 26.0;
const GAP: f32 = 2.0;
const LABEL_SIZE: f32 = 13.0;

/// Which tabs fit in `available` points: the leading ones that fit, keeping room for the
/// overflow button when some do not, and always the active tab (in place of the last one shown
/// when needed).
pub fn fit(widths: &[f32], available: f32, active: Option<usize>) -> Vec<bool> {
    let total: f32 = widths.iter().map(|w| w + GAP).sum();
    if total <= available {
        return vec![true; widths.len()];
    }
    let room = (available - MORE_WIDTH - GAP).max(0.0);
    let mut shown = vec![false; widths.len()];
    let mut used = 0.0;
    for (i, w) in widths.iter().enumerate() {
        if used + w + GAP > room {
            break;
        }
        used += w + GAP;
        shown[i] = true;
    }
    if let Some(active) = active.filter(|&a| a < widths.len() && !shown[a]) {
        // Drop shown tabs from the end until the active one fits.
        while used + widths[active] + GAP > room {
            let Some(last) = shown.iter().rposition(|s| *s) else {
                break;
            };
            shown[last] = false;
            used -= widths[last] + GAP;
        }
        shown[active] = true;
    }
    shown
}

/// The family for the open tab's label: the interface's medium weight when the font set has
/// one, else the regular one.
fn medium(ui: &Ui) -> FontFamily {
    let medium = FontFamily::Name("medium".into());
    if ui
        .ctx()
        .fonts(|f| f.definitions().families.contains_key(&medium))
    {
        medium
    } else {
        FontFamily::Proportional
    }
}

/// Draw `tabs` left to right in the rest of the row.
pub fn strip(ui: &mut Ui, tabs: &Tabs) -> Response {
    let visuals = ui.visuals().clone();
    let text = visuals.text_color();
    let dim = visuals.weak_text_color();
    let accent = visuals.selection.bg_fill;
    let hover = visuals.widgets.hovered.weak_bg_fill;
    let regular = FontId::new(LABEL_SIZE, FontFamily::Proportional);
    let strong = FontId::new(LABEL_SIZE, medium(ui));
    let height = ui.available_height().clamp(24.0, 30.0);

    let active = tabs
        .active
        .as_ref()
        .and_then(|id| tabs.tabs.iter().position(|t| &t.id == id));
    let text_widths: Vec<f32> = tabs
        .tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let font = if Some(i) == active {
                strong.clone()
            } else {
                regular.clone()
            };
            ui.painter()
                .layout_no_wrap(tab.label.clone(), font, text)
                .size()
                .x
        })
        .collect();
    let widths: Vec<f32> = text_widths.iter().map(|w| w + TAB_PADDING).collect();
    let shown = fit(&widths, ui.available_width(), active);

    let mut response = Response::default();
    ui.spacing_mut().item_spacing.x = GAP;
    for (i, tab) in tabs.tabs.iter().enumerate().filter(|(i, _)| shown[*i]) {
        let is_active = Some(i) == active;
        let (rect, resp) = ui.allocate_exact_size(vec2(widths[i], height), Sense::click());
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Button, true, is_active, &tab.label)
        });
        if resp.hovered() && !is_active {
            ui.painter()
                .rect_filled(rect.shrink2(vec2(1.0, 3.0)), 4.0, hover);
        }
        let color = if tab.contextual {
            accent
        } else if is_active {
            text
        } else {
            dim
        };
        let font = if is_active {
            strong.clone()
        } else {
            regular.clone()
        };
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            &tab.label,
            font,
            color,
        );
        if is_active {
            let line = Rect::from_center_size(
                pos2(rect.center().x, rect.bottom() - 2.5),
                vec2(text_widths[i].clamp(16.0, 40.0), 3.0),
            );
            ui.painter().rect_filled(line, 1.5, accent);
        }
        if resp.double_clicked() {
            response.event = Some(Event::DoubleClicked(tab.id.clone()));
        } else if resp.clicked() {
            response.event = Some(Event::Clicked(tab.id.clone()));
        }
        response.rects.push((tab.id.clone(), rect));
    }
    if shown.iter().any(|s| !s) {
        ui.menu_button("»", |ui| {
            for (tab, _) in tabs.tabs.iter().zip(&shown).filter(|(_, s)| !**s) {
                let label = egui::RichText::new(&tab.label).color(if tab.contextual {
                    accent
                } else {
                    Color32::PLACEHOLDER
                });
                if ui.selectable_label(false, label).clicked() {
                    response.event = Some(Event::Clicked(tab.id.clone()));
                    ui.close();
                }
            }
        });
    }
    response
}

#[cfg(test)]
mod tests;
