//! Changing documents by path: `set` properties on a node, `add` a node, `remove` a node.
//! Each verb is a short run of the engines' own commands (place the caret, select, format …),
//! so a change made here is exactly what the same clicks in the editor would make.

use serde_json::{Map, Value, json};

use crate::engines::{Doc, Kind};
use crate::error::CliError;
use crate::view::{self, Step};

/// Where `add` puts the new node.
#[derive(Clone, Debug, Default)]
pub enum Position {
    #[default]
    End,
    After(String),
    Before(String),
    /// 1-based position among the parent's children.
    Index(usize),
}

/// What a change did: the engine commands it ran, for the reply.
struct Log<'a> {
    doc: &'a mut Doc,
    steps: Vec<String>,
}

impl<'a> Log<'a> {
    fn new(doc: &'a mut Doc) -> Self {
        Self {
            doc,
            steps: Vec::new(),
        }
    }
    fn run(&mut self, id: &str, params: Value) -> Result<Value, CliError> {
        self.steps.push(id.to_string());
        self.doc.execute(id, params)
    }
}

fn unsupported(kind: &str, prop: &str, known: &[&str]) -> CliError {
    CliError::new("invalid_params", format!("{kind} has no property `{prop}`"))
        .suggest(format!("properties: {}", known.join(", ")))
        .with_candidates(crate::suggest::closest(prop, known.iter().copied(), 3))
}

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn as_bool(key: &str, v: &Value) -> Result<bool, CliError> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::String(s) if s == "true" || s == "on" || s == "yes" => Ok(true),
        Value::String(s) if s == "false" || s == "off" || s == "no" => Ok(false),
        _ => Err(CliError::new(
            "invalid_params",
            format!("{key}: give true or false"),
        )),
    }
}

fn as_num(key: &str, v: &Value) -> Result<f64, CliError> {
    v.as_f64()
        .or_else(|| {
            v.as_str()
                .and_then(|s| s.trim_end_matches("pt").parse().ok())
        })
        .ok_or_else(|| CliError::new("invalid_params", format!("{key}: give a number")))
}

/// `#RRGGBB` → `RRGGBB` (document engine) or the other way round.
fn hex(v: &Value, with_hash: bool) -> String {
    let s = as_text(v);
    let bare = s.trim_start_matches('#');
    let is_hex = bare.len() == 6 && bare.chars().all(|c| c.is_ascii_hexdigit());
    match (is_hex, with_hash) {
        (true, true) => format!("#{}", bare.to_ascii_uppercase()),
        (true, false) => bare.to_ascii_uppercase(),
        _ => s,
    }
}

fn reply(path: &str, log: Log, extra: Value) -> Value {
    let mut v = json!({"path": path, "commands": log.steps});
    if let (Some(o), Value::Object(e)) = (v.as_object_mut(), extra) {
        o.extend(e);
    }
    v
}

pub fn set(doc: &mut Doc, path: &str, props: &Map<String, Value>) -> Result<Value, CliError> {
    if props.is_empty() {
        return Err(CliError::usage("say what to change: key=value …").suggest(
            "e.g. text=\"New text\" bold=true; `weave-office help set` lists properties",
        ));
    }
    let steps = view::parse_path(path)?;
    match doc.kind() {
        Kind::Word => word::set(doc, path, &steps, props),
        Kind::Sheet => sheet::set(doc, path, &steps, props),
        Kind::Slides => slides::set(doc, path, &steps, props),
        Kind::Pdf => pdf::set(doc, path, &steps, props),
    }
}

pub fn add(
    doc: &mut Doc,
    parent: &str,
    kind: &str,
    at: &Position,
    props: &Map<String, Value>,
) -> Result<Value, CliError> {
    let steps = view::parse_path(parent)?;
    match doc.kind() {
        Kind::Word => word::add(doc, parent, &steps, kind, at, props),
        Kind::Sheet => sheet::add(doc, parent, &steps, kind, at, props),
        Kind::Slides => slides::add(doc, parent, &steps, kind, at, props),
        Kind::Pdf => pdf::add(doc, parent, &steps, kind, at, props),
    }
}

pub fn remove(doc: &mut Doc, path: &str) -> Result<Value, CliError> {
    let steps = view::parse_path(path)?;
    if steps.is_empty() {
        return Err(CliError::new(
            "invalid_path",
            "remove needs a node, not the whole document",
        ));
    }
    match doc.kind() {
        Kind::Word => word::remove(doc, path, &steps),
        Kind::Sheet => sheet::remove(doc, path, &steps),
        Kind::Slides => slides::remove(doc, path, &steps),
        Kind::Pdf => pdf::remove(doc, path, &steps),
    }
}

/// The properties each kind of node takes (for help and errors).
pub const PROPS_HELP: &str = "Properties for set/add (key=value; key:=JSON for lists)

Documents     /body/p[N]       text style (Heading1, Normal, Title …) align (left|center|right|justify)
                               bold italic underline strike (true|false) font size color (RRGGBB) highlight
              add /body --type paragraph|heading|table|pagebreak
                               heading: level=1..9 · table: rows cols
Spreadsheets  /Sheet1/A1       value (text, number or =formula)
              /Sheet1/A1:C3    values:=[[…],[…]] (from the range's top-left cell)
              cell or range    bold italic underline (true|false) color fill (#RRGGBB) font size
                               format (General|Number|Currency|Percentage|Short Date|… or a code like 0.00)
                               align (left|center|right) wrap (true|false) merge (true|false)
              /Sheet1          name tabColor
              add / --type sheet name=…      remove /Sheet2  ·  remove /Sheet1/A1:B2 clears the cells
Presentations /slide[N]        layout notes hidden
              /slide[N]/shape[M]  text (\\n starts a paragraph) bold italic underline font size color
                               align fill (#RRGGBB) x y w h (points) name
              add / --type slide layout title body · add /slide[N] --type textbox text x y w h
PDF           /page[N]         rotate (90|180|270)
              add / --type page width height · add /page[N] --type text text x y (and page_add_text options)

Position for add: --after PATH, --before PATH or --index N (1-based); default: at the end.
";

// ------------------------------------------------------------------ documents

mod word {
    use super::*;

    pub(super) const PROPS: &[&str] = &[
        "text",
        "style",
        "align",
        "bold",
        "italic",
        "underline",
        "strike",
        "font",
        "size",
        "color",
        "highlight",
    ];

    fn pos(block: u64, off: u64) -> Value {
        json!({"story": "body", "path": [block], "off": off})
    }

    /// (engine block index, text length) of `/body/p[N]`, `/body/table[N]` or `/body/block[N]`.
    fn block(doc: &mut Doc, path: &str, steps: &[Step]) -> Result<(u64, u64, bool), CliError> {
        let node = view::get(doc, path)?;
        if steps.len() != 2 {
            return Err(CliError::new(
                "invalid_path",
                format!("`{path}`: name one block, e.g. /body/p[2]"),
            ));
        }
        let index = node
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| CliError::new("engine", "the block has no index"))?;
        let len = node.get("len").and_then(Value::as_u64).unwrap_or(0);
        let table = node.get("type").and_then(Value::as_str) == Some("table");
        Ok((index, len, table))
    }

    fn select_all(log: &mut Log, i: u64, len: u64) -> Result<(), CliError> {
        log.run(
            "select.range",
            json!({"anchor": pos(i, 0), "focus": pos(i, len)}),
        )
        .map(drop)
    }

    fn para_len(log: &mut Log, i: u64) -> Result<u64, CliError> {
        let tree = log.doc.inspect()?;
        Ok(tree["blocks"]
            .as_array()
            .and_then(|b| b.iter().find(|x| x["index"].as_u64() == Some(i)))
            .and_then(|b| b["len"].as_u64())
            .unwrap_or(0))
    }

    /// Apply `props` to the paragraph at engine block `i`.
    pub(super) fn apply(log: &mut Log, i: u64, props: &Map<String, Value>) -> Result<(), CliError> {
        if let Some(p) = props.keys().find(|k| !PROPS.contains(&k.as_str())) {
            return Err(unsupported("a paragraph", p, PROPS));
        }
        if let Some(t) = props.get("text") {
            let len = para_len(log, i)?;
            select_all(log, i, len)?;
            let t = as_text(t);
            if t.is_empty() {
                if len > 0 {
                    log.run("text.delete", json!({}))?;
                }
            } else {
                // `raw`: the text as given, without AutoCorrect (which would capitalise "a" to "A").
                log.run(
                    "text.insert",
                    json!({"text": t, "paragraphs": true, "raw": true}),
                )?;
            }
        }
        log.run("caret.set", json!({"pos": pos(i, 0)}))?;
        if let Some(s) = props.get("style") {
            log.run("para.style", json!({"style": as_text(s)}))?;
        }
        if let Some(a) = props.get("align") {
            log.run("para.align", json!({"value": as_text(a)}))?;
        }
        let chars: Vec<(&str, Value)> = props
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "text" | "style" | "align"))
            .map(|(k, v)| -> Result<(&str, Value), CliError> {
                Ok(match k.as_str() {
                    "bold" => ("format.bold", json!({"value": as_bool(k, v)?})),
                    "italic" => ("format.italic", json!({"value": as_bool(k, v)?})),
                    "underline" => ("format.underline", json!({"value": as_bool(k, v)?})),
                    "strike" => ("format.strikethrough", json!({"value": as_bool(k, v)?})),
                    "font" => ("format.font", json!({"name": as_text(v)})),
                    "size" => ("format.size", json!({"size": as_num(k, v)?})),
                    "color" => ("format.color", json!({"color": hex(v, false)})),
                    _ => ("format.highlight", json!({"color": as_text(v)})),
                })
            })
            .collect::<Result<_, _>>()?;
        if !chars.is_empty() {
            let len = para_len(log, i)?;
            select_all(log, i, len)?;
            for (id, p) in chars {
                log.run(id, p)?;
            }
        }
        Ok(())
    }

    pub(super) fn set(
        doc: &mut Doc,
        path: &str,
        steps: &[Step],
        props: &Map<String, Value>,
    ) -> Result<Value, CliError> {
        let (i, _, table) = block(doc, path, steps)?;
        if table {
            return Err(CliError::new(
                "invalid_path",
                "tables are changed with `run` (table.* commands) for now",
            )
            .suggest("weave-office commands word table"));
        }
        let mut log = Log::new(doc);
        apply(&mut log, i, props)?;
        let node = view::get(log.doc, path)?;
        Ok(reply(path, log, json!({"node": node})))
    }

    pub(super) fn add(
        doc: &mut Doc,
        _parent: &str,
        steps: &[Step],
        kind: &str,
        at: &Position,
        props: &Map<String, Value>,
    ) -> Result<Value, CliError> {
        if steps.len() != 1 || steps[0].name != "body" {
            return Err(CliError::new(
                "invalid_path",
                "documents add blocks to /body",
            ));
        }
        let mut props = props.clone();
        match kind {
            "paragraph" | "p" | "para" => {}
            "heading" | "title" => {
                let level = props
                    .remove("level")
                    .map(|l| as_num("level", &l))
                    .transpose()?
                    .unwrap_or(1.0) as u32;
                props.entry("style").or_insert(json!(if kind == "title" {
                    "Title".to_string()
                } else {
                    format!("Heading{}", level.clamp(1, 9))
                }));
            }
            "table" | "pagebreak" | "break" => {}
            other => {
                return Err(CliError::new(
                    "invalid_params",
                    format!("documents can't add `{other}`"),
                )
                .suggest("types: paragraph, heading, table, pagebreak"));
            }
        }
        let blocks = doc.inspect()?["blocks"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        // The block the new one goes after or before, as an engine index and length.
        let find = |doc: &mut Doc, p: &str| -> Result<(u64, u64), CliError> {
            let steps = view::parse_path(p)?;
            let (i, len, table) = block(doc, p, &steps)?;
            if table {
                return Err(CliError::new(
                    "invalid_path",
                    "place new blocks next to a paragraph, not a table",
                ));
            }
            Ok((i, len))
        };
        let last = blocks.last().map_or((0, 0), |b| {
            (
                b["index"].as_u64().unwrap_or(0),
                b["len"].as_u64().unwrap_or(0),
            )
        });
        // An empty last paragraph (a blank document's only one, or the one a table leaves
        // after itself) is filled rather than left above the new block.
        let only_empty = last.1 == 0 && blocks.last().is_some_and(|b| b["type"] != "table");
        let mut log = Log::new(doc);
        let (target, before) = match at {
            Position::End => (last, false),
            Position::After(p) => (find(log.doc, p)?, false),
            Position::Before(p) => (find(log.doc, p)?, true),
            Position::Index(n) => {
                let paras: Vec<&Value> = blocks.iter().filter(|b| b["type"] != "table").collect();
                match paras.get(n.saturating_sub(1)) {
                    Some(b) => (
                        (
                            b["index"].as_u64().unwrap_or(0),
                            b["len"].as_u64().unwrap_or(0),
                        ),
                        true,
                    ),
                    None => (last, false),
                }
            }
        };
        let new_index = if only_empty && matches!(at, Position::End) {
            log.run("caret.set", json!({"pos": pos(last.0, 0)}))?;
            last.0
        } else if before {
            log.run("caret.set", json!({"pos": pos(target.0, 0)}))?;
            log.run("text.newParagraph", json!({}))?;
            log.run("caret.set", json!({"pos": pos(target.0, 0)}))?;
            target.0
        } else {
            log.run("caret.set", json!({"pos": pos(target.0, target.1)}))?;
            log.run("text.newParagraph", json!({}))?;
            target.0 + 1
        };
        match kind {
            "table" => {
                let rows = props
                    .get("rows")
                    .map(|v| as_num("rows", v))
                    .transpose()?
                    .unwrap_or(2.0);
                let cols = props
                    .get("cols")
                    .map(|v| as_num("cols", v))
                    .transpose()?
                    .unwrap_or(2.0);
                log.run("para.normal", json!({}))?;
                log.run(
                    "insert.table",
                    json!({"rows": rows as u64, "cols": cols as u64}),
                )?;
            }
            "pagebreak" | "break" => {
                log.run("text.pageBreak", json!({}))?;
            }
            _ => {
                props.entry("style").or_insert(json!("Normal"));
                apply(&mut log, new_index, &props)?;
            }
        }
        let path = new_path(log.doc, new_index)?;
        let node = match &path {
            Some(p) => view::get(log.doc, p)?,
            None => Value::Null,
        };
        Ok(reply(
            path.as_deref().unwrap_or("/body"),
            log,
            json!({"node": node}),
        ))
    }

    /// The `/body/p[N]` or `/body/table[N]` path of engine block `index`.
    fn new_path(doc: &mut Doc, index: u64) -> Result<Option<String>, CliError> {
        let blocks = doc.inspect()?["blocks"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let (mut p, mut t) = (0, 0);
        for b in &blocks {
            let table = b["type"] == "table";
            if table {
                t += 1
            } else {
                p += 1
            }
            if b["index"].as_u64() == Some(index) {
                return Ok(Some(if table {
                    format!("/body/table[{t}]")
                } else {
                    format!("/body/p[{p}]")
                }));
            }
        }
        Ok(None)
    }

    pub(super) fn remove(doc: &mut Doc, path: &str, steps: &[Step]) -> Result<Value, CliError> {
        let (i, len, table) = block(doc, path, steps)?;
        let blocks = doc.inspect()?["blocks"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let at = blocks
            .iter()
            .position(|b| b["index"].as_u64() == Some(i))
            .unwrap_or(0);
        let mut log = Log::new(doc);
        if table {
            log.run(
                "caret.set",
                json!({"pos": {"story": "body", "path": [i, 0, 0, 0], "off": 0}}),
            )?;
            log.run("table.deleteTable", json!({}))?;
        } else if let Some(next) = blocks.get(at + 1).filter(|b| b["type"] != "table") {
            log.run("select.range", json!({"anchor": pos(i, 0), "focus": pos(next["index"].as_u64().unwrap_or(i + 1), 0)}))?;
            log.run("text.delete", json!({}))?;
        } else if let Some(prev) = at
            .checked_sub(1)
            .and_then(|p| blocks.get(p))
            .filter(|b| b["type"] != "table")
        {
            let (pi, pl) = (
                prev["index"].as_u64().unwrap_or(0),
                prev["len"].as_u64().unwrap_or(0),
            );
            log.run(
                "select.range",
                json!({"anchor": pos(pi, pl), "focus": pos(i, len)}),
            )?;
            log.run("text.delete", json!({}))?;
        } else if len > 0 {
            // The only paragraph (or one between tables): a document keeps it, emptied.
            log.run(
                "select.range",
                json!({"anchor": pos(i, 0), "focus": pos(i, len)}),
            )?;
            log.run("text.delete", json!({}))?;
        }
        Ok(reply(path, log, json!({})))
    }
}

// ------------------------------------------------------------------ spreadsheets

mod sheet {
    use super::*;

    const CELL_PROPS: &[&str] = &[
        "value",
        "values",
        "bold",
        "italic",
        "underline",
        "color",
        "fill",
        "font",
        "size",
        "format",
        "align",
        "wrap",
        "merge",
    ];
    const SHEET_PROPS: &[&str] = &["name", "tabColor"];

    fn sheet_name(doc: &mut Doc, step: &Step) -> Result<String, CliError> {
        let p = match step.index {
            Some(n) => format!("/sheet[{n}]"),
            None => format!("/{}", step.name),
        };
        let v = view::get(doc, &p)?;
        Ok(v["sheet"]["name"].as_str().unwrap_or_default().to_string())
    }

    pub(super) fn set(
        doc: &mut Doc,
        path: &str,
        steps: &[Step],
        props: &Map<String, Value>,
    ) -> Result<Value, CliError> {
        let name = sheet_name(doc, &steps[0])?;
        let mut log = Log::new(doc);
        if steps.len() == 1 {
            if let Some(p) = props.keys().find(|k| !SHEET_PROPS.contains(&k.as_str())) {
                return Err(unsupported("a sheet", p, SHEET_PROPS));
            }
            // The tab colour first: renaming changes the name the sheet is found by.
            if let Some(v) = props.get("tabColor") {
                log.run(
                    "sheet.tabColor",
                    json!({"sheet": name, "color": hex(v, true)}),
                )?;
            }
            if let Some(v) = props.get("name") {
                log.run("sheet.rename", json!({"sheet": name, "name": as_text(v)}))?;
            }
            return Ok(reply(path, log, json!({})));
        }
        if steps.len() != 2 {
            return Err(CliError::new(
                "invalid_path",
                "spreadsheet paths are /Sheet, /Sheet/A1 or /Sheet/A1:C3",
            ));
        }
        let range = steps[1].name.to_ascii_uppercase();
        if let Some(p) = props.keys().find(|k| !CELL_PROPS.contains(&k.as_str())) {
            return Err(unsupported("a cell", p, CELL_PROPS));
        }
        let start = range.split(':').next().unwrap_or(&range).to_string();
        if let Some(v) = props.get("value") {
            if range.contains(':') {
                return Err(CliError::new(
                    "invalid_params",
                    "a range takes values:=[[…]]; value= is for one cell",
                ));
            }
            log.run(
                "cell.set",
                json!({"sheet": name, "cell": start, "input": as_text(v)}),
            )?;
        }
        if let Some(v) = props.get("values") {
            if !v.is_array() {
                return Err(CliError::new(
                    "invalid_params",
                    "values is a list of rows: values:='[[1,2],[3,4]]'",
                ));
            }
            log.run(
                "range.setValues",
                json!({"sheet": name, "range": start, "values": v}),
            )?;
        }
        let formats: Vec<(&String, &Value)> = props
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "value" | "values"))
            .collect();
        if !formats.is_empty() {
            log.run("sheet.activate", json!({"sheet": name}))?;
            for (k, v) in formats {
                let r = json!(range);
                let (id, p) = match k.as_str() {
                    "bold" => ("home.bold", json!({"range": r, "on": as_bool(k, v)?})),
                    "italic" => ("home.italic", json!({"range": r, "on": as_bool(k, v)?})),
                    "underline" => ("home.underline", json!({"range": r, "on": as_bool(k, v)?})),
                    "color" => ("home.fontColor", json!({"range": r, "color": hex(v, true)})),
                    "fill" => ("home.fillColor", json!({"range": r, "color": hex(v, true)})),
                    "font" => ("home.fontName", json!({"range": r, "name": as_text(v)})),
                    "size" => ("home.fontSize", json!({"range": r, "size": as_num(k, v)?})),
                    "format" => {
                        let f = as_text(v);
                        let named = [
                            "General",
                            "Number",
                            "Currency",
                            "Accounting",
                            "Short Date",
                            "Long Date",
                            "Time",
                            "Percentage",
                            "Fraction",
                            "Scientific",
                            "Text",
                        ];
                        if named.iter().any(|n| n.eq_ignore_ascii_case(&f)) {
                            ("home.numberFormat", json!({"range": r, "format": f}))
                        } else {
                            ("home.numberFormat", json!({"range": r, "code": f}))
                        }
                    }
                    "align" => match as_text(v).as_str() {
                        "left" => ("home.alignLeft", json!({"range": r})),
                        "center" | "centre" => ("home.alignCenter", json!({"range": r})),
                        "right" => ("home.alignRight", json!({"range": r})),
                        other => {
                            return Err(CliError::new(
                                "invalid_params",
                                format!("align: `{other}` (use left, center or right)"),
                            ));
                        }
                    },
                    "wrap" => ("home.wrapText", json!({"range": r, "on": as_bool(k, v)?})),
                    _ => {
                        if as_bool(k, v)? {
                            ("home.mergeCells", json!({"range": r}))
                        } else {
                            ("home.unmergeCells", json!({"range": r}))
                        }
                    }
                };
                log.run(id, p)?;
            }
        }
        let node = view::get(log.doc, &format!("/{name}/{range}"))?;
        Ok(reply(path, log, json!({"node": node})))
    }

    pub(super) fn add(
        doc: &mut Doc,
        parent: &str,
        steps: &[Step],
        kind: &str,
        at: &Position,
        props: &Map<String, Value>,
    ) -> Result<Value, CliError> {
        if kind != "sheet" || !steps.is_empty() {
            return Err(CliError::new(
                "invalid_params",
                "spreadsheets add sheets: add <file> / --type sheet name=…",
            )
            .suggest("cells already exist: change them with set /Sheet1/A1 value=…"));
        }
        let mut p = json!({});
        if let Some(n) = props.get("name") {
            p["name"] = json!(as_text(n));
        }
        if let Position::Index(n) = at {
            p["before"] = json!(n.saturating_sub(1));
        }
        let mut log = Log::new(doc);
        log.run("home.insertSheet", p)?;
        let _ = parent;
        let tree = log.doc.inspect()?;
        let active = tree["activeSheet"].as_u64().unwrap_or(0) as usize;
        let name = tree["sheets"][active]["name"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        Ok(reply(&format!("/{name}"), log, json!({})))
    }

    pub(super) fn remove(doc: &mut Doc, path: &str, steps: &[Step]) -> Result<Value, CliError> {
        let name = sheet_name(doc, &steps[0])?;
        let mut log = Log::new(doc);
        match steps.get(1) {
            None => log.run("home.deleteSheet", json!({"sheet": name}))?,
            Some(cells) => {
                log.run("sheet.activate", json!({"sheet": name}))?;
                log.run(
                    "edit.clearContents",
                    json!({"range": cells.name.to_ascii_uppercase()}),
                )?
            }
        };
        Ok(reply(path, log, json!({})))
    }
}

// ------------------------------------------------------------------ presentations

mod slides {
    use super::*;

    const SLIDE_PROPS: &[&str] = &["layout", "notes", "hidden"];
    const SHAPE_PROPS: &[&str] = &[
        "text",
        "bold",
        "italic",
        "underline",
        "font",
        "size",
        "color",
        "align",
        "fill",
        "x",
        "y",
        "w",
        "h",
        "name",
    ];

    /// (0-based slide index, shape id) of `/slide[N]` or `/slide[N]/shape[M]`.
    fn target(doc: &mut Doc, path: &str, steps: &[Step]) -> Result<(u64, Option<Value>), CliError> {
        let node = view::get(doc, path)?;
        let slide = steps[0].index.unwrap_or(1) as u64 - 1;
        Ok((
            slide,
            if steps.len() > 1 {
                node.get("id").cloned()
            } else {
                None
            },
        ))
    }

    /// Replace a shape's text: enter it, select everything, type, leave.
    fn set_text(log: &mut Log, id: &Value, text: &str) -> Result<(), CliError> {
        log.run("text.edit", json!({"id": id}))?;
        let current = log.run("text.get", json!({"id": id}))?;
        let paras = current["paragraphs"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let last = paras.len().saturating_sub(1);
        let last_len = paras.last().map_or(0, |p| {
            p.get("text")
                .and_then(Value::as_str)
                .or(p.as_str())
                .map_or(0, |t| t.chars().count())
        });
        log.run(
            "text.select",
            json!({"anchor": [0, 0], "caret": [last, last_len]}),
        )?;
        if text.is_empty() {
            log.run("text.delete", json!({}))?;
        } else {
            log.run("text.insert", json!({"text": text}))?;
        }
        log.run("text.exit", json!({})).map(drop)
    }

    fn apply_shape(log: &mut Log, id: &Value, props: &Map<String, Value>) -> Result<(), CliError> {
        if let Some(p) = props.keys().find(|k| !SHAPE_PROPS.contains(&k.as_str())) {
            return Err(unsupported("a shape", p, SHAPE_PROPS));
        }
        if let Some(t) = props.get("text") {
            set_text(log, id, &as_text(t))?;
        }
        let ids = json!([id]);
        if ["x", "y", "w", "h"].iter().any(|k| props.contains_key(*k)) {
            let full = log.run("shape.inspect", json!({"id": id}))?;
            let b = full
                .get("box")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let cur = |i: usize| b.get(i).and_then(Value::as_f64).unwrap_or(0.0);
            let get = |k: &str, i: usize| {
                props
                    .get(k)
                    .map(|v| as_num(k, v))
                    .transpose()
                    .map(|v| v.unwrap_or(cur(i)))
            };
            log.run("shape.setBounds", json!({"x": get("x", 0)?, "y": get("y", 1)?, "w": get("w", 2)?, "h": get("h", 3)?, "ids": ids}))?;
        }
        if let Some(n) = props.get("name") {
            log.run("shape.rename", json!({"name": as_text(n), "id": id}))?;
        }
        if let Some(f) = props.get("fill") {
            let s = as_text(f);
            let p = if s == "none" {
                json!({"none": true, "ids": ids})
            } else {
                json!({"color": hex(f, true), "ids": ids})
            };
            log.run("shape.fill", p)?;
        }
        let text_props: Vec<(&String, &Value)> = props
            .iter()
            .filter(|(k, _)| {
                matches!(
                    k.as_str(),
                    "bold" | "italic" | "underline" | "font" | "size" | "color" | "align"
                )
            })
            .collect();
        if !text_props.is_empty() {
            // Formatting a selected shape formats all of its text.
            log.run("edit.select", json!({"ids": ids}))?;
            for (k, v) in text_props {
                let (cmd, p) = match k.as_str() {
                    "bold" => ("format.bold", json!({"on": as_bool(k, v)?})),
                    "italic" => ("format.italic", json!({"on": as_bool(k, v)?})),
                    "underline" => ("format.underline", json!({"on": as_bool(k, v)?})),
                    "font" => ("format.font", json!({"family": as_text(v)})),
                    "size" => ("format.size", json!({"size": as_num(k, v)?})),
                    "color" => ("format.color", json!({"color": hex(v, true)})),
                    _ => ("format.align", json!({"align": as_text(v)})),
                };
                log.run(cmd, p)?;
            }
        }
        Ok(())
    }

    pub(super) fn set(
        doc: &mut Doc,
        path: &str,
        steps: &[Step],
        props: &Map<String, Value>,
    ) -> Result<Value, CliError> {
        let (slide, shape) = target(doc, path, steps)?;
        let mut log = Log::new(doc);
        log.run("slide.go", json!({"index": slide}))?;
        match shape {
            None => {
                for (k, v) in props {
                    match k.as_str() {
                        "layout" => log.run(
                            "slide.layout",
                            json!({"layout": as_text(v), "index": slide}),
                        )?,
                        "notes" => {
                            log.run("slide.notes", json!({"text": as_text(v), "index": slide}))?
                        }
                        "hidden" => log.run(
                            "slide.hide",
                            json!({"index": slide, "hidden": as_bool(k, v)?}),
                        )?,
                        other => return Err(unsupported("a slide", other, SLIDE_PROPS)),
                    };
                }
            }
            Some(id) => apply_shape(&mut log, &id, props)?,
        }
        let node = view::get(log.doc, path)?;
        Ok(reply(path, log, json!({"node": node})))
    }

    pub(super) fn add(
        doc: &mut Doc,
        parent: &str,
        steps: &[Step],
        kind: &str,
        at: &Position,
        props: &Map<String, Value>,
    ) -> Result<Value, CliError> {
        match (kind, steps.len()) {
            ("slide", 0) => {
                let count = view::get(doc, "/")?["slides"]
                    .as_array()
                    .map_or(0, Vec::len);
                let at = match at {
                    Position::End => count,
                    Position::Index(n) => n.saturating_sub(1).min(count),
                    Position::After(p) | Position::Before(p) => {
                        let s = view::parse_path(p)?;
                        let n = s.first().and_then(|s| s.index).ok_or_else(|| {
                            CliError::new("invalid_path", "anchor a slide as /slide[N]")
                        })?;
                        if matches!(at, Position::After(_)) {
                            n
                        } else {
                            n - 1
                        }
                    }
                };
                let mut p = json!({"at": at});
                for (k, v) in props {
                    match k.as_str() {
                        "layout" | "title" | "body" => p[k] = json!(as_text(v)),
                        other => {
                            return Err(unsupported(
                                "a new slide",
                                other,
                                &["layout", "title", "body"],
                            ));
                        }
                    }
                }
                let mut log = Log::new(doc);
                log.run("slide.new", p)?;
                Ok(reply(&format!("/slide[{}]", at + 1), log, json!({})))
            }
            ("textbox" | "text" | "shape", 1) if steps[0].name == "slide" => {
                let (slide, _) = target(doc, parent, steps)?;
                let before: Vec<Value> = view::get(doc, parent)?["shapes"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let mut log = Log::new(doc);
                log.run("slide.go", json!({"index": slide}))?;
                let num = |k: &str, d: f64| {
                    props
                        .get(k)
                        .map(|v| as_num(k, v))
                        .transpose()
                        .map(|v| v.unwrap_or(d))
                };
                let rect = json!([
                    num("x", 72.0)?,
                    num("y", 72.0)?,
                    num("w", 400.0)?,
                    num("h", 60.0)?
                ]);
                let text = props.get("text").map(as_text).unwrap_or_default();
                log.run("insert.textBox", json!({"rect": rect, "text": text}))?;
                let after: Vec<Value> = view::get(log.doc, parent)?["shapes"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let added = after
                    .iter()
                    .position(|s| !before.iter().any(|b| b["id"] == s["id"]));
                let path =
                    added.map_or(parent.to_string(), |m| format!("{parent}/shape[{}]", m + 1));
                if let Some(id) = added.and_then(|m| after[m].get("id").cloned()) {
                    let rest: Map<String, Value> = props
                        .iter()
                        .filter(|(k, _)| !matches!(k.as_str(), "text" | "x" | "y" | "w" | "h"))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    if !rest.is_empty() {
                        apply_shape(&mut log, &id, &rest)?;
                    }
                }
                Ok(reply(&path, log, json!({})))
            }
            _ => Err(CliError::new(
                "invalid_params",
                format!("can't add `{kind}` under `{parent}`"),
            )
            .suggest("add <file> / --type slide …, or add <file> /slide[N] --type textbox …")),
        }
    }

    pub(super) fn remove(doc: &mut Doc, path: &str, steps: &[Step]) -> Result<Value, CliError> {
        let (slide, shape) = target(doc, path, steps)?;
        let mut log = Log::new(doc);
        match shape {
            None => log.run("slide.delete", json!({"index": slide}))?,
            Some(id) => {
                log.run("slide.go", json!({"index": slide}))?;
                log.run("edit.delete", json!({"ids": [id]}))?
            }
        };
        Ok(reply(path, log, json!({})))
    }
}

// ------------------------------------------------------------------ PDF

mod pdf {
    use super::*;

    fn page(doc: &mut Doc, path: &str, steps: &[Step]) -> Result<u64, CliError> {
        view::get(doc, path)?;
        Ok(steps[0].index.unwrap_or(1) as u64)
    }

    pub(super) fn set(
        doc: &mut Doc,
        path: &str,
        steps: &[Step],
        props: &Map<String, Value>,
    ) -> Result<Value, CliError> {
        let n = page(doc, path, steps)?;
        let mut log = Log::new(doc);
        for (k, v) in props {
            match k.as_str() {
                "rotate" | "rotation" => log.run(
                    "page_rotate",
                    json!({"pages": [n], "degrees": as_num(k, v)? as i64}),
                )?,
                other => return Err(unsupported("a page", other, &["rotate"])),
            };
        }
        let node = view::get(log.doc, path)?;
        Ok(reply(path, log, json!({"node": node})))
    }

    pub(super) fn add(
        doc: &mut Doc,
        parent: &str,
        steps: &[Step],
        kind: &str,
        at: &Position,
        props: &Map<String, Value>,
    ) -> Result<Value, CliError> {
        match (kind, steps.len()) {
            ("page", 0) => {
                let count = doc.inspect()?["pages"].as_array().map_or(0, Vec::len) as u64;
                let anchor = |p: &str| -> Result<u64, CliError> {
                    view::parse_path(p)?
                        .first()
                        .and_then(|s| s.index)
                        .map(|n| n as u64)
                        .ok_or_else(|| CliError::new("invalid_path", "anchor a page as /page[N]"))
                };
                let at = match at {
                    Position::End => count + 1,
                    Position::Index(n) => (*n as u64).min(count + 1),
                    Position::After(p) => anchor(p)? + 1,
                    Position::Before(p) => anchor(p)?,
                };
                let mut p = json!({"at": at});
                for (k, v) in props {
                    match k.as_str() {
                        "width" | "height" => p[k] = json!(as_num(k, v)?),
                        other => {
                            return Err(unsupported("a new page", other, &["width", "height"]));
                        }
                    }
                }
                let mut log = Log::new(doc);
                log.run("page_insert_blank", p)?;
                Ok(reply(&format!("/page[{at}]"), log, json!({})))
            }
            ("text", 1) if steps[0].name == "page" => {
                let n = page(doc, parent, steps)?;
                let mut p: Map<String, Value> = props
                    .iter()
                    .filter(|(k, _)| !matches!(k.as_str(), "x" | "y"))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                p.insert("page".into(), json!(n));
                if !props.contains_key("at") && !props.contains_key("rect") {
                    let x = props
                        .get("x")
                        .map(|v| as_num("x", v))
                        .transpose()?
                        .unwrap_or(72.0);
                    let y = props
                        .get("y")
                        .map(|v| as_num("y", v))
                        .transpose()?
                        .unwrap_or(72.0);
                    p.insert("at".into(), json!([x, y]));
                }
                let mut log = Log::new(doc);
                log.run("page_add_text", Value::Object(p))?;
                Ok(reply(parent, log, json!({})))
            }
            _ => Err(CliError::new(
                "invalid_params",
                format!("can't add `{kind}` under `{parent}`"),
            )
            .suggest(
                "add <file> / --type page, or add <file> /page[N] --type text text=… x=… y=…",
            )),
        }
    }

    pub(super) fn remove(doc: &mut Doc, path: &str, steps: &[Step]) -> Result<Value, CliError> {
        let n = page(doc, path, steps)?;
        let mut log = Log::new(doc);
        log.run("page_delete", json!({"pages": [n]}))?;
        Ok(reply(path, log, json!({})))
    }
}
