//! Computed styles: the browser's defaults for each tag, presentational attributes and the
//! inline `style` attribute, with CSS inheritance.
//!
//! Only the subset rich-text layout uses is understood (see the crate docs); other properties
//! are ignored, so a page using them is laid out as plain flow.

use crate::dom::{Document, Kind, NodeId};
use egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    InlineBlock,
    ListItem,
    Table,
    TableRowGroup,
    TableRow,
    TableCell,
    Flex,
    None,
}

impl Display {
    pub fn is_inline_level(self) -> bool {
        matches!(self, Display::Inline | Display::InlineBlock)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Auto,
    Px(f32),
    Percent(f32),
}

impl Length {
    /// The length in points, against a containing width.
    pub fn resolve(self, of: f32) -> Option<f32> {
        match self {
            Length::Auto => None,
            Length::Px(v) => Some(v),
            Length::Percent(p) => Some(of * p / 100.0),
        }
    }

    pub fn or_zero(self, of: f32) -> f32 {
        self.resolve(of).unwrap_or(0.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    Normal,
    Factor(f32),
    Px(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStyle {
    Disc,
    Circle,
    Square,
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Baseline,
    Super,
    Sub,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: f32,
    pub color: Color32,
    pub dashed: bool,
}

impl Border {
    pub const NONE: Border = Border {
        width: 0.0,
        color: Color32::TRANSPARENT,
        dashed: false,
    };
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    // Inherited.
    pub color: Color32,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    pub underline: bool,
    pub strike: bool,
    pub line_height: LineHeight,
    pub letter_spacing: f32,
    pub text_align: Align,
    pub text_indent: Length,
    /// `white-space: pre*`: keep spaces and line breaks.
    pub pre: bool,
    pub nowrap: bool,
    pub list_style: ListStyle,
    // Not inherited.
    pub display: Display,
    pub vertical_align: VAlign,
    pub background: Option<Color32>,
    /// Top, right, bottom, left.
    pub margin: [Length; 4],
    pub padding: [Length; 4],
    pub border: [Border; 4],
    pub radius: f32,
    pub width: Length,
    pub max_width: Length,
    pub height: Length,
    pub border_box: bool,
    pub flex_column: bool,
    pub justify: Justify,
    /// `align-items: center` (flex) / `vertical-align: middle` (table cells).
    pub align_center: bool,
    pub flex_grow: f32,
    pub gap: f32,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: Color32::from_rgb(0x22, 0x22, 0x22),
            font_size: 16.0,
            bold: false,
            italic: false,
            mono: false,
            underline: false,
            strike: false,
            line_height: LineHeight::Normal,
            letter_spacing: 0.0,
            text_align: Align::Left,
            text_indent: Length::Px(0.0),
            pre: false,
            nowrap: false,
            list_style: ListStyle::Disc,
            display: Display::Inline,
            vertical_align: VAlign::Baseline,
            background: None,
            margin: [Length::Px(0.0); 4],
            padding: [Length::Px(0.0); 4],
            border: [Border::NONE; 4],
            radius: 0.0,
            width: Length::Auto,
            max_width: Length::Auto,
            height: Length::Auto,
            border_box: false,
            flex_column: false,
            justify: Justify::Start,
            align_center: false,
            flex_grow: 0.0,
            gap: 0.0,
            opacity: 1.0,
        }
    }
}

impl Style {
    /// The style a child starts from: inherited properties kept, the rest reset.
    fn inherit(&self) -> Self {
        Self {
            color: self.color,
            font_size: self.font_size,
            bold: self.bold,
            italic: self.italic,
            mono: self.mono,
            underline: self.underline,
            strike: self.strike,
            line_height: self.line_height,
            letter_spacing: self.letter_spacing,
            text_align: self.text_align,
            text_indent: self.text_indent,
            pre: self.pre,
            nowrap: self.nowrap,
            list_style: self.list_style,
            ..Self::default()
        }
    }

    /// The line height in points.
    pub fn line_px(&self) -> f32 {
        match self.line_height {
            LineHeight::Normal => self.font_size * 1.4,
            LineHeight::Factor(f) => self.font_size * f,
            LineHeight::Px(px) => px,
        }
    }
}

/// The browser's default style sheet, for the tags that need one.
fn defaults(tag: &str) -> &'static str {
    match tag {
        "html" | "body" | "div" | "section" | "article" | "aside" | "header" | "footer" | "nav"
        | "main" | "address" | "details" | "summary" | "dl" | "dt" | "figcaption" | "center"
        | "form" | "fieldset" | "hgroup" | "legend" | "picture" => match tag {
            "body" => "display: block; margin: 8px",
            "center" => "display: block; text-align: center",
            _ => "display: block",
        },
        "p" => "display: block; margin: 1em 0",
        "h1" => "display: block; font-size: 2em; font-weight: bold; margin: 0.67em 0",
        "h2" => "display: block; font-size: 1.5em; font-weight: bold; margin: 0.83em 0",
        "h3" => "display: block; font-size: 1.17em; font-weight: bold; margin: 1em 0",
        "h4" => "display: block; font-weight: bold; margin: 1.33em 0",
        "h5" => "display: block; font-size: 0.83em; font-weight: bold; margin: 1.67em 0",
        "h6" => "display: block; font-size: 0.67em; font-weight: bold; margin: 2.33em 0",
        "blockquote" => "display: block; margin: 1em 40px",
        "figure" => "display: block; margin: 1em 40px",
        "dd" => "display: block; margin-left: 40px",
        "ul" | "menu" => "display: block; margin: 1em 0; padding-left: 40px; list-style-type: disc",
        "ol" => "display: block; margin: 1em 0; padding-left: 40px; list-style-type: decimal",
        "li" => "display: list-item",
        "pre" | "listing" | "xmp" => {
            "display: block; font-family: monospace; white-space: pre; margin: 1em 0"
        }
        "code" | "kbd" | "samp" | "tt" => "font-family: monospace",
        "b" | "strong" => "font-weight: bold",
        "i" | "em" | "cite" | "var" | "dfn" => "font-style: italic",
        "u" | "ins" => "text-decoration: underline",
        "s" | "strike" | "del" => "text-decoration: line-through",
        "a" => "color: #576b95; text-decoration: none",
        "mark" => "background-color: #ffeb3b; color: black",
        "small" => "font-size: 0.83em",
        "big" => "font-size: 1.2em",
        "sup" => "vertical-align: super; font-size: 0.75em",
        "sub" => "vertical-align: sub; font-size: 0.75em",
        "hr" => "display: block; margin: 0.5em 0; border-top: 1px solid #cccccc",
        "table" => "display: table; border-spacing: 2px",
        "thead" | "tbody" | "tfoot" => "display: table-row-group",
        "tr" => "display: table-row",
        "td" => "display: table-cell; padding: 1px",
        "th" => "display: table-cell; padding: 1px; font-weight: bold; text-align: center",
        "caption" => "display: block; text-align: center",
        "img" | "video" | "svg" | "canvas" | "iframe" | "input" | "button" | "select"
        | "textarea" => "display: inline-block",
        "head" | "script" | "style" | "title" | "meta" | "link" | "base" | "template"
        | "noscript" | "datalist" | "param" | "source" | "track" | "area" | "map" => {
            "display: none"
        }
        _ => "",
    }
}

/// Declarations as `(property, value)` pairs, lower-case properties, `!important` dropped.
pub fn declarations(css: &str) -> Vec<(String, String)> {
    css.split(';')
        .filter_map(|item| {
            let (key, value) = item.split_once(':')?;
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().trim_end_matches("!important").trim();
            (!key.is_empty() && !value.is_empty()).then(|| (key, value.to_string()))
        })
        .collect()
}

/// Styles for every node, computed once per document (indexed by node id).
pub fn compute_all(doc: &Document) -> Vec<Style> {
    let mut styles = vec![Style::default(); doc.nodes.len()];
    let root = Style {
        display: Display::Block,
        ..Style::default()
    };
    styles[crate::dom::ROOT] = root;
    // Parents come before their children in the arena only when built by the parser; walk
    // the tree instead so edited documents work too.
    let mut stack = vec![crate::dom::ROOT];
    while let Some(id) = stack.pop() {
        for &child in doc.children(id) {
            styles[child] = compute(doc, child, &styles[id]);
            stack.push(child);
        }
    }
    styles
}

pub fn compute(doc: &Document, id: NodeId, parent: &Style) -> Style {
    let mut style = parent.inherit();
    let Kind::Element { name, attrs } = &doc.node(id).kind else {
        // Text and other nodes: inline, inheriting everything.
        style.display = match doc.node(id).kind {
            Kind::Text(_) => Display::Inline,
            _ => Display::None,
        };
        return style;
    };
    let mut list = declarations(defaults(name));
    // Presentational attributes, weaker than the style attribute.
    for (key, value) in attrs {
        match (key.as_str(), name.as_str()) {
            ("align", "img") => {}
            ("align", _) => list.push(("text-align".into(), value.clone())),
            ("bgcolor", _) => list.push(("background-color".into(), value.clone())),
            ("color", "font") => list.push(("color".into(), value.clone())),
            ("size", "font") => {
                let size = match value.trim() {
                    "1" => "10px",
                    "2" => "13px",
                    "3" => "16px",
                    "4" => "18px",
                    "5" => "24px",
                    "6" => "32px",
                    "7" => "48px",
                    _ => "",
                };
                if !size.is_empty() {
                    list.push(("font-size".into(), size.into()));
                }
            }
            ("face", "font") => list.push(("font-family".into(), value.clone())),
            ("width", "img" | "td" | "th" | "table" | "col" | "video" | "iframe" | "hr") => {
                list.push(("width".into(), attr_length(value)))
            }
            ("height", "img" | "video" | "iframe") => {
                list.push(("height".into(), attr_length(value)))
            }
            ("border", "table") if value.trim() != "0" => list.push((
                "border".into(),
                format!("{}px solid #888888", attr_number(value)),
            )),
            ("hidden", _) => list.push(("display".into(), "none".into())),
            _ => {}
        }
    }
    if let Some((_, css)) = attrs.iter().find(|(k, _)| k == "style") {
        list.extend(declarations(css));
    }
    // Font size first: `em` lengths in the other properties are relative to it.
    for (key, value) in &list {
        if key == "font-size" {
            if let Some(size) = font_size(value, parent.font_size) {
                style.font_size = size;
            }
        } else if key == "font"
            && let Some(size) = value.split_whitespace().find_map(|w| {
                let w = w.split('/').next().unwrap_or(w);
                w.chars()
                    .next()
                    .filter(|c| c.is_ascii_digit())
                    .and_then(|_| font_size(w, parent.font_size))
            })
        {
            style.font_size = size;
        }
    }
    for (key, value) in &list {
        apply(&mut style, key, value, parent);
    }
    // A table cell in a table with a border gets one too.
    if matches!(name.as_str(), "td" | "th")
        && let Some(table) = ancestor(doc, id, "table")
        && doc.attr(table, "border").is_some_and(|b| b.trim() != "0")
        && style.border.iter().all(|b| b.width == 0.0)
    {
        style.border = [Border {
            width: 1.0,
            color: Color32::from_gray(0x88),
            dashed: false,
        }; 4];
    }
    style
}

fn ancestor(doc: &Document, id: NodeId, name: &str) -> Option<NodeId> {
    let mut at = doc.node(id).parent;
    while let Some(id) = at {
        if doc.name(id) == Some(name) {
            return Some(id);
        }
        at = doc.node(id).parent;
    }
    None
}

fn attr_number(value: &str) -> String {
    value.trim().trim_end_matches("px").to_string()
}

fn attr_length(value: &str) -> String {
    let value = value.trim();
    if value.ends_with('%') {
        value.to_string()
    } else {
        format!("{}px", attr_number(value))
    }
}

fn apply(style: &mut Style, key: &str, value: &str, parent: &Style) {
    let em = style.font_size;
    let lower = value.to_ascii_lowercase();
    let value = lower.as_str();
    match key {
        "display" => {
            style.display = match value {
                "block" | "flow-root" => Display::Block,
                "inline" => Display::Inline,
                "inline-block" | "inline-flex" | "inline-table" => Display::InlineBlock,
                "list-item" => Display::ListItem,
                "table" => Display::Table,
                "table-row-group" | "table-header-group" | "table-footer-group" => {
                    Display::TableRowGroup
                }
                "table-row" => Display::TableRow,
                "table-cell" => Display::TableCell,
                "flex" => Display::Flex,
                "grid" => Display::Block,
                "none" => Display::None,
                "contents" => Display::Inline,
                _ => style.display,
            }
        }
        "color" => {
            if let Some(color) = color(value) {
                style.color = color;
            } else if value == "inherit" || value == "currentcolor" {
                style.color = parent.color;
            }
        }
        "background-color" | "background" | "background-image" => {
            if let Some(color) = first_color(value) {
                style.background = (color.a() > 0).then_some(color);
            } else if value == "none" || value == "transparent" {
                style.background = None;
            }
        }
        "font-weight" => {
            style.bold = match value {
                "bold" | "bolder" => true,
                "normal" | "lighter" => false,
                n => n.parse::<u32>().map(|w| w >= 600).unwrap_or(style.bold),
            }
        }
        "font-style" => style.italic = value == "italic" || value == "oblique",
        "font-family" => style.mono = is_monospace(value),
        "font" => {
            for word in value.split_whitespace() {
                match word {
                    "bold" | "bolder" | "600" | "700" | "800" | "900" => style.bold = true,
                    "italic" | "oblique" => style.italic = true,
                    _ => {}
                }
                if let Some((_, line)) = word.split_once('/') {
                    apply(style, "line-height", line, parent);
                }
            }
            style.mono = is_monospace(value);
        }
        "text-decoration" | "text-decoration-line" => {
            if value.contains("none") {
                style.underline = false;
                style.strike = false;
            }
            if value.contains("underline") {
                style.underline = true;
            }
            if value.contains("line-through") {
                style.strike = true;
            }
        }
        "line-height" => {
            style.line_height = if value == "normal" {
                LineHeight::Normal
            } else if let Ok(factor) = value.parse::<f32>() {
                LineHeight::Factor(factor)
            } else {
                match length(value, em) {
                    Some(Length::Px(px)) => LineHeight::Px(px),
                    Some(Length::Percent(p)) => LineHeight::Px(em * p / 100.0),
                    _ => style.line_height,
                }
            }
        }
        "letter-spacing" => {
            style.letter_spacing = match length(value, em) {
                Some(Length::Px(px)) => px,
                _ => 0.0,
            }
        }
        "text-align" => {
            style.text_align = match value {
                "center" | "-webkit-center" => Align::Center,
                "right" | "end" | "-webkit-right" => Align::Right,
                "justify" => Align::Justify,
                "left" | "start" | "-webkit-left" => Align::Left,
                _ => style.text_align,
            }
        }
        "text-indent" => {
            if let Some(len) = length(value, em) {
                style.text_indent = len;
            }
        }
        "white-space" => {
            style.pre = value.starts_with("pre") || value == "break-spaces";
            style.nowrap = value == "nowrap" || value == "pre";
        }
        "list-style-type" | "list-style" => {
            for word in value.split_whitespace() {
                style.list_style = match word {
                    "disc" => ListStyle::Disc,
                    "circle" => ListStyle::Circle,
                    "square" => ListStyle::Square,
                    "decimal" | "decimal-leading-zero" | "cjk-decimal" => ListStyle::Decimal,
                    "lower-alpha" | "lower-latin" => ListStyle::LowerAlpha,
                    "upper-alpha" | "upper-latin" => ListStyle::UpperAlpha,
                    "lower-roman" => ListStyle::LowerRoman,
                    "upper-roman" => ListStyle::UpperRoman,
                    "none" => ListStyle::None,
                    _ => continue,
                };
            }
        }
        "vertical-align" => {
            if value == "middle" {
                style.align_center = true;
            }
            style.vertical_align = match value {
                "super" | "top" | "text-top" => VAlign::Super,
                "sub" | "bottom" | "text-bottom" => VAlign::Sub,
                _ => VAlign::Baseline,
            }
        }
        "margin" => {
            if let Some(sides) = sides(value, em) {
                style.margin = sides;
            }
        }
        "padding" => {
            if let Some(sides) = sides(value, em) {
                style.padding = sides;
            }
        }
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
            if let Some(len) = length(value, em) {
                style.margin[side_index(key)] = len;
            }
        }
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            if let Some(len) = length(value, em) {
                style.padding[side_index(key)] = len;
            }
        }
        "border" => style.border = [border(value, em, style.color); 4],
        "border-top" | "border-right" | "border-bottom" | "border-left" => {
            style.border[side_index(key)] = border(value, em, style.color);
        }
        "border-width" => {
            if let Some(widths) = sides(value, em) {
                for (b, w) in style.border.iter_mut().zip(widths) {
                    b.width = w.or_zero(0.0);
                    if b.color == Color32::TRANSPARENT {
                        b.color = style.color;
                    }
                }
            }
        }
        "border-color" => {
            if let Some(c) = color(value.split_whitespace().next().unwrap_or("")) {
                for b in &mut style.border {
                    b.color = c;
                }
            }
        }
        "border-style" => {
            if value.starts_with("none") || value.starts_with("hidden") {
                style.border = [Border::NONE; 4];
            } else {
                for b in &mut style.border {
                    b.dashed = value.starts_with("dashed") || value.starts_with("dotted");
                    if b.width == 0.0 {
                        b.width = 3.0;
                        b.color = style.color;
                    }
                }
            }
        }
        "border-radius" => {
            if let Some(Length::Px(px)) =
                value.split_whitespace().next().and_then(|v| length(v, em))
            {
                style.radius = px;
            } else if let Some(Length::Percent(p)) =
                value.split_whitespace().next().and_then(|v| length(v, em))
            {
                // Resolved against the box's width at layout; store a large radius for 50%.
                style.radius = if p >= 50.0 { 9999.0 } else { p };
            }
        }
        "width" => {
            if let Some(len) = length(value, em) {
                style.width = len;
            } else if value == "auto" {
                style.width = Length::Auto;
            }
        }
        "max-width" => {
            if let Some(len) = length(value, em) {
                style.max_width = len;
            }
        }
        "height" => {
            if let Some(len) = length(value, em) {
                style.height = len;
            }
        }
        "box-sizing" => style.border_box = value == "border-box",
        "flex-direction" => style.flex_column = value.starts_with("column"),
        "justify-content" => {
            style.justify = match value {
                "center" => Justify::Center,
                "flex-end" | "end" | "right" => Justify::End,
                "space-between" => Justify::SpaceBetween,
                "space-around" | "space-evenly" => Justify::SpaceAround,
                _ => Justify::Start,
            }
        }
        "align-items" => style.align_center = value == "center",
        "flex" => {
            style.flex_grow = match value {
                "none" => 0.0,
                "auto" => 1.0,
                _ => value
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<f32>().ok())
                    .unwrap_or(0.0),
            }
        }
        "flex-grow" => style.flex_grow = value.parse().unwrap_or(0.0),
        "gap" | "column-gap" => {
            if let Some(Length::Px(px)) = length(value.split_whitespace().last().unwrap_or(""), em)
            {
                style.gap = px;
            }
        }
        "opacity" => {
            if let Ok(o) = value.parse::<f32>() {
                style.opacity = o.clamp(0.0, 1.0);
            }
        }
        "visibility" if value == "hidden" => style.opacity = 0.0,
        _ => {}
    }
}

fn side_index(key: &str) -> usize {
    if key.ends_with("top") {
        0
    } else if key.ends_with("right") {
        1
    } else if key.ends_with("bottom") {
        2
    } else {
        3
    }
}

fn is_monospace(family: &str) -> bool {
    let family = family.to_ascii_lowercase();
    [
        "monospace",
        "courier",
        "consolas",
        "menlo",
        "monaco",
        "mono",
    ]
    .iter()
    .any(|m| family.contains(m))
}

/// `1px 2px`, `1px 2px 3px`, … to top, right, bottom, left.
fn sides(value: &str, em: f32) -> Option<[Length; 4]> {
    let parts: Vec<Length> = value
        .split_whitespace()
        .map(|v| length(v, em).or((v == "auto").then_some(Length::Auto)))
        .collect::<Option<_>>()?;
    Some(match parts.as_slice() {
        [a] => [*a; 4],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d, ..] => [*a, *b, *c, *d],
        [] => return None,
    })
}

fn border(value: &str, em: f32, current: Color32) -> Border {
    let mut border = Border {
        width: 3.0,
        color: current,
        dashed: false,
    };
    let mut styled = false;
    for word in split_css_words(value) {
        match word.as_str() {
            "none" | "hidden" => return Border::NONE,
            "solid" | "double" | "groove" | "ridge" | "inset" | "outset" => styled = true,
            "dashed" | "dotted" => {
                styled = true;
                border.dashed = true;
            }
            "thin" => border.width = 1.0,
            "medium" => border.width = 3.0,
            "thick" => border.width = 5.0,
            _ => {
                if let Some(Length::Px(px)) = length(&word, em) {
                    border.width = px;
                } else if let Some(c) = color(&word) {
                    border.color = c;
                }
            }
        }
    }
    if !styled || border.width <= 0.0 {
        return Border::NONE;
    }
    border
}

/// Words of a CSS value, keeping `rgb(…)` together.
fn split_css_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    for c in value.chars() {
        match c {
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                depth -= 1;
                current.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn font_size(value: &str, parent: f32) -> Option<f32> {
    let value = value.trim().to_ascii_lowercase();
    Some(match value.as_str() {
        "xx-small" => 9.0,
        "x-small" => 10.0,
        "small" => 13.0,
        "medium" => 16.0,
        "large" => 18.0,
        "x-large" => 24.0,
        "xx-large" => 32.0,
        "smaller" => parent / 1.2,
        "larger" => parent * 1.2,
        _ => match length(&value, parent)? {
            Length::Px(px) => px,
            Length::Percent(p) => parent * p / 100.0,
            Length::Auto => return None,
        },
    })
    .map(|size| size.clamp(1.0, 200.0))
}

/// A CSS length; `em` is the font size `em` units refer to.
pub fn length(value: &str, em: f32) -> Option<Length> {
    let value = value.trim();
    if value == "0" {
        return Some(Length::Px(0.0));
    }
    if value == "auto" {
        return Some(Length::Auto);
    }
    let split = value
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(value.len());
    let number: f32 = value[..split].parse().ok()?;
    let unit = value[split..].trim();
    Some(match unit {
        "px" | "" => Length::Px(number),
        "pt" => Length::Px(number * 4.0 / 3.0),
        "em" => Length::Px(number * em),
        "rem" => Length::Px(number * 16.0),
        "%" => Length::Percent(number),
        "vw" => Length::Percent(number),
        "pc" => Length::Px(number * 16.0),
        "in" => Length::Px(number * 96.0),
        "cm" => Length::Px(number * 96.0 / 2.54),
        "mm" => Length::Px(number * 96.0 / 25.4),
        "ex" | "ch" => Length::Px(number * em * 0.5),
        _ => return None,
    })
}

/// The first color in a value (`background: url(x) #fff`, gradients).
fn first_color(value: &str) -> Option<Color32> {
    if let Some(c) = color(value) {
        return Some(c);
    }
    // Inside gradients the colors are comma-separated arguments.
    for word in value.split([' ', ',', '(', ')']).filter(|w| !w.is_empty()) {
        if let Some(c) = color(word) {
            return Some(c);
        }
    }
    let mut rest = value;
    while let Some(at) = rest.find("rgb") {
        let tail = &rest[at..];
        if let Some(end) = tail.find(')')
            && let Some(c) = color(&tail[..=end])
        {
            return Some(c);
        }
        rest = &rest[at + 3..];
    }
    None
}

pub fn color(value: &str) -> Option<Color32> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<_>>()?;
        return Some(match digits.len() {
            3 => Color32::from_rgb(digits[0] * 17, digits[1] * 17, digits[2] * 17),
            4 => Color32::from_rgba_unmultiplied(
                digits[0] * 17,
                digits[1] * 17,
                digits[2] * 17,
                digits[3] * 17,
            ),
            6 => Color32::from_rgb(
                digits[0] * 16 + digits[1],
                digits[2] * 16 + digits[3],
                digits[4] * 16 + digits[5],
            ),
            8 => Color32::from_rgba_unmultiplied(
                digits[0] * 16 + digits[1],
                digits[2] * 16 + digits[3],
                digits[4] * 16 + digits[5],
                digits[6] * 16 + digits[7],
            ),
            _ => return None,
        });
    }
    if let Some(args) = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))
        .and_then(|v| v.strip_suffix(')'))
    {
        let parts: Vec<&str> = args
            .split([',', ' ', '/'])
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() < 3 {
            return None;
        }
        let channel = |p: &str| -> Option<u8> {
            if let Some(percent) = p.strip_suffix('%') {
                percent
                    .parse::<f32>()
                    .ok()
                    .map(|v| (v * 2.55).round().clamp(0.0, 255.0) as u8)
            } else {
                p.parse::<f32>()
                    .ok()
                    .map(|v| v.round().clamp(0.0, 255.0) as u8)
            }
        };
        let alpha = match parts.get(3) {
            Some(a) => match a.strip_suffix('%') {
                Some(p) => p.parse::<f32>().ok()? / 100.0,
                None => a.parse::<f32>().ok()?,
            },
            None => 1.0,
        };
        return Some(Color32::from_rgba_unmultiplied(
            channel(parts[0])?,
            channel(parts[1])?,
            channel(parts[2])?,
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
        ));
    }
    Some(match value.as_str() {
        "black" => Color32::BLACK,
        "white" => Color32::WHITE,
        "red" => Color32::from_rgb(255, 0, 0),
        "green" => Color32::from_rgb(0, 128, 0),
        "blue" => Color32::from_rgb(0, 0, 255),
        "yellow" => Color32::from_rgb(255, 255, 0),
        "orange" => Color32::from_rgb(255, 165, 0),
        "purple" => Color32::from_rgb(128, 0, 128),
        "pink" => Color32::from_rgb(255, 192, 203),
        "gray" | "grey" => Color32::from_rgb(128, 128, 128),
        "silver" => Color32::from_rgb(192, 192, 192),
        "lightgray" | "lightgrey" => Color32::from_rgb(211, 211, 211),
        "darkgray" | "darkgrey" => Color32::from_rgb(169, 169, 169),
        "navy" => Color32::from_rgb(0, 0, 128),
        "teal" => Color32::from_rgb(0, 128, 128),
        "maroon" => Color32::from_rgb(128, 0, 0),
        "olive" => Color32::from_rgb(128, 128, 0),
        "lime" => Color32::from_rgb(0, 255, 0),
        "aqua" | "cyan" => Color32::from_rgb(0, 255, 255),
        "fuchsia" | "magenta" => Color32::from_rgb(255, 0, 255),
        "brown" => Color32::from_rgb(165, 42, 42),
        "gold" => Color32::from_rgb(255, 215, 0),
        "skyblue" => Color32::from_rgb(135, 206, 235),
        "tomato" => Color32::from_rgb(255, 99, 71),
        "crimson" => Color32::from_rgb(220, 20, 60),
        "coral" => Color32::from_rgb(255, 127, 80),
        "whitesmoke" => Color32::from_rgb(245, 245, 245),
        "gainsboro" => Color32::from_rgb(220, 220, 220),
        "beige" => Color32::from_rgb(245, 245, 220),
        "ivory" => Color32::from_rgb(255, 255, 240),
        "transparent" => Color32::TRANSPARENT,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styled(html: &str, tag: &str) -> Style {
        let doc = Document::parse(html);
        let styles = compute_all(&doc);
        styles[doc.find(tag).unwrap()].clone()
    }

    #[test]
    fn colors() {
        assert_eq!(color("#fff"), Some(Color32::WHITE));
        assert_eq!(color("#FF6827"), Some(Color32::from_rgb(255, 104, 39)));
        assert_eq!(color("rgb(1, 2, 3)"), Some(Color32::from_rgb(1, 2, 3)));
        assert_eq!(color("rgba(0,0,0,0)"), Some(Color32::TRANSPARENT));
        assert_eq!(color("rgb(255 0 0 / 50%)").map(|c| c.a()), Some(128));
        assert_eq!(color("nope"), None);
        assert_eq!(
            first_color("linear-gradient(90deg, #ff0000, blue)"),
            Some(Color32::from_rgb(255, 0, 0))
        );
        assert_eq!(
            first_color("url(a.png) no-repeat rgb(9, 9, 9)"),
            Some(Color32::from_rgb(9, 9, 9))
        );
    }

    #[test]
    fn lengths_and_shorthands() {
        assert_eq!(length("12pt", 16.0), Some(Length::Px(16.0)));
        assert_eq!(length("1.5em", 10.0), Some(Length::Px(15.0)));
        assert_eq!(length("50%", 10.0), Some(Length::Percent(50.0)));
        assert_eq!(length("bogus", 10.0), None);
        assert_eq!(
            sides("1px 2px 3px", 16.0),
            Some([
                Length::Px(1.0),
                Length::Px(2.0),
                Length::Px(3.0),
                Length::Px(2.0)
            ])
        );
        assert_eq!(sides("0 auto", 16.0).unwrap()[1], Length::Auto);
    }

    #[test]
    fn inheritance_and_defaults() {
        let s = styled(
            "<section style=\"color: #ff0000; font-size: 20px; text-align: center; background: #eee\"><p><span>x</span></p></section>",
            "span",
        );
        assert_eq!(s.color, Color32::from_rgb(255, 0, 0));
        assert_eq!(s.font_size, 20.0);
        assert_eq!(s.text_align, Align::Center);
        assert_eq!(s.background, None, "background is not inherited");
        let p = styled("<p style=\"font-size: 20px; margin: 1em 2em\">x</p>", "p");
        assert_eq!(p.display, Display::Block);
        assert_eq!(
            p.margin,
            [
                Length::Px(20.0),
                Length::Px(40.0),
                Length::Px(20.0),
                Length::Px(40.0)
            ]
        );
        let h2 = styled("<h2>x</h2>", "h2");
        assert!(h2.bold);
        assert_eq!(h2.font_size, 24.0);
    }

    #[test]
    fn borders() {
        let s = styled(
            "<p style=\"border-left: 3px solid rgb(255, 104, 39); border-radius: 6px\">x</p>",
            "p",
        );
        assert_eq!(s.border[3].width, 3.0);
        assert_eq!(s.border[3].color, Color32::from_rgb(255, 104, 39));
        assert_eq!(s.border[0], Border::NONE);
        assert_eq!(s.radius, 6.0);
        let dashed = styled(
            "<hr style=\"border: 0; border-top: 1px dashed #ccc\">",
            "hr",
        );
        assert!(dashed.border[0].dashed);
        assert_eq!(dashed.border[2], Border::NONE);
        let no_style = styled("<p style=\"border: 1px #000\">x</p>", "p");
        assert_eq!(
            no_style.border[0],
            Border::NONE,
            "a border without a style draws nothing"
        );
    }

    #[test]
    fn presentational_attributes() {
        let s = styled("<font color=\"red\" size=\"5\">x</font>", "font");
        assert_eq!(s.color, Color32::from_rgb(255, 0, 0));
        assert_eq!(s.font_size, 24.0);
        let img = styled("<img width=\"120\" height=\"50%\">", "img");
        assert_eq!(img.width, Length::Px(120.0));
        assert_eq!(img.display, Display::InlineBlock);
        let td = styled("<table border=1><tr><td>x</td></tr></table>", "td");
        assert_eq!(td.border[0].width, 1.0);
    }

    #[test]
    fn hidden_things() {
        assert_eq!(
            styled("<head><title>x</title></head>", "head").display,
            Display::None
        );
        assert_eq!(
            styled("<p style=\"display:none\">x</p>", "p").display,
            Display::None
        );
        assert_eq!(styled("<div hidden>x</div>", "div").display, Display::None);
    }
}
