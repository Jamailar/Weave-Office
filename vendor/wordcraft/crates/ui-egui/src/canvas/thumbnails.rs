//! Render newly exposed overview pages without holding up wheel and pointer input.
//! There is one outstanding page per canvas. The next page is chosen from the
//! current viewport after completion, so a scrollbar jump cannot build a backlog.

use std::sync::{Arc, Weak, mpsc};

use wordcraft_doc::Document;
use wordcraft_layout::{DocLayout, Page};
use wordcraft_render::RenderOptions;

pub(super) struct Thumbnail {
    pub page: usize,
    pub key: u64,
    pub image: egui::ColorImage,
    pub layout: Weak<DocLayout>,
    pub rev: u64,
}

struct Job {
    layout: Weak<DocLayout>,
    rev: u64,
    page: Arc<Page>,
    index: usize,
    key: u64,
    doc: Arc<Document>,
    scale: f32,
    opts: RenderOptions,
    ctx: egui::Context,
}

pub(super) struct Worker {
    jobs: mpsc::Sender<Job>,
    results: mpsc::Receiver<Thumbnail>,
    pub busy: bool,
    snapshot: Option<(Weak<DocLayout>, u64, Arc<Document>)>,
}

impl Worker {
    pub fn start() -> Option<Self> {
        let (jobs, input) = mpsc::channel::<Job>();
        let (output, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("wordcraft-thumbnail".into())
            .spawn(move || {
                for job in input {
                    let img = wordcraft_render::render_page(&job.doc, &job.page, job.scale, &job.opts);
                    let result = Thumbnail { layout: job.layout, rev: job.rev, page: job.index, key: job.key, image: super::page_image(&img) };
                    if output.send(result).is_err() {
                        break;
                    }
                    job.ctx.request_repaint();
                }
            })
            .ok()?;
        Some(Self { jobs, results, busy: false, snapshot: None })
    }

    pub fn release_snapshot(&mut self) {
        self.snapshot = None;
    }

    pub fn poll(&mut self) -> Result<Option<Thumbnail>, mpsc::TryRecvError> {
        match self.results.try_recv() {
            Ok(result) => {
                self.busy = false;
                Ok(Some(result))
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(disconnected) => Err(disconnected),
        }
    }

    pub fn submit(&mut self, layout: &Arc<DocLayout>, doc: &Document, rev: u64, page: (usize, u64, f32), opts: RenderOptions, ctx: &egui::Context) {
        let (index, key, scale) = page;
        let Some(page) = layout.pages.get(index) else { return };
        let weak = Arc::downgrade(layout);
        if !self.snapshot.as_ref().is_some_and(|(l, r, _)| *r == rev && l.ptr_eq(&weak)) {
            self.snapshot = Some((weak.clone(), rev, Arc::new(doc.clone())));
        }
        if let Some((_, _, doc)) = &self.snapshot {
            self.busy =
                self.jobs.send(Job { layout: weak, rev, page: page.clone(), index, key, doc: doc.clone(), scale, opts, ctx: ctx.clone() }).is_ok();
        }
    }
}
