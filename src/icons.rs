//! Original, resolution-independent icons for the application chrome.
//! All geometry is drawn in a 20 × 20 design space; no icon font or image is used.

use eframe::egui;
use egui::{
    Color32, CornerRadius, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2,
};
use weave_workspace::DocumentKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Icon {
    OpenFolder,
    Save,
    SaveAs,
    New,
    Undo,
    Redo,
    Print,
    ExportPdf,
    Search,
    Settings,
    More,
    Close,
    ChevronDown,
    Home,
    Recent,
    Pin,
    ArrowBack,
}

struct Canvas<'a> {
    painter: &'a Painter,
    rect: Rect,
    stroke: Stroke,
}

impl Canvas<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(
            self.rect.min.x + x * self.rect.width() / 20.0,
            self.rect.min.y + y * self.rect.height() / 20.0,
        )
    }

    fn line(&self, a: (f32, f32), b: (f32, f32)) {
        self.painter
            .line_segment([self.p(a.0, a.1), self.p(b.0, b.1)], self.stroke);
    }

    fn path(&self, points: &[(f32, f32)]) {
        self.painter.add(Shape::line(
            points.iter().map(|&(x, y)| self.p(x, y)).collect(),
            self.stroke,
        ));
    }

    fn closed(&self, points: &[(f32, f32)]) {
        self.painter.add(Shape::closed_line(
            points.iter().map(|&(x, y)| self.p(x, y)).collect(),
            self.stroke,
        ));
    }

    fn fill(&self, points: &[(f32, f32)], color: Color32) {
        self.painter.add(Shape::convex_polygon(
            points.iter().map(|&(x, y)| self.p(x, y)).collect(),
            color,
            Stroke::NONE,
        ));
    }

    fn circle(&self, x: f32, y: f32, radius: f32) {
        self.painter.circle_stroke(
            self.p(x, y),
            radius * self.rect.width().min(self.rect.height()) / 20.0,
            self.stroke,
        );
    }

    fn dot(&self, x: f32, y: f32, radius: f32) {
        self.painter.circle_filled(
            self.p(x, y),
            radius * self.rect.width().min(self.rect.height()) / 20.0,
            self.stroke.color,
        );
    }

    fn box_outline(&self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.closed(&[(x1, y1), (x2, y1), (x2, y2), (x1, y2)]);
    }
}

/// Paint an action icon inside any rectangular area, normally 18–20 pt square.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let scale = rect.width().min(rect.height()) / 20.0;
    let c = Canvas {
        painter,
        rect,
        stroke: Stroke::new((1.5 * scale).max(1.1), color),
    };
    use Icon::*;
    match icon {
        OpenFolder => {
            c.path(&[
                (2.5, 6.5),
                (2.5, 4.5),
                (7.5, 4.5),
                (9.5, 6.5),
                (16.5, 6.5),
                (17.5, 7.5),
            ]);
            c.closed(&[(2.5, 7.5), (17.5, 7.5), (15.5, 15.5), (4.5, 15.5)]);
        }
        Save | SaveAs => {
            c.closed(&[
                (4.0, 3.5),
                (14.5, 3.5),
                (16.0, 5.0),
                (16.0, 16.5),
                (4.0, 16.5),
            ]);
            c.box_outline(7.0, 3.5, 12.5, 8.0);
            c.path(&[(7.0, 16.0), (7.0, 11.5), (13.0, 11.5), (13.0, 16.0)]);
            if icon == SaveAs {
                c.line((13.7, 9.7), (18.0, 5.4));
                c.path(&[(12.8, 11.0), (14.3, 10.5), (18.0, 6.8), (16.7, 5.5)]);
            }
        }
        New => {
            c.closed(&[
                (4.0, 2.5),
                (11.5, 2.5),
                (16.0, 7.0),
                (16.0, 17.5),
                (4.0, 17.5),
            ]);
            c.path(&[(11.5, 2.5), (11.5, 7.0), (16.0, 7.0)]);
            c.line((7.0, 12.0), (13.0, 12.0));
            c.line((10.0, 9.0), (10.0, 15.0));
        }
        Undo | Redo => {
            // An arrow turning back: the head points left (right for redo) and sits on the tail's
            // end, the tail runs across and turns down. The head used to point the wrong way and
            // float above the tail, which read as a stray chevron beside a hook; the head and the
            // tail are also kept short, so the icon stays light at 28 pt.
            let flip = |x: f32| if icon == Redo { 20.0 - x } else { x };
            c.path(&[(flip(7.7), 12.7), (flip(3.5), 8.5), (flip(7.7), 4.3)]);
            c.path(&[
                (flip(3.5), 8.5),
                (flip(12.6), 8.5),
                (flip(15.4), 10.2),
                (flip(16.5), 12.7),
                (flip(16.5), 16.2),
            ]);
        }
        Print => {
            c.box_outline(5.0, 2.5, 15.0, 7.0);
            c.path(&[
                (5.0, 14.0),
                (3.5, 14.0),
                (3.0, 12.5),
                (3.0, 8.5),
                (4.5, 7.0),
                (15.5, 7.0),
                (17.0, 8.5),
                (17.0, 14.0),
                (15.0, 14.0),
            ]);
            c.box_outline(5.0, 11.5, 15.0, 17.5);
            c.dot(14.5, 9.2, 0.7);
        }
        ExportPdf => {
            c.closed(&[
                (3.5, 3.0),
                (10.5, 3.0),
                (14.5, 7.0),
                (14.5, 14.0),
                (3.5, 14.0),
            ]);
            c.path(&[(10.5, 3.0), (10.5, 7.0), (14.5, 7.0)]);
            c.line((8.5, 17.0), (17.0, 17.0));
            c.path(&[(14.0, 14.0), (17.0, 17.0), (14.0, 20.0)]);
        }
        Search => {
            c.circle(8.4, 8.4, 5.1);
            c.line((12.3, 12.3), (17.2, 17.2));
        }
        Settings => {
            c.circle(10.0, 10.0, 3.0);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let (s, co) = a.sin_cos();
                c.line(
                    (10.0 + 6.1 * co, 10.0 + 6.1 * s),
                    (10.0 + 8.0 * co, 10.0 + 8.0 * s),
                );
            }
            c.circle(10.0, 10.0, 6.1);
        }
        More => {
            for x in [4.5, 10.0, 15.5] {
                c.dot(x, 10.0, 1.35);
            }
        }
        Close => {
            c.line((4.5, 4.5), (15.5, 15.5));
            c.line((15.5, 4.5), (4.5, 15.5));
        }
        ChevronDown => {
            c.path(&[(4.5, 7.0), (10.0, 12.5), (15.5, 7.0)]);
        }
        Home => {
            c.path(&[(2.5, 9.0), (10.0, 3.0), (17.5, 9.0)]);
            c.path(&[(4.5, 7.5), (4.5, 17.0), (15.5, 17.0), (15.5, 7.5)]);
            c.path(&[(8.0, 17.0), (8.0, 11.5), (12.0, 11.5), (12.0, 17.0)]);
        }
        Recent => {
            c.path(&[(4.2, 6.3), (2.5, 9.0), (5.7, 9.0)]);
            c.path(&[
                (3.3, 9.0),
                (4.1, 6.5),
                (6.3, 4.5),
                (10.0, 3.5),
                (14.1, 5.0),
                (16.5, 8.5),
                (16.5, 12.0),
                (14.2, 15.6),
                (10.0, 16.5),
                (6.3, 15.1),
            ]);
            c.path(&[(10.0, 6.5), (10.0, 10.0), (12.8, 11.7)]);
        }
        Pin => {
            c.path(&[
                (7.0, 3.5),
                (13.0, 3.5),
                (12.5, 7.0),
                (15.3, 10.0),
                (15.3, 11.5),
                (4.7, 11.5),
                (4.7, 10.0),
                (7.5, 7.0),
                (7.0, 3.5),
            ]);
            c.line((10.0, 11.5), (10.0, 17.5));
        }
        ArrowBack => {
            c.path(&[(9.0, 4.0), (3.0, 10.0), (9.0, 16.0)]);
            c.line((3.0, 10.0), (17.0, 10.0));
        }
    }
}

/// Paint a code or data file icon: a colored page with a short white tag, such as `{ }` for
/// JSON. Unknown extensions fall back to the plain text icon.
pub fn text_document(painter: &Painter, rect: Rect, extension: &str) {
    let (base, fold, tag) = match extension {
        "json" => ((196, 132, 22), (240, 200, 120), "{ }"),
        "yaml" | "yml" => ((180, 62, 92), (232, 158, 178), "YML"),
        "py" => ((52, 102, 168), (150, 186, 232), "PY"),
        "js" => ((168, 140, 18), (232, 214, 120), "JS"),
        "sh" => ((44, 120, 72), (140, 202, 160), "SH"),
        "sql" => ((24, 128, 140), (124, 200, 208), "SQL"),
        "toml" => ((140, 92, 58), (214, 180, 150), "TOML"),
        "xml" => ((104, 118, 52), (186, 198, 138), "XML"),
        "ts" => ((40, 116, 196), (146, 188, 236), "TS"),
        "css" => ((68, 82, 186), (156, 166, 232), "CSS"),
        "go" => ((22, 140, 196), (132, 202, 232), "GO"),
        "rs" => ((176, 74, 44), (232, 162, 136), "RS"),
        _ => return document(painter, rect, DocumentKind::PlainText),
    };
    let rgb = |(r, g, b): (u8, u8, u8)| Color32::from_rgb(r, g, b);
    let c = Canvas {
        painter,
        rect,
        stroke: Stroke::NONE,
    };
    c.fill(
        &[
            (4.5, 1.5),
            (12.0, 1.5),
            (16.0, 5.5),
            (16.0, 18.5),
            (4.5, 18.5),
        ],
        rgb(base),
    );
    c.fill(&[(12.0, 1.5), (12.0, 5.5), (16.0, 5.5)], rgb(fold));
    painter.text(
        c.p(10.25, 12.2),
        egui::Align2::CENTER_CENTER,
        tag,
        // Four-letter tags shrink so they stay inside the page.
        egui::FontId::proportional(
            (rect.height() * if tag.len() > 3 { 0.2 } else { 0.24 }).max(6.0),
        ),
        Color32::WHITE,
    );
}

/// Paint a colored document icon. Its white marks remain legible on light and dark UI.
pub fn document(painter: &Painter, rect: Rect, kind: DocumentKind) {
    let (base, fold) = match kind {
        DocumentKind::Text => (
            Color32::from_rgb(47, 105, 202),
            Color32::from_rgb(143, 183, 239),
        ),
        DocumentKind::Spreadsheet => (
            Color32::from_rgb(28, 139, 97),
            Color32::from_rgb(133, 205, 169),
        ),
        DocumentKind::Presentation => (
            Color32::from_rgb(210, 105, 48),
            Color32::from_rgb(244, 186, 127),
        ),
        DocumentKind::Pdf => (
            Color32::from_rgb(191, 65, 69),
            Color32::from_rgb(238, 155, 156),
        ),
        DocumentKind::Markdown => (
            Color32::from_rgb(112, 84, 200),
            Color32::from_rgb(186, 170, 240),
        ),
        DocumentKind::PlainText => (
            Color32::from_rgb(92, 104, 120),
            Color32::from_rgb(170, 180, 194),
        ),
        DocumentKind::RichText => (
            Color32::from_rgb(0, 141, 150),
            Color32::from_rgb(128, 210, 214),
        ),
    };
    let c = Canvas {
        painter,
        rect,
        stroke: Stroke::new(
            (1.3 * rect.width().min(rect.height()) / 20.0).max(0.9),
            Color32::WHITE,
        ),
    };
    // The page silhouette and folded corner are original simple polygons.
    c.fill(
        &[
            (4.5, 1.5),
            (12.0, 1.5),
            (16.0, 5.5),
            (16.0, 18.5),
            (4.5, 18.5),
        ],
        base,
    );
    c.fill(&[(12.0, 1.5), (12.0, 5.5), (16.0, 5.5)], fold);
    match kind {
        DocumentKind::Text => {
            c.line((7.0, 8.0), (13.2, 8.0));
            c.line((7.0, 10.5), (13.2, 10.5));
            c.line((7.0, 13.0), (13.2, 13.0));
            c.line((7.0, 15.5), (11.0, 15.5));
        }
        DocumentKind::Spreadsheet => {
            c.box_outline(7.0, 8.0, 13.5, 15.7);
            c.line((10.2, 8.0), (10.2, 15.7));
            c.line((7.0, 10.6), (13.5, 10.6));
            c.line((7.0, 13.2), (13.5, 13.2));
        }
        DocumentKind::Presentation => {
            c.box_outline(6.7, 8.0, 13.8, 13.6);
            c.line((10.2, 13.6), (10.2, 16.0));
            c.line((7.8, 16.0), (12.6, 16.0));
            c.path(&[(8.0, 11.8), (9.5, 10.0), (10.8, 11.0), (12.4, 9.2)]);
        }
        DocumentKind::Pdf => {
            // Three flowing strokes give the small icon a PDF-like mark without tiny text.
            c.path(&[(8.5, 8.0), (9.6, 10.2), (8.3, 14.8)]);
            c.path(&[(9.6, 10.2), (12.8, 14.0), (13.5, 14.1)]);
            c.path(&[(8.3, 14.8), (10.7, 13.5), (13.3, 13.2)]);
        }
        DocumentKind::Markdown => {
            // An "M" with a down arrow beside it.
            c.path(&[
                (6.6, 14.6),
                (6.6, 9.0),
                (8.4, 11.4),
                (10.2, 9.0),
                (10.2, 14.6),
            ]);
            c.line((12.6, 9.0), (12.6, 14.4));
            c.path(&[(11.4, 13.0), (12.6, 14.4), (13.8, 13.0)]);
        }
        DocumentKind::PlainText => {
            // Angle brackets: code and text files.
            c.path(&[(9.0, 9.0), (6.8, 11.8), (9.0, 14.6)]);
            c.path(&[(11.6, 9.0), (13.8, 11.8), (11.6, 14.6)]);
        }
        DocumentKind::RichText => {
            // A picture over two lines of text: a laid-out article.
            c.box_outline(7.0, 7.8, 13.5, 11.6);
            c.path(&[(7.6, 11.0), (9.4, 9.4), (10.6, 10.4), (12.9, 8.6)]);
            c.line((7.0, 13.6), (13.5, 13.6));
            c.line((7.0, 15.8), (11.2, 15.8));
        }
    }
}

/// Frameless 28 pt click target with an 18 pt icon and accessible widget label.
/// `size` may increase the click target; callers can also use `paint` directly.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, size: f32) -> Response {
    let side = size.max(28.0);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let fill = if response.hovered() || response.has_focus() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect.shrink(1.0), CornerRadius::same(5), fill);
    }
    let ink = if response.is_pointer_button_down_on() {
        ui.visuals().widgets.active.fg_stroke.color
    } else if response.hovered() || response.has_focus() {
        ui.visuals().widgets.hovered.fg_stroke.color
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    let glyph = Rect::from_center_size(rect.center(), vec2(18.0, 18.0));
    paint(ui.painter(), glyph, icon, ink);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    response.on_hover_text(label)
}
