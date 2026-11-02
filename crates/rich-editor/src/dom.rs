//! The document: a faithful HTML tree.
//!
//! The parser is tolerant (tag soup in, a tree out) and keeps what it does not understand:
//! unknown elements and attributes, comments, the doctype, `<head>`, and the raw contents of
//! `<script>`, `<style>`, `<textarea>` and `<title>`. Writing the tree back gives equivalent
//! HTML; a document nobody edited is saved from its original text, so its bytes never change.

use std::fmt::Write as _;

pub type NodeId = usize;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// The tree's root; its children are the top-level nodes of the file.
    Root,
    Element {
        /// Lower-case tag name.
        name: String,
        /// Attributes in source order; names lower-case, values decoded.
        attrs: Vec<(String, String)>,
    },
    /// Decoded text.
    Text(String),
    Comment(String),
    /// `<!DOCTYPE …>` or another `<!…>` declaration, kept verbatim (without the brackets).
    Declaration(String),
    /// The undecoded contents of a raw-text element (`<script>`, `<style>`, …).
    Raw(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub nodes: Vec<Node>,
}

pub const ROOT: NodeId = 0;

/// Elements that never have content.
pub fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Elements whose content is text up to their end tag.
fn is_raw_text(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "textarea" | "title" | "xmp" | "noscript"
    )
}

/// Starting one of these closes an open `<p>`.
fn closes_paragraph(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "details"
            | "div"
            | "dl"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "ul"
    )
}

impl Document {
    pub fn new() -> Self {
        Self {
            nodes: vec![Node {
                kind: Kind::Root,
                parent: None,
                children: Vec::new(),
            }],
        }
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    /// The tag name of an element, `None` for other nodes.
    pub fn name(&self, id: NodeId) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn attr(&self, id: NodeId, key: &str) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { attrs, .. } => attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.nodes[id].children
    }

    pub fn push(&mut self, parent: NodeId, kind: Kind) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind,
            parent: Some(parent),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        id
    }

    /// Insert a new node as child `index` of `parent`.
    pub fn insert(&mut self, parent: NodeId, index: usize, kind: Kind) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind,
            parent: Some(parent),
            children: Vec::new(),
        });
        let index = index.min(self.nodes[parent].children.len());
        self.nodes[parent].children.insert(index, id);
        id
    }

    /// Take a node out of the tree (it stays in the arena, unreachable).
    pub fn detach(&mut self, id: NodeId) {
        if let Some(parent) = self.nodes[id].parent.take() {
            self.nodes[parent].children.retain(|&c| c != id);
        }
    }

    /// Put a detached node back as child `index` of `parent`.
    pub fn attach(&mut self, id: NodeId, parent: NodeId, index: usize) {
        self.detach(id);
        let index = index.min(self.nodes[parent].children.len());
        self.nodes[parent].children.insert(index, id);
        self.nodes[id].parent = Some(parent);
    }

    pub fn index_in_parent(&self, id: NodeId) -> Option<usize> {
        let parent = self.nodes[id].parent?;
        self.nodes[parent].children.iter().position(|&c| c == id)
    }

    /// Child indices from the root to `id`.
    pub fn path(&self, id: NodeId) -> Vec<usize> {
        let mut path = Vec::new();
        let mut at = id;
        while let Some(index) = self.index_in_parent(at) {
            path.push(index);
            at = self.nodes[at].parent.unwrap_or(ROOT);
        }
        path.reverse();
        path
    }

    pub fn at_path(&self, path: &[usize]) -> Option<NodeId> {
        let mut at = ROOT;
        for &index in path {
            at = *self.nodes[at].children.get(index)?;
        }
        Some(at)
    }

    /// Whether `ancestor` contains `id` (or is it).
    pub fn contains(&self, ancestor: NodeId, id: NodeId) -> bool {
        let mut at = Some(id);
        while let Some(node) = at {
            if node == ancestor {
                return true;
            }
            at = self.nodes[node].parent;
        }
        false
    }

    pub fn text(&self, id: NodeId) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn text_mut(&mut self, id: NodeId) -> Option<&mut String> {
        match &mut self.nodes[id].kind {
            Kind::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn set_attr(&mut self, id: NodeId, key: &str, value: Option<&str>) {
        if let Kind::Element { attrs, .. } = &mut self.nodes[id].kind {
            match value {
                Some(value) => match attrs.iter_mut().find(|(k, _)| k == key) {
                    Some(slot) => slot.1 = value.to_string(),
                    None => attrs.push((key.to_string(), value.to_string())),
                },
                None => attrs.retain(|(k, _)| k != key),
            }
        }
    }

    pub fn rename(&mut self, id: NodeId, new_name: &str) {
        if let Kind::Element { name, .. } = &mut self.nodes[id].kind {
            *name = new_name.to_string();
        }
    }

    /// A new, detached element with the same name and attributes as `id`.
    pub fn clone_shallow(&mut self, id: NodeId) -> NodeId {
        let kind = self.nodes[id].kind.clone();
        let new = self.nodes.len();
        self.nodes.push(Node {
            kind,
            parent: None,
            children: Vec::new(),
        });
        new
    }

    /// A new, detached node.
    pub fn create(&mut self, kind: Kind) -> NodeId {
        let new = self.nodes.len();
        self.nodes.push(Node {
            kind,
            parent: None,
            children: Vec::new(),
        });
        new
    }

    /// Copy the subtree at `id` of `other` into this arena, detached.
    pub fn import(&mut self, other: &Document, id: NodeId) -> NodeId {
        let new = self.create(other.nodes[id].kind.clone());
        for &child in &other.nodes[id].children {
            let copy = self.import(other, child);
            self.nodes[copy].parent = Some(new);
            self.nodes[new].children.push(copy);
        }
        new
    }

    /// Nodes in document order (depth first) under `id`, `id` included.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(node) = stack.pop() {
            out.push(node);
            stack.extend(self.nodes[node].children.iter().rev());
        }
        out
    }

    /// The first element named `name`, depth first.
    pub fn find(&self, name: &str) -> Option<NodeId> {
        let mut stack = vec![ROOT];
        while let Some(id) = stack.pop() {
            if self.name(id) == Some(name) {
                return Some(id);
            }
            stack.extend(self.nodes[id].children.iter().rev());
        }
        None
    }

    /// Where the visible page starts: `<body>`, else the whole file.
    pub fn body(&self) -> NodeId {
        self.find("body").unwrap_or(ROOT)
    }

    /// The `<title>` text.
    pub fn title(&self) -> Option<String> {
        let title = self.find("title")?;
        let text: String = self.nodes[title]
            .children
            .iter()
            .filter_map(|&c| match &self.nodes[c].kind {
                Kind::Raw(t) | Kind::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        let text = decode_entities(text.trim());
        (!text.is_empty()).then_some(text)
    }

    /// Whether the page relies on things the design view does not apply.
    pub fn has_scripts_or_stylesheets(&self) -> bool {
        self.nodes.iter().enumerate().any(|(id, _)| {
            matches!(self.name(id), Some("script") | Some("style"))
                || (self.name(id) == Some("link")
                    && self
                        .attr(id, "rel")
                        .is_some_and(|r| r.eq_ignore_ascii_case("stylesheet")))
        })
    }

    pub fn parse(source: &str) -> Self {
        Parser::new(source).run()
    }

    /// HTML for the whole tree.
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for &child in &self.nodes[ROOT].children {
            self.write(child, &mut out);
        }
        out
    }

    /// HTML for one node and its subtree.
    pub fn serialize_node(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.write(id, &mut out);
        out
    }

    fn write(&self, id: NodeId, out: &mut String) {
        let node = &self.nodes[id];
        match &node.kind {
            Kind::Root => {
                for &child in &node.children {
                    self.write(child, out);
                }
            }
            Kind::Text(text) => escape_text(text, out),
            Kind::Raw(text) => out.push_str(text),
            Kind::Comment(text) => {
                let _ = write!(out, "<!--{text}-->");
            }
            Kind::Declaration(text) => {
                let _ = write!(out, "<!{text}>");
            }
            Kind::Element { name, attrs } => {
                out.push('<');
                out.push_str(name);
                for (key, value) in attrs {
                    out.push(' ');
                    out.push_str(key);
                    out.push_str("=\"");
                    escape_attr(value, out);
                    out.push('"');
                }
                out.push('>');
                if is_void(name) {
                    return;
                }
                for &child in &node.children {
                    self.write(child, out);
                }
                let _ = write!(out, "</{name}>");
            }
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            _ => out.push(c),
        }
    }
}

fn escape_attr(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            _ => out.push(c),
        }
    }
}

/// Named entities a document is likely to use; others stay as written.
fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "middot" => '·',
        "bull" => '•',
        "times" => '×',
        "divide" => '÷',
        "deg" => '°',
        "plusmn" => '±',
        "yen" => '¥',
        "euro" => '€',
        "pound" => '£',
        "cent" => '¢',
        "sect" => '§',
        "para" => '¶',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "hearts" => '♥',
        "star" => '☆',
        "check" => '✓',
        "zwj" => '\u{200d}',
        "zwnj" => '\u{200c}',
        "shy" => '\u{ad}',
        _ => return None,
    })
}

pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after
            .char_indices()
            .take(32)
            .find(|(_, c)| *c == ';')
            .map(|(i, _)| i);
        let decoded = end.and_then(|end| {
            let name = &after[..end];
            let c = if let Some(number) = name.strip_prefix('#') {
                let value = match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => number.parse().ok(),
                };
                value.and_then(char::from_u32)
            } else {
                named_entity(name)
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

struct Parser<'a> {
    source: &'a str,
    at: usize,
    doc: Document,
    /// Open elements, innermost last.
    stack: Vec<NodeId>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        let source = source.strip_prefix('\u{feff}').unwrap_or(source);
        Self {
            source,
            at: 0,
            doc: Document::new(),
            stack: Vec::new(),
        }
    }

    fn parent(&self) -> NodeId {
        self.stack.last().copied().unwrap_or(ROOT)
    }

    fn rest(&self) -> &'a str {
        &self.source[self.at..]
    }

    fn run(mut self) -> Document {
        while self.at < self.source.len() {
            let rest = self.rest();
            if let Some(after) = rest.strip_prefix("<!--") {
                let end = after.find("-->").unwrap_or(after.len());
                let text = after[..end].to_string();
                self.at += 4 + (end + 3).min(after.len());
                let parent = self.parent();
                self.doc.push(parent, Kind::Comment(text));
            } else if rest.starts_with("<!") || rest.starts_with("<?") {
                let end = rest.find('>').unwrap_or(rest.len());
                let text = rest[2.min(end)..end].to_string();
                self.at += (end + 1).min(rest.len());
                let parent = self.parent();
                if rest.starts_with("<!") {
                    self.doc.push(parent, Kind::Declaration(text));
                } else {
                    // A processing instruction (`<?xml …?>`): kept as a comment-like node.
                    self.doc.push(parent, Kind::Raw(format!("<?{text}>")));
                }
            } else if rest.starts_with("</")
                && rest[2..].starts_with(|c: char| c.is_ascii_alphabetic())
            {
                let end = rest.find('>').unwrap_or(rest.len());
                let name = rest[2..end]
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                self.at += (end + 1).min(rest.len());
                self.close(&name);
            } else if rest.starts_with('<')
                && rest[1..].starts_with(|c: char| c.is_ascii_alphabetic())
            {
                self.start_tag();
            } else {
                // Text up to the next thing that looks like markup.
                let mut end = rest.len();
                let mut search = rest.chars().next().map_or(1, char::len_utf8);
                while let Some(found) = rest[search..].find('<') {
                    let at = search + found;
                    let next = &rest[at + 1..];
                    if next.starts_with(|c: char| {
                        c.is_ascii_alphabetic() || c == '/' || c == '!' || c == '?'
                    }) {
                        end = at;
                        break;
                    }
                    search = at + 1;
                }
                let text = decode_entities(&rest[..end]);
                self.at += end;
                self.text(text);
            }
        }
        self.doc
    }

    fn text(&mut self, text: String) {
        let parent = self.parent();
        if let Some(&last) = self.doc.nodes[parent].children.last()
            && let Kind::Text(previous) = &mut self.doc.nodes[last].kind
        {
            previous.push_str(&text);
            return;
        }
        self.doc.push(parent, Kind::Text(text));
    }

    fn start_tag(&mut self) {
        let rest = self.rest();
        let bytes = rest.as_bytes();
        let mut i = 1;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'>'
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let name = rest[1..i].to_ascii_lowercase();
        let mut attrs: Vec<(String, String)> = Vec::new();
        let mut self_close = false;
        loop {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            match bytes[i] {
                b'>' => {
                    i += 1;
                    break;
                }
                b'/' => {
                    i += 1;
                    if bytes.get(i) == Some(&b'>') {
                        self_close = true;
                    }
                    continue;
                }
                _ => {}
            }
            let start = i;
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && !matches!(bytes[i], b'=' | b'>')
                && (bytes[i] != b'/' || bytes.get(i + 1) != Some(&b'>'))
            {
                i += 1;
            }
            let key = rest[start..i].to_ascii_lowercase();
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let mut value = String::new();
            if bytes.get(i) == Some(&b'=') {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                match bytes.get(i) {
                    Some(&quote @ (b'"' | b'\'')) => {
                        let end = rest[i + 1..]
                            .find(quote as char)
                            .map(|e| i + 1 + e)
                            .unwrap_or(rest.len());
                        value = decode_entities(&rest[i + 1..end]);
                        i = (end + 1).min(rest.len());
                    }
                    _ => {
                        let start = i;
                        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>'
                        {
                            i += 1;
                        }
                        value = decode_entities(&rest[start..i]);
                    }
                }
            }
            if !key.is_empty() && !attrs.iter().any(|(k, _)| *k == key) {
                attrs.push((key, value));
            }
        }
        self.at += i.min(rest.len());
        self.open(name, attrs, self_close);
    }

    fn open(&mut self, name: String, attrs: Vec<(String, String)>, self_close: bool) {
        self.implicit_closes(&name);
        let parent = self.parent();
        let raw = is_raw_text(&name);
        let void = is_void(&name);
        let id = self.doc.push(
            parent,
            Kind::Element {
                name: name.clone(),
                attrs,
            },
        );
        if void || (self_close && !raw) {
            // `<div/>` in HTML opens a div; in practice it is XHTML meaning an empty one.
            return;
        }
        if raw {
            let rest = self.rest();
            let end = find_end_tag(rest, &name).unwrap_or(rest.len());
            if end > 0 {
                self.doc.push(id, Kind::Raw(rest[..end].to_string()));
            }
            self.at += end;
            let after = self.rest();
            if after.starts_with("</") {
                let close = after.find('>').map(|e| e + 1).unwrap_or(after.len());
                self.at += close;
            }
            return;
        }
        self.stack.push(id);
    }

    /// HTML's implied end tags.
    fn implicit_closes(&mut self, name: &str) {
        let open = |parser: &Self, wanted: &[&str], stop: &[&str]| -> Option<usize> {
            for (depth, &id) in parser.stack.iter().enumerate().rev() {
                let tag = parser.doc.name(id).unwrap_or("");
                if wanted.contains(&tag) {
                    return Some(depth);
                }
                if stop.contains(&tag) {
                    return None;
                }
            }
            None
        };
        let close = |parser: &mut Self, depth: Option<usize>| {
            if let Some(depth) = depth {
                parser.stack.truncate(depth);
            }
        };
        if closes_paragraph(name) {
            let depth = open(
                self,
                &["p"],
                &[
                    "div",
                    "section",
                    "td",
                    "th",
                    "li",
                    "blockquote",
                    "body",
                    "table",
                    "article",
                    "figure",
                ],
            );
            close(self, depth);
        }
        match name {
            "li" => {
                let depth = open(self, &["li"], &["ul", "ol", "menu"]);
                close(self, depth);
            }
            "dt" | "dd" => {
                let depth = open(self, &["dt", "dd"], &["dl"]);
                close(self, depth);
            }
            "tr" => {
                let depth = open(self, &["tr"], &["table"]);
                close(self, depth);
            }
            "td" | "th" => {
                let depth = open(self, &["td", "th"], &["tr", "table"]);
                close(self, depth);
            }
            "thead" | "tbody" | "tfoot" => {
                let depth = open(self, &["thead", "tbody", "tfoot"], &["table"]);
                close(self, depth);
            }
            "option" => {
                let depth = open(self, &["option"], &["select"]);
                close(self, depth);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let depth = open(
                    self,
                    &["h1", "h2", "h3", "h4", "h5", "h6"],
                    &["div", "section", "body"],
                );
                if depth == Some(self.stack.len().saturating_sub(1)) {
                    close(self, depth);
                }
            }
            _ => {}
        }
    }

    fn close(&mut self, name: &str) {
        if name == "br" {
            // `</br>` is read as `<br>`.
            let parent = self.parent();
            self.doc.push(
                parent,
                Kind::Element {
                    name: "br".into(),
                    attrs: Vec::new(),
                },
            );
            return;
        }
        if let Some(depth) = self
            .stack
            .iter()
            .rposition(|&id| self.doc.name(id) == Some(name))
        {
            self.stack.truncate(depth);
        }
    }
}

/// Where `</name` starts in `text`, ASCII case-insensitively.
fn find_end_tag(text: &str, name: &str) -> Option<usize> {
    let needle = format!("</{name}");
    let bytes = text.as_bytes();
    let n = needle.len();
    let mut i = 0;
    while i + n <= bytes.len() {
        if bytes[i] == b'<' && bytes[i..i + n].eq_ignore_ascii_case(needle.as_bytes()) {
            let next = bytes.get(i + n);
            if next.is_none_or(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/') {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(source: &str) -> String {
        Document::parse(source).serialize()
    }

    #[test]
    fn well_formed_html_round_trips() {
        let source = "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>标题</title><style>p { color: red } a<b</style></head><body><!-- 注释 --><section style=\"color: #333; padding: 4px\"><p>你好 <strong>世界</strong><br>第二行</p><img src=\"a.png\" alt=\"图\"></section><script>if (a < b && c) {}</script></body></html>";
        assert_eq!(roundtrip(source), source);
    }

    #[test]
    fn entities_decode_and_re_escape() {
        let doc =
            Document::parse("<p title=\"a &amp; b\">x &lt; y &nbsp;&#169;&#x4E2D;&unknown; &</p>");
        let p = doc.find("p").unwrap();
        assert_eq!(doc.attr(p, "title"), Some("a & b"));
        let Kind::Text(text) = &doc.node(doc.children(p)[0]).kind else {
            panic!()
        };
        assert_eq!(text, "x < y \u{a0}©中&unknown; &");
        assert_eq!(
            doc.serialize(),
            "<p title=\"a &amp; b\">x &lt; y &nbsp;©中&amp;unknown; &amp;</p>"
        );
    }

    #[test]
    fn implied_end_tags() {
        let doc = Document::parse(
            "<p>one<p>two<div>three</div><ul><li>a<li>b</ul><table><tr><td>1<td>2<tr><td>3</table>",
        );
        assert_eq!(
            doc.serialize(),
            "<p>one</p><p>two</p><div>three</div><ul><li>a</li><li>b</li></ul><table><tr><td>1</td><td>2</td></tr><tr><td>3</td></tr></table>"
        );
    }

    #[test]
    fn stray_and_unclosed_tags() {
        assert_eq!(
            roundtrip("<div><span>a</div>b</span>c"),
            "<div><span>a</span></div>bc"
        );
        assert_eq!(roundtrip("a < b > c"), "a &lt; b &gt; c");
        assert_eq!(roundtrip("<br/><img src=x />"), "<br><img src=\"x\">");
        assert_eq!(roundtrip("<div/>x"), "<div></div>x");
        assert_eq!(roundtrip("x</br>y"), "x<br>y");
        assert_eq!(roundtrip("<!-- open"), "<!-- open-->");
    }

    #[test]
    fn attributes_in_every_form() {
        let doc = Document::parse("<input disabled value=plain data-x='single \"q\"' VALUE=dup>");
        let input = doc.find("input").unwrap();
        assert_eq!(doc.attr(input, "disabled"), Some(""));
        assert_eq!(doc.attr(input, "value"), Some("plain"));
        assert_eq!(doc.attr(input, "data-x"), Some("single \"q\""));
    }

    #[test]
    fn raw_text_ends_only_at_its_own_end_tag() {
        let doc = Document::parse("<script>document.write('</div>')</SCRIPT ><p>x</p>");
        let script = doc.find("script").unwrap();
        assert_eq!(
            doc.node(doc.children(script)[0]).kind,
            Kind::Raw("document.write('</div>')".into())
        );
        assert!(doc.find("p").is_some());
        assert!(doc.has_scripts_or_stylesheets());
    }

    #[test]
    fn title_and_body() {
        let doc = Document::parse(
            "<html><head><title> 我的 &amp; 页面 </title></head><body><p>x</p></body></html>",
        );
        assert_eq!(doc.title().as_deref(), Some("我的 & 页面"));
        assert_eq!(doc.name(doc.body()), Some("body"));
        assert_eq!(Document::parse("<p>x</p>").body(), ROOT);
    }
}
