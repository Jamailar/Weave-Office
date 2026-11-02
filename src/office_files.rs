//! Document-owned native file requests from the three Office editors.
//! The shell chooses paths; each completion stays attached to its originating editor.
use crate::{
    editor::Editor,
    file_dialog::{FileDialogMode, FileDialogOptions},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

const GRID_IMAGE_LIMIT: u64 = 16 * 1024 * 1024;
type Completion = Box<dyn FnOnce(Option<Vec<PathBuf>>) -> Result<(), String> + Send>;
pub type OfficeExportWriter =
    Arc<dyn Fn(&[(PathBuf, Vec<u8>)], bool) -> Result<(), String> + Send + Sync>;

pub struct OfficeFileRequest {
    pub id: u64,
    pub mode: FileDialogMode,
    pub options: FileDialogOptions,
}

struct Pending {
    mode: FileDialogMode,
    operation: Operation,
}

enum Operation {
    Read(Completion),
    Export {
        request: deckcraft_ui_egui::HostExportRequest,
        writer: OfficeExportWriter,
    },
}

impl Operation {
    fn cancel(self) -> Result<(), String> {
        match self {
            Self::Read(complete) => complete(None),
            Self::Export { .. } => Ok(()),
        }
    }
}

#[derive(Default)]
struct State {
    next_id: u64,
    requests: VecDeque<OfficeFileRequest>,
    pending: HashMap<u64, Pending>,
    errors: Vec<String>,
}

#[derive(Clone, Default)]
pub struct OfficeFiles(Arc<Mutex<State>>, Arc<Mutex<()>>);

impl OfficeFiles {
    /// Bytes belong to the request snapshot; completion never saves or mutates the editor.
    pub fn install_exports(&self, editor: &mut Editor, writer: OfficeExportWriter) {
        if let Editor::Slides(app) = editor {
            let broker = self.clone();
            app.services.export_files = Some(Box::new(move |request| {
                broker.export(request, writer.clone())
            }));
        }
    }

    fn export(
        &self,
        request: deckcraft_ui_egui::HostExportRequest,
        writer: OfficeExportWriter,
    ) -> Result<(), String> {
        if request.files.is_empty() || (!request.directory && request.files.len() != 1) {
            return Err("导出文件数量与目标类型不符".into());
        }
        let mut names = HashSet::new();
        for file in &request.files {
            let mut components = Path::new(&file.name).components();
            if file.name.trim().is_empty()
                || file.name.contains(['/', '\\', '\0'])
                || !matches!(components.next(), Some(Component::Normal(_)))
                || components.next().is_some()
            {
                return Err("导出文件名必须是单一普通文件名".into());
            }
            if !names.insert(file.name.to_lowercase()) {
                return Err("导出文件名重复".into());
            }
        }
        let directory = request.directory;
        let name = &request.files[0].name;
        let filters = Path::new(name)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                vec![(
                    crate::language::tr("{extension} 文件")
                        .replace("{extension}", &ext.to_uppercase()),
                    vec![ext.into()],
                )]
            })
            .unwrap_or_default();
        self.enqueue_pending(
            if directory {
                FileDialogMode::Folder
            } else {
                FileDialogMode::Save
            },
            FileDialogOptions {
                title: if directory {
                    "选择导出文件夹"
                } else {
                    "导出演示"
                }
                .into(),
                file_name: if directory {
                    String::new()
                } else {
                    name.clone()
                },
                filters: if directory { Vec::new() } else { filters },
                ..Default::default()
            },
            Operation::Export { request, writer },
        );
        Ok(())
    }

    /// Preserve the editor's existing clipboard and other services.
    pub fn install(&self, editor: &mut Editor) {
        match editor {
            Editor::Word(app) => {
                let broker = self.clone();
                app.services.file_dialog = Some(Box::new(move |request| {
                    use wordcraft_ui_egui::FileDialogRequest;
                    let (sender, receiver) = std::sync::mpsc::channel();
                    let (purpose, multiple) = match request {
                        FileDialogRequest::Open { purpose } => (purpose, false),
                        FileDialogRequest::OpenMany { purpose } => (purpose, true),
                        FileDialogRequest::Save { .. } => {
                            // An error answer activates WordCraft's own file browser. Cancel
                            // instead and let the shell explain the unsupported write request.
                            let _ = sender.send(Ok(Vec::new()));
                            broker.error(
                                "此文字导出操作尚未接入统一工作区；请使用 Weave 菜单保存或导出"
                                    .into(),
                            );
                            return receiver;
                        }
                    };
                    let options = FileDialogOptions {
                        title: if purpose == "picture" {
                            "选择图片"
                        } else {
                            "选择文件"
                        }
                        .into(),
                        filters: wordcraft_ui_egui::open_filters(&purpose)
                            .iter()
                            .map(|(name, extensions)| {
                                (
                                    (*name).into(),
                                    extensions.iter().map(|ext| (*ext).into()).collect(),
                                )
                            })
                            .collect(),
                        ..Default::default()
                    };
                    broker.enqueue(
                        FileDialogMode::Open { multiple },
                        options,
                        Box::new(move |paths| {
                            let paths = paths.unwrap_or_default();
                            let answer = paths
                                .iter()
                                .map(|path| {
                                    path.to_str()
                                        .map(str::to_owned)
                                        .ok_or_else(|| "文件路径不是有效的 Unicode".to_string())
                                })
                                .collect::<Result<Vec<_>, _>>();
                            match answer {
                                Ok(paths) => {
                                    let _ = sender.send(Ok(paths));
                                    Ok(())
                                }
                                Err(error) => {
                                    let _ = sender.send(Ok(Vec::new()));
                                    Err(error)
                                }
                            }
                        }),
                    );
                    receiver
                }));
            }
            Editor::Sheet(app) => {
                let broker = self.clone();
                app.services.pick_picture_async = Some(Box::new(move |inbox| {
                    broker.enqueue(
                        FileDialogMode::Open { multiple: false },
                        picture_options(&["png", "jpg", "jpeg"]),
                        Box::new(move |paths| {
                            let Some(path) = paths.and_then(|paths| paths.into_iter().next())
                            else {
                                return Ok(());
                            };
                            if !has_extension(&path, &["png", "jpg", "jpeg"]) {
                                return Err("表格图片仅支持 PNG 或 JPEG".into());
                            }
                            let bytes = read_limited(&path, GRID_IMAGE_LIMIT)?;
                            let name = file_name(&path)?;
                            inbox
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .push((name, bytes));
                            Ok(())
                        }),
                    );
                }));
            }
            Editor::Slides(app) => {
                let broker = self.clone();
                let inbox = app
                    .services
                    .inbox
                    .get_or_insert_with(Default::default)
                    .clone();
                app.services.open_async = Some(Box::new(move |purpose, source_id| {
                    if purpose != "picture" {
                        inbox
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(deckcraft_ui_egui::InboxEvent::Cancelled { id: source_id });
                        broker.error(format!("此演示文件选择操作尚未接入统一工作区：{purpose}"));
                        return;
                    }
                    let inbox = inbox.clone();
                    broker.enqueue(
                        FileDialogMode::Open { multiple: false },
                        picture_options(&["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"]),
                        Box::new(move |paths| {
                            let result: Result<Option<deckcraft_ui_egui::InboxFile>, String> =
                                paths
                                    .and_then(|paths| paths.into_iter().next())
                                    .map(|path| {
                                        let name = file_name(&path)?;
                                        let path_text =
                                            path.to_str().ok_or("文件路径不是有效的 Unicode")?;
                                        let bytes = deckcraft_engine::io::read_file(path_text)?;
                                        Ok(deckcraft_ui_egui::InboxFile {
                                            id: source_id,
                                            name,
                                            bytes,
                                        })
                                    })
                                    .transpose();
                            let (event, error) = match result {
                                Ok(Some(file)) => (deckcraft_ui_egui::InboxEvent::File(file), None),
                                Ok(None) => (
                                    deckcraft_ui_egui::InboxEvent::Cancelled { id: source_id },
                                    None,
                                ),
                                Err(error) => (
                                    deckcraft_ui_egui::InboxEvent::Cancelled { id: source_id },
                                    Some(error),
                                ),
                            };
                            inbox
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .push(event);
                            error.map_or(Ok(()), Err)
                        }),
                    );
                }));
            }
            Editor::Pdf(_) | Editor::TextFile(_) => {}
        }
    }

    pub fn take_requests(&self) -> Vec<OfficeFileRequest> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .requests
            .drain(..)
            .collect()
    }

    /// Read callbacks are one-shot. Failed exports retain their original snapshot for
    /// an explicit native-panel retry. Cancellation and success consume either kind.
    /// All callbacks and file I/O run outside the state lock.
    pub fn complete(&self, id: u64, paths: Option<Vec<PathBuf>>) -> Result<(), String> {
        // Closing waits for an already-started completion, and prevents a queued
        // completion from escaping cancellation after its origin has been removed.
        let _completion = self.1.lock().unwrap_or_else(PoisonError::into_inner);
        let pending = {
            let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            state.requests.retain(|request| request.id != id);
            state.pending.remove(&id)
        };
        let result = match pending {
            None => Err("文件选择请求已结束或不存在".into()),
            Some(pending) => {
                let validation = validate_paths(pending.mode, paths.as_deref());
                match pending.operation {
                    Operation::Read(complete) => match validation {
                        Ok(()) => complete(paths),
                        Err(error) => {
                            let _ = complete(None);
                            Err(error)
                        }
                    },
                    operation @ Operation::Export { .. } => {
                        let result = validation.and_then(|()| match paths {
                            None => Ok(()),
                            Some(paths) => attempt_export(&operation, &paths[0]),
                        });
                        if result.is_err() {
                            // The completion lock stays held across removal and reinsertion;
                            // cancel_all cannot race with a failure and revive this request.
                            self.0
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .pending
                                .insert(
                                    id,
                                    Pending {
                                        mode: pending.mode,
                                        operation,
                                    },
                                );
                        }
                        result
                    }
                }
            }
        };
        if let Err(error) = &result {
            self.error(error.clone());
        }
        result
    }

    /// Only exports retain a snapshot after failure and can reopen the same panel.
    pub fn can_retry(&self, id: u64) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            .get(&id)
            .is_some_and(|pending| matches!(pending.operation, Operation::Export { .. }))
    }

    pub fn cancel_all(&self) {
        let _completion = self.1.lock().unwrap_or_else(PoisonError::into_inner);
        let pending = {
            let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            state.requests.clear();
            std::mem::take(&mut state.pending)
        };
        for (_, pending) in pending {
            if let Err(error) = pending.operation.cancel() {
                self.error(error);
            }
        }
    }

    pub fn take_errors(&self) -> Vec<String> {
        std::mem::take(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner).errors)
    }

    fn error(&self, error: String) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .errors
            .push(error);
    }

    fn enqueue(&self, mode: FileDialogMode, options: FileDialogOptions, complete: Completion) {
        self.enqueue_pending(mode, options, Operation::Read(complete));
    }

    fn enqueue_pending(
        &self,
        mode: FileDialogMode,
        options: FileDialogOptions,
        operation: Operation,
    ) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(id) = state.next_id.checked_add(1) else {
            state.errors.push("文件选择请求标识已耗尽".into());
            drop(state);
            let _ = operation.cancel();
            return;
        };
        state.next_id = id;
        state.pending.insert(id, Pending { mode, operation });
        state
            .requests
            .push_back(OfficeFileRequest { id, mode, options });
    }
}

fn attempt_export(operation: &Operation, path: &Path) -> Result<(), String> {
    let Operation::Export { request, writer } = operation else {
        unreachable!()
    };
    if !request.directory {
        let expected = Path::new(&request.files[0].name)
            .extension()
            .and_then(|ext| ext.to_str());
        let chosen = path.extension().and_then(|ext| ext.to_str());
        if !match (expected, chosen) {
            (Some(expected), Some(chosen)) => expected.eq_ignore_ascii_case(chosen),
            (None, None) => true,
            _ => false,
        } {
            return Err(format!(
                "导出文件扩展名必须为 .{}，请选择正确的文件名",
                expected.unwrap_or_default()
            ));
        }
    }
    let outputs = request
        .files
        .iter()
        .map(|file| {
            (
                if request.directory {
                    path.join(&file.name)
                } else {
                    path.to_owned()
                },
                file.bytes.clone(),
            )
        })
        .collect::<Vec<_>>();
    writer(&outputs, !request.directory)
}

fn picture_options(extensions: &[&str]) -> FileDialogOptions {
    FileDialogOptions {
        title: "选择图片".into(),
        filters: vec![(
            "图片".into(),
            extensions.iter().map(|ext| (*ext).into()).collect(),
        )],
        ..Default::default()
    }
}

fn validate_paths(mode: FileDialogMode, paths: Option<&[PathBuf]>) -> Result<(), String> {
    let Some(paths) = paths else { return Ok(()) };
    let multiple = matches!(mode, FileDialogMode::Open { multiple: true });
    if paths.is_empty() || (!multiple && paths.len() != 1) {
        return Err("文件选择数量与请求不符".into());
    }
    for path in paths {
        if mode == FileDialogMode::Save {
            file_name(path)?;
            if path.is_dir() {
                return Err("请选择文件作为导出目标".into());
            }
            continue;
        }
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
        if mode == FileDialogMode::Folder {
            if !metadata.is_dir() {
                return Err(format!("请选择文件夹：{}", path.display()));
            }
            continue;
        }
        if !metadata.is_file() {
            return Err(format!("请选择文件：{}", path.display()));
        }
        File::open(path).map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
        file_name(path)?;
        if path.to_str().is_none() {
            return Err("文件路径不是有效的 Unicode".into());
        }
    }
    Ok(())
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "文件名不是有效的 Unicode".into())
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            extensions
                .iter()
                .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        })
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    if bytes.len() as u64 > limit {
        return Err("请选择不超过 16 MB 的图片".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(broker: &OfficeFiles) -> (u64, std::sync::mpsc::Receiver<Option<Vec<PathBuf>>>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let reentrant = broker.clone();
        broker.enqueue(
            FileDialogMode::Open { multiple: false },
            Default::default(),
            Box::new(move |paths| {
                // Completion must release the lock before calling into a consumer.
                let _ = reentrant.take_errors();
                sender.send(paths).unwrap();
                Ok(())
            }),
        );
        (broker.take_requests().pop().unwrap().id, receiver)
    }

    #[test]
    fn cancellation_completes_once_and_late_results_cannot_reach_a_new_request() {
        let broker = OfficeFiles::default();
        let (first, answer) = pending(&broker);
        broker.cancel_all();
        assert_eq!(answer.recv().unwrap(), None);
        let (second, second_answer) = pending(&broker);
        assert_ne!(first, second);
        assert!(broker.complete(first, None).is_err());
        assert!(second_answer.try_recv().is_err());
        broker.complete(second, None).unwrap();
        assert_eq!(second_answer.recv().unwrap(), None);
        assert!(broker.complete(second, None).is_err());
    }

    #[test]
    fn invalid_selection_cancels_without_creating_or_changing_files() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        let missing = directory.path().join("missing.png");
        std::fs::write(&source, b"original").unwrap();
        let broker = OfficeFiles::default();
        for paths in [
            vec![source.clone(), missing.clone()],
            vec![missing.clone()],
            vec![directory.path().to_path_buf()],
        ] {
            let (id, answer) = pending(&broker);
            assert!(broker.complete(id, Some(paths)).is_err());
            assert_eq!(answer.recv().unwrap(), None);
            assert!(!broker.take_errors().is_empty());
        }
        assert_eq!(std::fs::read(source).unwrap(), b"original");
        assert!(!missing.exists());
    }

    #[test]
    fn grid_size_limit_reads_without_truncating_the_source() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.png");
        let file = File::create(&path).unwrap();
        file.set_len(GRID_IMAGE_LIMIT + 1).unwrap();
        assert!(read_limited(&path, GRID_IMAGE_LIMIT).is_err());
        assert_eq!(std::fs::metadata(path).unwrap().len(), GRID_IMAGE_LIMIT + 1);
    }
}
