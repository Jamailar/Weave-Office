//! Host coordination for a cut whose source and destination have independent editor sessions.

use crate::{editor::Editor, host::Host};
use gridcraft_engine::{Clipboard, DocState, Selection, model::Workbook};
use serde_json::Value;
use std::sync::Arc;
use weave_workspace::TabId;

pub(crate) struct Request {
    pub generation: u64,
    pub command: String,
    pub params: Value,
    pub clipboard: Clipboard,
    pub target_uid: u64,
    target_wb: Arc<Workbook>,
    target_revision: u64,
    target_selection: Selection,
}

impl Request {
    pub fn new(
        generation: u64,
        command: &str,
        params: Value,
        clipboard: Clipboard,
        target: &DocState,
    ) -> Self {
        Self {
            generation,
            command: command.into(),
            params,
            clipboard,
            target_uid: target.uid,
            target_wb: target.wb.clone(),
            target_revision: target.revision,
            target_selection: target.selection.clone(),
        }
    }

    pub fn involves(&self, uid: u64) -> bool {
        self.target_uid == uid || self.clipboard.doc_uid == uid
    }
}

fn tab_for_uid(host: &Host, uid: u64) -> Result<TabId, String> {
    let mut matches = host
        .documents
        .iter()
        .filter_map(|(&id, document)| match &document.editor {
            Editor::Sheet(app) if app.session.active().is_some_and(|doc| doc.uid == uid) => {
                Some(id)
            }
            _ => None,
        });
    let id = matches
        .next()
        .ok_or("剪切的来源或目标工作簿已关闭，移动未执行")?;
    if matches.next().is_some() {
        return Err("工作簿身份不唯一，移动未执行".into());
    }
    Ok(id)
}

pub(crate) fn apply(host: &mut Host, request: &Request) -> Result<(), String> {
    if !matches!(request.command.as_str(), "edit.paste" | "edit.pasteSpecial") {
        return Err("跨工作簿剪切目前请使用普通粘贴；插入剪切单元格尚未接通，来源保持不变".into());
    }
    let source_id = tab_for_uid(host, request.clipboard.doc_uid)?;
    let target_id = tab_for_uid(host, request.target_uid)?;
    if source_id == target_id {
        return Err("跨工作簿移动的来源和目标不能是同一份文档".into());
    }
    let [Some(source), Some(target)] = host.documents.get_disjoint_mut([&source_id, &target_id])
    else {
        return Err("工作簿已关闭，移动未执行".into());
    };
    let (Editor::Sheet(source), Editor::Sheet(target)) = (&mut source.editor, &mut target.editor)
    else {
        return Err("移动只适用于表格工作簿".into());
    };
    if source.hosted_has_edit_draft() || target.hosted_has_edit_draft() {
        return Err("来源或目标工作簿仍有编辑中的草稿，请先完成编辑后重试移动".into());
    }
    let current = target.session.active().ok_or("目标工作簿已关闭")?;
    if current.uid != request.target_uid
        || current.revision != request.target_revision
        || !Arc::ptr_eq(&current.wb, &request.target_wb)
        || current.selection != request.target_selection
    {
        return Err("目标工作簿在粘贴请求后发生变化，移动未执行，请重新粘贴".into());
    }
    gridcraft_engine::Session::move_cut_between(
        &mut source.session,
        &mut target.session,
        &request.clipboard,
        &request.params,
    )
    .map_err(|error| format!("无法移动剪切的单元格：{error}"))?;
    source.hosted_sync_external_edit();
    target.hosted_sync_external_edit();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::Clipboard as HostClipboard;
    use serde_json::json;
    use weave_workspace::{CloseResult, DocumentKind};

    fn fixture() -> (Host, HostClipboard, TabId, TabId) {
        let clipboard = HostClipboard::memory();
        let mut host = Host::with_clipboard(clipboard.clone());
        let source = host.new_document(DocumentKind::Spreadsheet).unwrap();
        let target = host.new_document(DocumentKind::Spreadsheet).unwrap();
        command(
            &mut host,
            source,
            "cell.set",
            json!({"cell":"A1", "input":"7"}),
        );
        command(
            &mut host,
            source,
            "cell.set",
            json!({"cell":"A2", "input":"=$A$1*2"}),
        );
        command(&mut host, source, "home.bold", json!({"range":"A1:A2"}));
        (host, clipboard, source, target)
    }
    fn command(host: &mut Host, tab: TabId, id: &str, params: Value) -> Value {
        host.documents
            .get_mut(&tab)
            .unwrap()
            .editor
            .command(id, params)
            .unwrap()
    }
    fn sheet(host: &Host, tab: TabId) -> &gridcraft_ui_egui::SheetApp {
        let Editor::Sheet(sheet) = &host.documents[&tab].editor else {
            panic!("sheet fixture")
        };
        sheet
    }
    fn sheet_mut(host: &mut Host, tab: TabId) -> &mut gridcraft_ui_egui::SheetApp {
        let Editor::Sheet(sheet) = &mut host.documents.get_mut(&tab).unwrap().editor else {
            panic!("sheet fixture")
        };
        sheet
    }
    fn text(host: &mut Host, tab: TabId, cell: &str) -> Value {
        command(host, tab, "cell.get", json!({"cell":cell}))["text"].clone()
    }
    fn queue(host: &mut Host, source: TabId, target: TabId) {
        command(host, source, "edit.cut", json!({"range":"A1:A2"}));
        assert_eq!(
            command(host, target, "edit.paste", json!({"at":"C3"}))["pending"],
            "host"
        );
    }
    fn snapshot(host: &Host, tab: TabId) -> (Arc<Workbook>, String) {
        let doc = sheet(host, tab).session.active().unwrap();
        (doc.wb.clone(), format!("{doc:?}"))
    }
    fn unchanged(host: &Host, tab: TabId, before: &(Arc<Workbook>, String)) {
        let doc = sheet(host, tab).session.active().unwrap();
        assert!(Arc::ptr_eq(&doc.wb, &before.0));
        assert_eq!(format!("{doc:?}"), before.1);
    }

    #[test]
    fn host_move_preserves_values_styles_formulas_and_independent_history() {
        let (mut host, clipboard, source, target) = fixture();
        let original = command(&mut host, source, "cell.get", json!({"cell":"A2"}));
        queue(&mut host, source, target);
        let before_source = snapshot(&host, source);
        let before_target = snapshot(&host, target);
        assert_eq!(text(&mut host, target, "C3"), "");
        assert_eq!(host.pending_background(&[source]), Some(source));
        assert_eq!(host.pending_background(&[target]), Some(target));
        assert_eq!(host.close(source, true), CloseResult::NeedsSave);
        assert_eq!(host.close(target, true), CloseResult::NeedsSave);
        assert!(host.receive_sheet_transfers().is_empty());
        assert!(clipboard.sheet().is_none());
        assert_eq!(text(&mut host, source, "A1"), "");
        assert_eq!(text(&mut host, source, "A2"), "");
        assert_eq!(text(&mut host, target, "C3"), "7");
        assert_eq!(text(&mut host, target, "C4"), "14");
        let moved = command(&mut host, target, "cell.get", json!({"cell":"C4"}));
        assert_eq!(moved["formula"], "=$C$3*2");
        assert_eq!(moved["style"], original["style"]);
        assert!(host.dirty(source) && host.dirty(target));
        let after_source = snapshot(&host, source);
        let after_target = snapshot(&host, target);
        command(&mut host, source, "edit.undo", json!({}));
        assert!(Arc::ptr_eq(&snapshot(&host, source).0, &before_source.0));
        unchanged(&host, target, &after_target);
        command(&mut host, source, "edit.redo", json!({}));
        assert!(Arc::ptr_eq(&snapshot(&host, source).0, &after_source.0));
        command(&mut host, target, "edit.undo", json!({}));
        assert!(Arc::ptr_eq(&snapshot(&host, target).0, &before_target.0));
        command(&mut host, target, "edit.redo", json!({}));
        assert!(Arc::ptr_eq(&snapshot(&host, target).0, &after_target.0));
        assert!(
            host.documents
                .get_mut(&target)
                .unwrap()
                .editor
                .command("edit.paste", json!({"at":"F1"}))
                .is_err()
        );
    }

    #[test]
    fn stale_source_target_selection_or_drafts_refuse_without_mutating_either_side() {
        for scenario in [
            "source",
            "target",
            "selection",
            "source_draft",
            "target_draft",
        ] {
            let (mut host, clipboard, source, target) = fixture();
            queue(&mut host, source, target);
            match scenario {
                "source" => {
                    command(
                        &mut host,
                        source,
                        "cell.set",
                        json!({"cell":"Z1", "input":"changed"}),
                    );
                }
                "target" => {
                    command(
                        &mut host,
                        target,
                        "cell.set",
                        json!({"cell":"Z1", "input":"changed"}),
                    );
                }
                "selection" => {
                    command(&mut host, target, "selection.set", json!({"range":"F1"}));
                }
                "source_draft" => {
                    sheet_mut(&mut host, source).begin_edit(Some("draft".into()), false)
                }
                _ => sheet_mut(&mut host, target).begin_edit(Some("draft".into()), false),
            }
            let before_source = snapshot(&host, source);
            let before_target = snapshot(&host, target);
            assert_eq!(host.receive_sheet_transfers().len(), 1, "{scenario}");
            unchanged(&host, source, &before_source);
            unchanged(&host, target, &before_target);
            assert!(clipboard.sheet().is_some_and(|copy| copy.cut));
            assert!(!host.has_sheet_transfers());
        }
    }

    #[test]
    fn existing_target_draft_is_not_committed_by_cross_workbook_paste() {
        let (mut host, clipboard, source, target) = fixture();
        command(&mut host, source, "edit.cut", json!({"range":"A1:A2"}));
        sheet_mut(&mut host, target).begin_edit(Some("unfinished".into()), false);
        let before_source = snapshot(&host, source);
        let before_target = snapshot(&host, target);
        assert!(
            host.documents
                .get_mut(&target)
                .unwrap()
                .editor
                .command("edit.paste", json!({"at":"C3"}))
                .is_err()
        );
        unchanged(&host, source, &before_source);
        unchanged(&host, target, &before_target);
        assert_eq!(
            sheet(&host, target).editor.as_ref().unwrap().text,
            "unfinished"
        );
        assert!(clipboard.sheet().is_some_and(|copy| copy.cut));
        assert!(!host.has_sheet_transfers());
    }

    #[test]
    fn closed_source_and_late_result_never_write_a_replacement_document() {
        let (mut host, clipboard, source, target) = fixture();
        command(&mut host, source, "edit.cut", json!({"range":"A1:A2"}));
        assert_eq!(host.close(source, true), CloseResult::Closed);
        let replacement = host.new_document(DocumentKind::Spreadsheet).unwrap();
        command(&mut host, target, "edit.paste", json!({"at":"C3"}));
        let before_target = snapshot(&host, target);
        let before_replacement = snapshot(&host, replacement);
        assert_eq!(host.receive_sheet_transfers().len(), 1);
        unchanged(&host, target, &before_target);
        unchanged(&host, replacement, &before_replacement);
        assert!(clipboard.sheet().is_some_and(|copy| copy.cut));
    }

    #[test]
    fn cancellation_new_generation_and_external_copy_cancel_queued_moves() {
        for scenario in ["cancel", "new_copy", "external"] {
            let (mut host, clipboard, source, target) = fixture();
            queue(&mut host, source, target);
            match scenario {
                "cancel" => {
                    command(&mut host, target, "edit.clearClipboard", json!({}));
                }
                "new_copy" => {
                    command(&mut host, target, "edit.copy", json!({"range":"F1"}));
                }
                _ => clipboard.external_copy("external", "<b>external</b>"),
            }
            let before_source = snapshot(&host, source);
            let before_target = snapshot(&host, target);
            assert!(host.receive_sheet_transfers().is_empty());
            unchanged(&host, source, &before_source);
            unchanged(&host, target, &before_target);
            if scenario == "new_copy" {
                assert!(clipboard.sheet().is_some_and(|copy| !copy.cut));
            }
            if scenario == "external" {
                assert_eq!(clipboard.text().as_deref(), Some("external"));
            }
        }
    }

    #[test]
    fn protected_overflow_special_and_insert_moves_are_refused_and_keep_cut() {
        for scenario in ["protected", "overflow", "special", "insert"] {
            let (mut host, clipboard, source, target) = fixture();
            if scenario == "protected" {
                command(&mut host, target, "review.protectSheet", json!({}));
            }
            command(&mut host, source, "edit.cut", json!({"range":"A1:A2"}));
            let (id, params) = match scenario {
                "overflow" => ("edit.paste", json!({"at":"XFD1048576"})),
                "special" => ("edit.pasteSpecial", json!({"at":"C3", "what":"values"})),
                "insert" => ("home.insertCopiedCells", json!({"at":"C3"})),
                _ => ("edit.paste", json!({"at":"C3"})),
            };
            command(&mut host, target, id, params);
            let before_source = snapshot(&host, source);
            let before_target = snapshot(&host, target);
            assert_eq!(host.receive_sheet_transfers().len(), 1, "{scenario}");
            unchanged(&host, source, &before_source);
            unchanged(&host, target, &before_target);
            assert!(clipboard.sheet().is_some_and(|copy| copy.cut));
        }
    }

    #[test]
    fn immediate_save_drains_move_and_both_xlsx_files_reopen_independently() {
        let (mut host, clipboard, source, target) = fixture();
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("source.xlsx");
        let target_path = directory.path().join("target.xlsx");
        queue(&mut host, source, target);
        host.save_as(target, &target_path).unwrap();
        assert!(clipboard.sheet().is_none());
        assert!(host.dirty(source));
        assert!(!host.dirty(target));
        host.save_as(source, &source_path).unwrap();
        let mut reopened_source = Host::with_clipboard(HostClipboard::memory());
        let mut reopened_target = Host::with_clipboard(HostClipboard::memory());
        let source = reopened_source.open(&source_path).unwrap();
        let target = reopened_target.open(&target_path).unwrap();
        assert_eq!(text(&mut reopened_source, source, "A1"), "");
        assert_eq!(text(&mut reopened_source, source, "A2"), "");
        assert_eq!(text(&mut reopened_target, target, "C3"), "7");
        assert_eq!(text(&mut reopened_target, target, "C4"), "14");
        let cell = command(
            &mut reopened_target,
            target,
            "cell.get",
            json!({"cell":"C4"}),
        );
        assert_eq!(cell["formula"], "=$C$3*2");
        assert!(cell["style"]["font"]["bold"].as_bool().unwrap());
        assert!(!reopened_source.dirty(source));
        assert!(!reopened_target.dirty(target));
    }

    #[test]
    fn immediate_save_refusal_never_creates_output_file() {
        let (mut host, clipboard, source, target) = fixture();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("refused.xlsx");
        queue(&mut host, source, target);
        command(
            &mut host,
            source,
            "cell.set",
            json!({"cell":"Z1", "input":"changed"}),
        );
        let before_source = snapshot(&host, source);
        let before_target = snapshot(&host, target);
        assert!(host.save_as(target, &path).is_err());
        assert!(!path.exists());
        unchanged(&host, source, &before_source);
        unchanged(&host, target, &before_target);
        assert!(clipboard.sheet().is_some_and(|copy| copy.cut));
    }
}
