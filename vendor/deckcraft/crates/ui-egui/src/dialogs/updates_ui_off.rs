//! Built without the `updater` feature: an embedding host owns release checks, so this editor never
//! reaches the network or shows an update prompt of its own.

use crate::SlideApp;

#[derive(Default)]
pub(crate) struct Runtime;

pub(crate) fn enable(_app: &mut SlideApp) {}

pub(crate) fn request_check(_app: &mut SlideApp) {}

pub(crate) fn tick(_app: &mut SlideApp, _ctx: &egui::Context) {}

pub(crate) fn show(_app: &mut SlideApp, _ctx: &egui::Context) {}
