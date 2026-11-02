//! A small external-producer interop check using a workbook created and saved in WPS.
//! It covers these cells and styles, rather than general XLSX fidelity.

use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use weave_office::{editor::Editor, host::Host};
use weave_workspace::TabId;
use wordcraft_doc::{
    Block,
    props::{Align, Rgb, TextColor},
};

fn cell(host: &mut Host, id: TabId, address: &str) -> Value {
    let editor = &mut host.documents.get_mut(&id).expect("open workbook").editor;
    let Editor::Sheet(app) = editor else {
        panic!("WPS workbook opened in a non-spreadsheet editor");
    };
    app.hosted_command("cell.get", json!({"cell": address}))
        .unwrap_or_else(|error| panic!("read {address}: {error}"))
}

fn assert_number(host: &mut Host, id: TabId, address: &str, expected: f64) {
    let actual = cell(host, id, address);
    assert_eq!(
        actual["value"].as_f64(),
        Some(expected),
        "{address}: {actual}"
    );
}

fn assert_formula(host: &mut Host, id: TabId, address: &str, formula: &str, expected: f64) {
    let actual = cell(host, id, address);
    assert_eq!(actual["formula"], formula, "{address}: {actual}");
    assert_eq!(
        actual["value"].as_f64(),
        Some(expected),
        "{address}: {actual}"
    );
}

fn assert_workbook(host: &mut Host, id: TabId, quantity: f64, first_total: f64, grand_total: f64) {
    for (address, expected) in [
        ("A1", "项目"),
        ("B1", "数量"),
        ("C1", "单价"),
        ("D1", "金额"),
        ("E1", "日期"),
        ("F1", "备注"),
        ("A2", "纸张"),
        ("F2", "中文与公式"),
        ("A3", "墨盒"),
        ("F3", "第二条"),
        ("A4", "装订"),
        ("F4", "含小数"),
        ("A5", "合计"),
        ("F5", "只编辑此样本"),
    ] {
        let actual = cell(host, id, address);
        assert_eq!(actual["value"], expected, "{address}: {actual}");
    }

    for address in ["A1", "B1", "C1", "D1", "E1", "F1"] {
        let actual = cell(host, id, address);
        assert_eq!(actual["style"]["font"]["bold"], true, "{address}: {actual}");
        assert_eq!(
            actual["style"]["font"]["name"], "宋体",
            "{address}: {actual}"
        );
        assert_eq!(
            actual["style"]["font"]["size"].as_f64(),
            Some(11.0),
            "{address}: {actual}"
        );
    }

    for (address, serial) in [("E2", 46327.0), ("E3", 46328.0), ("E4", 46329.0)] {
        let actual = cell(host, id, address);
        assert_eq!(
            actual["value"].as_f64(),
            Some(serial),
            "{address}: {actual}"
        );
        assert_eq!(
            actual["style"]["num_fmt"], "m/d/yyyy",
            "{address}: {actual}"
        );
    }

    for (address, expected) in [
        ("B2", quantity),
        ("C2", 15.0),
        ("B3", 3.0),
        ("C3", 80.0),
        ("B4", 5.0),
        ("C4", 4.5),
    ] {
        assert_number(host, id, address, expected);
    }
    assert_formula(host, id, "D2", "=B2*C2", first_total);
    assert_formula(host, id, "D3", "=B3*C3", 240.0);
    assert_formula(host, id, "D4", "=B4*C4", 22.5);
    assert_formula(host, id, "D5", "=SUM(D2:D4)", grand_total);
}

#[test]
fn wps_workbook_edit_save_and_fresh_host_reopen() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wps-origin-pricing.xlsx");
    let original_bytes = fs::read(&fixture).expect("WPS fixture");
    let directory = tempfile::tempdir().expect("output directory");
    let output = directory.path().join("edited.xlsx");

    let mut host = Host::default();
    let id = host.open(&fixture).expect("open WPS workbook");
    assert_workbook(&mut host, id, 2.0, 30.0, 292.5);

    let editor = &mut host.documents.get_mut(&id).expect("open workbook").editor;
    let Editor::Sheet(app) = editor else {
        panic!("expected spreadsheet editor")
    };
    app.hosted_command("cell.set", json!({"cell": "B2", "input": "4"}))
        .expect("change paper quantity");
    assert_workbook(&mut host, id, 4.0, 60.0, 322.5);
    host.save_as(id, &output).expect("save edited XLSX");
    assert_eq!(
        fs::read(&fixture).expect("original fixture"),
        original_bytes
    );

    let mut reopened_host = Host::default();
    let reopened = reopened_host
        .open(&output)
        .expect("open edited XLSX in fresh host");
    assert_workbook(&mut reopened_host, reopened, 4.0, 60.0, 322.5);
    assert_eq!(
        fs::read(&fixture).expect("original fixture"),
        original_bytes
    );
}

// Inspect the actual OOXML package independently of the WordCraft parser. Python's standard
// library keeps this test free of a second DOCX implementation and checks the WPS producer too.
const DOCX_XML_PROBE: &str = r#"
import json, sys, zipfile, xml.etree.ElementTree as ET
w = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'
a = lambda el, key: None if el is None else el.get(w + key)
with zipfile.ZipFile(sys.argv[1]) as z:
    doc = ET.fromstring(z.read('word/document.xml'))
    app = ET.fromstring(z.read('docProps/app.xml'))
body = doc.find(w + 'body')
paras = body.findall(w + 'p')
def text(el):
    return ''.join(t.text or '' for t in el.iter(w + 't'))
def run_named(p, value):
    return next(r for r in p.findall(w + 'r') if text(r) == value)
title, intro, emphasis = paras[:3]
title_run = title.find(w + 'r')
red_run = run_named(emphasis, '红色强调')
bold_run = run_named(emphasis, '加粗文本 Bold')
table = body.find(w + 'tbl')
rows = table.findall(w + 'tr')
cells = [row.findall(w + 'tc') for row in rows]
print(json.dumps({
    'application': app.findtext('{http://schemas.openxmlformats.org/officeDocument/2006/extended-properties}Application'),
    'title': text(title),
    'title_align': a(title.find(w + 'pPr/' + w + 'jc'), 'val'),
    'title_color': a(title_run.find(w + 'rPr/' + w + 'color'), 'val'),
    'intro': text(intro),
    'space_before': a(intro.find(w + 'pPr/' + w + 'spacing'), 'before'),
    'space_after': a(intro.find(w + 'pPr/' + w + 'spacing'), 'after'),
    'bold_text': text(bold_run),
    'bold': bold_run.find(w + 'rPr/' + w + 'b') is not None,
    'red_text': text(red_run),
    'red_color': a(red_run.find(w + 'rPr/' + w + 'color'), 'val'),
    'cells': [[text(c) for c in row] for row in cells],
    'header_shades': [a(c.find(w + 'tcPr/' + w + 'shd'), 'fill') for c in cells[0]],
    'tail': text(next(p for p in paras if text(p).startswith('尾段：'))),
}, ensure_ascii=False))
"#;

fn docx_xml(path: &Path) -> Value {
    let output = Command::new("python3")
        .arg("-c")
        .arg(DOCX_XML_PROBE)
        .arg(path)
        .output()
        .expect("run Python standard-library ZIP/XML probe");
    assert!(
        output.status.success(),
        "DOCX XML probe: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("DOCX XML probe JSON")
}

fn assert_word_xml(xml: &Value, edited: bool) {
    assert_eq!(xml["title"], "互操作样张 · Weave Office");
    assert_eq!(xml["title_align"], "center");
    assert_eq!(xml["title_color"], "17365D");
    assert_eq!(
        xml["intro"],
        "这是一份在 WPS 新建的文字样本，用于核对中文、English 与段落格式。"
    );
    assert_eq!(xml["space_before"], "240");
    assert_eq!(xml["space_after"], "160");
    assert_eq!(xml["bold_text"], "加粗文本 Bold");
    assert_eq!(xml["bold"], true);
    assert_eq!(xml["red_text"], "红色强调");
    assert_eq!(xml["red_color"], "C00000");
    assert_eq!(
        xml["cells"],
        json!([["项目", "说明"], ["文字与表格", "中文内容 123.45"]])
    );
    assert_eq!(xml["header_shades"], json!(["D9E2F3", "D9E2F3"]));
    assert_eq!(
        xml["tail"],
        if edited {
            "尾段：保存后重开确实保留标题、强调与两行表格。"
        } else {
            "尾段：保存后重开仍应保留标题、强调与两行表格。"
        }
    );
}

fn assert_word_model(host: &mut Host, id: TabId, edited: bool) {
    let Editor::Word(app) = &mut host.documents.get_mut(&id).expect("open DOCX").editor else {
        panic!("WPS DOCX opened in a non-Word editor");
    };
    let body = &app.session.doc.body;
    let Block::Para(title) = body[0].as_ref() else {
        panic!("title paragraph")
    };
    assert_eq!(title.text, "互操作样张 · Weave Office");
    assert_eq!(title.props.align, Some(Align::Center));
    assert!(
        title
            .runs
            .iter()
            .any(|run| run.props.color == Some(TextColor::Rgb(Rgb(0x17, 0x36, 0x5D))))
    );

    let Block::Para(intro) = body[1].as_ref() else {
        panic!("intro paragraph")
    };
    assert_eq!(intro.props.space_before, Some(12.0));
    assert_eq!(intro.props.space_after, Some(8.0));
    let Block::Para(emphasis) = body[2].as_ref() else {
        panic!("emphasis paragraph")
    };
    assert!(emphasis.text.contains("加粗文本 Bold"));
    assert!(emphasis.runs.iter().any(|run| run.props.bold == Some(true)));
    assert!(
        emphasis
            .runs
            .iter()
            .any(|run| run.props.color == Some(TextColor::Rgb(Rgb(0xC0, 0, 0))))
    );

    let table = body
        .iter()
        .find_map(|block| block.as_table())
        .expect("WPS table");
    assert_eq!(table.rows.len(), 2);
    assert!(table.rows.iter().all(|row| row.cells.len() == 2));
    assert!(
        table.rows[0]
            .cells
            .iter()
            .all(|cell| cell.props.shading == Some(Rgb(0xD9, 0xE2, 0xF3)))
    );

    let text = app
        .hosted_command("document.text", json!({}))
        .expect("read DOCX text");
    let expected = if edited {
        "尾段：保存后重开确实保留"
    } else {
        "尾段：保存后重开仍应保留"
    };
    assert!(
        text["text"]
            .as_str()
            .expect("document text")
            .contains(expected)
    );
}

#[test]
fn wps_docx_edit_save_and_fresh_host_reopen_preserves_layout_semantics() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wps-origin-text.docx");
    let original_bytes = fs::read(&fixture).expect("WPS DOCX fixture");
    let source_xml = docx_xml(&fixture);
    assert!(
        source_xml["application"]
            .as_str()
            .expect("producer metadata")
            .starts_with("WPS Office_")
    );
    assert_word_xml(&source_xml, false);

    let directory = tempfile::tempdir().expect("output directory");
    let output = directory.path().join("edited.docx");
    let mut host = Host::default();
    let id = host.open(&fixture).expect("open WPS DOCX");
    assert_word_model(&mut host, id, false);

    let Editor::Word(app) = &mut host.documents.get_mut(&id).expect("open DOCX").editor else {
        panic!("expected Word editor");
    };
    app.hosted_command("select.text", json!({"text": "仍应"}))
        .expect("select tail text");
    app.hosted_command("text.insert", json!({"text": "确实"}))
        .expect("edit tail text");
    assert_word_model(&mut host, id, true);
    host.save_as(id, &output).expect("save edited DOCX");
    assert_eq!(
        fs::read(&fixture).expect("original fixture"),
        original_bytes
    );
    assert_word_xml(&docx_xml(&output), true);

    let mut reopened_host = Host::default();
    let reopened = reopened_host
        .open(&output)
        .expect("reopen edited DOCX in fresh host");
    assert_word_model(&mut reopened_host, reopened, true);
    assert_eq!(
        fs::read(&fixture).expect("original fixture"),
        original_bytes
    );
}

// Follow presentation relationships instead of assuming slideN.xml names encode display order.
const PPTX_XML_PROBE: &str = r#"
import json, posixpath, sys, zipfile, xml.etree.ElementTree as ET
p = '{http://schemas.openxmlformats.org/presentationml/2006/main}'
a = '{http://schemas.openxmlformats.org/drawingml/2006/main}'
r = '{http://schemas.openxmlformats.org/officeDocument/2006/relationships}'
rel_ns = '{http://schemas.openxmlformats.org/package/2006/relationships}'
with zipfile.ZipFile(sys.argv[1]) as z:
    pres = ET.fromstring(z.read('ppt/presentation.xml'))
    rels = {rel.get('Id'): rel.get('Target') for rel in ET.fromstring(z.read('ppt/_rels/presentation.xml.rels'))}
    app = ET.fromstring(z.read('docProps/app.xml'))
    size = pres.find(p + 'sldSz')
    slides = []
    for ref in pres.findall(p + 'sldIdLst/' + p + 'sldId'):
        path = posixpath.normpath(posixpath.join('ppt', rels[ref.get(r + 'id')]))
        root = ET.fromstring(z.read(path))
        shapes = []
        for shape in root.findall('.//' + p + 'spTree/' + p + 'sp'):
            xf = shape.find(p + 'spPr/' + a + 'xfrm')
            off = xf.find(a + 'off') if xf is not None else None
            ext = xf.find(a + 'ext') if xf is not None else None
            fill = shape.find(p + 'spPr/' + a + 'solidFill')
            fill_color = next(iter(fill), None) if fill is not None else None
            autofit = shape.find(p + 'txBody/' + a + 'bodyPr/' + a + 'spAutoFit') is not None
            paragraphs = []
            for para in shape.findall(p + 'txBody/' + a + 'p'):
                runs = []
                for run in para.findall(a + 'r'):
                    props = run.find(a + 'rPr')
                    color = props.find(a + 'solidFill/' + a + 'srgbClr') if props is not None else None
                    latin = props.find(a + 'latin') if props is not None else None
                    runs.append({'text': run.findtext(a + 't', default=''), 'size': props.get('sz') if props is not None else None,
                                 'bold': props.get('b') if props is not None else None,
                                 'color': color.get('val') if color is not None else None,
                                 'font': latin.get('typeface') if latin is not None else None})
                paragraphs.append({'text': ''.join(run['text'] for run in runs), 'runs': runs})
            shapes.append({'xfrm': [off.get('x'), off.get('y'), ext.get('cx'), ext.get('cy')] if off is not None and ext is not None else None,
                           'fill': [fill_color.tag.removeprefix(a), fill_color.get('val')] if fill_color is not None else None,
                           'autofit': autofit,
                           'paragraphs': paragraphs})
        slides.append({'shapes': shapes})
    print(json.dumps({'application': app.findtext('{http://schemas.openxmlformats.org/officeDocument/2006/extended-properties}Application'),
                      'size': [size.get('cx'), size.get('cy')], 'slides': slides}, ensure_ascii=False))
"#;

fn pptx_xml(path: &Path) -> Value {
    let output = Command::new("python3")
        .arg("-c")
        .arg(PPTX_XML_PROBE)
        .arg(path)
        .output()
        .expect("run Python standard-library PPTX ZIP/XML probe");
    assert!(
        output.status.success(),
        "PPTX XML probe: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("PPTX XML probe JSON")
}

fn assert_pptx_run_style(paragraph: &Value, size: &str, bold: Option<&str>, color: Option<&str>) {
    let runs = paragraph["runs"].as_array().expect("text runs");
    assert!(!runs.is_empty());
    for run in runs {
        assert_eq!(run["size"], size, "{run}");
        assert_eq!(run["bold"].as_str(), bold, "{run}");
        assert_eq!(run["color"].as_str(), color, "{run}");
        assert_eq!(run["font"], "Arial", "{run}");
    }
}

fn assert_pptx_xml(xml: &Value, edited: bool) {
    assert_eq!(xml["size"], json!(["12192000", "6858000"]));
    let slides = xml["slides"]
        .as_array()
        .expect("slides in presentation order");
    assert_eq!(slides.len(), 2);
    assert!(slides.iter().all(|slide| {
        slide["shapes"]
            .as_array()
            .is_some_and(|shapes| shapes.len() == 1)
    }));
    let first = &slides[0]["shapes"][0];
    assert_eq!(
        first["xfrm"],
        json!(["3556000", "3044825", "5080000", "768350"])
    );
    assert_eq!(first["fill"], Value::Null);
    assert_eq!(first["autofit"], true);
    assert_eq!(first["paragraphs"][0]["text"], "互操作演示 · Weave Office");
    assert_eq!(
        first["paragraphs"][1]["text"],
        "中文与 English，保存后重开核对文字和格式。"
    );
    assert_pptx_run_style(&first["paragraphs"][0], "2800", Some("1"), Some("17365D"));
    assert_pptx_run_style(&first["paragraphs"][1], "1600", None, None);

    let second = &slides[1]["shapes"][0];
    let second_xfrm = second["xfrm"].as_array().expect("second shape geometry");
    assert_eq!(
        &second_xfrm[..3],
        &json!(["3556000", "3098800", "5080000"]).as_array().unwrap()[..]
    );
    if edited {
        // <a:spAutoFit/> explicitly allows the box height to change when text changes.
        assert!(
            second_xfrm[3]
                .as_str()
                .expect("height EMU")
                .parse::<i64>()
                .expect("numeric height")
                > 0
        );
    } else {
        assert_eq!(second_xfrm[3], "660400");
    }
    assert_eq!(second["fill"], json!(["schemeClr", "accent1"]));
    assert_eq!(second["autofit"], true);
    assert_eq!(second["paragraphs"][0]["text"], "第二页 · Geometry & Color");
    assert_eq!(
        second["paragraphs"][1]["text"],
        if edited {
            "位置、尺寸与红色文字需保留！"
        } else {
            "位置、尺寸与红色文字需保留。"
        }
    );
    assert_pptx_run_style(&second["paragraphs"][0], "2100", Some("1"), Some("17365D"));
    assert_pptx_run_style(&second["paragraphs"][1], "1600", None, Some("C00000"));
}

fn assert_pptx_model(host: &Host, id: TabId, edited: bool) {
    let Editor::Slides(app) = &host.documents.get(&id).expect("open PPTX").editor else {
        panic!("WPS PPTX opened in a non-presentation editor");
    };
    let presentation = &app.session.active().expect("active presentation").doc;
    assert_eq!(presentation.slides.len(), 2);
    assert_eq!(presentation.slide_size.width, 960.0);
    assert_eq!(presentation.slide_size.height, 540.0);
    let first = &presentation.slides[0].shapes;
    let second = &presentation.slides[1].shapes;
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].xfrm.expect("first shape geometry").x, 280.0);
    assert_eq!(second[0].xfrm.expect("second shape geometry").y, 244.0);
    assert_eq!(
        first[0].text.as_ref().expect("first text").paragraphs[0].text(),
        "互操作演示 · Weave Office"
    );
    assert_eq!(
        second[0].text.as_ref().expect("second text").paragraphs[0].text(),
        "第二页 · Geometry & Color"
    );
    assert_eq!(
        second[0].text.as_ref().expect("second text").paragraphs[1].text(),
        if edited {
            "位置、尺寸与红色文字需保留！"
        } else {
            "位置、尺寸与红色文字需保留。"
        }
    );
    let fill = serde_json::to_value(&second[0].fill).expect("shape fill JSON");
    assert_eq!(fill["kind"], "solid");
    assert_eq!(fill["color"]["base"]["kind"], "scheme");
    assert_eq!(fill["color"]["base"]["slot"], "accent1");
}

#[test]
fn wps_pptx_edit_save_and_fresh_host_reopen_preserves_shape_semantics() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wps-origin-slides.pptx");
    let original_bytes = fs::read(&fixture).expect("WPS PPTX fixture");
    let source_xml = pptx_xml(&fixture);
    assert_eq!(source_xml["application"], "WPS 表格");
    assert_pptx_xml(&source_xml, false);

    let directory = tempfile::tempdir().expect("output directory");
    let output = directory.path().join("edited.pptx");
    let mut host = Host::default();
    let id = host.open(&fixture).expect("open WPS PPTX");
    assert_pptx_model(&host, id, false);
    let Editor::Slides(app) = &mut host.documents.get_mut(&id).expect("open PPTX").editor else {
        panic!("expected presentation editor");
    };
    app.hosted_command("slide.go", json!({"index": 1}))
        .expect("select second slide");
    let shape_id = app.session.active().expect("presentation").doc.slides[1].shapes[0]
        .id
        .0;
    app.hosted_command("text.edit", json!({"id": shape_id}))
        .expect("edit second text box");
    app.hosted_command("text.select", json!({"anchor": [1, 13], "caret": [1, 14]}))
        .expect("select final punctuation");
    app.hosted_command("text.insert", json!({"text": "！"}))
        .expect("edit final punctuation");
    assert_pptx_model(&host, id, true);
    let Editor::Slides(app) = &host.documents.get(&id).expect("open PPTX").editor else {
        panic!("expected presentation editor");
    };
    let edited_height = app.session.active().expect("presentation").doc.slides[1].shapes[0]
        .xfrm
        .expect("edited shape geometry")
        .h;
    assert!(edited_height > 0.0);
    host.save_as(id, &output).expect("save edited PPTX");
    assert_eq!(
        fs::read(&fixture).expect("original fixture"),
        original_bytes
    );
    let output_xml = pptx_xml(&output);
    assert_pptx_xml(&output_xml, true);
    assert_eq!(
        output_xml["slides"][1]["shapes"][0]["xfrm"][3],
        ((edited_height * 12_700.0).round() as i64).to_string(),
        "PPTX writer must serialize the auto-fitted model height"
    );

    let mut reopened_host = Host::default();
    let reopened = reopened_host
        .open(&output)
        .expect("reopen edited PPTX in fresh host");
    assert_pptx_model(&reopened_host, reopened, true);
    assert_eq!(
        fs::read(&fixture).expect("original fixture"),
        original_bytes
    );
}

#[test]
fn wps_pptx_unedited_save_preserves_exact_shape_geometry() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wps-origin-slides.pptx");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("unedited.pptx");
    let mut host = Host::default();
    let id = host.open(&fixture).unwrap();
    host.save_as(id, &output).unwrap();
    let source = pptx_xml(&fixture);
    let saved = pptx_xml(&output);
    assert_pptx_xml(&source, false);
    assert_pptx_xml(&saved, false);
    for index in 0..2 {
        assert_eq!(
            saved["slides"][index]["shapes"][0]["xfrm"],
            source["slides"][index]["shapes"][0]["xfrm"]
        );
    }
}
