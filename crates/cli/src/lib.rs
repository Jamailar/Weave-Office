//! Weave Office on the command line: create, read and change documents, spreadsheets,
//! presentations and PDFs without a window, from scripts and AI agents.
//!
//! The app's own executable runs it: `weave-office <verb> …` (see [`is_cli`]). Every verb
//! works on the same engines as the editors; `--json` wraps every answer in one envelope:
//! `{"ok":true,"data":…}` or `{"ok":false,"error":{"code","message","suggestion"}}`.

pub mod edit;
pub mod engines;
pub mod error;
pub mod save;
pub mod suggest;
pub mod view;

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use engines::{Doc, Kind};
use error::CliError;

/// The verbs the CLI answers. `weave-office <verb>` runs the CLI instead of the app.
pub const VERBS: &[&str] = &[
    "help", "create", "info", "view", "get", "set", "add", "remove", "commands", "run", "batch",
    "export",
];

/// The guide for AI agents (`weave-office help agent`).
pub const AGENT_GUIDE: &str = include_str!("../AGENT.md");

const USAGE: &str = "Weave Office command line

Usage: weave-office <command> [arguments] [--json]

Reading
  info     <file>                       File facts: kind, pages/sheets/slides, metadata
  view     <file> [outline|text|json]   Outline with paths (default), plain text, or the full structure
  get      <file> <path>                One node: /body/p[3]  /Sheet1/A1:C5  /slide[2]/shape[1]  /page[4]

Changing
  create   <file> [--template NAME] [--force]   New blank document; the extension picks the kind
  set      <file> <path> key=value ...  Change a node: text, style, bold, value, layout … (help props)
  add      <file> <parent> --type T [key=value ...] [--after PATH|--before PATH|--index N]
                                        Add a paragraph, heading, table, sheet, slide, textbox, page …
  remove   <file> <path>                Remove a node (paragraph, table, sheet, cells, slide, shape, page)
  run      <file> <command> [key=value ...] [--params JSON]
                                        Run one editor command, then save the file
  batch    <file> [--input FILE | --commands JSON]
                                        Run a JSON list of commands (stdin by default); the file
                                        is written only if every command succeeds
  export   <file> <output> [--page N]   Save as another format: pdf, png, csv, html, md, txt, docx…

Discovering
  commands <kind|file> [keyword]        Every command with its parameters (kinds: word, sheet, slides, pdf)
  help [command|agent|paths|props]      This text, help on one command, the AI agent guide, path
                                        syntax, or the properties set/add take

Options
  --json          Answer in JSON: {\"ok\":true,\"data\":…} or {\"ok\":false,\"error\":{code,message,suggestion}}
  --out FILE      set/add/remove/run/batch: save to FILE instead of changing the original
  --dry-run       set/add/remove/run/batch: run the commands but write nothing
  --keep-going    batch: run every command even after one fails (still writes nothing on failure)
  --save          run/batch: write the file even when the commands changed nothing
  --password PW   Open a password-protected file

Parameters: key=value gives numbers and true/false as such and everything else as text
(text, input, name, title, color and other text keys always stay text); key:=JSON passes any
JSON value. Paths and indexes are 1-based. Exit status: 0 success, 1 failure, 2 usage error.
";

const PATHS_HELP: &str = "Paths (1-based; / is the whole document)

  Documents      /body                 every block
                 /body/p[N]            the N-th paragraph (headings and list items included)
                 /body/table[N]        the N-th table
                 /body/block[N]        the N-th block of any kind
  Spreadsheets   /Sheet1               a sheet's summary and values (or /sheet[N])
                 /Sheet1/B2            one cell: value, formula, style
                 /Sheet1/A1:C10        a range's values
  Presentations  /slide[N]             a slide: layout, title, shapes, notes
                 /slide[N]/shape[M]    a shape (or shape[@id=ID], shape[@name=Title 1])
  PDF            /page[N]              a page: size, rotation, text

`weave-office view <file> outline` prints the path of every node.
";

/// Whether `args` (the command line after the program name) asks for the CLI rather than the
/// app: the first argument is a verb and not also the name of an existing file.
pub fn is_cli(args: &[OsString]) -> bool {
    args.first()
        .and_then(|a| a.to_str())
        .is_some_and(|first| VERBS.contains(&first) && !Path::new(first).exists())
}

/// What one run printed, and its exit status.
#[derive(Debug, Default)]
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Run the CLI on `args` (after the program name) and print what it says. Returns the exit
/// status.
pub fn main(args: &[OsString]) -> i32 {
    let args: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let out = run(&args, &mut std::io::stdin());
    print!("{}", out.stdout);
    eprint!("{}", out.stderr);
    out.code
}

/// Run the CLI on `args`, reading stdin from `input` (for `batch`).
pub fn run(args: &[String], input: &mut dyn Read) -> Outcome {
    let opts = match Opts::parse(args) {
        Ok(o) => o,
        Err(e) => return failure(&e, args.iter().any(|a| a == "--json")),
    };
    let json_mode = opts.json;
    match dispatch(&opts, input) {
        Ok(reply) => success(reply, json_mode),
        Err(e) => failure(&e, json_mode),
    }
}

/// A finished command: its data, a line for people, and notes for stderr.
struct Reply {
    data: Value,
    /// Text mode: what to print instead of the data as JSON.
    text: Option<String>,
    notes: Vec<String>,
}

impl Reply {
    fn data(data: Value) -> Self {
        Self {
            data,
            text: None,
            notes: Vec::new(),
        }
    }
    fn text(data: Value, text: String) -> Self {
        Self {
            data,
            text: Some(text),
            notes: Vec::new(),
        }
    }
    fn note(mut self, n: impl Into<String>) -> Self {
        self.notes.push(n.into());
        self
    }
}

fn success(reply: Reply, json_mode: bool) -> Outcome {
    if json_mode {
        let mut env = json!({"ok": true, "data": reply.data});
        if !reply.notes.is_empty() {
            env["notes"] = json!(reply.notes);
        }
        return Outcome {
            code: 0,
            stdout: format!(
                "{}\n",
                serde_json::to_string_pretty(&env).unwrap_or_default()
            ),
            stderr: String::new(),
        };
    }
    let stdout = match reply.text {
        Some(t) if t.is_empty() => String::new(),
        Some(t) => format!("{}\n", t.trim_end_matches('\n')),
        None => format!("{}\n", pretty(&reply.data)),
    };
    let stderr = reply.notes.iter().map(|n| format!("{n}\n")).collect();
    Outcome {
        code: 0,
        stdout,
        stderr,
    }
}

fn failure(e: &CliError, json_mode: bool) -> Outcome {
    if json_mode {
        let env = json!({"ok": false, "error": e.to_json()});
        Outcome {
            code: e.exit_code(),
            stdout: format!(
                "{}\n",
                serde_json::to_string_pretty(&env).unwrap_or_default()
            ),
            stderr: String::new(),
        }
    } else {
        Outcome {
            code: e.exit_code(),
            stdout: String::new(),
            stderr: format!("{e}\n"),
        }
    }
}

fn pretty(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

// ------------------------------------------------------------------ options

#[derive(Debug, Default)]
struct Opts {
    positional: Vec<String>,
    /// `key=value` / `key:=json` parameters, in order.
    params: Map<String, Value>,
    json: bool,
    out: Option<PathBuf>,
    dry_run: bool,
    keep_going: bool,
    force: bool,
    save: bool,
    password: Option<String>,
    template: Option<String>,
    page: Option<u64>,
    params_json: Option<String>,
    input: Option<PathBuf>,
    commands: Option<String>,
    /// `add --type`.
    node_type: Option<String>,
    position: edit::Position,
}

/// Keys whose values stay text even when they look like numbers.
const TEXT_KEYS: &[&str] = &[
    "text",
    "input",
    "name",
    "title",
    "path",
    "find",
    "replace",
    "with",
    "label",
    "formula",
    "color",
    "colour",
    "fill",
    "style",
    "font",
    "family",
    "author",
    "value_text",
    "password",
    "url",
    "address",
    "sheet",
    "cell",
    "range",
    "template",
    "layout",
    "query",
    "search",
    "key",
    "comment",
    "note",
    "notes",
    "language",
    "value",
    "format",
    "code",
    "body",
    "type",
];

impl Opts {
    fn parse(args: &[String]) -> Result<Opts, CliError> {
        let mut o = Opts::default();
        let mut it = args.iter();
        let value = |it: &mut std::slice::Iter<String>, flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| CliError::usage(format!("{flag} needs a value")))
        };
        // Positionals end at the command id for `run`; after that, bare words are parameters.
        while let Some(a) = it.next() {
            match a.as_str() {
                "--json" => o.json = true,
                "--dry-run" => o.dry_run = true,
                "--keep-going" => o.keep_going = true,
                "--force" => o.force = true,
                "--save" => o.save = true,
                "--out" | "-o" => o.out = Some(value(&mut it, a)?.into()),
                "--password" => o.password = Some(value(&mut it, a)?),
                "--template" => o.template = Some(value(&mut it, a)?),
                "--params" => o.params_json = Some(value(&mut it, a)?),
                "--input" | "-i" => o.input = Some(value(&mut it, a)?.into()),
                "--commands" => o.commands = Some(value(&mut it, a)?),
                "--type" | "-t" => o.node_type = Some(value(&mut it, a)?.to_ascii_lowercase()),
                "--after" => o.position = edit::Position::After(value(&mut it, a)?),
                "--before" => o.position = edit::Position::Before(value(&mut it, a)?),
                "--index" => {
                    let v = value(&mut it, a)?;
                    let n = v.parse().ok().filter(|n| *n >= 1).ok_or_else(|| {
                        CliError::usage(format!("--index: `{v}` is not a position (1, 2, …)"))
                    })?;
                    o.position = edit::Position::Index(n);
                }
                "--page" | "--slide" => {
                    let v = value(&mut it, a)?;
                    o.page = Some(v.parse().ok().filter(|n| *n >= 1).ok_or_else(|| {
                        CliError::usage(format!("{a}: `{v}` is not a page number (1, 2, …)"))
                    })?);
                }
                "-h" | "--help" => o.positional.insert(0, "help".into()),
                flag if flag.starts_with("--") => {
                    return Err(CliError::usage(format!("unknown option `{flag}`")));
                }
                word if o.takes_params() && is_param(word) => {
                    let (k, v) = parse_param(word)?;
                    o.params.insert(k, v);
                }
                word => o.positional.push(word.to_string()),
            }
        }
        Ok(o)
    }

    /// `run <file> <id>`, `set <file> <path>` and `add <file> <parent>` have their target:
    /// what follows may be parameters.
    fn takes_params(&self) -> bool {
        matches!(
            self.positional.first().map(String::as_str),
            Some("run" | "set" | "add")
        ) && self.positional.len() >= 3
    }

    fn arg(&self, i: usize, what: &str) -> Result<&str, CliError> {
        self.positional
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| CliError::usage(format!("missing {what}")))
    }

    fn no_more(&self, n: usize) -> Result<(), CliError> {
        match self.positional.get(n) {
            Some(x) => Err(CliError::usage(format!("unexpected argument `{x}`"))),
            None => Ok(()),
        }
    }
}

fn is_param(word: &str) -> bool {
    word.split_once('=').is_some_and(|(k, _)| {
        let k = k.trim_end_matches(':');
        !k.is_empty()
            && k.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    })
}

/// `key=value` (numbers and booleans typed, text otherwise) or `key:=json`.
fn parse_param(word: &str) -> Result<(String, Value), CliError> {
    let (k, v) = word
        .split_once('=')
        .ok_or_else(|| CliError::usage(format!("`{word}` is not key=value")))?;
    if let Some(k) = k.strip_suffix(':') {
        let v = serde_json::from_str(v).map_err(|e| {
            CliError::new("invalid_params", format!("{k}: `{v}` is not JSON: {e}"))
                .suggest("quote text as JSON, e.g. text:='\"2024\"'")
        })?;
        return Ok((k.to_string(), v));
    }
    let typed = if TEXT_KEYS.contains(&k.to_ascii_lowercase().as_str()) {
        Value::String(v.to_string())
    } else {
        match v {
            "true" => json!(true),
            "false" => json!(false),
            "null" => Value::Null,
            _ => match v.parse::<i64>() {
                Ok(n) => json!(n),
                Err(_) => match v.parse::<f64>() {
                    Ok(f) if f.is_finite() && !v.starts_with('.') => json!(f),
                    _ if v.starts_with('[') || v.starts_with('{') => {
                        serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.to_string()))
                    }
                    _ => Value::String(v.to_string()),
                },
            },
        }
    };
    Ok((k.to_string(), typed))
}

// ------------------------------------------------------------------ commands

fn dispatch(o: &Opts, input: &mut dyn Read) -> Result<Reply, CliError> {
    let verb = o.positional.first().map(String::as_str).unwrap_or("help");
    match verb {
        "help" => help(o),
        "create" => create(o),
        "info" => {
            let mut doc = open(o, 1)?;
            o.no_more(2)?;
            let mut info = doc.info()?;
            if let Some(obj) = info.as_object_mut() {
                obj.insert("kind".into(), json!(doc.kind().name()));
            }
            Ok(Reply::data(info))
        }
        "view" => view_cmd(o),
        "get" => {
            let mut doc = open(o, 1)?;
            let path = o.arg(2, "path (e.g. /body/p[1]; `weave-office help paths`)")?;
            o.no_more(3)?;
            Ok(Reply::data(view::get(&mut doc, path)?))
        }
        "set" | "add" | "remove" => edit_cmd(o),
        "commands" => commands_cmd(o),
        "run" => run_cmd(o),
        "batch" => batch_cmd(o, input),
        "export" => {
            let mut doc = open(o, 1)?;
            let out = PathBuf::from(o.arg(2, "output file")?);
            o.no_more(3)?;
            let r = doc.export(&out, o.page)?;
            if !out.is_file() {
                return Err(CliError::new(
                    "engine",
                    format!("{}: nothing was written", out.display()),
                ));
            }
            Ok(
                Reply::text(json!({"output": out, "result": r}), String::new())
                    .note(format!("wrote {}", out.display())),
            )
        }
        other => {
            let close = suggest::closest(other, VERBS.iter().copied(), 3);
            let e = CliError::new("usage", format!("unknown command `{other}`"));
            Err(if close.is_empty() {
                e.suggest("run `weave-office help`")
            } else {
                e.suggest(format!("did you mean: {}?", close.join(", ")))
            })
        }
    }
}

fn open(o: &Opts, at: usize) -> Result<Doc, CliError> {
    let file = o.arg(at, "file")?;
    Doc::open(Path::new(file), o.password.as_deref())
}

fn help(o: &Opts) -> Result<Reply, CliError> {
    let topic = o.positional.get(1).map(String::as_str);
    let text = match topic {
        None => USAGE.to_string(),
        Some("agent" | "ai" | "skill") => AGENT_GUIDE.to_string(),
        Some("paths" | "path") => PATHS_HELP.to_string(),
        Some("props" | "properties" | "set" | "add") => edit::PROPS_HELP.to_string(),
        Some(verb) if VERBS.contains(&verb) => {
            let lines: Vec<&str> = USAGE
                .lines()
                .filter(|l| l.trim_start().starts_with(verb))
                .collect();
            format!(
                "{}\n\n{}",
                lines.join("\n"),
                "Run `weave-office help` for options and `weave-office help agent` for a worked guide."
            )
        }
        Some(other) => {
            return Err(CliError::usage(format!("no help on `{other}`"))
                .suggest("topics: a command name, `agent` or `paths`"));
        }
    };
    Ok(Reply::text(json!(text), text))
}

fn create(o: &Opts) -> Result<Reply, CliError> {
    let file = PathBuf::from(o.arg(1, "file")?);
    o.no_more(2)?;
    if file.exists() && !o.force {
        return Err(
            CliError::new("exists", format!("{} already exists", file.display()))
                .suggest("pass --force to replace it"),
        );
    }
    let kind = Kind::from_path(&file)?;
    let mut doc = Doc::blank(kind, o.template.as_deref())?;
    save::save_doc(&mut doc, &file)?;
    Ok(
        Reply::text(json!({"file": file, "kind": kind.name()}), String::new())
            .note(format!("created {}", file.display())),
    )
}

fn view_cmd(o: &Opts) -> Result<Reply, CliError> {
    let mut doc = open(o, 1)?;
    let mode = o.positional.get(2).map(String::as_str).unwrap_or("outline");
    o.no_more(3)?;
    match mode {
        "outline" => {
            let lines = view::outline(&mut doc)?;
            let text = if lines.is_empty() {
                "(empty)".to_string()
            } else {
                lines.join("\n")
            };
            Ok(Reply::text(json!(lines), text))
        }
        "text" => {
            let t = view::text(&mut doc)?;
            Ok(Reply::text(json!(t), t))
        }
        "json" | "inspect" | "tree" => Ok(Reply::data(doc.inspect()?)),
        other => Err(CliError::usage(format!("unknown view `{other}`"))
            .suggest("views: outline, text, json")),
    }
}

fn commands_cmd(o: &Opts) -> Result<Reply, CliError> {
    let which = o.arg(1, "kind (word, sheet, slides, pdf) or a file")?;
    let kind = Kind::from_alias(which)
        .or_else(|| Kind::from_path(Path::new(which)).ok())
        .ok_or_else(|| {
            CliError::usage(format!("`{which}` is not a kind of document"))
                .suggest(format!("kinds: {}", Kind::ALL.map(Kind::name).join(", ")))
        })?;
    let keyword = o.positional.get(2).map(|k| k.to_ascii_lowercase());
    o.no_more(3)?;
    let all = engines::commands(kind);
    // An exact id gives that command alone, with its full description.
    if let Some(k) = &keyword
        && let Some(c) = all.iter().find(|c| c.id.eq_ignore_ascii_case(k))
    {
        let text = format!(
            "{}  {}\n  params: {}\n{}{}",
            c.id,
            c.label,
            pretty(&c.params),
            if c.location.is_empty() {
                String::new()
            } else {
                format!("  where: {}\n", c.location)
            },
            c.description
        );
        return Ok(Reply::text(c.to_json(), text));
    }
    let list: Vec<_> = all
        .into_iter()
        .filter(|c| {
            keyword.as_ref().is_none_or(|k| {
                c.id.to_ascii_lowercase().contains(k.as_str())
                    || c.label.to_ascii_lowercase().contains(k.as_str())
                    || c.location.to_ascii_lowercase().contains(k.as_str())
            })
        })
        .collect();
    if list.is_empty() {
        return Err(CliError::new(
            "not_found",
            format!(
                "no {} command matches `{}`",
                kind.name(),
                keyword.unwrap_or_default()
            ),
        )
        .suggest(format!(
            "list them all with `weave-office commands {}`",
            kind.name()
        )));
    }
    let text = list
        .iter()
        .map(|c| {
            let params = match &c.params {
                Value::String(s) => s.clone(),
                Value::Object(schema) => schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|p| {
                        p.keys()
                            .filter(|k| *k != "doc")
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default(),
                other => other.to_string(),
            };
            format!("{:<34} {:<30} {}", c.id, c.label, params)
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Reply::text(
        json!(
            list.iter()
                .map(engines::CommandDesc::to_json)
                .collect::<Vec<_>>()
        ),
        text,
    ))
}

/// Merge `--params JSON` under the `key=value` parameters.
fn params_of(o: &Opts) -> Result<Value, CliError> {
    let mut base = match &o.params_json {
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(m)) => m,
            Ok(_) => {
                return Err(CliError::new(
                    "invalid_params",
                    "--params must be a JSON object",
                ));
            }
            Err(e) => {
                return Err(CliError::new(
                    "invalid_params",
                    format!("--params is not JSON: {e}"),
                ));
            }
        },
        None => Map::new(),
    };
    for (k, v) in &o.params {
        base.insert(k.clone(), v.clone());
    }
    Ok(Value::Object(base))
}

fn run_cmd(o: &Opts) -> Result<Reply, CliError> {
    let file = PathBuf::from(o.arg(1, "file")?);
    let id = o
        .arg(
            2,
            "command id (list them with `weave-office commands <kind>`)",
        )?
        .to_string();
    o.no_more(3)?;
    let params = params_of(o)?;
    let mut doc = Doc::open(&file, o.password.as_deref())?;
    let result = doc.execute(&id, params)?;
    let saved = finish(o, &mut doc, &file)?;
    let mut reply = Reply::data(json!({"command": id, "result": result, "saved": saved}));
    reply.text = Some(pretty(&result));
    Ok(match &saved {
        Some(p) => reply.note(format!("saved {}", p.display())),
        None if o.dry_run => reply.note("dry run: nothing written"),
        None => reply,
    })
}

/// Save after `run`/`batch`: to `--out`, or in place when something changed (or `--save`).
fn finish(o: &Opts, doc: &mut Doc, file: &Path) -> Result<Option<PathBuf>, CliError> {
    if o.dry_run {
        return Ok(None);
    }
    if let Some(out) = &o.out {
        save::save_doc(doc, out)?;
        return Ok(Some(out.clone()));
    }
    if o.save || doc.dirty() {
        save::save_doc(doc, file)?;
        return Ok(Some(file.to_path_buf()));
    }
    Ok(None)
}

fn batch_cmd(o: &Opts, input: &mut dyn Read) -> Result<Reply, CliError> {
    let file = PathBuf::from(o.arg(1, "file")?);
    o.no_more(2)?;
    let text = match (&o.commands, &o.input) {
        (Some(c), _) => c.clone(),
        (None, Some(p)) => std::fs::read_to_string(p)
            .map_err(|e| CliError::new("io", format!("{}: {e}", p.display())))?,
        (None, None) => {
            let mut s = String::new();
            input.read_to_string(&mut s)?;
            s
        }
    };
    let items = parse_batch(&text)?;
    let mut doc = Doc::open(&file, o.password.as_deref())?;
    let mut results = Vec::new();
    let mut failed = 0usize;
    for (n, item) in items.iter().enumerate() {
        let label = item.label();
        if failed > 0 && !o.keep_going {
            results.push(json!({"index": n + 1, "command": label, "status": "skipped"}));
            continue;
        }
        match item.apply(&mut doc) {
            Ok(v) => {
                results.push(json!({"index": n + 1, "command": label, "status": "ok", "result": v}))
            }
            Err(e) => {
                failed += 1;
                results.push(json!({"index": n + 1, "command": label, "status": "failed", "error": e.to_json()}));
            }
        }
    }
    if failed > 0 {
        let first = results
            .iter()
            .find(|r| r["status"] == "failed")
            .cloned()
            .unwrap_or_default();
        let msg = format!(
            "{failed} of {} commands failed; {} was not changed (first failure: command {} `{}`: {})",
            items.len(),
            file.display(),
            first["index"],
            first["command"].as_str().unwrap_or(""),
            first["error"]["message"].as_str().unwrap_or("")
        );
        let mut e = CliError::new("batch_failed", msg);
        if let Some(s) = first["error"]["suggestion"].as_str() {
            e = e.suggest(s.to_string());
        }
        e.details = Some(json!({"results": results}));
        return Err(e);
    }
    let saved = finish(o, &mut doc, &file)?;
    let text = results
        .iter()
        .map(|r| {
            format!(
                "{}. {} ok{}",
                r["index"],
                r["command"].as_str().unwrap_or(""),
                short_result(&r["result"])
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut reply = Reply::text(json!({"results": results, "saved": saved}), text);
    if let Some(p) = &saved {
        reply = reply.note(format!("saved {}", p.display()));
    } else if o.dry_run {
        reply = reply.note("dry run: nothing written");
    }
    Ok(reply)
}

fn short_result(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        other => {
            let s = other.to_string();
            if s.chars().count() > 120 {
                format!(": {}…", s.chars().take(120).collect::<String>())
            } else {
                format!(": {s}")
            }
        }
    }
}

/// One step of a batch.
enum Item {
    Command(String, Value),
    Get(String),
    Set(String, Map<String, Value>),
    Add {
        parent: String,
        kind: String,
        at: edit::Position,
        props: Map<String, Value>,
    },
    Remove(String),
}

impl Item {
    fn label(&self) -> String {
        match self {
            Item::Command(id, _) => id.clone(),
            Item::Get(p) => format!("get {p}"),
            Item::Set(p, _) => format!("set {p}"),
            Item::Add { parent, kind, .. } => format!("add {kind} to {parent}"),
            Item::Remove(p) => format!("remove {p}"),
        }
    }
    fn apply(&self, doc: &mut Doc) -> Result<Value, CliError> {
        match self {
            Item::Command(id, params) => doc.execute(id, params.clone()),
            Item::Get(p) => view::get(doc, p),
            Item::Set(p, props) => edit::set(doc, p, props),
            Item::Add {
                parent,
                kind,
                at,
                props,
            } => edit::add(doc, parent, kind, at, props),
            Item::Remove(p) => edit::remove(doc, p),
        }
    }
}

/// A batch: a JSON list (or `{"commands": [...]}`) of steps, each one of
/// `{"command": id, "params": {...}}` (`op`/`args` also accepted), `{"get": path}`,
/// `{"set": path, "props": {...}}`, `{"add": parent, "type": T, "props": {...}, "after"|"before"|"index": …}`
/// or `{"remove": path}`.
fn parse_batch(text: &str) -> Result<Vec<Item>, CliError> {
    let v: Value = serde_json::from_str(text).map_err(|e| {
        CliError::new("invalid_params", format!("the batch is not JSON: {e}"))
            .suggest(r#"give a list like [{"command":"text.insert","params":{"text":"Hi"}}]"#)
    })?;
    let list = match v {
        Value::Array(a) => a,
        Value::Object(mut m) => match m.remove("commands") {
            Some(Value::Array(a)) => a,
            _ => {
                return Err(CliError::new(
                    "invalid_params",
                    "a batch is a JSON list of commands",
                ));
            }
        },
        _ => {
            return Err(CliError::new(
                "invalid_params",
                "a batch is a JSON list of commands",
            ));
        }
    };
    if list.is_empty() {
        return Err(CliError::new("invalid_params", "the batch is empty"));
    }
    list.into_iter()
        .enumerate()
        .map(|(n, item)| {
            let bad = |what: &str| {
                CliError::new("invalid_params", format!("batch item {}: {what}", n + 1))
            };
            let props = || -> Result<Map<String, Value>, CliError> {
                match item.get("props") {
                    None => Ok(Map::new()),
                    Some(Value::Object(m)) => Ok(m.clone()),
                    Some(_) => Err(bad("`props` must be an object")),
                }
            };
            if let Some(p) = item.get("get").and_then(Value::as_str) {
                return Ok(Item::Get(p.into()));
            }
            if let Some(p) = item.get("set").and_then(Value::as_str) {
                return Ok(Item::Set(p.into(), props()?));
            }
            if let Some(p) = item.get("remove").and_then(Value::as_str) {
                return Ok(Item::Remove(p.into()));
            }
            if let Some(p) = item.get("add").and_then(Value::as_str) {
                let kind = item
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or_else(|| bad("`add` needs a `type`"))?
                    .to_ascii_lowercase();
                let at = if let Some(a) = item.get("after").and_then(Value::as_str) {
                    edit::Position::After(a.into())
                } else if let Some(b) = item.get("before").and_then(Value::as_str) {
                    edit::Position::Before(b.into())
                } else if let Some(i) = item
                    .get("index")
                    .and_then(Value::as_u64)
                    .filter(|i| *i >= 1)
                {
                    edit::Position::Index(i as usize)
                } else {
                    edit::Position::End
                };
                return Ok(Item::Add {
                    parent: p.into(),
                    kind,
                    at,
                    props: props()?,
                });
            }
            let id = item
                .get("command")
                .or_else(|| item.get("op"))
                .and_then(Value::as_str)
                .ok_or_else(|| bad("give `command`, `get`, `set`, `add` or `remove`"))?;
            let params = item
                .get("params")
                .or_else(|| item.get("args"))
                .cloned()
                .unwrap_or(json!({}));
            Ok(Item::Command(id.to_string(), params))
        })
        .collect()
}

/// `set`, `add` and `remove`: change one node, then save like `run`.
fn edit_cmd(o: &Opts) -> Result<Reply, CliError> {
    let verb = o.positional[0].as_str();
    let file = PathBuf::from(o.arg(1, "file")?);
    let path = o
        .arg(
            2,
            if verb == "add" {
                "parent path (/body, /, /slide[1] …)"
            } else {
                "path (see `weave-office view <file> outline`)"
            },
        )?
        .to_string();
    o.no_more(3)?;
    let props = match params_of(o)? {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let mut doc = Doc::open(&file, o.password.as_deref())?;
    let result = match verb {
        "set" => edit::set(&mut doc, &path, &props)?,
        "add" => {
            let kind = o.node_type.as_deref().ok_or_else(|| CliError::usage("add needs --type (paragraph, heading, table, sheet, slide, textbox, page, text …)"))?;
            edit::add(&mut doc, &path, kind, &o.position, &props)?
        }
        _ => {
            if !props.is_empty() {
                return Err(CliError::usage("remove takes no properties"));
            }
            edit::remove(&mut doc, &path)?
        }
    };
    let saved = finish(o, &mut doc, &file)?;
    let mut data = result;
    data["saved"] = json!(saved);
    let text = format!("{} {}", verb, data["path"].as_str().unwrap_or(&path));
    let mut reply = Reply::text(data, text);
    if let Some(p) = &saved {
        reply = reply.note(format!("saved {}", p.display()));
    } else if o.dry_run {
        reply = reply.note("dry run: nothing written");
    }
    Ok(reply)
}

#[cfg(test)]
mod tests;
