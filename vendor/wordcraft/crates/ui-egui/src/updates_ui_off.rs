//! Built without the `updater` feature: an embedding host owns release checks, so this editor never
//! reaches the network or shows an update prompt of its own.

use crate::WordApp;

#[derive(Default)]
pub(crate) struct Runtime;

impl WordApp {
    /// Kept for API parity with the `updater` build; there is nothing to check.
    pub fn set_startup_update_check(&mut self, _on: bool) {}
}

pub(crate) fn check(_app: &mut WordApp) {}

pub(crate) fn tick(_app: &mut WordApp, _ctx: &egui::Context) {}

pub(crate) fn show(_app: &mut WordApp, _ctx: &egui::Context) {}
