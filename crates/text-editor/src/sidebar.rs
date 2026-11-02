//! The left sidebar, as in Obsidian and VS Code: a narrow strip of pane buttons, and behind it
//! the file tree of the note's folder, full-text search across that folder, the outline, the
//! folder's tags and the notes linking to this one.
//!
//! Everything here reads the disk only when a pane is opened or refreshed, never per frame, and
//! stops at fixed limits so a huge folder cannot stall the window.

use crate::{Kind, TextApp, file, markdown};
use egui::{Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Files and folders listed before a scan stops.
pub const MAX_ENTRIES: usize = 5000;
/// Folder depth below the root a scan descends to.
const MAX_DEPTH: usize = 12;
/// Larger files are skipped by search and by the note index.
const MAX_FILE_BYTES: u64 = 4 << 20;
/// Search stops after this many matching lines.
pub const MAX_HITS: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    Files,
    Search,
    Outline,
    Tags,
    Backlinks,
}

impl Pane {
    pub const ALL: [Pane; 5] = [
        Pane::Files,
        Pane::Search,
        Pane::Outline,
        Pane::Tags,
        Pane::Backlinks,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Pane::Files => "files",
            Pane::Search => "search",
            Pane::Outline => "outline",
            Pane::Tags => "tags",
            Pane::Backlinks => "backlinks",
        }
    }

    pub fn from_name(name: &str) -> Option<Pane> {
        Pane::ALL.into_iter().find(|p| p.name() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Pane::Files => "文件",
            Pane::Search => "搜索",
            Pane::Outline => "大纲",
            Pane::Tags => "标签",
            Pane::Backlinks => "反向链接",
        }
    }

    /// Whether the pane applies to this kind of document.
    pub fn available(self, kind: Kind) -> bool {
        kind == Kind::Markdown || matches!(self, Pane::Files | Pane::Search)
    }
}

/// A folder's files as a tree of names.
#[derive(Default, Debug)]
pub struct Node {
    pub dirs: BTreeMap<String, Node>,
    pub files: Vec<String>,
}

#[derive(Debug)]
pub struct Folder {
    pub root: PathBuf,
    pub tree: Node,
    /// Every file, absolute, in tree order.
    pub files: Vec<PathBuf>,
    /// The scan stopped at [`MAX_ENTRIES`].
    pub truncated: bool,
}

/// The lines of one file that matched.
#[derive(Debug, Clone)]
pub struct FileHits {
    pub path: PathBuf,
    /// `(zero-based line, line text, byte range of the match in that text)`.
    pub lines: Vec<(usize, String, std::ops::Range<usize>)>,
}

#[derive(Debug, Default)]
pub struct SearchResults {
    pub query: String,
    pub files: Vec<FileHits>,
    pub total: usize,
    pub truncated: bool,
}

/// What the folder's Markdown notes say: their tags, and which of them link to the open note.
#[derive(Debug, Default)]
pub struct Notes {
    /// `(tag without '#', count)`, most used first.
    pub tags: Vec<(String, usize)>,
    pub backlinks: Vec<FileHits>,
    /// The note the backlinks were collected for.
    pub target: Option<PathBuf>,
}

#[derive(Default)]
pub struct Sidebar {
    pub pane: Option<Pane>,
    pub folder: Option<Folder>,
    expanded: HashSet<PathBuf>,
    pub query: String,
    pub match_case: bool,
    pub results: Option<SearchResults>,
    pub notes: Option<Notes>,
    /// Focus the search box on the next frame.
    focus_query: bool,
}

/// The folder a note's sidebar shows: the nearest enclosing vault (`.obsidian`) or repository
/// (`.git`) within a few levels, otherwise the note's own folder.
pub fn root_for(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    for dir in parent.ancestors().take(5) {
        if home.as_deref() == Some(dir) || dir.parent().is_none() {
            break;
        }
        if dir.join(".obsidian").is_dir() || dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
    }
    Some(parent.to_path_buf())
}

fn skipped(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules" | "target" | "__pycache__" | "dist" | "build" | "venv"
        )
}

/// List `root`'s files and folders, folders first, each sorted by name ignoring case.
pub fn scan(root: &Path) -> Folder {
    fn walk(
        dir: &Path,
        node: &mut Node,
        depth: usize,
        count: &mut usize,
        files: &mut Vec<PathBuf>,
    ) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return true;
        };
        let mut entries: Vec<_> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let is_dir = e.file_type().ok()?.is_dir();
                (!skipped(&name)).then_some((name, is_dir))
            })
            .collect();
        entries.sort_by_key(|(name, is_dir)| (!is_dir, name.to_lowercase()));
        for (name, is_dir) in entries {
            if *count >= MAX_ENTRIES {
                return false;
            }
            *count += 1;
            let path = dir.join(&name);
            if is_dir {
                let child = node.dirs.entry(name).or_default();
                if depth < MAX_DEPTH && !walk(&path, child, depth + 1, count, files) {
                    return false;
                }
            } else {
                node.files.push(name);
                files.push(path);
            }
        }
        true
    }
    let mut tree = Node::default();
    let mut files = Vec::new();
    let mut count = 0;
    let complete = walk(root, &mut tree, 0, &mut count, &mut files);
    Folder {
        root: root.to_path_buf(),
        tree,
        files,
        truncated: !complete,
    }
}

/// The text of a file if it is a text file within the size limit.
fn read_text(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if !file::looks_like_text(&bytes) {
        return None;
    }
    file::decode(&bytes).ok().map(|(text, _)| text)
}

fn is_markdown(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "md" | "markdown" | "mdown" | "mkd" | "mdx"
        )
    })
}

/// A line around `range`, shortened to about 100 characters, with the range moved to match.
fn excerpt(line: &str, range: std::ops::Range<usize>) -> (String, std::ops::Range<usize>) {
    let trimmed = line.trim_end();
    let mut start = 0;
    if range.start > 60 {
        start = range.start - 40;
        while !trimmed.is_char_boundary(start) {
            start += 1;
        }
    }
    let mut end = trimmed
        .len()
        .min(start + 160)
        .max(range.end.min(trimmed.len()));
    while !trimmed.is_char_boundary(end) {
        end += 1;
    }
    let lead = if start > 0 { "…" } else { "" };
    let text = format!("{lead}{}", &trimmed[start..end]);
    let shift = lead.len() as isize - start as isize;
    let moved = |at: usize| ((at as isize + shift).max(0) as usize).min(text.len());
    let range = moved(range.start)..moved(range.end.min(end));
    (text, range)
}

/// Search `files` for `query`, line by line. `open` supplies the unsaved text of the open
/// document so its results match what is on screen.
pub fn search(
    files: &[PathBuf],
    query: &str,
    match_case: bool,
    open: Option<(&Path, &str)>,
) -> SearchResults {
    let mut results = SearchResults {
        query: query.to_owned(),
        ..Default::default()
    };
    if query.is_empty() {
        return results;
    }
    let needle = if match_case {
        query.to_owned()
    } else {
        query.to_lowercase()
    };
    for path in files {
        let text = match open {
            Some((open_path, text)) if open_path == path => Some(text.to_owned()),
            _ => read_text(path),
        };
        let Some(text) = text else { continue };
        let mut hits = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let found = if match_case {
                line.find(&needle).map(|at| at..at + needle.len())
            } else {
                find_ignoring_case(line, &needle)
            };
            if let Some(range) = found {
                let (text, range) = excerpt(line, range);
                hits.push((number, text, range));
                results.total += 1;
                if results.total >= MAX_HITS {
                    results.truncated = true;
                    break;
                }
            }
        }
        if !hits.is_empty() {
            results.files.push(FileHits {
                path: path.clone(),
                lines: hits,
            });
        }
        if results.truncated {
            break;
        }
    }
    results
}

/// The byte range of the first case-insensitive match of the lowercase `needle` in `line`.
fn find_ignoring_case(line: &str, needle: &str) -> Option<std::ops::Range<usize>> {
    if line.is_ascii() && needle.is_ascii() {
        let haystack = line.to_ascii_lowercase();
        return haystack.find(needle).map(|at| at..at + needle.len());
    }
    // Lowercasing can change byte lengths, so compare from every character boundary.
    let target: Vec<char> = needle.chars().collect();
    for (start, _) in line.char_indices() {
        let mut end = start;
        let mut matched = 0;
        let mut source = line[start..].char_indices();
        while matched < target.len() {
            let Some((offset, ch)) = source.next() else {
                break;
            };
            let lower: Vec<char> = ch.to_lowercase().collect();
            if target[matched..].starts_with(&lower) {
                matched += lower.len();
                end = start + offset + ch.len_utf8();
            } else {
                break;
            }
        }
        if matched == target.len() {
            return Some(start..end);
        }
    }
    None
}

/// The note a link target names, reduced for comparison: its file name without `.md`,
/// lowercase, and without any `#heading` or `|alias`.
fn note_key(target: &str) -> Option<String> {
    let target = target.split(['#', '|', '^']).next()?.trim();
    if target.is_empty() || target.contains("://") || target.starts_with("mailto:") {
        return None;
    }
    let decoded = crate::ui::percent_decode(target);
    let name = decoded.rsplit(['/', '\\']).next()?.to_owned();
    let name = match name.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("md") => stem.to_owned(),
        _ => name,
    };
    Some(name.to_lowercase())
}

/// Read the folder's Markdown notes for their tags and the notes linking to `target`.
/// `open` supplies the open document's unsaved text.
pub fn index_notes(files: &[PathBuf], target: Option<&Path>, open: Option<(&Path, &str)>) -> Notes {
    let key = target
        .and_then(Path::file_stem)
        .map(|s| s.to_string_lossy().to_lowercase());
    let mut counts: HashMap<String, (String, usize)> = HashMap::new();
    let mut backlinks = Vec::new();
    for path in files.iter().filter(|p| is_markdown(p)) {
        let text = match open {
            Some((open_path, text)) if open_path == path => Some(text.to_owned()),
            _ => read_text(path),
        };
        let Some(text) = text else { continue };
        let parsed = markdown::parse(&text);
        let mut run: Option<usize> = None;
        for (at, flags) in parsed.flags.iter().chain(std::iter::once(&0)).enumerate() {
            let tag = flags & markdown::flag::TAG != 0;
            match (tag, run) {
                (true, None) => run = Some(at),
                (false, Some(start)) => {
                    let name = text[start..at].trim_start_matches('#');
                    if !name.is_empty() {
                        let entry = counts
                            .entry(name.to_lowercase())
                            .or_insert_with(|| (name.to_owned(), 0));
                        entry.1 += 1;
                    }
                    run = None;
                }
                _ => {}
            }
        }
        if let Some(key) = &key
            && Some(path.as_path()) != target
        {
            let mut lines = Vec::new();
            for link in &parsed.links {
                if note_key(&link.target).as_ref() == Some(key) {
                    let start = crate::edit::line_start(&text, link.range.start);
                    let end = crate::edit::line_end(&text, link.range.start);
                    let line = text[..start].matches('\n').count();
                    let range = link.range.start - start..link.range.end.min(end) - start;
                    let (excerpt, range) = excerpt(&text[start..end], range);
                    if lines.last().map(|(l, _, _)| *l) != Some(line) {
                        lines.push((line, excerpt, range));
                    }
                }
            }
            if !lines.is_empty() {
                backlinks.push(FileHits {
                    path: path.clone(),
                    lines,
                });
            }
        }
    }
    let mut tags: Vec<(String, usize)> = counts.into_values().collect();
    tags.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    Notes {
        tags,
        backlinks,
        target: target.map(Path::to_path_buf),
    }
}

/// A new note's path in `dir` that does not exist yet: `未命名.md`, `未命名 1.md`, …
pub fn new_note_path(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.md"));
    if !first.exists() {
        return first;
    }
    (1..)
        .map(|n| dir.join(format!("{stem} {n}.md")))
        .find(|p| !p.exists())
        .expect("a free name")
}

// ---- State shared by every text tab ----
//
// The host swaps egui's memory per tab, so what all text tabs share lives here: which pane the
// sidebar shows (it stays put as you move between notes, as in Obsidian), and a line to jump to
// once a file opened from search or backlinks shows up in its tab.

#[derive(Default)]
struct Shared {
    /// `None` until a tab with a window has chosen.
    pane: Option<Option<Pane>>,
    jump: Option<(PathBuf, usize)>,
    /// The search query and its case option, so search results follow you to the next tab.
    query: Option<(String, bool)>,
}

static SHARED: std::sync::Mutex<Shared> = std::sync::Mutex::new(Shared {
    pane: None,
    jump: None,
    query: None,
});

fn shared() -> std::sync::MutexGuard<'static, Shared> {
    SHARED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Show `pane` (or close the sidebar) here and, for tabs in a window, in every other text tab.
pub fn set_pane(app: &mut TextApp, pane: Option<Pane>) {
    app.sidebar.pane = pane;
    if app.ctx.is_some() {
        shared().pane = Some(pane);
    }
    match pane {
        Some(Pane::Search) => app.sidebar.focus_query = true,
        Some(Pane::Tags | Pane::Backlinks) => app.sidebar.notes = None,
        Some(Pane::Files) => app.sidebar.folder = None,
        _ => {}
    }
}

pub fn toggle_pane(app: &mut TextApp, pane: Pane) {
    let next = (app.sidebar.pane != Some(pane)).then_some(pane);
    set_pane(app, next);
}

/// Open `path` in a tab (the host reuses an open one) and put the caret on `line` there.
fn open_at(app: &mut TextApp, path: &Path, line: Option<usize>) {
    if app.path.as_deref() == Some(path) {
        if let Some(line) = line {
            app.jump_to_line(line);
        }
        return;
    }
    shared().jump = line.map(|line| (path.to_path_buf(), line));
    app.host_commands
        .push(("file.open".into(), json!({"path": path})));
}

/// Take the shared pane choice, and a jump another tab left for this file.
pub fn begin_frame(app: &mut TextApp) {
    let mut shared = shared();
    match shared.pane {
        Some(pane) if pane != app.sidebar.pane => {
            app.sidebar.pane = pane;
            // What other notes say may have changed while this tab was hidden.
            app.sidebar.notes = None;
            app.sidebar.folder = None;
        }
        Some(_) => {}
        None => shared.pane = Some(app.sidebar.pane),
    }
    if let Some((query, match_case)) = &shared.query
        && (query != &app.sidebar.query || *match_case != app.sidebar.match_case)
    {
        app.sidebar.query = query.clone();
        app.sidebar.match_case = *match_case;
        app.sidebar.results = None;
    }
    if let Some((path, line)) = shared.jump.clone()
        && let Some(own) = &app.path
        && (own == &path || own.canonicalize().ok() == path.canonicalize().ok())
    {
        shared.jump = None;
        drop(shared);
        app.jump_to_line(line);
    }
}

// ---- Drawing ----

fn tr<'a>(app: &TextApp, text: &'a str) -> &'a str {
    crate::i18n::tr(app.zh, text)
}

/// The strip of pane buttons at the window's left edge.
pub fn activity_bar(app: &mut TextApp, ui: &mut Ui) {
    ui.add_space(6.0);
    let mut clicked = None;
    for pane in Pane::ALL {
        if !pane.available(app.kind) {
            continue;
        }
        let selected = app.sidebar.pane == Some(pane);
        let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
        let visuals = ui.visuals();
        if selected || response.hovered() {
            ui.painter().rect_filled(
                rect.shrink(1.0),
                5,
                if selected {
                    visuals.widgets.active.weak_bg_fill
                } else {
                    visuals.widgets.hovered.weak_bg_fill
                },
            );
        }
        let ink = if selected {
            visuals.strong_text_color()
        } else {
            visuals.weak_text_color()
        };
        paint_icon(ui.painter(), rect.shrink(7.0), pane, ink);
        let label = tr(app, pane.label());
        response
            .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
        if response.on_hover_text(label).clicked() {
            clicked = Some(pane);
        }
        ui.add_space(2.0);
    }
    if let Some(pane) = clicked {
        toggle_pane(app, pane);
    }
}

/// Simple line icons for the pane buttons, drawn on a 16-unit square.
fn paint_icon(painter: &egui::Painter, rect: Rect, pane: Pane, ink: Color32) {
    let s = rect.width() / 16.0;
    let p = |x: f32, y: f32| pos2(rect.min.x + x * s, rect.min.y + y * s);
    let stroke = Stroke::new(1.4, ink);
    let line = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().map(|&(x, y)| p(x, y)).collect(),
            stroke,
        ));
    };
    match pane {
        Pane::Files => {
            line(&[
                (1.0, 3.0),
                (6.0, 3.0),
                (7.5, 4.8),
                (15.0, 4.8),
                (15.0, 13.5),
                (1.0, 13.5),
                (1.0, 3.0),
            ]);
            line(&[(1.0, 6.8), (15.0, 6.8)]);
        }
        Pane::Search => {
            painter.circle_stroke(p(6.5, 6.5), 4.8 * s, stroke);
            line(&[(10.0, 10.0), (14.8, 14.8)]);
        }
        Pane::Outline => {
            line(&[(1.0, 3.0), (15.0, 3.0)]);
            line(&[(4.0, 7.0), (15.0, 7.0)]);
            line(&[(4.0, 11.0), (15.0, 11.0)]);
            line(&[(7.0, 15.0), (15.0, 15.0)]);
        }
        Pane::Tags => {
            line(&[(5.5, 1.5), (4.0, 14.5)]);
            line(&[(11.5, 1.5), (10.0, 14.5)]);
            line(&[(1.5, 5.5), (14.5, 5.5)]);
            line(&[(1.0, 10.5), (14.0, 10.5)]);
        }
        Pane::Backlinks => {
            // Two links of a chain.
            painter.rect_stroke(
                Rect::from_min_max(p(0.8, 5.0), p(9.0, 11.0)),
                3.0 * s,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.rect_stroke(
                Rect::from_min_max(p(7.0, 5.0), p(15.2, 11.0)),
                3.0 * s,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
    }
}

fn header(app: &TextApp, ui: &mut Ui, title: &str, actions: &mut dyn FnMut(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            actions(ui);
        });
    });
    let _ = app;
    ui.add_space(4.0);
}

/// One row of a list: indent, optional disclosure triangle, and a label clipped to the width.
fn row(
    ui: &mut Ui,
    depth: usize,
    label: &str,
    selected: bool,
    folder: Option<bool>,
    weak: bool,
) -> egui::Response {
    let height = 22.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let visuals = ui.visuals();
    if selected {
        ui.painter()
            .rect_filled(rect, 4, visuals.selection.bg_fill.gamma_multiply(0.35));
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, 4, visuals.widgets.hovered.weak_bg_fill);
    }
    let x = rect.left() + 4.0 + depth as f32 * 14.0;
    let ink = if weak {
        visuals.weak_text_color()
    } else {
        visuals.text_color()
    };
    if let Some(open) = folder {
        let c = pos2(x + 5.0, rect.center().y);
        let points = if open {
            vec![
                c + vec2(-3.5, -2.0),
                c + vec2(3.5, -2.0),
                c + vec2(0.0, 2.5),
            ]
        } else {
            vec![
                c + vec2(-2.0, -3.5),
                c + vec2(2.5, 0.0),
                c + vec2(-2.0, 3.5),
            ]
        };
        ui.painter().add(egui::Shape::convex_polygon(
            points,
            visuals.weak_text_color(),
            Stroke::NONE,
        ));
    }
    ui.painter().with_clip_rect(rect).text(
        Pos2::new(x + 14.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        ink,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn small_button(ui: &mut Ui, text: &str, hint: &str) -> bool {
    ui.add(
        egui::Button::new(RichText::new(text).size(13.0))
            .frame(false)
            .small(),
    )
    .on_hover_text(hint)
    .clicked()
}

/// The pane behind the selected button.
pub fn pane(app: &mut TextApp, ui: &mut Ui) {
    let Some(pane) = app.sidebar.pane.filter(|p| p.available(app.kind)) else {
        return;
    };
    match pane {
        Pane::Files => files_pane(app, ui),
        Pane::Search => search_pane(app, ui),
        Pane::Outline => {
            header(app, ui, tr(app, "大纲"), &mut |_| {});
            crate::ui::outline(app, ui);
        }
        Pane::Tags => tags_pane(app, ui),
        Pane::Backlinks => backlinks_pane(app, ui),
    }
}

fn ensure_folder(app: &mut TextApp) -> bool {
    let Some(root) = app.path.as_deref().and_then(root_for) else {
        return false;
    };
    if app.sidebar.folder.as_ref().map(|f| &f.root) != Some(&root) {
        // Open the folders on the way to the current file.
        if let Some(path) = &app.path {
            for dir in path.ancestors().skip(1) {
                if !dir.starts_with(&root) {
                    break;
                }
                app.sidebar.expanded.insert(dir.to_path_buf());
            }
        }
        app.sidebar.folder = Some(scan(&root));
    }
    true
}

fn no_folder(app: &TextApp, ui: &mut Ui) {
    ui.add_space(8.0);
    ui.weak(tr(app, "保存后可浏览所在文件夹"));
}

fn files_pane(app: &mut TextApp, ui: &mut Ui) {
    if !ensure_folder(app) {
        header(app, ui, tr(app, "文件"), &mut |_| {});
        no_folder(app, ui);
        return;
    }
    let root = app
        .sidebar
        .folder
        .as_ref()
        .map(|f| f.root.clone())
        .unwrap_or_default();
    let title = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string());
    let mut refresh = false;
    let mut new_note = false;
    let mut collapse = false;
    let (new_hint, refresh_hint, collapse_hint) =
        (tr(app, "新建笔记"), tr(app, "刷新"), tr(app, "全部折叠"));
    header(app, ui, &title, &mut |ui| {
        collapse = small_button(ui, "⊟", collapse_hint);
        refresh = small_button(ui, "⟳", refresh_hint);
        new_note = small_button(ui, "+", new_hint);
    });
    if collapse {
        app.sidebar.expanded.clear();
    }
    if new_note {
        let dir = app
            .path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or(&root)
            .to_path_buf();
        let path = new_note_path(&dir, tr(app, "未命名"));
        match file::write_atomically(&path, b"") {
            Ok(()) => {
                refresh = true;
                open_at(app, &path, None);
            }
            Err(error) => app.notice = Some(error),
        }
    }
    if refresh {
        app.sidebar.folder = None;
        ensure_folder(app);
    }
    let Some(folder) = app.sidebar.folder.take() else {
        return;
    };
    let mut open = None;
    let mut toggled = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            tree(
                app,
                ui,
                &folder.tree,
                &folder.root,
                0,
                &mut open,
                &mut toggled,
            );
            if folder.truncated {
                ui.add_space(6.0);
                ui.weak(tr(app, "文件过多，只列出前 5000 项"));
            }
        });
    app.sidebar.folder = Some(folder);
    if let Some(dir) = toggled
        && !app.sidebar.expanded.remove(&dir)
    {
        app.sidebar.expanded.insert(dir);
    }
    if let Some(path) = open {
        open_at(app, &path, None);
    }
}

fn tree(
    app: &TextApp,
    ui: &mut Ui,
    node: &Node,
    dir: &Path,
    depth: usize,
    open: &mut Option<PathBuf>,
    toggled: &mut Option<PathBuf>,
) {
    for (name, child) in &node.dirs {
        let path = dir.join(name);
        let expanded = app.sidebar.expanded.contains(&path);
        if row(ui, depth, name, false, Some(expanded), false).clicked() {
            *toggled = Some(path.clone());
        }
        if expanded {
            tree(app, ui, child, &path, depth + 1, open, toggled);
        }
    }
    for name in &node.files {
        let path = dir.join(name);
        let markdown = is_markdown(&path);
        let label = if markdown {
            name.rsplit_once('.')
                .map_or(name.as_str(), |(stem, _)| stem)
        } else {
            name.as_str()
        };
        let current = app.path.as_deref() == Some(path.as_path());
        let response = row(ui, depth, label, current, None, false);
        if response.on_hover_text(name).clicked() {
            *open = Some(path);
        }
    }
}

fn hit_list(
    app: &mut TextApp,
    ui: &mut Ui,
    files: &[FileHits],
    root: Option<&Path>,
) -> Option<(PathBuf, usize)> {
    let mut chosen = None;
    let visuals = ui.visuals().clone();
    for hits in files {
        let name = root
            .and_then(|r| hits.path.strip_prefix(r).ok())
            .unwrap_or(&hits.path)
            .to_string_lossy()
            .into_owned();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&name).strong().size(12.5));
            ui.weak(hits.lines.len().to_string());
        });
        for (line, text, range) in &hits.lines {
            let mut job = egui::text::LayoutJob::default();
            let font = FontId::proportional(12.5);
            let plain = egui::TextFormat::simple(font.clone(), visuals.text_color());
            let mut found = plain.clone();
            found.background = Color32::from_rgba_unmultiplied(250, 200, 40, 110);
            let range = range.start.min(text.len())..range.end.min(text.len());
            job.append(&text[..range.start], 0.0, plain.clone());
            job.append(&text[range.clone()], 0.0, found);
            job.append(&text[range.end..], 0.0, plain);
            job.wrap.max_rows = 2;
            job.wrap.max_width = ui.available_width() - 12.0;
            let response = ui
                .add(
                    egui::Label::new(job)
                        .sense(Sense::click())
                        .selectable(false),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.hovered() {
                ui.painter().rect_stroke(
                    response.rect.expand(2.0),
                    3,
                    Stroke::new(1.0, visuals.widgets.hovered.bg_stroke.color),
                    egui::StrokeKind::Outside,
                );
            }
            if response.clicked() {
                chosen = Some((hits.path.clone(), *line));
            }
        }
    }
    let _ = app;
    chosen
}

fn search_pane(app: &mut TextApp, ui: &mut Ui) {
    header(app, ui, tr(app, "搜索"), &mut |_| {});
    if !ensure_folder(app) {
        no_folder(app, ui);
        return;
    }
    let hint = tr(app, "在文件夹中搜索");
    let response = ui.add(
        egui::TextEdit::singleline(&mut app.sidebar.query)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );
    if std::mem::take(&mut app.sidebar.focus_query) {
        response.request_focus();
    }
    let case_label = tr(app, "区分大小写");
    let case_changed = ui
        .checkbox(&mut app.sidebar.match_case, case_label)
        .changed();
    if response.changed() || case_changed {
        shared().query = Some((app.sidebar.query.clone(), app.sidebar.match_case));
    }
    let submitted = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let stale = app
        .sidebar
        .results
        .as_ref()
        .is_none_or(|r| r.query != app.sidebar.query);
    // Short queries search on Enter; longer ones as you type.
    if submitted
        || case_changed
        || (stale && response.changed() && app.sidebar.query.chars().count() >= 3)
        || (stale && app.sidebar.query.is_empty())
        || app.sidebar.results.is_none()
    {
        shared().query = Some((app.sidebar.query.clone(), app.sidebar.match_case));
        let files = app
            .sidebar
            .folder
            .as_ref()
            .map(|f| f.files.clone())
            .unwrap_or_default();
        let open = app.path.clone();
        let results = search(
            &files,
            &app.sidebar.query,
            app.sidebar.match_case,
            open.as_deref().map(|p| (p, app.text.as_str())),
        );
        app.sidebar.results = Some(results);
    }
    let Some(results) = app.sidebar.results.take() else {
        return;
    };
    if !results.query.is_empty() {
        let summary = if results.total == 0 {
            tr(app, "没有结果").to_owned()
        } else {
            tr(app, "{files} 个文件中有 {hits} 处结果")
                .replace("{files}", &results.files.len().to_string())
                .replace("{hits}", &results.total.to_string())
        };
        ui.weak(summary);
        if results.truncated {
            ui.weak(tr(app, "结果过多，只显示前 2000 处"));
        }
    }
    let root = app.sidebar.folder.as_ref().map(|f| f.root.clone());
    let chosen = egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| hit_list(app, ui, &results.files, root.as_deref()))
        .inner;
    app.sidebar.results = Some(results);
    if let Some((path, line)) = chosen {
        open_at(app, &path, Some(line));
    }
}

fn ensure_notes(app: &mut TextApp) -> bool {
    if !ensure_folder(app) {
        return false;
    }
    if app.sidebar.notes.as_ref().map(|n| n.target.as_deref()) != Some(app.path.as_deref()) {
        let files = app
            .sidebar
            .folder
            .as_ref()
            .map(|f| f.files.clone())
            .unwrap_or_default();
        let path = app.path.clone();
        app.sidebar.notes = Some(index_notes(
            &files,
            path.as_deref(),
            path.as_deref().map(|p| (p, app.text.as_str())),
        ));
    }
    true
}

fn tags_pane(app: &mut TextApp, ui: &mut Ui) {
    let mut refresh = false;
    let hint = tr(app, "刷新");
    header(app, ui, tr(app, "标签"), &mut |ui| {
        refresh = small_button(ui, "⟳", hint)
    });
    if refresh {
        app.sidebar.notes = None;
        app.sidebar.folder = None;
    }
    if !ensure_notes(app) {
        no_folder(app, ui);
        return;
    }
    let tags = app
        .sidebar
        .notes
        .as_ref()
        .map(|n| n.tags.clone())
        .unwrap_or_default();
    if tags.is_empty() {
        ui.weak(tr(app, "文件夹里的笔记还没有 #标签"));
        return;
    }
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (tag, count) in &tags {
                let response = row(ui, 0, &format!("#{tag}"), false, None, false);
                ui.painter().text(
                    pos2(response.rect.right() - 8.0, response.rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    count.to_string(),
                    FontId::proportional(12.0),
                    ui.visuals().weak_text_color(),
                );
                if response.clicked() {
                    chosen = Some(tag.clone());
                }
            }
        });
    if let Some(tag) = chosen {
        set_query(app, format!("#{tag}"));
        set_pane(app, Some(Pane::Search));
        app.sidebar.focus_query = false;
    }
}

/// Search the folder for `query` (shared with the other text tabs) when the search pane shows.
pub fn set_query(app: &mut TextApp, query: String) {
    if app.ctx.is_some() {
        shared().query = Some((query.clone(), app.sidebar.match_case));
    }
    app.sidebar.query = query;
    app.sidebar.results = None;
}

fn backlinks_pane(app: &mut TextApp, ui: &mut Ui) {
    let mut refresh = false;
    let hint = tr(app, "刷新");
    header(app, ui, tr(app, "反向链接"), &mut |ui| {
        refresh = small_button(ui, "⟳", hint)
    });
    if refresh {
        app.sidebar.notes = None;
        app.sidebar.folder = None;
    }
    if !ensure_notes(app) {
        no_folder(app, ui);
        return;
    }
    let backlinks = app
        .sidebar
        .notes
        .as_ref()
        .map(|n| n.backlinks.clone())
        .unwrap_or_default();
    if backlinks.is_empty() {
        ui.weak(tr(app, "还没有笔记链接到这里"));
        return;
    }
    let root = app.sidebar.folder.as_ref().map(|f| f.root.clone());
    let chosen = egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| hit_list(app, ui, &backlinks, root.as_deref()))
        .inner;
    if let Some((path, line)) = chosen {
        open_at(app, &path, Some(line));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sub/deep")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(
            root.join("Index.md"),
            "# Home\nSee [[Target]] and #project/alpha\n",
        )
        .unwrap();
        std::fs::write(
            root.join("sub/notes.md"),
            "link [t](../Target.md#part) #Project/alpha #idea\n",
        )
        .unwrap();
        std::fs::write(
            root.join("sub/deep/code.rs"),
            "fn main() { println!(\"Ünïcode needle\"); }\n",
        )
        .unwrap();
        std::fs::write(root.join("Target.md"), "# Target\nnothing #idea\n").unwrap();
        std::fs::write(root.join("image.png"), [0x89, b'P', b'N', b'G', 0, 0, 0, 0]).unwrap();
        std::fs::write(root.join(".hidden/secret.md"), "[[Target]]").unwrap();
        std::fs::write(root.join("node_modules/x.md"), "[[Target]]").unwrap();
        dir
    }

    #[test]
    fn scans_folders_first_and_skips_hidden_and_vendored() {
        let dir = folder();
        let folder = scan(dir.path());
        assert!(!folder.truncated);
        assert_eq!(folder.tree.dirs.keys().collect::<Vec<_>>(), ["sub"]);
        assert_eq!(folder.tree.files, ["image.png", "Index.md", "Target.md"]);
        assert_eq!(folder.files.len(), 5);
        assert!(
            folder
                .files
                .iter()
                .all(|p| !p.to_string_lossy().contains("hidden"))
        );
    }

    #[test]
    fn searches_text_files_ignoring_case_and_binaries() {
        let dir = folder();
        let folder = scan(dir.path());
        let results = search(&folder.files, "ÜNÏCODE", false, None);
        assert_eq!(results.total, 1);
        let (line, text, range) = &results.files[0].lines[0];
        assert_eq!(*line, 0);
        assert_eq!(&text[range.clone()], "Ünïcode");
        assert_eq!(search(&folder.files, "ÜNÏCODE", true, None).total, 0);
        assert_eq!(search(&folder.files, "PNG", false, None).total, 0);
        // The open document's unsaved text is searched instead of the file on disk.
        let target = dir.path().join("Target.md");
        let unsaved = search(
            &folder.files,
            "unsaved words",
            false,
            Some((&target, "unsaved words")),
        );
        assert_eq!(unsaved.total, 1);
    }

    #[test]
    fn collects_tags_and_backlinks_from_wiki_and_markdown_links() {
        let dir = folder();
        let folder = scan(dir.path());
        let target = dir.path().join("Target.md");
        let notes = index_notes(&folder.files, Some(&target), None);
        assert_eq!(notes.tags[0], ("idea".into(), 2));
        // Tags group ignoring case, as in Obsidian.
        assert_eq!(notes.tags[1].0.to_lowercase(), "project/alpha");
        assert_eq!(notes.tags[1].1, 2);
        let linking: Vec<_> = notes
            .backlinks
            .iter()
            .map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        // In tree order: subfolders first.
        assert_eq!(linking, ["notes.md", "Index.md"]);
        let (line, text, range) = &notes.backlinks[1].lines[0];
        assert_eq!(*line, 1);
        assert_eq!(&text[range.clone()], "Target");
    }

    #[test]
    fn link_targets_reduce_to_note_names() {
        assert_eq!(note_key("Target#Heading").as_deref(), Some("target"));
        assert_eq!(note_key("../dir/My%20Note.md").as_deref(), Some("my note"));
        assert_eq!(note_key("Target|alias").as_deref(), Some("target"));
        assert_eq!(note_key("https://example.com/a.md"), None);
        assert_eq!(note_key("#local"), None);
    }

    #[test]
    fn new_notes_get_a_free_name() {
        let dir = tempfile::tempdir().unwrap();
        let first = new_note_path(dir.path(), "未命名");
        std::fs::write(&first, "").unwrap();
        let second = new_note_path(dir.path(), "未命名");
        assert_eq!(second.file_name().unwrap(), "未命名 1.md");
    }

    #[test]
    fn excerpts_keep_the_match_visible() {
        let line = format!("{}needle tail", "x".repeat(200));
        let (text, range) = excerpt(&line, 200..206);
        assert!(text.starts_with('…'));
        assert_eq!(&text[range], "needle");
    }

    #[test]
    fn root_prefers_an_enclosing_vault() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("vault/.obsidian")).unwrap();
        std::fs::create_dir_all(dir.path().join("vault/a/b")).unwrap();
        let note = dir.path().join("vault/a/b/n.md");
        assert_eq!(root_for(&note).unwrap(), dir.path().join("vault"));
        let loose = dir.path().join("n.md");
        assert_eq!(root_for(&loose).unwrap(), dir.path());
    }
}
