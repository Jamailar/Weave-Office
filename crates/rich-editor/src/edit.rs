//! Editing the HTML tree: typing, deleting, splitting paragraphs, inline and paragraph
//! formatting, copying and pasting fragments.
//!
//! Positions follow the DOM's range model: in a text node the offset is a byte offset into
//! its text; in an element it is a child index (the position before that child).

use crate::dom::{Document, Kind, NodeId, ROOT, is_void};
use crate::style;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub node: NodeId,
    pub offset: usize,
}

impl Pos {
    pub fn new(node: NodeId, offset: usize) -> Self {
        Self { node, offset }
    }
}

/// A position that survives re-parsing the serialized document: the node's path from the root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub path: Vec<usize>,
    pub offset: usize,
}

impl Anchor {
    pub fn of(doc: &Document, pos: Pos) -> Self {
        Self {
            path: doc.path(pos.node),
            offset: pos.offset,
        }
    }

    /// The position in `doc`, clamped to what exists.
    pub fn resolve(&self, doc: &Document) -> Option<Pos> {
        let node = doc.at_path(&self.path)?;
        let max = match doc.text(node) {
            Some(text) => {
                let mut offset = self.offset.min(text.len());
                while !text.is_char_boundary(offset) {
                    offset -= 1;
                }
                return Some(Pos::new(node, offset));
            }
            None => doc.children(node).len(),
        };
        Some(Pos::new(node, self.offset.min(max)))
    }
}

/// Elements that hold paragraphs of text (where Enter splits and alignment applies).
pub fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "li"
            | "div"
            | "section"
            | "blockquote"
            | "td"
            | "th"
            | "figure"
            | "figcaption"
            | "pre"
            | "body"
            | "article"
            | "header"
            | "footer"
            | "aside"
            | "main"
            | "nav"
            | "dd"
            | "dt"
            | "address"
            | "center"
            | "caption"
            | "html"
            | "ul"
            | "ol"
            | "table"
            | "tr"
            | "tbody"
            | "thead"
            | "tfoot"
            | "hr"
            | "dl"
    )
}

fn is_block(doc: &Document, id: NodeId) -> bool {
    id == ROOT || doc.name(id).is_some_and(is_block_tag)
}

/// The nearest block element around a position.
pub fn block_of(doc: &Document, pos: Pos) -> NodeId {
    let mut at = if doc.text(pos.node).is_some() || !is_block(doc, pos.node) {
        doc.node(pos.node).parent.unwrap_or(ROOT)
    } else {
        pos.node
    };
    while !is_block(doc, at) {
        at = doc.node(at).parent.unwrap_or(ROOT);
    }
    at
}

/// Sort key for a position in document order.
fn key(doc: &Document, pos: Pos) -> Vec<i64> {
    let mut key: Vec<i64> = doc.path(pos.node).into_iter().map(|i| i as i64).collect();
    if doc.text(pos.node).is_some() {
        key.push(pos.offset as i64);
    } else {
        key.push(pos.offset as i64);
        key.push(-1);
    }
    key
}

pub fn compare(doc: &Document, a: Pos, b: Pos) -> Ordering {
    key(doc, a).cmp(&key(doc, b))
}

/// `(start, end)` in document order.
pub fn ordered(doc: &Document, a: Pos, b: Pos) -> (Pos, Pos) {
    if compare(doc, a, b) == Ordering::Greater {
        (b, a)
    } else {
        (a, b)
    }
}

/// Leaves in document order: text nodes and void elements (pictures, line breaks).
fn leaves(doc: &Document, root: NodeId) -> Vec<NodeId> {
    doc.descendants(root)
        .into_iter()
        .filter(|&id| match &doc.node(id).kind {
            Kind::Text(_) => true,
            Kind::Element { name, .. } => is_void(name),
            _ => false,
        })
        .collect()
}

/// Text that layout throws away: whitespace between blocks.
fn is_collapsible(doc: &Document, id: NodeId) -> bool {
    doc.text(id)
        .is_some_and(|t| t.chars().all(|c| matches!(c, ' ' | '\n' | '\t' | '\r')))
}

/// The position just before a leaf, and just after it.
fn before(doc: &Document, leaf: NodeId) -> Pos {
    match doc.text(leaf) {
        Some(_) => Pos::new(leaf, 0),
        None => Pos::new(
            doc.node(leaf).parent.unwrap_or(ROOT),
            doc.index_in_parent(leaf).unwrap_or(0),
        ),
    }
}

fn after(doc: &Document, leaf: NodeId) -> Pos {
    match doc.text(leaf) {
        Some(text) => Pos::new(leaf, text.len()),
        None => Pos::new(
            doc.node(leaf).parent.unwrap_or(ROOT),
            doc.index_in_parent(leaf).map_or(0, |i| i + 1),
        ),
    }
}

/// Make `pos` a position in a text node, splitting nothing: an element position becomes the
/// end of a text node before it or the start of one after it, else a new empty text node.
fn to_text(doc: &mut Document, pos: Pos) -> Pos {
    if doc.text(pos.node).is_some() {
        return pos;
    }
    let children = doc.children(pos.node).to_vec();
    if pos.offset > 0
        && let Some(&prev) = children.get(pos.offset - 1)
        && let Some(text) = doc.text(prev)
    {
        return Pos::new(prev, text.len());
    }
    if let Some(&next) = children.get(pos.offset)
        && doc.text(next).is_some()
    {
        return Pos::new(next, 0);
    }
    let id = doc.insert(pos.node, pos.offset, Kind::Text(String::new()));
    Pos::new(id, 0)
}

/// Insert typed text. Spaces that HTML would collapse become non-breaking spaces.
pub fn insert_text(doc: &mut Document, pos: Pos, text: &str) -> Pos {
    if text.is_empty() {
        return pos;
    }
    // The line break that holds an empty paragraph open goes once it has text.
    let block = block_of(doc, pos);
    if !has_content(doc, block) && !has_child_block(doc, block) {
        let breaks: Vec<NodeId> = doc
            .descendants(block)
            .into_iter()
            .filter(|&n| doc.name(n) == Some("br"))
            .collect();
        if let [only] = breaks[..] {
            let pos_before = pos.node == doc.node(only).parent.unwrap_or(ROOT)
                && doc.index_in_parent(only).is_some_and(|i| pos.offset <= i);
            if pos_before || doc.text(pos.node).is_some() {
                doc.detach(only);
            }
        }
    }
    let pos = to_text(doc, pos);
    let current = doc.text(pos.node).unwrap_or("").to_string();
    let before = current[..pos.offset].chars().next_back();
    let after = current[pos.offset..].chars().next();
    let mut out = String::with_capacity(text.len());
    let mut previous = before;
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied().or(after);
        let c = if c == '\t'
            || c == ' '
                && (previous.is_none_or(|p| p == ' ' || p == '\u{a0}')
                    || next.is_none_or(|n| n == ' '))
        {
            '\u{a0}'
        } else {
            c
        };
        out.push(c);
        previous = Some(c);
    }
    // A plain space typed after a non-breaking one lets the pair wrap again.
    if let Some(t) = doc.text_mut(pos.node) {
        t.insert_str(pos.offset, &out);
    }
    Pos::new(pos.node, pos.offset + out.len())
}

/// Delete what is between two positions, merging the end's paragraph into the start's.
pub fn delete_range(doc: &mut Document, a: Pos, b: Pos) -> Pos {
    let (start, end) = ordered(doc, a, b);
    if start == end {
        return start;
    }
    let start_block = block_of(doc, start);
    let end_block = block_of(doc, end);
    remove_between(doc, start, end);
    // The caret lives in a text node so merging and cleanup cannot move it.
    let caret = to_text(doc, start);
    if start_block != end_block
        && doc.contains(ROOT, end_block)
        && !doc.contains(end_block, start_block)
        && !doc.contains(start_block, end_block)
    {
        merge_blocks(doc, start_block, end_block);
    }
    cleanup(doc, start_block, Some(caret.node));
    // An empty caret node does not keep empty inline tags around it.
    if doc.text(caret.node) == Some("") {
        while let Some(parent) = doc.node(caret.node).parent
            && parent != start_block
            && !is_block(doc, parent)
            && doc.children(parent).len() == 1
        {
            let grand = doc.node(parent).parent.unwrap_or(ROOT);
            let at = doc.index_in_parent(parent).unwrap_or(0);
            doc.detach(parent);
            doc.attach(caret.node, grand, at);
        }
    }
    caret
}

/// The position just before any node, and just after it.
fn node_start(doc: &Document, id: NodeId) -> Pos {
    if doc.text(id).is_some() {
        return Pos::new(id, 0);
    }
    Pos::new(
        doc.node(id).parent.unwrap_or(ROOT),
        doc.index_in_parent(id).unwrap_or(0),
    )
}

fn node_end(doc: &Document, id: NodeId) -> Pos {
    if let Some(text) = doc.text(id) {
        return Pos::new(id, text.len());
    }
    Pos::new(
        doc.node(id).parent.unwrap_or(ROOT),
        doc.index_in_parent(id).map_or(0, |i| i + 1),
    )
}

/// Remove what lies between two positions, without merging paragraphs: the ends of the two
/// boundary text nodes and every node wholly inside. Whitespace between blocks stays, so the
/// source keeps its line breaks.
fn remove_between(doc: &mut Document, start: Pos, end: Pos) {
    let (sk, ek) = (key(doc, start), key(doc, end));
    if sk >= ek {
        return;
    }
    // Decide everything before changing anything: removing a node shifts its siblings' keys.
    let mut doomed = Vec::new();
    let mut stack = vec![ROOT];
    while let Some(node) = stack.pop() {
        if node != ROOT {
            let (b, a) = (
                key(doc, node_start(doc, node)),
                key(doc, node_end(doc, node)),
            );
            if a <= sk || b >= ek {
                continue;
            }
            if b >= sk && a <= ek && node != start.node && node != end.node {
                let between_blocks = is_collapsible(doc, node)
                    && doc
                        .node(node)
                        .parent
                        .is_some_and(|p| has_child_block(doc, p));
                if !between_blocks {
                    doomed.push(node);
                }
                continue;
            }
        }
        stack.extend(doc.children(node).iter().rev());
    }
    if start.node == end.node {
        if let Some(text) = doc.text_mut(start.node) {
            text.replace_range(start.offset..end.offset, "");
        }
    } else {
        if let Some(text) = doc.text_mut(start.node) {
            text.truncate(start.offset);
        }
        if let Some(text) = doc.text_mut(end.node) {
            text.replace_range(..end.offset, "");
        }
    }
    for node in doomed {
        doc.detach(node);
    }
}

/// Move `from`'s content to the end of `into` and remove `from` (and wrappers it leaves empty).
fn merge_blocks(doc: &mut Document, into: NodeId, from: NodeId) {
    // A placeholder line break in an empty paragraph goes away when text joins it.
    if let Some(&last) = doc
        .children(into)
        .iter()
        .rev()
        .find(|&&c| doc.text(c).is_none_or(|t| !t.is_empty()))
        && doc.name(last) == Some("br")
        && has_content(doc, from)
    {
        doc.detach(last);
    }
    let children = doc.children(from).to_vec();
    if doc.contains(from, into) {
        return;
    }
    for child in children {
        let index = doc.children(into).len();
        doc.attach(child, into, index);
    }
    let mut parent = doc.node(from).parent;
    doc.detach(from);
    while let Some(p) = parent {
        if p == ROOT
            || doc.contains(p, into)
            || has_content(doc, p)
            || !doc.children(p).iter().all(|&c| is_collapsible(doc, c))
        {
            break;
        }
        parent = doc.node(p).parent;
        doc.detach(p);
    }
}

/// Whether a subtree shows anything: non-whitespace text, a picture, a rule.
fn has_content(doc: &Document, id: NodeId) -> bool {
    doc.descendants(id)
        .into_iter()
        .any(|n| match &doc.node(n).kind {
            Kind::Text(t) => !t.chars().all(|c| matches!(c, ' ' | '\n' | '\t' | '\r')),
            Kind::Element { name, .. } => {
                matches!(name.as_str(), "img" | "hr" | "video" | "iframe" | "svg")
            }
            _ => false,
        })
}

/// Remove empty inline wrappers and empty text nodes under `id`; keep an emptied paragraph
/// one line tall with a `<br>`.
fn cleanup(doc: &mut Document, id: NodeId, keep: Option<NodeId>) {
    if !doc.contains(ROOT, id) {
        return;
    }
    for node in doc.descendants(id).into_iter().rev() {
        if node == id || Some(node) == keep {
            continue;
        }
        let remove = match &doc.node(node).kind {
            Kind::Text(t) => t.is_empty(),
            Kind::Element { name, .. } => {
                !is_void(name) && !is_block_tag(name) && doc.children(node).is_empty()
            }
            _ => false,
        };
        if remove {
            doc.detach(node);
        }
    }
    if is_block(doc, id) && id != ROOT && !has_content(doc, id) && !has_child_block(doc, id) {
        let has_br = doc
            .descendants(id)
            .iter()
            .any(|&n| doc.name(n) == Some("br"));
        if !has_br {
            let index = doc.children(id).len();
            doc.insert(id, index, element("br", Vec::new()));
        }
    }
}

fn has_child_block(doc: &Document, id: NodeId) -> bool {
    doc.children(id)
        .iter()
        .any(|&c| doc.name(c).is_some_and(is_block_tag))
}

fn element(name: &str, attrs: Vec<(String, String)>) -> Kind {
    Kind::Element {
        name: name.into(),
        attrs,
    }
}

/// The range Backspace deletes at `pos`.
pub fn backward_range(doc: &Document, pos: Pos) -> Option<(Pos, Pos)> {
    if let Some(text) = doc.text(pos.node)
        && pos.offset > 0
    {
        let previous = text[..pos.offset]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i);
        return Some((Pos::new(pos.node, previous), pos));
    }
    let here = key(doc, pos);
    let block = block_of(doc, pos);
    let leaf = leaves(doc, ROOT).into_iter().rfind(|&l| {
        !is_collapsible(doc, l)
            && key(doc, after(doc, l)) <= here
            && doc.text(l).is_none_or(|t| !t.is_empty())
    })?;
    let leaf_block = block_of(doc, before(doc, leaf));
    if leaf_block != block && !doc.contains(leaf_block, block) {
        // At the start of a paragraph: join it to the one before.
        return Some((after(doc, leaf), pos));
    }
    match doc.text(leaf) {
        Some(text) => {
            let previous = text.char_indices().next_back().map_or(0, |(i, _)| i);
            Some((Pos::new(leaf, previous), pos))
        }
        None => Some((before(doc, leaf), pos)),
    }
}

/// The range Delete removes at `pos`.
pub fn forward_range(doc: &Document, pos: Pos) -> Option<(Pos, Pos)> {
    if let Some(text) = doc.text(pos.node)
        && pos.offset < text.len()
    {
        let next = text[pos.offset..].chars().next().map_or(0, char::len_utf8);
        return Some((pos, Pos::new(pos.node, pos.offset + next)));
    }
    let here = key(doc, pos);
    let block = block_of(doc, pos);
    let leaf = leaves(doc, ROOT)
        .into_iter()
        .filter(|&l| !is_collapsible(doc, l))
        .filter(|&l| doc.text(l).is_none_or(|t| !t.is_empty()))
        .find(|&l| key(doc, before(doc, l)) >= here)?;
    let leaf_block = block_of(doc, before(doc, leaf));
    if leaf_block != block && !doc.contains(block, leaf_block) {
        return Some((pos, before(doc, leaf)));
    }
    match doc.text(leaf) {
        Some(text) => {
            let next = text.chars().next().map_or(0, char::len_utf8);
            Some((pos, Pos::new(leaf, next)))
        }
        None => Some((pos, after(doc, leaf))),
    }
}

/// Split the paragraph at `pos` (Enter). Returns the start of the new paragraph.
#[allow(clippy::unnecessary_to_owned)] // the children move while the copy is walked
pub fn split_block(doc: &mut Document, pos: Pos) -> Pos {
    let block = block_of(doc, pos);
    if block == ROOT
        || matches!(
            doc.name(block),
            Some("body" | "html" | "td" | "th" | "ul" | "ol" | "table" | "tr" | "tbody" | "thead")
        )
    {
        return insert_break(doc, pos);
    }
    let (container, index) = split_to(doc, pos, block);
    debug_assert_eq!(container, block);
    let new = doc.clone_shallow(block);
    let parent = doc.node(block).parent.unwrap_or(ROOT);
    let at = doc.index_in_parent(block).map_or(0, |i| i + 1);
    doc.attach(new, parent, at);
    for child in doc.children(block)[index..].to_vec() {
        let i = doc.children(new).len();
        doc.attach(child, new, i);
    }
    // Enter at the end of a heading continues with a paragraph.
    if doc
        .name(new)
        .is_some_and(|n| matches!(n, "h1" | "h2" | "h3" | "h4" | "h5" | "h6"))
        && !has_content(doc, new)
    {
        doc.rename(new, "p");
        doc.set_attr(new, "style", None);
    }
    cleanup(doc, block, None);
    cleanup(doc, new, None);
    start_of(doc, new)
}

/// Split the tree from `pos` up to `until`, so `pos` falls between two children of `until`.
/// Returns `(until, index)` of that boundary.
#[allow(clippy::unnecessary_to_owned)] // the children move while the copy is walked
fn split_to(doc: &mut Document, pos: Pos, until: NodeId) -> (NodeId, usize) {
    let (mut parent, mut index) = match doc.text(pos.node) {
        Some(text) => {
            let tail = text[pos.offset..].to_string();
            let head = text[..pos.offset].to_string();
            let parent = doc.node(pos.node).parent.unwrap_or(ROOT);
            let at = doc.index_in_parent(pos.node).unwrap_or(0);
            if let Some(t) = doc.text_mut(pos.node) {
                *t = head;
            }
            doc.insert(parent, at + 1, Kind::Text(tail));
            (parent, at + 1)
        }
        None => (pos.node, pos.offset),
    };
    while parent != until && parent != ROOT {
        let grand = doc.node(parent).parent.unwrap_or(ROOT);
        let at = doc.index_in_parent(parent).unwrap_or(0);
        let copy = doc.clone_shallow(parent);
        doc.attach(copy, grand, at + 1);
        for child in doc.children(parent)[index..].to_vec() {
            let i = doc.children(copy).len();
            doc.attach(child, copy, i);
        }
        parent = grand;
        index = at + 1;
    }
    (parent, index)
}

/// The first caret position inside `id`.
pub fn start_of(doc: &mut Document, id: NodeId) -> Pos {
    match leaves(doc, id)
        .into_iter()
        .find(|&l| !is_collapsible(doc, l) || doc.text(l).is_some_and(str::is_empty))
    {
        Some(leaf) => before(doc, leaf),
        None => Pos::new(id, 0),
    }
}

/// The last caret position inside `id`.
pub fn end_of(doc: &Document, id: NodeId) -> Pos {
    match leaves(doc, id)
        .into_iter()
        .rfind(|&l| !is_collapsible(doc, l))
    {
        Some(leaf) if doc.name(leaf) == Some("br") => before(doc, leaf),
        Some(leaf) => after(doc, leaf),
        None => Pos::new(id, doc.children(id).len()),
    }
}

/// Insert a line break (Shift+Enter).
pub fn insert_break(doc: &mut Document, pos: Pos) -> Pos {
    let (parent, index) = match doc.text(pos.node) {
        Some(_) => {
            let block = doc.node(pos.node).parent.unwrap_or(ROOT);
            split_to(doc, pos, block)
        }
        None => (pos.node, pos.offset),
    };
    doc.insert(parent, index, element("br", Vec::new()));
    // A break at the very end needs a second one to show the new line.
    let rest_visible = doc.children(parent)[index + 1..]
        .iter()
        .any(|&c| has_content(doc, c) || doc.name(c) == Some("br"));
    if !rest_visible {
        doc.insert(parent, index + 1, element("br", Vec::new()));
    }
    Pos::new(parent, index + 1)
}

/// Change one property in an element's `style` attribute (`None` removes it).
pub fn set_style_property(doc: &mut Document, id: NodeId, property: &str, value: Option<&str>) {
    let mut declarations = style::declarations(doc.attr(id, "style").unwrap_or(""));
    declarations.retain(|(k, _)| k != property);
    if let Some(value) = value {
        declarations.push((property.to_string(), value.to_string()));
    }
    let css = declarations
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("; ");
    doc.set_attr(id, "style", (!css.is_empty()).then_some(css.as_str()));
}

/// Text nodes covered by a range, split at its ends so each lies wholly inside.
/// Returns them in order, with the range re-expressed over them.
fn covered_text(doc: &mut Document, start: Pos, end: Pos) -> Vec<NodeId> {
    let (start, end) = ordered(doc, start, end);
    let mut end = end;
    // Split the end first so the start's offsets stay valid.
    if let Some(text) = doc.text(end.node)
        && end.offset < text.len()
        && end.offset > 0
    {
        let tail = text[end.offset..].to_string();
        let head = text[..end.offset].to_string();
        let parent = doc.node(end.node).parent.unwrap_or(ROOT);
        let at = doc.index_in_parent(end.node).unwrap_or(0);
        if let Some(t) = doc.text_mut(end.node) {
            *t = head;
        }
        doc.insert(parent, at + 1, Kind::Text(tail));
    }
    let mut start = start;
    if let Some(text) = doc.text(start.node)
        && start.offset > 0
        && start.offset < text.len()
    {
        let tail = text[start.offset..].to_string();
        let head = text[..start.offset].to_string();
        let parent = doc.node(start.node).parent.unwrap_or(ROOT);
        let at = doc.index_in_parent(start.node).unwrap_or(0);
        if let Some(t) = doc.text_mut(start.node) {
            *t = head;
        }
        let new = doc.insert(parent, at + 1, Kind::Text(tail));
        if end.node == start.node {
            end = Pos::new(new, end.offset - start.offset);
        }
        start = Pos::new(new, 0);
    }
    let start_key = key(doc, start);
    let end_key = key(doc, end);
    leaves(doc, ROOT)
        .into_iter()
        .filter(|&l| doc.text(l).is_some_and(|t| !t.is_empty()) && !is_collapsible(doc, l))
        .filter(|&l| key(doc, before(doc, l)) >= start_key && key(doc, after(doc, l)) <= end_key)
        .collect()
}

/// Set an inline CSS property on the text in a range. Returns the range over the same text.
pub fn style_range(
    doc: &mut Document,
    a: Pos,
    b: Pos,
    property: &str,
    value: Option<&str>,
) -> (Pos, Pos) {
    let texts = covered_text(doc, a, b);
    for &text in &texts {
        let parent = doc.node(text).parent.unwrap_or(ROOT);
        let sole_span = doc.name(parent) == Some("span") && doc.children(parent).len() == 1;
        let target = if sole_span {
            parent
        } else {
            let at = doc.index_in_parent(text).unwrap_or(0);
            let span = doc.insert(parent, at, element("span", Vec::new()));
            doc.attach(text, span, 0);
            span
        };
        set_style_property(doc, target, property, value);
        if doc.attr(target, "style").is_none() && doc.name(target) == Some("span") {
            unwrap(doc, target);
        }
    }
    match (texts.first(), texts.last()) {
        (Some(&first), Some(&last)) => (
            Pos::new(first, 0),
            Pos::new(last, doc.text(last).map_or(0, str::len)),
        ),
        _ => ordered(doc, a, b),
    }
}

/// Replace an element by its children.
fn unwrap(doc: &mut Document, id: NodeId) {
    let Some(parent) = doc.node(id).parent else {
        return;
    };
    let at = doc.index_in_parent(id).unwrap_or(0);
    for (i, child) in doc.children(id).to_vec().into_iter().enumerate() {
        doc.attach(child, parent, at + 1 + i);
    }
    doc.detach(id);
}

/// Whether every piece of text in a range has a computed property (for toggles).
pub fn all_text<F: Fn(&style::Style) -> bool>(doc: &Document, a: Pos, b: Pos, test: F) -> bool {
    all_text_with(doc, &style::compute_all(doc), a, b, test)
}

/// [`all_text`] with the document's computed styles at hand.
pub fn all_text_with<F: Fn(&style::Style) -> bool>(
    doc: &Document,
    styles: &[style::Style],
    a: Pos,
    b: Pos,
    test: F,
) -> bool {
    let (start, end) = ordered(doc, a, b);
    let (sk, ek) = (key(doc, start), key(doc, end));
    let mut any = false;
    for leaf in leaves(doc, ROOT) {
        let Some(text) = doc.text(leaf) else { continue };
        if text.trim().is_empty() {
            continue;
        }
        let (lb, la) = (key(doc, before(doc, leaf)), key(doc, after(doc, leaf)));
        if la <= sk || lb >= ek {
            continue;
        }
        any = true;
        if !test(&styles[leaf]) {
            return false;
        }
    }
    any
}

/// The blocks a range touches (innermost text blocks).
pub fn blocks_in(doc: &Document, a: Pos, b: Pos) -> Vec<NodeId> {
    let (start, end) = ordered(doc, a, b);
    let mut out = vec![block_of(doc, start)];
    let (sk, ek) = (key(doc, start), key(doc, end));
    for leaf in leaves(doc, ROOT) {
        if is_collapsible(doc, leaf) {
            continue;
        }
        let k = key(doc, before(doc, leaf));
        if k >= sk && k < ek {
            let block = block_of(doc, before(doc, leaf));
            if !out.contains(&block) {
                out.push(block);
            }
        }
    }
    out.retain(|&b| b != ROOT);
    out
}

/// Turn the blocks in a range into `tag` (`p`, `h1`…), keeping their other styles.
pub fn set_block_tag(doc: &mut Document, a: Pos, b: Pos, tag: &str) {
    for block in blocks_in(doc, a, b) {
        if !matches!(
            doc.name(block),
            Some("p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "div" | "section" | "blockquote")
        ) {
            continue;
        }
        // A heading's size comes from the tag; drop sizes set on the paragraph.
        set_style_property(doc, block, "font-size", None);
        set_style_property(doc, block, "font-weight", None);
        doc.rename(block, tag);
    }
}

pub fn style_blocks(doc: &mut Document, a: Pos, b: Pos, property: &str, value: Option<&str>) {
    for block in blocks_in(doc, a, b) {
        set_style_property(doc, block, property, value);
    }
}

/// The text of a range, paragraphs on their own lines.
pub fn range_text(doc: &Document, a: Pos, b: Pos) -> String {
    let (start, end) = ordered(doc, a, b);
    let (sk, ek) = (key(doc, start), key(doc, end));
    let mut out = String::new();
    let mut last_block = None;
    for leaf in leaves(doc, ROOT) {
        let (lb, la) = (key(doc, before(doc, leaf)), key(doc, after(doc, leaf)));
        if la <= sk || lb >= ek {
            continue;
        }
        let block = block_of(doc, before(doc, leaf));
        if last_block.is_some_and(|b| b != block) && !is_collapsible(doc, leaf) {
            out.push('\n');
        }
        match doc.text(leaf) {
            Some(text) => {
                if is_collapsible(doc, leaf) {
                    continue;
                }
                let from = if leaf == start.node { start.offset } else { 0 };
                let to = if leaf == end.node {
                    end.offset
                } else {
                    text.len()
                };
                out.push_str(&text[from.min(to)..to].replace('\u{a0}', " "));
            }
            None if doc.name(leaf) == Some("br") => out.push('\n'),
            None => {}
        }
        if !is_collapsible(doc, leaf) {
            last_block = Some(block);
        }
    }
    out
}

/// The HTML of a range: its nodes with their inline styles, cut at the ends.
pub fn range_html(doc: &Document, a: Pos, b: Pos) -> String {
    let mut copy = doc.clone();
    let (start, end) = ordered(doc, a, b);
    // Delete everything after the end first, then everything before the start.
    let last = end_of(&copy, ROOT);
    let last = if compare(&copy, last, end) == Ordering::Greater {
        last
    } else {
        end
    };
    trim_outside(&mut copy, end, last);
    let first = Pos::new(ROOT, 0);
    trim_outside(&mut copy, first, start);
    // A range inside one paragraph copies as inline content.
    let block = block_of(doc, start);
    let one_paragraph =
        block == block_of(doc, end) && !has_child_block(doc, block) && block != ROOT;
    // The smallest element holding what is left.
    let mut root = if one_paragraph && copy.contains(ROOT, block) {
        block
    } else {
        copy.body()
    };
    // Descend through lone wrappers (sections and inline elements) around the content.
    let mut descend = !one_paragraph;
    while descend {
        let visible: Vec<NodeId> = copy
            .children(root)
            .iter()
            .copied()
            .filter(|&c| !is_collapsible(&copy, c) && has_content_or_break(&copy, c))
            .collect();
        if visible.len() == 1
            && copy
                .name(visible[0])
                .is_some_and(|n| !is_block_tag(n) || n == "section" || n == "div")
        {
            root = visible[0];
            if copy.text(root).is_some() {
                root = copy.node(root).parent.unwrap_or(ROOT);
                descend = false;
            }
        } else {
            descend = false;
        }
    }
    copy.children(root)
        .iter()
        .filter(|&&c| has_content_or_break(&copy, c))
        .map(|&c| copy.serialize_node(c))
        .collect()
}

fn has_content_or_break(doc: &Document, id: NodeId) -> bool {
    has_content(doc, id)
        || doc
            .descendants(id)
            .iter()
            .any(|&n| doc.name(n) == Some("br"))
}

/// Remove the content between two positions without merging paragraphs.
fn trim_outside(doc: &mut Document, start: Pos, end: Pos) {
    remove_between(doc, start, end);
    // Blocks left empty are dropped.
    for node in doc.descendants(ROOT).into_iter().rev() {
        if node != ROOT
            && doc
                .name(node)
                .is_some_and(|n| !is_void(n) && !matches!(n, "html" | "body" | "head"))
            && !has_content_or_break(doc, node)
            && doc
                .descendants(node)
                .iter()
                .all(|&n| doc.text(n).is_none_or(|t| t.trim().is_empty()))
        {
            doc.detach(node);
        }
    }
}

/// Elements and attributes a paste keeps out.
fn sanitize(doc: &mut Document) {
    for node in doc.descendants(ROOT) {
        match &doc.node(node).kind {
            Kind::Comment(_) | Kind::Declaration(_) | Kind::Raw(_) => doc.detach(node),
            Kind::Element { name, .. } => {
                if matches!(
                    name.as_str(),
                    "script"
                        | "style"
                        | "meta"
                        | "link"
                        | "title"
                        | "head"
                        | "iframe"
                        | "object"
                        | "embed"
                        | "form"
                        | "input"
                        | "button"
                        | "textarea"
                        | "select"
                ) {
                    doc.detach(node);
                    continue;
                }
                if let Kind::Element { attrs, .. } = &mut doc.nodes[node].kind {
                    attrs.retain(|(k, v)| {
                        !k.starts_with("on")
                            && !matches!(k.as_str(), "class" | "id" | "contenteditable")
                            && (k != "href"
                                || !v
                                    .trim_start()
                                    .to_ascii_lowercase()
                                    .starts_with("javascript:"))
                    });
                }
            }
            _ => {}
        }
    }
}

/// Paste HTML at `pos`. Returns the caret after the pasted content.
pub fn insert_html(doc: &mut Document, pos: Pos, html: &str) -> Pos {
    let mut fragment = Document::parse(html);
    sanitize(&mut fragment);
    let root = fragment.body();
    let top: Vec<NodeId> = fragment
        .children(root)
        .iter()
        .copied()
        .filter(|&c| fragment.node(c).parent.is_some())
        .collect();
    let has_blocks = top
        .iter()
        .any(|&c| fragment.name(c).is_some_and(is_block_tag));
    if !has_blocks {
        // Inline content goes in at the caret.
        let (parent, mut index) = match doc.text(pos.node) {
            Some(_) => {
                let parent = doc.node(pos.node).parent.unwrap_or(ROOT);
                split_to(doc, pos, parent)
            }
            None => (pos.node, pos.offset),
        };
        let mut last = None;
        for c in top {
            let copy = doc.import(&fragment, c);
            doc.attach(copy, parent, index);
            index += 1;
            last = Some(copy);
        }
        return match last {
            Some(node) => end_after(doc, node),
            None => pos,
        };
    }
    // Blocks: split the paragraph and put them between the halves.
    let block = block_of(doc, pos);
    let start = split_block(doc, pos);
    let new_block = block_of(doc, start);
    let parent = doc.node(new_block).parent.unwrap_or(ROOT);
    let mut index = doc.index_in_parent(new_block).unwrap_or(0);
    let mut last = None;
    let mut pending: Vec<NodeId> = Vec::new();
    let flush = |doc: &mut Document,
                 pending: &mut Vec<NodeId>,
                 index: &mut usize,
                 last: &mut Option<NodeId>| {
        if pending.is_empty() {
            return;
        }
        let p = doc.insert(parent, *index, element("p", Vec::new()));
        for (i, n) in pending.drain(..).enumerate() {
            doc.attach(n, p, i);
        }
        *index += 1;
        *last = Some(p);
    };
    for c in top {
        let copy = doc.import(&fragment, c);
        if doc.name(copy).is_some_and(is_block_tag) {
            flush(doc, &mut pending, &mut index, &mut last);
            doc.attach(copy, parent, index);
            index += 1;
            last = Some(copy);
        } else if !(doc.text(copy).is_some_and(|t| t.trim().is_empty())) {
            pending.push(copy);
        }
    }
    flush(doc, &mut pending, &mut index, &mut last);
    // The halves left empty by the split are removed.
    for half in [block, new_block] {
        if doc.contains(ROOT, half) && !has_content(doc, half) && !has_child_block(doc, half) {
            doc.detach(half);
        }
    }
    match last {
        Some(node) => end_of(doc, node),
        None => start,
    }
}

fn end_after(doc: &Document, node: NodeId) -> Pos {
    match doc.text(node) {
        Some(text) => Pos::new(node, text.len()),
        None => {
            if doc.children(node).is_empty() || is_void(doc.name(node).unwrap_or("")) {
                after(doc, node)
            } else {
                end_of(doc, node)
            }
        }
    }
}

/// Paste plain text: one paragraph per line.
pub fn insert_plain(doc: &mut Document, pos: Pos, text: &str) -> Pos {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut caret = pos;
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            caret = split_block(doc, caret);
        }
        caret = insert_text(doc, caret, line);
    }
    caret
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(html: &str) -> Document {
        Document::parse(html)
    }

    /// The position `marker` bytes into the text node holding `needle`.
    fn at(doc: &Document, needle: &str, offset: usize) -> Pos {
        let node = doc
            .descendants(ROOT)
            .into_iter()
            .find(|&n| doc.text(n).is_some_and(|t| t.contains(needle)))
            .expect("text exists");
        let base = doc.text(node).unwrap().find(needle).unwrap();
        Pos::new(node, base + offset)
    }

    #[test]
    fn typing_inserts_text_and_protects_spaces() {
        let mut d = doc("<p>ab</p>");
        let p = at(&d, "ab", 1);
        let caret = insert_text(&mut d, p, "X");
        assert_eq!(d.serialize(), "<p>aXb</p>");
        let caret = insert_text(&mut d, caret, " ");
        assert_eq!(d.serialize(), "<p>aX b</p>");
        insert_text(&mut d, caret, " ");
        assert_eq!(d.serialize(), "<p>aX &nbsp;b</p>");
        let mut empty = doc("<p><br></p>");
        let p = d_first_block(&empty);
        insert_text(&mut empty, Pos::new(p, 0), "你好");
        assert_eq!(empty.serialize(), "<p>你好</p>");
    }

    fn d_first_block(d: &Document) -> NodeId {
        d.find("p").unwrap()
    }

    #[test]
    fn backspace_within_text_and_across_paragraphs() {
        let mut d = doc("<p>ab</p>\n<p>cd</p>");
        let p = at(&d, "cd", 0);
        let (a, b) = backward_range(&d, p).unwrap();
        let caret = delete_range(&mut d, a, b);
        assert_eq!(d.serialize(), "<p>abcd</p>\n");
        assert_eq!(
            d.text(caret.node).unwrap()[..caret.offset].to_string(),
            "ab"
        );
        let (a, b) = backward_range(&d, caret).unwrap();
        delete_range(&mut d, a, b);
        assert_eq!(d.serialize(), "<p>acd</p>\n");
    }

    #[test]
    fn backspace_joins_into_an_empty_paragraph() {
        let mut d = doc("<p><br></p><p>x</p>");
        let p = at(&d, "x", 0);
        let (a, b) = backward_range(&d, p).unwrap();
        delete_range(&mut d, a, b);
        assert_eq!(d.serialize(), "<p>x</p>");
    }

    #[test]
    fn delete_forward_joins_the_next_paragraph() {
        let mut d = doc("<p>ab</p><p><strong>cd</strong></p>");
        let p = at(&d, "ab", 2);
        let (a, b) = forward_range(&d, p).unwrap();
        delete_range(&mut d, a, b);
        assert_eq!(d.serialize(), "<p>ab<strong>cd</strong></p>");
    }

    #[test]
    fn deleting_a_range_across_styles_and_paragraphs() {
        let mut d = doc("<p>a<b>bc</b>d</p><section><p>ef</p><p>gh</p></section>");
        let s = at(&d, "bc", 1);
        let e = at(&d, "gh", 1);
        delete_range(&mut d, s, e);
        assert_eq!(d.serialize(), "<p>a<b>b</b>h</p>");
    }

    #[test]
    fn deleting_everything_in_a_paragraph_keeps_it_open() {
        let mut d = doc("<p><span>ab</span></p>");
        let s = at(&d, "ab", 0);
        let e = at(&d, "ab", 2);
        delete_range(&mut d, s, e);
        assert_eq!(d.serialize(), "<p><br></p>");
    }

    #[test]
    fn enter_splits_paragraphs_and_inline_styles() {
        let mut d = doc("<p style=\"color: red\">a<b>bc</b>d</p>");
        let p = at(&d, "bc", 1);
        let caret = split_block(&mut d, p);
        assert_eq!(
            d.serialize(),
            "<p style=\"color: red\">a<b>b</b></p><p style=\"color: red\"><b>c</b>d</p>"
        );
        assert_eq!(d.text(caret.node), Some("c"));
        assert_eq!(caret.offset, 0);
    }

    #[test]
    fn enter_at_ends() {
        let mut d = doc("<h2>标题</h2>");
        let p = at(&d, "标题", "标题".len());
        let caret = split_block(&mut d, p);
        assert_eq!(d.serialize(), "<h2>标题</h2><p><br></p>");
        insert_text(&mut d, caret, "正文");
        assert_eq!(d.serialize(), "<h2>标题</h2><p>正文</p>");
        let mut d = doc("<p>x</p>");
        let p = at(&d, "x", 0);
        split_block(&mut d, p);
        assert_eq!(d.serialize(), "<p><br></p><p>x</p>");
    }

    #[test]
    fn shift_enter_inserts_a_break() {
        let mut d = doc("<p>ab</p>");
        let p = at(&d, "ab", 1);
        insert_break(&mut d, p);
        assert_eq!(d.serialize(), "<p>a<br>b</p>");
        let mut d = doc("<p>ab</p>");
        let p = at(&d, "ab", 2);
        insert_break(&mut d, p);
        assert_eq!(d.serialize(), "<p>ab<br><br></p>");
    }

    #[test]
    fn inline_style_wraps_and_reuses_spans() {
        let mut d = doc("<p>abcd</p>");
        let s = at(&d, "abcd", 1);
        let e = at(&d, "abcd", 3);
        let (s, e) = style_range(&mut d, s, e, "font-weight", Some("bold"));
        assert_eq!(
            d.serialize(),
            "<p>a<span style=\"font-weight: bold\">bc</span>d</p>"
        );
        assert!(all_text(&d, s, e, |st| st.bold));
        style_range(&mut d, s, e, "color", Some("#ff0000"));
        assert_eq!(
            d.serialize(),
            "<p>a<span style=\"font-weight: bold; color: #ff0000\">bc</span>d</p>"
        );
        style_range(&mut d, s, e, "font-weight", None);
        style_range(&mut d, s, e, "color", None);
        assert_eq!(d.serialize(), "<p>abcd</p>");
    }

    #[test]
    fn block_formatting() {
        let mut d = doc("<p style=\"font-size: 20px\">a</p><p>b</p>");
        let s = at(&d, "a", 0);
        let e = at(&d, "b", 1);
        style_blocks(&mut d, s, e, "text-align", Some("center"));
        assert_eq!(
            d.serialize(),
            "<p style=\"font-size: 20px; text-align: center\">a</p><p style=\"text-align: center\">b</p>"
        );
        set_block_tag(&mut d, s, s, "h2");
        assert_eq!(
            d.serialize(),
            "<h2 style=\"text-align: center\">a</h2><p style=\"text-align: center\">b</p>"
        );
    }

    #[test]
    fn copying_a_range() {
        let d = doc("<body><p>ab<b>cd</b></p><p style=\"color: red\">ef</p></body>");
        let s = at(&d, "cd", 1);
        let e = at(&d, "ef", 1);
        assert_eq!(range_text(&d, s, e), "d\ne");
        assert_eq!(
            range_html(&d, s, e),
            "<p><b>d</b></p><p style=\"color: red\">e</p>"
        );
        let s = at(&d, "ab", 1);
        let e = at(&d, "cd", 1);
        assert_eq!(range_html(&d, s, e), "b<b>c</b>");
    }

    #[test]
    fn pasting_inline_and_block_html() {
        let mut d = doc("<p>ab</p>");
        let p = at(&d, "ab", 1);
        insert_html(&mut d, p, "<b onclick=\"x\" class=\"c\">X</b>");
        assert_eq!(d.serialize(), "<p>a<b>X</b>b</p>");
        let mut d = doc("<p>ab</p>");
        let p = at(&d, "ab", 1);
        let caret = insert_html(
            &mut d,
            p,
            "<meta charset=utf-8><h2>T</h2><p>P</p><script>x</script>",
        );
        assert_eq!(d.serialize(), "<p>a</p><h2>T</h2><p>P</p><p>b</p>");
        assert_eq!(d.text(caret.node), Some("P"));
    }

    #[test]
    fn pasting_plain_lines() {
        let mut d = doc("<p>ab</p>");
        let p = at(&d, "ab", 1);
        insert_plain(&mut d, p, "1\n2");
        assert_eq!(d.serialize(), "<p>a1</p><p>2b</p>");
    }

    #[test]
    fn anchors_survive_reparsing() {
        let d = doc("<p>a</p><p>b<i>cd</i></p>");
        let p = at(&d, "cd", 1);
        let anchor = Anchor::of(&d, p);
        let again = Document::parse(&d.serialize());
        let resolved = anchor.resolve(&again).unwrap();
        assert_eq!(again.text(resolved.node), Some("cd"));
        assert_eq!(resolved.offset, 1);
    }

    #[test]
    fn ordering() {
        let d = doc("<p>ab</p><p>cd</p>");
        let a = at(&d, "ab", 1);
        let b = at(&d, "cd", 0);
        assert_eq!(compare(&d, a, b), Ordering::Less);
        assert_eq!(ordered(&d, b, a), (a, b));
        let p = d.find("p").unwrap();
        assert_eq!(
            compare(&d, Pos::new(p, 0), Pos::new(d.children(p)[0], 0)),
            Ordering::Less
        );
    }
}
