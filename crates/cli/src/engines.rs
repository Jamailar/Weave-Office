//! One adapter per document engine. Every engine is already a headless session driven by
//! "command id + JSON params → JSON"; the adapters only give them one shape.

use std::path::Path;

use serde_json::{Value, json};

use crate::error::CliError;

/// Which engine handles a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Documents: .docx .doc .odt .rtf .md .txt .html …
    Word,
    /// Spreadsheets: .xlsx .xls .ods .csv …
    Sheet,
    /// Presentations: .pptx .ppt .odp
    Slides,
    Pdf,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Word, Kind::Sheet, Kind::Slides, Kind::Pdf];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Word => "word",
            Kind::Sheet => "sheet",
            Kind::Slides => "slides",
            Kind::Pdf => "pdf",
        }
    }

    /// The engine for a file, from its extension.
    pub fn from_path(path: &Path) -> Result<Kind, CliError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        Self::from_extension(&ext).ok_or_else(|| {
            CliError::new("unsupported_format", format!("{}: unknown file type `.{ext}`", path.display()))
                .suggest("use .docx/.odt/.rtf/.md/.txt (documents), .xlsx/.ods/.csv (spreadsheets), .pptx/.odp (presentations) or .pdf")
        })
    }

    pub fn from_extension(ext: &str) -> Option<Kind> {
        Some(match ext {
            "docx" | "docm" | "dotx" | "doc" | "odt" | "rtf" | "md" | "markdown" | "txt"
            | "text" | "html" | "htm" | "tex" | "json" => Kind::Word,
            "xlsx" | "xlsm" | "xltx" | "xls" | "ods" | "csv" | "tsv" => Kind::Sheet,
            "pptx" | "pptm" | "potx" | "ppt" | "odp" => Kind::Slides,
            "pdf" => Kind::Pdf,
            _ => return None,
        })
    }

    /// A kind named on the command line: `word`, `docx`, `excel`, `ppt`, …
    pub fn from_alias(name: &str) -> Option<Kind> {
        match name.to_ascii_lowercase().as_str() {
            "word" | "doc" | "document" | "text" | "writer" => Some(Kind::Word),
            "sheet" | "excel" | "spreadsheet" | "grid" | "calc" => Some(Kind::Sheet),
            "slides" | "ppt" | "powerpoint" | "presentation" | "deck" | "impress" => {
                Some(Kind::Slides)
            }
            "pdf" => Some(Kind::Pdf),
            other => Self::from_extension(other.trim_start_matches('.')),
        }
    }
}

/// One command an engine understands, as `commands` lists it.
#[derive(Clone, Debug)]
pub struct CommandDesc {
    pub id: String,
    pub label: String,
    pub location: String,
    /// Parameter summary (`rows:int, cols:int`) or, for PDF tools, a JSON Schema.
    pub params: Value,
    pub description: String,
}

impl CommandDesc {
    pub fn to_json(&self) -> Value {
        let mut v = json!({"id": self.id, "label": self.label, "params": self.params});
        if !self.location.is_empty() {
            v["location"] = json!(self.location);
        }
        if !self.description.is_empty() {
            v["description"] = json!(self.description);
        }
        v
    }
}

/// An open document in one engine.
pub enum Doc {
    Word(Box<wordcraft_engine::Session>),
    Sheet(Box<gridcraft_engine::Session>),
    Slides(Box<deckcraft_engine::Session>),
    Pdf {
        automation: Box<pdfcraft_automation::Automation>,
        id: u64,
    },
}

fn engine(e: impl std::fmt::Display) -> CliError {
    CliError::new("engine", e.to_string())
}

/// Text from the command line goes in as given: AutoCorrect off for this session, so Enter
/// after "a" leaves "a" (not "A") and "(c)" stays "(c)". Only this headless session's
/// options change; the app's saved preferences are untouched.
fn literal_text(s: &mut wordcraft_engine::Session) -> Result<(), CliError> {
    s.run("tools.autocorrect", &json!({"enabled": false}))
        .map(drop)
        .map_err(engine)
}

fn word_region() -> wordcraft_engine::Region {
    wordcraft_engine::Region::from_locale_vars(|name| std::env::var(name).ok(), || None)
}

impl Doc {
    pub fn kind(&self) -> Kind {
        match self {
            Doc::Word(_) => Kind::Word,
            Doc::Sheet(_) => Kind::Sheet,
            Doc::Slides(_) => Kind::Slides,
            Doc::Pdf { .. } => Kind::Pdf,
        }
    }

    /// A new, empty document. `template` picks a starting point where the engine has some.
    pub fn blank(kind: Kind, template: Option<&str>) -> Result<Doc, CliError> {
        Ok(match kind {
            Kind::Word => {
                let mut s = wordcraft_engine::Session::new(wordcraft_doc::Document::new());
                s.system_region = word_region();
                literal_text(&mut s)?;
                if let Some(t) = template {
                    // `file.new` opens beside the first document; keep only the new one.
                    let first = s.document_id();
                    s.run("file.new", &json!({"template": t})).map_err(engine)?;
                    s.close_document(first).map_err(engine)?;
                }
                Doc::Word(Box::new(s))
            }
            Kind::Sheet => {
                let mut s = gridcraft_engine::Session::new();
                s.new_workbook();
                Doc::Sheet(Box::new(s))
            }
            Kind::Slides => {
                let mut s = deckcraft_engine::Session::new();
                let params = template.map_or_else(|| json!({}), |t| json!({"template": t}));
                s.execute("file.new", &params).map_err(engine)?;
                Doc::Slides(Box::new(s))
            }
            Kind::Pdf => {
                let mut automation = Box::new(pdfcraft_automation::Automation::new());
                let reply = pdf_value(
                    automation
                        .call("doc_create", &json!({"from": "blank"}))
                        .map_err(pdf_error)?,
                );
                let id = reply
                    .get("doc")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| engine("the PDF engine gave no document id"))?;
                Doc::Pdf { automation, id }
            }
        })
    }

    pub fn open(path: &Path, password: Option<&str>) -> Result<Doc, CliError> {
        if !path.is_file() {
            return Err(
                CliError::new("not_found", format!("{}: no such file", path.display()))
                    .suggest("create it first with `weave-office create <file>`"),
            );
        }
        let kind = Kind::from_path(path)?;
        let text = path.to_string_lossy().into_owned();
        Ok(match kind {
            Kind::Word => {
                let (doc, encrypted) =
                    wordcraft_engine::io::open_path_in(path, password, &word_region())
                        .map_err(|e| password_or(e, password))?;
                let mut s = wordcraft_engine::Session::new(doc);
                s.system_region = word_region();
                literal_text(&mut s)?;
                s.path = Some(path.into());
                if encrypted {
                    s.password = password.map(wordcraft_engine::io::Password::new);
                }
                Doc::Word(Box::new(s))
            }
            Kind::Sheet => {
                let mut s = gridcraft_engine::Session::new();
                let mut params = json!({"path": text});
                if let Some(p) = password {
                    params["password"] = json!(p);
                }
                s.execute_headless("file.open", params)
                    .map_err(|e| password_or(e.to_string(), password))?;
                Doc::Sheet(Box::new(s))
            }
            Kind::Slides => {
                let mut s = deckcraft_engine::Session::new();
                let mut params = json!({"path": text});
                if let Some(p) = password {
                    params["password"] = json!(p);
                }
                s.execute("file.open", &params)
                    .map_err(|e| password_or(e.to_string(), password))?;
                Doc::Slides(Box::new(s))
            }
            Kind::Pdf => {
                let mut automation = Box::new(pdfcraft_automation::Automation::new());
                let mut params = json!({"path": text});
                if let Some(p) = password {
                    params["password"] = json!(p);
                }
                let reply = pdf_value(
                    automation
                        .call("doc_open", &params)
                        .map_err(|e| password_or(e.to_string(), password))?,
                );
                let id = reply
                    .get("doc")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| engine("the PDF engine gave no document id"))?;
                Doc::Pdf { automation, id }
            }
        })
    }

    /// Run one engine command (a PDF tool for PDFs). Unknown ids fail with `unknown_command`
    /// and the closest known ids as the suggestion.
    pub fn execute(&mut self, id: &str, params: Value) -> Result<Value, CliError> {
        let params = if params.is_null() { json!({}) } else { params };
        if !params.is_object() {
            return Err(CliError::new(
                "invalid_params",
                format!("{id}: params must be a JSON object"),
            ));
        }
        if !self.knows(id) {
            return Err(unknown_command(self.kind(), id));
        }
        match self {
            Doc::Word(s) => s
                .run(id, &params)
                .map_err(|e| params_or_engine(id, e.to_string())),
            Doc::Sheet(s) => s
                .execute_headless(id, params)
                .map_err(|e| params_or_engine(id, e.to_string())),
            Doc::Slides(s) => s
                .execute(id, &params)
                .map_err(|e| params_or_engine(id, e.to_string())),
            Doc::Pdf {
                automation,
                id: doc,
            } => {
                let mut params = params;
                if pdf_takes_doc(id)
                    && let Some(o) = params.as_object_mut()
                {
                    o.entry("doc").or_insert(json!(*doc));
                }
                let out = automation.call(id, &params).map_err(pdf_error)?;
                Ok(pdf_value(out))
            }
        }
    }

    fn knows(&self, id: &str) -> bool {
        match self {
            Doc::Word(s) => {
                s.registry.get(id).is_some() || matches!(id, "document.inspect" | "file.info")
            }
            Doc::Sheet(_) => gridcraft_engine::find_command(id).is_some(),
            Doc::Slides(s) => s.commands().iter().any(|c| c.id == id),
            Doc::Pdf { .. } => pdfcraft_automation::tools().iter().any(|t| t.name == id),
        }
    }

    /// Whether the document has changes not yet on disk.
    pub fn dirty(&mut self) -> bool {
        match self {
            Doc::Word(s) => s.dirty,
            Doc::Sheet(s) => s.active().is_some_and(|d| d.is_dirty()),
            Doc::Slides(s) => s.active().is_some_and(|d| d.is_dirty()),
            Doc::Pdf { automation, id } => automation
                .call("doc_list", &json!({}))
                .ok()
                .map(pdf_value)
                .and_then(|v| v.get("documents").and_then(Value::as_array).cloned())
                .is_some_and(|docs| {
                    docs.iter().any(|d| {
                        d.get("doc").and_then(Value::as_u64) == Some(*id)
                            && d.get("dirty") == Some(&json!(true))
                    })
                }),
        }
    }

    /// Write the document to `path`, in the format its extension names. The caller makes the
    /// write atomic (`crate::save`).
    pub fn write(&mut self, path: &Path) -> Result<(), CliError> {
        let text = path.to_string_lossy().into_owned();
        match self {
            Doc::Word(s) => s
                .run("file.save", &json!({"path": text}))
                .map(drop)
                .map_err(engine),
            Doc::Sheet(s) => s
                .execute_headless("file.save", json!({"path": text}))
                .map(drop)
                .map_err(engine),
            Doc::Slides(s) => s
                .execute("file.saveAs", &json!({"path": text}))
                .map(drop)
                .map_err(engine),
            Doc::Pdf { automation, id } => automation
                .call("doc_save", &json!({"doc": *id, "path": text}))
                .map(drop)
                .map_err(pdf_error),
        }
    }

    /// Export to another format (PDF, PNG, CSV, HTML, Markdown …). `page` picks the page or
    /// slide of a picture export.
    pub fn export(&mut self, path: &Path, page: Option<u64>) -> Result<Value, CliError> {
        let text = path.to_string_lossy().into_owned();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let picture = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "svg");
        match self {
            Doc::Word(s) if ext == "png" => s
                .run(
                    "file.exportPng",
                    &json!({"path": text, "page": page.unwrap_or(1), "scale": 2.0}),
                )
                .map_err(engine),
            Doc::Word(s) => s.run("file.save", &json!({"path": text})).map_err(engine),
            Doc::Sheet(s) => {
                let id = match ext.as_str() {
                    "pdf" => "file.exportPdf",
                    "csv" => "file.exportCsv",
                    "html" | "htm" => "file.exportHtml",
                    _ => "file.save",
                };
                s.execute_headless(id, json!({"path": text}))
                    .map_err(engine)
            }
            Doc::Slides(s) => {
                let mut params = json!({"path": text});
                if picture {
                    params["slide"] = json!(page.unwrap_or(1).saturating_sub(1));
                }
                if picture || ext == "pdf" {
                    s.execute("file.export", &params).map_err(engine)
                } else {
                    s.execute("file.saveAs", &params).map_err(engine)
                }
            }
            Doc::Pdf { automation, id } => {
                if ext == "png" {
                    let out = automation
                        .call(
                            "page_render",
                            &json!({"doc": *id, "page": page.unwrap_or(1), "dpi": 144}),
                        )
                        .map_err(pdf_error)?;
                    let png = out
                        .iter()
                        .find_map(|c| match c {
                            pdfcraft_automation::Content::Png { data, .. } => Some(data.clone()),
                            pdfcraft_automation::Content::Json(_) => None,
                        })
                        .ok_or_else(|| engine("the page did not render"))?;
                    crate::save::write_bytes_atomic(path, &png)?;
                    Ok(json!({"path": text, "page": page.unwrap_or(1)}))
                } else if matches!(ext.as_str(), "txt" | "text") {
                    let v = pdf_value(
                        automation
                            .call("text_extract", &json!({"doc": *id}))
                            .map_err(pdf_error)?,
                    );
                    let body = pdf_text(&v);
                    crate::save::write_bytes_atomic(path, body.as_bytes())?;
                    Ok(json!({"path": text}))
                } else if matches!(ext.as_str(), "docx" | "xlsx" | "pptx" | "rtf") {
                    let v = automation
                        .call("doc_export_office", &json!({"doc": *id, "path": text}))
                        .map_err(pdf_error)?;
                    Ok(pdf_value(v))
                } else {
                    automation
                        .call("doc_save", &json!({"doc": *id, "path": text}))
                        .map(pdf_value)
                        .map_err(pdf_error)
                }
            }
        }
    }

    /// The document's text: paragraphs (documents), tab-separated cells (spreadsheets), slide
    /// text (presentations) or page text (PDF).
    pub fn text(&mut self) -> Result<String, CliError> {
        match self {
            Doc::Word(s) => {
                let bytes = wordcraft_engine::io::save_bytes("out.txt", &s.doc).map_err(engine)?;
                Ok(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
            }
            Doc::Sheet(_) | Doc::Slides(_) => crate::view::text(self),
            Doc::Pdf { automation, id } => {
                let v = pdf_value(
                    automation
                        .call("text_extract", &json!({"doc": *id}))
                        .map_err(pdf_error)?,
                );
                Ok(pdf_text(&v))
            }
        }
    }

    /// The engine's full structure dump.
    pub fn inspect(&mut self) -> Result<Value, CliError> {
        match self {
            Doc::Word(s) => s.run("document.inspect", &json!({})).map_err(engine),
            Doc::Sheet(s) => s
                .execute_headless("document.inspect", json!({}))
                .map_err(engine),
            Doc::Slides(s) => s.execute("document.inspect", &json!({})).map_err(engine),
            Doc::Pdf { automation, id } => automation
                .call("doc_inspect", &json!({"doc": *id}))
                .map(pdf_value)
                .map_err(pdf_error),
        }
    }

    /// File-level facts: kind, size, counts, metadata.
    pub fn info(&mut self) -> Result<Value, CliError> {
        match self {
            Doc::Word(s) => s.run("file.info", &json!({})).map_err(engine),
            Doc::Sheet(s) => s
                .execute_headless("file.properties", json!({}))
                .map_err(engine),
            Doc::Slides(_) => {
                let tree = self.inspect()?;
                let mut info = json!({});
                for key in [
                    "title",
                    "path",
                    "slideCount",
                    "slide_count",
                    "size",
                    "slideSize",
                    "properties",
                ] {
                    if let Some(v) = tree.get(key) {
                        info[key] = v.clone();
                    }
                }
                if let Some(slides) = tree.get("slides").and_then(Value::as_array) {
                    info["slides"] = json!(slides.len());
                }
                Ok(info)
            }
            Doc::Pdf { automation, id } => automation
                .call("doc_info", &json!({"doc": *id}))
                .map(pdf_value)
                .map_err(pdf_error),
        }
    }
}

/// Every command `kind`'s engine understands.
pub fn commands(kind: Kind) -> Vec<CommandDesc> {
    match kind {
        Kind::Word => {
            let s = wordcraft_engine::Session::new(wordcraft_doc::Document::new());
            s.registry
                .all()
                .iter()
                .map(|c| CommandDesc {
                    id: c.id.into(),
                    label: c.label.into(),
                    location: c.location.into(),
                    params: json!(c.params),
                    description: String::new(),
                })
                .collect()
        }
        Kind::Sheet => {
            let mut s = gridcraft_engine::Session::new();
            s.new_workbook();
            s.commands()
                .into_iter()
                .map(|c| CommandDesc {
                    id: c.id.into(),
                    label: c.label.into(),
                    location: c.menu.join(" › "),
                    params: json!(c.params),
                    description: String::new(),
                })
                .collect()
        }
        Kind::Slides => {
            let mut s = deckcraft_engine::Session::new();
            let _ = s.execute("file.new", &json!({}));
            s.commands()
                .into_iter()
                .map(|c| CommandDesc {
                    id: c.id.into(),
                    label: c.label.into(),
                    location: c.menu.join(" › "),
                    params: json!(c.params),
                    description: String::new(),
                })
                .collect()
        }
        Kind::Pdf => pdfcraft_automation::tools()
            .into_iter()
            .map(|t| CommandDesc {
                id: t.name.into(),
                label: t.title.into(),
                location: String::new(),
                params: without_doc(t.input_schema.clone()),
                description: t.description.into(),
            })
            .collect(),
    }
}

/// A PDF tool's schema without `doc`: the CLI fills it in with the open document.
fn without_doc(mut schema: Value) -> Value {
    if let Some(p) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        p.remove("doc");
    }
    if let Some(r) = schema.get_mut("required").and_then(Value::as_array_mut) {
        r.retain(|k| k != "doc");
    }
    schema
}

/// `unknown_command`, with the closest ids as the suggestion.
pub fn unknown_command(kind: Kind, id: &str) -> CliError {
    let ids: Vec<String> = commands(kind).into_iter().map(|c| c.id).collect();
    let close = crate::suggest::closest(id, ids.iter().map(String::as_str), 5);
    let mut e = CliError::new(
        "unknown_command",
        format!("`{id}` is not a {} command", kind.name()),
    );
    e = if close.is_empty() {
        e.suggest(format!(
            "list them with `weave-office commands {}`",
            kind.name()
        ))
    } else {
        e.suggest(format!(
            "did you mean: {}? (`weave-office commands {} <keyword>` lists more)",
            close.join(", "),
            kind.name()
        ))
    };
    e.with_candidates(close)
}

fn params_or_engine(id: &str, message: String) -> CliError {
    let lower = message.to_ascii_lowercase();
    let code = if lower.contains("required")
        || lower.contains("param")
        || lower.contains("expected")
        || lower.contains("must be")
    {
        "invalid_params"
    } else {
        "engine"
    };
    let e = CliError::new(code, format!("{id}: {message}"));
    if code == "invalid_params" {
        e.suggest("see the parameters with `weave-office commands <kind> <id>`")
    } else {
        e
    }
}

fn password_or(message: String, password: Option<&str>) -> CliError {
    if password.is_none() && message.to_ascii_lowercase().contains("password") {
        CliError::new("password_required", message)
            .suggest("pass the password with --password <PASSWORD>")
    } else {
        CliError::new("open_failed", message)
    }
}

fn pdf_error(e: pdfcraft_automation::ToolError) -> CliError {
    let code = match e {
        pdfcraft_automation::ToolError::UnknownTool(_) => "unknown_command",
        pdfcraft_automation::ToolError::InvalidArgs(_) => "invalid_params",
        pdfcraft_automation::ToolError::Failed(_) => "engine",
    };
    let e = CliError::new(code, e.to_string());
    if code == "invalid_params" {
        e.suggest("see the tool's parameters with `weave-office commands pdf <tool>`")
    } else {
        e
    }
}

/// Whether a PDF tool's schema has a `doc` argument (filled in with the open document).
fn pdf_takes_doc(tool: &str) -> bool {
    pdfcraft_automation::tools()
        .iter()
        .find(|t| t.name == tool)
        .and_then(|t| t.input_schema.get("properties"))
        .is_some_and(|p| p.get("doc").is_some())
}

/// A PDF tool's reply as one JSON value; pictures are summarised (use `export` for files).
fn pdf_value(content: Vec<pdfcraft_automation::Content>) -> Value {
    let mut parts: Vec<Value> = content
        .into_iter()
        .map(|c| match c {
            pdfcraft_automation::Content::Json(v) => v,
            pdfcraft_automation::Content::Png { data, width, height } => {
                json!({"image": {"format": "png", "width": width, "height": height, "bytes": data.len(), "note": "use `weave-office export <file.pdf> <out.png> --page N` to write it"}})
            }
        })
        .collect();
    if parts.len() == 1 {
        parts.remove(0)
    } else {
        Value::Array(parts)
    }
}

/// The text of a `text_extract` reply.
fn pdf_text(v: &Value) -> String {
    if let Some(pages) = v.get("pages").and_then(Value::as_array) {
        return pages
            .iter()
            .map(|p| {
                p.get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n\u{c}\n");
    }
    v.get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
