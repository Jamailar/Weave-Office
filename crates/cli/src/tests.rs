use std::ffi::OsString;
use std::path::Path;

use serde_json::{Value, json};

use super::*;

/// Run the CLI with `--json` and stdin `input`; returns the exit code and the envelope.
fn cli(dir: &Path, args: &[&str], input: &str) -> (i32, Value) {
    let mut full: Vec<String> = args
        .iter()
        .map(|a| {
            // File arguments are relative to the test folder.
            if !a.starts_with('/') && !a.contains('=') && Kind::from_path(Path::new(a)).is_ok() {
                dir.join(a).to_string_lossy().into_owned()
            } else {
                a.to_string()
            }
        })
        .collect();
    full.push("--json".into());
    let out = run(&full, &mut input.as_bytes());
    let v: Value = serde_json::from_str(&out.stdout)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", out.stdout));
    (out.code, v)
}

fn ok(dir: &Path, args: &[&str]) -> Value {
    let (code, v) = cli(dir, args, "");
    assert_eq!(code, 0, "{args:?} failed: {v:#}");
    assert_eq!(v["ok"], true);
    v["data"].clone()
}

fn err(dir: &Path, args: &[&str], input: &str) -> (i32, Value) {
    let (code, v) = cli(dir, args, input);
    assert_ne!(code, 0, "{args:?} should fail: {v:#}");
    assert_eq!(v["ok"], false);
    (code, v["error"].clone())
}

#[test]
fn verbs_run_the_cli_but_files_named_like_verbs_open_the_app() {
    assert!(is_cli(&[OsString::from("view"), OsString::from("a.docx")]));
    assert!(!is_cli(&[OsString::from("a.docx")]));
    assert!(!is_cli(&[]));
    assert!(!is_cli(&[OsString::from("--smoke-startup")]));
}

#[test]
fn document_create_batch_view_and_get() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "r.docx"]);
    let steps = r#"[{"command":"text.insert","params":{"text":"Quarterly report"}},
        {"command":"para.heading1"},
        {"command":"caret.docEnd"},{"command":"text.newParagraph"},{"command":"para.normal"},
        {"command":"text.insert","params":{"text":"Revenue grew 25%.\nCosts fell.","paragraphs":true}},
        {"command":"insert.table","params":{"rows":2,"cols":3}},
        {"get":"/body/p[1]"}]"#;
    let (code, v) = cli(d, &["batch", "r.docx"], steps);
    assert_eq!(code, 0, "{v:#}");
    assert_eq!(v["data"]["results"][7]["result"]["style"], "Heading1");
    ok(
        d,
        &["run", "r.docx", "edit.replaceAll", "text=25%", "with=27%"],
    );
    let outline = ok(d, &["view", "r.docx", "outline"]);
    let lines: Vec<&str> = outline
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(lines[0], "/body/p[1] (Heading1) \"Quarterly report\"");
    assert_eq!(lines[1], "/body/p[2] (Normal) \"Revenue grew 27%.\"");
    assert!(lines.contains(&"/body/table[1] 2x3"), "{lines:?}");
    assert_eq!(
        ok(d, &["get", "r.docx", "/body/p[3]"])["text"],
        "Costs fell."
    );
    let text = ok(d, &["view", "r.docx", "text"]);
    assert!(
        text.as_str()
            .unwrap()
            .starts_with("Quarterly report\nRevenue grew 27%.")
    );
}

#[test]
fn a_failed_batch_leaves_the_file_untouched_and_reports_every_step() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "r.docx"]);
    ok(d, &["run", "r.docx", "text.insert", "text=keep me"]);
    let before = std::fs::read(d.join("r.docx")).unwrap();
    let (code, e) = err(
        d,
        &["batch", "r.docx"],
        r#"[{"command":"text.insert","params":{"text":"X"}},{"command":"format.bols"},{"command":"format.bold"}]"#,
    );
    assert_eq!(code, 1);
    assert_eq!(e["code"], "batch_failed");
    let results = e["details"]["results"].as_array().unwrap();
    assert_eq!(results[0]["status"], "ok");
    assert_eq!(results[1]["status"], "failed");
    assert_eq!(results[1]["error"]["code"], "unknown_command");
    assert_eq!(results[2]["status"], "skipped");
    assert_eq!(
        std::fs::read(d.join("r.docx")).unwrap(),
        before,
        "a failed batch must not write"
    );
    assert!(
        std::fs::read_dir(d).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("weave-tmp")),
        "no temporary file is left"
    );
}

#[test]
fn mistakes_come_back_with_codes_and_suggestions() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "r.docx"]);
    let (_, e) = err(d, &["run", "r.docx", "format.bols"], "");
    assert_eq!(e["code"], "unknown_command");
    assert_eq!(e["candidates"][0], "format.bold");
    let (_, e) = err(d, &["get", "r.docx", "/body/p[9]"], "");
    assert_eq!(e["code"], "not_found");
    assert_eq!(e["suggestion"], "valid paragraph range: 1-1");
    let (code, e) = err(d, &["veiw", "r.docx"], "");
    assert_eq!((code, e["code"].as_str()), (2, Some("usage")));
    assert!(e["suggestion"].as_str().unwrap().contains("view"));
    let (_, e) = err(d, &["view", "missing.docx"], "");
    assert_eq!(e["code"], "not_found");
    let (_, e) = err(d, &["create", "x.abc"], "");
    assert_eq!(e["code"], "unsupported_format");
    let (_, e) = err(d, &["create", "r.docx"], "");
    assert_eq!(e["code"], "exists");
    ok(d, &["create", "r.docx", "--force"]);
}

#[test]
fn spreadsheet_values_formulas_and_ranges() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "s.xlsx"]);
    ok(
        d,
        &[
            "run",
            "s.xlsx",
            "range.setValues",
            "range=A1",
            r#"values:=[["Region","Q1"],["North",120],["South",95],["Total","=SUM(B2:B3)"]]"#,
        ],
    );
    ok(d, &["run", "s.xlsx", "home.bold", "range=A1:B1"]);
    let cell = ok(d, &["get", "s.xlsx", "/Sheet1/B4"]);
    assert_eq!(cell["value"], json!(215.0));
    assert_eq!(cell["formula"], "=SUM(B2:B3)");
    assert_eq!(
        ok(d, &["get", "s.xlsx", "/Sheet1/A1"])["style"]["font"]["bold"],
        true
    );
    assert_eq!(
        ok(d, &["get", "s.xlsx", "/sheet[1]/A2:B2"])["values"],
        json!([["North", 120.0]])
    );
    // Text keys stay text even when they look like numbers.
    ok(d, &["run", "s.xlsx", "cell.set", "cell=C1", "input=007"]);
    assert_eq!(
        ok(d, &["view", "s.xlsx", "outline"]),
        json!(["/Sheet1 used=A1:C4 cells=9"])
    );
    let (_, e) = err(d, &["get", "s.xlsx", "/Nope/A1"], "");
    assert_eq!(e["candidates"], json!(["Sheet1"]));
    ok(d, &["export", "s.xlsx", "s.csv"]);
    assert!(
        std::fs::read_to_string(d.join("s.csv"))
            .unwrap()
            .starts_with("Region,Q1")
    );
}

#[test]
fn presentation_slides_shapes_and_text() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "p.pptx"]);
    ok(
        d,
        &[
            "run",
            "p.pptx",
            "slide.new",
            "layout=titleAndContent",
            "title=Q4 Report",
            "body=Revenue +25%",
        ],
    );
    let outline = ok(d, &["view", "p.pptx"]);
    let lines: Vec<&str> = outline
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        lines.iter().filter(|l| !l.contains("/shape[")).count(),
        2,
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("/slide[2]/shape[1]") && l.ends_with("\"Q4 Report\"")),
        "{lines:?}"
    );
    let title = ok(d, &["get", "p.pptx", "/slide[2]/shape[@name=Title 1]"]);
    assert_eq!(title["path"], "/slide[2]/shape[1]");
    let text = ok(d, &["view", "p.pptx", "text"]);
    assert!(
        text.as_str()
            .unwrap()
            .contains("## Slide 2\nQ4 Report\nRevenue +25%"),
        "{text}"
    );
    let (_, e) = err(d, &["get", "p.pptx", "/slide[5]"], "");
    assert_eq!(e["suggestion"], "valid slide range: 1-2");
}

#[test]
fn pdf_tools_get_the_open_document_filled_in() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "p.pdf"]);
    ok(
        d,
        &[
            "run",
            "p.pdf",
            "page_add_text",
            "page=1",
            "text=Hello PDF",
            "at:=[72,72]",
        ],
    );
    assert_eq!(ok(d, &["get", "p.pdf", "/page[1]"])["text"], "Hello PDF");
    assert_eq!(ok(d, &["info", "p.pdf"])["kind"], "pdf");
    // The schema shown hides `doc`, which the CLI fills in.
    let schema = ok(d, &["commands", "pdf", "page_insert_blank"]);
    assert!(schema["params"]["properties"].get("doc").is_none());
    assert_eq!(schema["params"]["required"], json!(["at"]));
}

#[test]
fn dry_run_and_out_leave_the_original_alone() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "r.docx"]);
    let before = std::fs::read(d.join("r.docx")).unwrap();
    let v = ok(
        d,
        &["run", "r.docx", "text.insert", "text=draft", "--dry-run"],
    );
    assert_eq!(v["saved"], Value::Null);
    let out = d.join("copy.docx").to_string_lossy().into_owned();
    ok(
        d,
        &["run", "r.docx", "text.insert", "text=copy", "--out", &out],
    );
    assert_eq!(std::fs::read(d.join("r.docx")).unwrap(), before);
    assert_eq!(ok(d, &["get", "copy.docx", "/body/p[1]"])["text"], "copy");
    // A read-only command does not rewrite the file.
    ok(d, &["run", "r.docx", "document.selection"]);
    assert_eq!(std::fs::read(d.join("r.docx")).unwrap(), before);
}

#[test]
fn commands_lists_and_explains() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    for kind in ["word", "sheet", "slides", "pdf", "docx", "excel", "ppt"] {
        assert!(
            ok(d, &["commands", kind]).as_array().unwrap().len() > 20,
            "{kind}"
        );
    }
    let one = ok(d, &["commands", "word", "format.bold"]);
    assert_eq!(one["id"], "format.bold");
    let found = ok(d, &["commands", "sheet", "rename"]);
    assert!(
        found
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "sheet.rename")
    );
    let help = run(&["help".into()], &mut "".as_bytes());
    assert!(help.stdout.contains("weave-office <command>"));
    let agent = run(&["help".into(), "agent".into()], &mut "".as_bytes());
    assert!(agent.stdout.contains("guide for AI agents"));
}

#[test]
fn parameters_parse_by_key() {
    let (_, v) = parse_param("rows=3").unwrap();
    assert_eq!(v, json!(3));
    let (_, v) = parse_param("text=2024").unwrap();
    assert_eq!(v, json!("2024"));
    let (_, v) = parse_param("color=000000").unwrap();
    assert_eq!(v, json!("000000"));
    let (_, v) = parse_param("on=true").unwrap();
    assert_eq!(v, json!(true));
    let (_, v) = parse_param("at:=[72,72]").unwrap();
    assert_eq!(v, json!([72, 72]));
    let (_, v) = parse_param("text:=\"x\"").unwrap();
    assert_eq!(v, json!("x"));
    assert_eq!(parse_param("at:=[oops").unwrap_err().code, "invalid_params");
}

#[test]
fn document_paths_set_add_and_remove() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "r.docx"]);
    // The blank document's empty paragraph is filled, not left above the heading.
    assert_eq!(
        ok(
            d,
            &["add", "r.docx", "/body", "--type", "heading", "text=Report"]
        )["path"],
        "/body/p[1]"
    );
    ok(
        d,
        &[
            "add",
            "r.docx",
            "/body",
            "--type",
            "paragraph",
            "text=Second",
        ],
    );
    ok(
        d,
        &[
            "add",
            "r.docx",
            "/body",
            "--type",
            "paragraph",
            "text=First",
            "--before",
            "/body/p[2]",
        ],
    );
    let set = ok(
        d,
        &[
            "set",
            "r.docx",
            "/body/p[3]",
            "text=Last one",
            "bold=true",
            "color=#C00000",
            "align=center",
        ],
    );
    assert_eq!(set["node"]["text"], "Last one");
    let run = &set["node"]["runs"][0]["props"];
    assert_eq!(run["bold"], true);
    assert_eq!(run["color"], json!({"Rgb": [192, 0, 0]}));
    ok(
        d,
        &[
            "add", "r.docx", "/body", "--type", "table", "rows=2", "cols=3",
        ],
    );
    ok(
        d,
        &[
            "add",
            "r.docx",
            "/body",
            "--type",
            "paragraph",
            "text=After the table",
        ],
    );
    let outline = ok(d, &["view", "r.docx"]);
    assert_eq!(
        outline,
        json!([
            "/body/p[1] (Heading1) \"Report\"",
            "/body/p[2] (Normal) \"First\"",
            "/body/p[3] (Normal) \"Last one\"",
            "/body/table[1] 2x3",
            "/body/p[4] (Normal) \"After the table\""
        ])
    );
    ok(d, &["remove", "r.docx", "/body/p[2]"]);
    ok(d, &["remove", "r.docx", "/body/table[1]"]);
    let text = ok(d, &["view", "r.docx", "text"]);
    assert_eq!(
        text.as_str()
            .unwrap()
            .split('\n')
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>(),
        ["Report", "Last one", "After the table"]
    );
    let (_, e) = err(d, &["set", "r.docx", "/body/p[1]", "colour=red"], "");
    assert_eq!(e["candidates"], json!(["color"]));
}

#[test]
fn spreadsheet_paths_set_add_and_remove() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "s.xlsx"]);
    ok(
        d,
        &[
            "set",
            "s.xlsx",
            "/Sheet1/A1",
            r#"values:=[["Region","Q1"],["North",120],["South",95]]"#,
        ],
    );
    let total = ok(d, &["set", "s.xlsx", "/Sheet1/B4", "value==SUM(B2:B3)"]);
    assert_eq!(total["node"]["value"], json!(215.0));
    ok(
        d,
        &[
            "set",
            "s.xlsx",
            "/Sheet1/A1:B1",
            "bold=true",
            "fill=FFFF00",
            "align=center",
        ],
    );
    let b2 = ok(d, &["set", "s.xlsx", "/Sheet1/B2", "format=0.00"]);
    assert_eq!(b2["node"]["text"], "120.00");
    let a1 = ok(d, &["get", "s.xlsx", "/Sheet1/A1"]);
    assert_eq!(a1["style"]["font"]["bold"], true);
    assert_eq!(a1["style"]["align"]["h"], "Center");
    ok(d, &["set", "s.xlsx", "/Sheet1/C1", "value=007"]);
    assert_eq!(ok(d, &["get", "s.xlsx", "/Sheet1/C1"])["text"], "7");
    // Renaming and colouring in one call: the colour must not look up the old name.
    ok(
        d,
        &["set", "s.xlsx", "/Sheet1", "name=Sales", "tabColor=00B050"],
    );
    assert_eq!(
        ok(d, &["add", "s.xlsx", "/", "--type", "sheet", "name=Notes"])["path"],
        "/Notes"
    );
    ok(d, &["remove", "s.xlsx", "/Notes"]);
    ok(d, &["remove", "s.xlsx", "/Sales/A3:B3"]);
    assert_eq!(
        ok(d, &["get", "s.xlsx", "/Sales/B4"])["value"],
        json!(120.0)
    );
    assert_eq!(
        ok(d, &["view", "s.xlsx"]),
        json!(["/Sales used=A1:C4 cells=6"])
    );
}

#[test]
fn presentation_paths_set_add_and_remove() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "p.pptx"]);
    let title = ok(
        d,
        &[
            "set",
            "p.pptx",
            "/slide[1]/shape[1]",
            "text=Annual review",
            "bold=true",
        ],
    );
    assert_eq!(title["node"]["text"], "Annual review");
    ok(
        d,
        &["set", "p.pptx", "/slide[1]/shape[2]", "text=First\nSecond"],
    );
    ok(
        d,
        &[
            "set",
            "p.pptx",
            "/slide[1]/shape[1]",
            "text=Annual review 2026",
        ],
    );
    assert_eq!(
        ok(
            d,
            &[
                "add",
                "p.pptx",
                "/",
                "--type",
                "slide",
                "layout=titleAndContent",
                "title=Q4",
                "body=Revenue +25%"
            ]
        )["path"],
        "/slide[2]"
    );
    let tb = ok(
        d,
        &[
            "add",
            "p.pptx",
            "/slide[2]",
            "--type",
            "textbox",
            "text=Note",
            "x=100",
            "y=400",
            "w=300",
            "h=40",
            "fill=FFF2CC",
        ],
    );
    assert_eq!(tb["path"], "/slide[2]/shape[3]");
    ok(d, &["set", "p.pptx", "/slide[2]", "notes=Speaker notes"]);
    assert_eq!(
        ok(
            d,
            &[
                "add",
                "p.pptx",
                "/",
                "--type",
                "slide",
                "layout=blank",
                "--index",
                "1"
            ]
        )["path"],
        "/slide[1]"
    );
    ok(d, &["remove", "p.pptx", "/slide[1]"]);
    ok(d, &["remove", "p.pptx", "/slide[2]/shape[3]"]);
    let text = ok(d, &["view", "p.pptx", "text"]);
    assert_eq!(
        text,
        json!(
            "## Slide 1\nAnnual review 2026\nFirst\nSecond\n## Slide 2\nQ4\nRevenue +25%\nNotes: Speaker notes"
        )
    );
}

#[test]
fn pdf_paths_set_add_and_remove() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "p.pdf"]);
    ok(
        d,
        &["add", "p.pdf", "/page[1]", "--type", "text", "text=Hello"],
    );
    assert_eq!(
        ok(d, &["add", "p.pdf", "/", "--type", "page"])["path"],
        "/page[2]"
    );
    ok(d, &["set", "p.pdf", "/page[2]", "rotate=90"]);
    assert_eq!(ok(d, &["get", "p.pdf", "/page[2]"])["rotation"], 90);
    ok(d, &["remove", "p.pdf", "/page[1]"]);
    assert_eq!(
        ok(d, &["view", "p.pdf"]),
        json!(["/page[1] 792x612pt \"\""])
    );
}

#[test]
fn batches_mix_path_edits_and_commands_and_stay_atomic() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "r.docx"]);
    let steps = r#"[{"add":"/body","type":"heading","props":{"text":"Plan"}},
        {"add":"/body","type":"paragraph","props":{"text":"Step one"}},
        {"set":"/body/p[2]","props":{"italic":true}},
        {"command":"edit.replaceAll","params":{"text":"one","with":"1"}},
        {"get":"/body/p[2]"}]"#;
    let (code, v) = cli(d, &["batch", "r.docx"], steps);
    assert_eq!(code, 0, "{v:#}");
    assert_eq!(v["data"]["results"][4]["result"]["text"], "Step 1");
    let before = std::fs::read(d.join("r.docx")).unwrap();
    let (_, e) = err(
        d,
        &["batch", "r.docx"],
        r#"[{"remove":"/body/p[1]"},{"set":"/body/p[9]","props":{"text":"x"}}]"#,
    );
    assert_eq!(e["details"]["results"][1]["error"]["code"], "not_found");
    assert_eq!(std::fs::read(d.join("r.docx")).unwrap(), before);
}

#[test]
fn document_text_goes_in_as_typed() {
    // AutoCorrect would capitalise "a" and turn "(c)" into a symbol once the next paragraph starts.
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    ok(d, &["create", "t.docx"]);
    ok(
        d,
        &["add", "t.docx", "/body", "--type", "paragraph", "text=a"],
    );
    ok(
        d,
        &[
            "add",
            "t.docx",
            "/body",
            "--type",
            "paragraph",
            "text=teh (c) b",
        ],
    );
    ok(
        d,
        &["add", "t.docx", "/body", "--type", "paragraph", "text=end"],
    );
    let text = ok(d, &["view", "t.docx", "text"]).to_string();
    assert!(text.contains("teh (c) b"), "{text}");
    let first = ok(d, &["get", "t.docx", "/body/p[1]"]);
    let second = ok(d, &["get", "t.docx", "/body/p[2]"]);
    assert!(
        [first["text"].clone(), second["text"].clone()].contains(&json!("a")),
        "{first:#} {second:#}"
    );
}
