//! Reading documents: plain text, an outline with addressable paths, and `get` by path.
//!
//! Paths are 1-based and follow the document's own structure:
//! - documents: `/body/p[N]` (N-th paragraph), `/body/table[N]`, `/body/block[N]` (any block)
//! - spreadsheets: `/Sheet1`, `/Sheet1/B2`, `/Sheet1/A1:C10` (a sheet may also be `sheet[N]`)
//! - presentations: `/slide[N]`, `/slide[N]/shape[M]`, `/slide[N]/shape[@id=ID]`
//! - PDF: `/page[N]`
//!
//! `/` is the whole document.

use serde_json::{Value, json};

use crate::engines::{Doc, Kind};
use crate::error::CliError;

/// One step of a path: `name`, `name[3]` or `name[@id=7]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub name: String,
    pub index: Option<usize>,
    pub attr: Option<(String, String)>,
}

pub fn parse_path(path: &str) -> Result<Vec<Step>, CliError> {
    if !path.starts_with('/') {
        return Err(bad_path(path, "a path starts with `/`"));
    }
    let mut steps = Vec::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        let (name, rest) = match part.find('[') {
            Some(i) if part.ends_with(']') => (&part[..i], Some(&part[i + 1..part.len() - 1])),
            Some(_) => return Err(bad_path(path, "unclosed `[`")),
            None => (part, None),
        };
        let mut step = Step {
            name: name.to_string(),
            index: None,
            attr: None,
        };
        match rest {
            None => {}
            Some(r) if r.starts_with('@') => {
                let (k, v) = r[1..]
                    .split_once('=')
                    .ok_or_else(|| bad_path(path, "write attributes as [@id=VALUE]"))?;
                step.attr = Some((k.to_string(), v.trim_matches(['"', '\'']).to_string()));
            }
            Some(r) => {
                let n: usize = r
                    .trim()
                    .parse()
                    .map_err(|_| bad_path(path, "an index is a whole number from 1"))?;
                if n == 0 {
                    return Err(bad_path(path, "indexes start at 1"));
                }
                step.index = Some(n);
            }
        }
        steps.push(step);
    }
    Ok(steps)
}

fn bad_path(path: &str, why: &str) -> CliError {
    CliError::new("invalid_path", format!("`{path}`: {why}"))
        .suggest("run `weave-office view <file> outline` to see the paths in a document")
}

fn out_of_range(what: &str, n: usize, count: usize) -> CliError {
    let range = if count == 0 {
        format!("there are no {what}s")
    } else {
        format!("valid {what} range: 1-{count}")
    };
    CliError::new("not_found", format!("no {what} {n}")).suggest(range)
}

/// The node at `path`.
pub fn get(doc: &mut Doc, path: &str) -> Result<Value, CliError> {
    let steps = parse_path(path)?;
    if steps.is_empty() {
        return doc.inspect();
    }
    match doc.kind() {
        Kind::Word => word_get(doc, path, &steps),
        Kind::Sheet => sheet_get(doc, path, &steps),
        Kind::Slides => slides_get(doc, path, &steps),
        Kind::Pdf => pdf_get(doc, path, &steps),
    }
}

// ------------------------------------------------------------------ documents

fn word_blocks(doc: &mut Doc) -> Result<Vec<Value>, CliError> {
    Ok(doc
        .inspect()?
        .get("blocks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

fn is_table(b: &Value) -> bool {
    b.get("type").and_then(Value::as_str) == Some("table")
}

fn word_get(doc: &mut Doc, path: &str, steps: &[Step]) -> Result<Value, CliError> {
    let blocks = word_blocks(doc)?;
    if steps[0].name != "body" {
        return Err(bad_path(path, "document paths start with /body"));
    }
    let Some(step) = steps.get(1) else {
        return Ok(json!({"path": "/body", "blocks": blocks}));
    };
    if steps.len() > 2 {
        return Err(bad_path(
            path,
            "document paths go one level below /body (p, table or block)",
        ));
    }
    let n = step
        .index
        .ok_or_else(|| bad_path(path, "say which one, e.g. p[1]"))?;
    let (what, list): (&str, Vec<&Value>) = match step.name.as_str() {
        "p" | "paragraph" => (
            "paragraph",
            blocks.iter().filter(|b| !is_table(b)).collect(),
        ),
        "table" | "tbl" => ("table", blocks.iter().filter(|b| is_table(b)).collect()),
        "block" => ("block", blocks.iter().collect()),
        other => {
            return Err(bad_path(
                path,
                &format!("unknown element `{other}` (use p, table or block)"),
            ));
        }
    };
    let node = list
        .get(n - 1)
        .ok_or_else(|| out_of_range(what, n, list.len()))?;
    let mut node = (*node).clone();
    node["path"] = json!(format!(
        "/body/{}[{n}]",
        if what == "paragraph" { "p" } else { what }
    ));
    Ok(node)
}

// ------------------------------------------------------------------ spreadsheets

fn sheet_names(doc: &mut Doc) -> Result<Vec<String>, CliError> {
    Ok(doc
        .inspect()?
        .get("sheets")
        .and_then(Value::as_array)
        .map(|s| {
            s.iter()
                .filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

/// The sheet a step names: `Sheet1` or `sheet[2]`.
fn sheet_of(doc: &mut Doc, step: &Step) -> Result<(String, Value), CliError> {
    let tree = doc.inspect()?;
    let sheets = tree
        .get("sheets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let found = match (step.name.as_str(), step.index) {
        ("sheet", Some(n)) => sheets
            .get(n - 1)
            .cloned()
            .ok_or_else(|| out_of_range("sheet", n, sheets.len()))?,
        (name, None) => sheets
            .iter()
            .find(|s| {
                s.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|x| x.eq_ignore_ascii_case(name))
            })
            .cloned()
            .ok_or_else(|| {
                let names: Vec<String> = sheets
                    .iter()
                    .filter_map(|s| s.get("name").and_then(Value::as_str).map(str::to_string))
                    .collect();
                CliError::new("not_found", format!("no sheet named `{name}`"))
                    .suggest(format!("sheets: {}", names.join(", ")))
                    .with_candidates(names)
            })?,
        _ => {
            return Err(CliError::new(
                "invalid_path",
                "name a sheet as /Sheet1 or /sheet[1]",
            ));
        }
    };
    let name = found
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok((name, found))
}

fn sheet_get(doc: &mut Doc, path: &str, steps: &[Step]) -> Result<Value, CliError> {
    let (name, summary) = sheet_of(doc, &steps[0])?;
    match steps.get(1) {
        None => {
            let values = doc.execute("sheet.read", json!({"sheet": name}))?;
            Ok(
                json!({"path": format!("/{name}"), "sheet": summary, "values": values.get("values").cloned().unwrap_or_default(), "range": values.get("range").cloned().unwrap_or_default()}),
            )
        }
        Some(cell) if steps.len() == 2 => {
            let a1 = cell.name.to_ascii_uppercase();
            if a1.contains(':') {
                let mut v = doc.execute("sheet.read", json!({"sheet": name, "range": a1}))?;
                v["path"] = json!(format!("/{name}/{a1}"));
                Ok(v)
            } else {
                let mut v = doc.execute("cell.get", json!({"sheet": name, "cell": a1}))?;
                v["path"] = json!(format!("/{name}/{a1}"));
                Ok(v)
            }
        }
        _ => Err(bad_path(
            path,
            "spreadsheet paths are /Sheet, /Sheet/A1 or /Sheet/A1:C3",
        )),
    }
}

// ------------------------------------------------------------------ presentations

fn slide_count(doc: &mut Doc) -> Result<usize, CliError> {
    Ok(doc
        .inspect()?
        .get("slides")
        .and_then(Value::as_array)
        .map_or(0, Vec::len))
}

fn slides_get(doc: &mut Doc, path: &str, steps: &[Step]) -> Result<Value, CliError> {
    if steps[0].name != "slide" {
        return Err(bad_path(path, "presentation paths start with /slide[N]"));
    }
    let n = steps[0]
        .index
        .ok_or_else(|| bad_path(path, "say which slide, e.g. /slide[1]"))?;
    let count = slide_count(doc)?;
    if n > count {
        return Err(out_of_range("slide", n, count));
    }
    let mut slide = doc.execute("slide.inspect", json!({"index": n - 1}))?;
    let Some(step) = steps.get(1) else {
        slide["path"] = json!(format!("/slide[{n}]"));
        return Ok(slide);
    };
    if step.name != "shape" || steps.len() > 2 {
        return Err(bad_path(
            path,
            "below a slide, address shape[M] or shape[@id=ID]",
        ));
    }
    let shapes = slide
        .get("shapes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let (m, summary) = match (&step.attr, step.index) {
        (Some((k, v)), _) if k == "id" || k == "name" => shapes
            .iter()
            .enumerate()
            .find(|(_, s)| {
                s.get(k.as_str())
                    .is_some_and(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string) == *v)
            })
            .map(|(i, s)| (i + 1, s.clone()))
            .ok_or_else(|| {
                CliError::new("not_found", format!("no shape with {k}={v} on slide {n}"))
            })?,
        (_, Some(m)) => (
            m,
            shapes
                .get(m - 1)
                .cloned()
                .ok_or_else(|| out_of_range("shape", m, shapes.len()))?,
        ),
        _ => {
            return Err(bad_path(
                path,
                "say which shape: shape[1] or shape[@id=258]",
            ));
        }
    };
    let id = summary.get("id").cloned().unwrap_or_default();
    // The slide's summary (id, kind, name, text, box) first, the engine's full shape after.
    let mut full = summary.clone();
    if let Ok(detail) = doc.execute("shape.inspect", json!({"id": id})) {
        full["detail"] = detail.get("shape").cloned().unwrap_or(detail);
    }
    full["path"] = json!(format!("/slide[{n}]/shape[{m}]"));
    Ok(full)
}

// ------------------------------------------------------------------ PDF

fn pdf_get(doc: &mut Doc, path: &str, steps: &[Step]) -> Result<Value, CliError> {
    if steps[0].name != "page" || steps.len() > 1 {
        return Err(bad_path(path, "PDF paths are /page[N]"));
    }
    let n = steps[0]
        .index
        .ok_or_else(|| bad_path(path, "say which page, e.g. /page[1]"))?;
    let tree = doc.inspect()?;
    let pages = tree
        .get("pages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut page = pages
        .get(n - 1)
        .cloned()
        .ok_or_else(|| out_of_range("page", n, pages.len()))?;
    let text = doc.execute("text_extract", json!({"pages": [n]}))?;
    page["text"] = text
        .get("pages")
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .cloned()
        .unwrap_or_default();
    page["path"] = json!(format!("/page[{n}]"));
    Ok(page)
}

// ------------------------------------------------------------------ text and outline

fn clip(s: &str, n: usize) -> String {
    let one_line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > n {
        format!("{}…", one_line.chars().take(n).collect::<String>())
    } else {
        one_line
    }
}

/// Plain text for spreadsheets (tab-separated, one section per sheet) and presentations (one
/// section per slide).
pub fn text(doc: &mut Doc) -> Result<String, CliError> {
    match doc.kind() {
        Kind::Sheet => {
            let mut out = Vec::new();
            for name in sheet_names(doc)? {
                let v = doc.execute("sheet.read", json!({"sheet": name, "formatted": true}))?;
                out.push(format!("## {name}"));
                for row in v
                    .get("values")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let cells: Vec<String> = row
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(cell_text)
                        .collect();
                    out.push(cells.join("\t"));
                }
            }
            Ok(out.join("\n"))
        }
        Kind::Slides => {
            let mut out = Vec::new();
            for i in 0..slide_count(doc)? {
                let s = doc.execute("slide.inspect", json!({"index": i}))?;
                out.push(format!("## Slide {}", i + 1));
                collect_shape_text(s.get("shapes"), &mut out);
                if let Some(notes) = s
                    .get("notes")
                    .and_then(Value::as_str)
                    .filter(|n| !n.trim().is_empty())
                {
                    out.push(format!("Notes: {notes}"));
                }
            }
            Ok(out.join("\n"))
        }
        _ => doc.text(),
    }
}

fn collect_shape_text(shapes: Option<&Value>, out: &mut Vec<String>) {
    for sh in shapes.and_then(Value::as_array).into_iter().flatten() {
        if let Some(t) = sh
            .get("text")
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
        {
            out.push(t.to_string());
        }
        collect_shape_text(sh.get("children"), out);
    }
}

fn cell_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// One line per addressable node, each starting with its path.
pub fn outline(doc: &mut Doc) -> Result<Vec<String>, CliError> {
    let mut lines = Vec::new();
    match doc.kind() {
        Kind::Word => {
            let (mut p, mut t) = (0, 0);
            for b in word_blocks(doc)? {
                if is_table(&b) {
                    t += 1;
                    let size = match (b.get("rows"), b.get("cols")) {
                        (Some(r), Some(c)) => format!(" {}x{}", r, c),
                        _ => String::new(),
                    };
                    lines.push(format!("/body/table[{t}]{size}"));
                } else {
                    p += 1;
                    let style = b.get("style").and_then(Value::as_str).unwrap_or("");
                    let text = b.get("text").and_then(Value::as_str).unwrap_or("");
                    lines.push(format!("/body/p[{p}] ({style}) \"{}\"", clip(text, 80)));
                }
            }
        }
        Kind::Sheet => {
            let tree = doc.inspect()?;
            for s in tree
                .get("sheets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = s.get("name").and_then(Value::as_str).unwrap_or("");
                let used = s
                    .get("usedRange")
                    .and_then(Value::as_str)
                    .unwrap_or("empty");
                let cells = s.get("cells").cloned().unwrap_or(json!(0));
                lines.push(format!("/{name} used={used} cells={cells}"));
            }
        }
        Kind::Slides => {
            for i in 0..slide_count(doc)? {
                let s = doc.execute("slide.inspect", json!({"index": i}))?;
                let layout = s.get("layout").and_then(Value::as_str).unwrap_or("");
                let title = s.get("title").and_then(Value::as_str).unwrap_or("");
                lines.push(format!(
                    "/slide[{}] ({layout}) \"{}\"",
                    i + 1,
                    clip(title, 60)
                ));
                for (m, sh) in s
                    .get("shapes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    let kind = sh.get("kind").and_then(Value::as_str).unwrap_or("");
                    let name = sh.get("name").and_then(Value::as_str).unwrap_or("");
                    let id = sh.get("id").cloned().unwrap_or_default();
                    let text = sh
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|t| format!(" \"{}\"", clip(t, 60)))
                        .unwrap_or_default();
                    lines.push(format!(
                        "/slide[{}]/shape[{}] id={id} {kind} \"{name}\"{text}",
                        i + 1,
                        m + 1
                    ));
                }
            }
        }
        Kind::Pdf => {
            let tree = doc.inspect()?;
            let texts = doc.execute("text_extract", json!({}))?;
            let texts = texts
                .get("pages")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for (i, p) in tree
                .get("pages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let (w, h) = (
                    p.get("width").and_then(Value::as_f64).unwrap_or(0.0),
                    p.get("height").and_then(Value::as_f64).unwrap_or(0.0),
                );
                let first = texts
                    .get(i)
                    .and_then(|t| t.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                lines.push(format!(
                    "/page[{}] {w:.0}x{h:.0}pt \"{}\"",
                    i + 1,
                    clip(first, 60)
                ));
            }
        }
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_parse() {
        let s = parse_path("/slide[2]/shape[@id=258]").unwrap();
        assert_eq!(
            s[0],
            Step {
                name: "slide".into(),
                index: Some(2),
                attr: None
            }
        );
        assert_eq!(s[1].attr, Some(("id".into(), "258".into())));
        assert_eq!(parse_path("/").unwrap(), vec![]);
        assert_eq!(parse_path("/body/p[0]").unwrap_err().code, "invalid_path");
        assert_eq!(parse_path("body").unwrap_err().code, "invalid_path");
    }
}
