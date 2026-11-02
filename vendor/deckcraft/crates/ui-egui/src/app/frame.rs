use super::{SlideApp, json, media, monotonic_secs, theme};

impl SlideApp {
    /// Install fonts before the host lays out its header whenever possible. egui applies them
    /// immediately when this pass has not used its atlas; otherwise they wait for the next pass.
    pub(super) fn prepare_interface_fonts(&mut self, ctx: &egui::Context) -> bool {
        if self.styled {
            return true;
        }
        let definitions = theme::font_definitions(&self.ui_fonts.faces);
        // A host may prepend its own fallback face. That does not invalidate our installed
        // families, and replacing it on every replay would keep queuing another font change.
        let installed = |ctx: &egui::Context| {
            ctx.fonts(|fonts| {
                let current = fonts.definitions();
                definitions.font_data.iter().all(|(name, data)| current.font_data.get(name) == Some(data))
                    && definitions.families.iter().all(|(family, names)| {
                        current.families.get(family).is_some_and(|current_names| {
                            let mut remaining = current_names.iter();
                            names.iter().all(|name| remaining.any(|current_name| current_name == name))
                        })
                    })
            })
        };
        if !installed(ctx) {
            ctx.set_fonts(definitions.clone());
        }
        if installed(ctx) {
            self.styled = true;
            self.restyle = true;
            true
        } else {
            false
        }
    }

    /// Style and zoom belong to the UI pass. Only discard a font transition when egui will
    /// actually replay it; a final pass can still draw with the already installed families.
    pub(super) fn ui_setup(&mut self, ctx: &egui::Context) -> bool {
        let scale = self.ui.ui_scale.clamp(0.5, 3.0);
        if !self.hosted && (ctx.zoom_factor() - scale).abs() > 1e-3 {
            ctx.set_zoom_factor(scale);
        }
        if !self.prepare_interface_fonts(ctx) {
            ctx.request_discard("install interface fonts before layout");
            if ctx.will_discard() {
                return false;
            }
            ctx.request_repaint();
            // A fresh context after an already drawn host header may have no named family yet.
            // Do not claim it is ready: theme::bold would panic until the queued set is applied.
            if !ctx.fonts(|fonts| fonts.definitions().families.contains_key(&egui::FontFamily::Name("bold".into()))) {
                self.fonts_ready = false;
                return false;
            }
        }
        if self.restyle {
            if !self.hosted {
                theme::apply(ctx, &theme::Tokens::for_brightness(self.ui.brightness));
            }
            self.restyle = false;
        }
        self.fonts_ready = true;
        true
    }

    /// AutoRecover on a monotonic deadline, even when nothing is painted.
    pub(super) fn autosave_if_due(&mut self, ctx: &egui::Context) {
        if self.session.recovery_dir.is_none() {
            return;
        }
        let every = (self.session.prefs.recovery_minutes * 60.0).max(10.0);
        let now = monotonic_secs();
        if now - self.last_recovery > every {
            self.last_recovery = now;
            if self.session.documents().iter().any(|d| d.is_dirty())
                && let Err(e) = self.session.execute("file.recovery.save", &json!({}))
            {
                self.set_status(format!("Couldn't save AutoRecover information: {e}"));
            }
        }
        if self.session.documents().iter().any(|d| d.is_dirty()) {
            let wait = (every - (monotonic_secs() - self.last_recovery)).clamp(0.5, every);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
    }

    /// Issue due screenshots and finish pass-local UI bookkeeping. Call once at the end of a real
    /// pass after layout; the outer [`Self::ui`] wrapper observes IME output after this returns.
    pub(super) fn ui_end(&mut self, ctx: &egui::Context) {
        self.ime_frame_viewport = None;
        self.capture.observe_ui(ctx);
        if !ctx.will_discard() {
            let doc = self.session.active().map(|d| d.uid);
            self.capture.issue(ctx, doc, web_time::Instant::now());
        }
        if self.fonts_ready && !ctx.will_discard() {
            self.synthetic_pending = false;
            self.synthetic_injected.clear();
            self.synthetic_injected_at = None;
            self.synthetic_injected_time = None;
            self.synthetic_focus = None;
            if self.synthetic.is_empty() {
                self.synthetic_target = None;
            }
            self.promote_next_synthetic();
        }
        if self.fonts_ready && !ctx.will_discard() {
            let now = ctx.input(|i| i.time);
            let dt = now - self.last_time;
            if dt > 0.0 {
                self.perf.fps = self.perf.fps * 0.9 + (1.0 / dt).min(240.0) * 0.1;
            }
            self.last_time = now;
        }
    }

    /// Media clocks. Voices belong to the open presentation: closing the last one, or switching
    /// decks, stops them. The editor's media also stops when its slide is left. This runs from
    /// `logic` on every call so a minimized window keeps playing; texture upload stays in `ui`.
    pub(super) fn tick_media_clock(&mut self, ctx: &egui::Context) {
        let uid = self.session.active().map(|d| d.uid);
        self.media.set_document(uid);
        if !self.media.any_active() {
            self.session.media_status.clear();
            return;
        }
        if self.show.is_none()
            && let Some(d) = self.session.active()
        {
            let here: Vec<deckcraft_model::ShapeId> = if d.selection.target == deckcraft_engine::Target::Slides {
                media::slide_media(&d.doc, d.selection.slide).into_iter().map(|m| m.0).collect()
            } else {
                vec![]
            };
            let gone: Vec<_> =
                self.media.ids().into_iter().filter(|id| !here.contains(id) && self.media.owner(*id) == Some(media::Owner::Editor)).collect();
            for id in gone {
                self.media.stop(id);
            }
        }
        let playing = self.media.tick_clock(monotonic_secs(), &mut self.session);
        if playing {
            ctx.request_repaint_after(std::time::Duration::from_millis(15));
        }
    }
}
