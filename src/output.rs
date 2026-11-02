//! Read-only PDF snapshots for the hosted editors.
//!
//! The host owns destination selection and writing. Preparing bytes does not
//! change the editor's path or acknowledge any unsaved changes.

use crate::editor::Editor;
use serde_json::json;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PdfPurpose {
    Export,
    Print,
}

pub fn pdf_bytes(editor: &mut Editor, purpose: PdfPurpose) -> Result<Vec<u8>, String> {
    // Flush transient edits first, including the PDF comment composer guard.
    editor.flush()?;
    match editor {
        // Text files print through the document engine: its Markdown and plain-text
        // import lays them out as pages.
        Editor::TextFile(app) => {
            let name = match app.kind {
                weave_text_editor::Kind::Markdown => "document.md",
                weave_text_editor::Kind::PlainText => "document.txt",
            };
            let document = wordcraft_engine::io::open_bytes(name, app.text().as_bytes())?;
            let session = wordcraft_engine::Session::new(document);
            let (bytes, _) = match purpose {
                PdfPurpose::Export => session.print_pdf(&json!({})),
                PdfPurpose::Print => session.print_job_pdf(&json!({})),
            }
            .map_err(|error| error.to_string())?;
            Ok(bytes)
        }
        Editor::Word(app) => {
            let (bytes, _) = match purpose {
                PdfPurpose::Export => app.session.print_pdf(&json!({})),
                PdfPurpose::Print => app.session.print_job_pdf(&json!({})),
            }
            .map_err(|error| error.to_string())?;
            Ok(bytes)
        }
        Editor::Sheet(app) => {
            let command = match purpose {
                PdfPurpose::Export => "file.exportPdf",
                PdfPurpose::Print => "file.print",
            };
            let result = app
                .session
                .execute(command, json!({}))
                .map_err(|error| error.to_string())?;
            let encoded = result
                .get("base64")
                .and_then(serde_json::Value::as_str)
                .ok_or("表格引擎没有返回 PDF 数据")?;
            gridcraft_engine::io::base64_decode(encoded)
                .ok_or_else(|| "表格引擎返回了无效的 PDF 数据".into())
        }
        Editor::Slides(app) => {
            let active = app.session.active().ok_or("没有打开的演示文稿")?;
            deckcraft_engine::cmd::file::pdf_bytes(&active.doc, &json!({}))
                .map_err(|error| error.to_string())
        }
        Editor::Pdf(app) => {
            let (_, id) = app.active_ids().ok_or("没有打开的 PDF")?;
            match purpose {
                PdfPurpose::Export => {
                    let bytes = app
                        .session
                        .save_bytes(id)
                        .map_err(|error| error.to_string())?;
                    app.session
                        .check_save(id, &bytes)
                        .map_err(|error| error.to_string())?;
                    Ok(bytes.as_ref().clone())
                }
                PdfPurpose::Print => {
                    let count = app
                        .session
                        .get(id)
                        .ok_or("没有打开的 PDF")?
                        .info
                        .pages
                        .len();
                    if count == 0 {
                        return Err("PDF 没有可打印的页面".into());
                    }
                    let settings = pdfcraft_engine::print::Settings {
                        pages: (0..count).collect(),
                        ..Default::default()
                    };
                    app.session
                        .print_pdf(id, &settings)
                        .map_err(|error| error.to_string())
                }
            }
        }
    }
}
