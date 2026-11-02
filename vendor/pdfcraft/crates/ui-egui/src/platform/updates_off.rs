//! Built without the `updater` feature: an embedding host owns release checks, so this editor never
//! reaches the network or shows an update prompt of its own.

use crate::PdfCraftApp;

/// Where every PdfCraft release is listed.
pub const RELEASES_PAGE: &str = "https://github.com/storytold/pdfcraft/releases";

#[derive(Default)]
pub(crate) struct Updates {
    /// Never set: there is no Updates dialog in this build.
    pub(crate) open: bool,
}

impl PdfCraftApp {
    /// Kept for API parity with the `updater` build; there is nothing to check.
    pub fn set_startup_update_check(&mut self, _on: bool) {}

    /// Help ▸ Check for updates has nothing to do: the host owns releases.
    pub fn check_for_updates(&mut self) {}

    pub(crate) fn poll_updates(&mut self) {}
}

pub(crate) fn dialog(_app: &mut PdfCraftApp, _ctx: &egui::Context) {}
