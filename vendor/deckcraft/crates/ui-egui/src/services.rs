pub type ReadFn = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;

pub type WriteFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;

pub type PickFn = Box<dyn FnMut(&str) -> Option<String>>;

/// `(purpose, request id)`. The host shows a picker and later pushes an [`InboxEvent`]
/// carrying that same id. Completing a cancelled or unknown id does nothing: the file is
/// not applied to whichever document happens to be active.
pub type OpenAsyncFn = Box<dyn FnMut(&str, u64)>;

pub type DownloadFn = Box<dyn FnMut(&str, &[u8])>;

/// In-memory export handed to the host for a native save or folder picker.
#[derive(Clone, Debug)]
pub struct HostExportFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct HostExportRequest {
    pub files: Vec<HostExportFile>,
    /// Choose a directory and write every file into it (including a one-slide PNG export).
    pub directory: bool,
}

/// A file the host finished reading for the request `id` from [`SlideApp::begin_external_file`].
#[derive(Clone, Debug)]
pub struct InboxFile {
    /// Cancellation identity. A file whose id was cancelled, or never started, is ignored.
    pub id: u64,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// What the host delivers through [`Inbox`] after an asynchronous read.
#[derive(Clone, Debug)]
pub enum InboxEvent {
    File(InboxFile),
    /// The picker was dismissed or the file could not be read. `id` will not be applied.
    Cancelled {
        id: u64,
    },
}

/// Files delivered asynchronously by the host (web file picker, dropped files).
pub type Inbox = std::sync::Arc<std::sync::Mutex<Vec<InboxEvent>>>;

/// Platform services injected by the host (desktop or web).
#[derive(Default)]
pub struct Services {
    /// Host-owned File and quick access controls drawn before the ribbon tabs.
    pub header_prefix: Option<Box<dyn FnMut(&mut egui::Ui, bool, bool) -> Vec<(String, serde_json::Value)>>>,
    /// Open-file dialog for a purpose (`open`, `picture`, `audio`, `video`) → path.
    pub pick_open: Option<PickFn>,
    /// Save dialog with a suggested name → path.
    pub pick_save: Option<PickFn>,
    pub read: Option<ReadFn>,
    pub write: Option<WriteFn>,
    /// Asynchronous open dialog (web). The callback receives the purpose and the request id
    /// from [`SlideApp::begin_external_file`]; the chosen file arrives through `inbox`.
    pub open_async: Option<OpenAsyncFn>,
    /// Hand bytes to the user as a named file (browser download).
    pub download: Option<DownloadFn>,
    /// Queue an export for the host's native picker and protected writer.
    pub export_files: Option<Box<dyn FnMut(HostExportRequest) -> Result<(), String>>>,
    pub inbox: Option<Inbox>,
    /// Image on the system clipboard (PNG bytes), when the host can read one.
    pub clipboard_image: Option<Box<dyn FnMut() -> Option<Vec<u8>>>>,
    /// HTML on the system clipboard (Pages, Keynote, and other apps that write `public.html`).
    pub clipboard_html: Option<Box<dyn FnMut() -> Option<String>>>,
    /// Put HTML and its plain-text alternative on the system clipboard. `true` when it landed.
    pub clipboard_set_html: Option<Box<dyn FnMut(&str, &str) -> bool>>,
    /// Plain text for menu/ribbon Paste, which has no egui paste event.
    pub clipboard_text: Option<Box<dyn FnMut() -> Option<String>>>,
    /// Workspace clipboard payload, already checked against the system clipboard's identity.
    /// `None` means external content: the session must forget its older internal copy.
    pub clipboard_content: Option<Box<dyn FnMut() -> Option<deckcraft_engine::Clipboard>>>,
    /// Publish a complete copy (including slides, media and text) with HTML and plain text.
    /// Success owns the system write; egui must not overwrite it at the end of the frame.
    pub clipboard_set_content: Option<Box<dyn FnMut(&deckcraft_engine::Clipboard, &str, &str) -> bool>>,
    /// Keep repeated-paste offsets in the shared payload without making another system copy.
    pub clipboard_update_content: Option<Box<dyn FnMut(&deckcraft_engine::Clipboard)>>,
    /// Audio output for media playback (cpal on desktop); without one media plays silently.
    pub audio_out: Option<Box<dyn deckcraft_media::AudioOut>>,
}
