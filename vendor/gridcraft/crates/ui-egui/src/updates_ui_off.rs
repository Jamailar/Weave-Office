//! Built without the `updater` feature: an embedding host owns release checks, so this editor never
//! reaches the network or shows an update prompt of its own.

use crate::SheetApp;

#[derive(Default)]
pub(crate) struct Runtime;

pub(crate) fn start(_app: &mut SheetApp, _manual: bool) {}

pub(crate) fn tick(_app: &mut SheetApp, _ctx: &egui::Context) {}

pub(crate) fn show(_app: &mut SheetApp, _ctx: &egui::Context) {}
