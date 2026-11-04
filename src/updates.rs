//! The update notice: a reminder when a new release is out, one click to download it, and a
//! restart that installs it. The work itself happens in `weave-updater`.
use crate::dialog::{self, ButtonKind};
use crate::language::tr;
use eframe::egui::{self, RichText};
use std::time::{Duration, Instant};
use weave_updater::{Failure, State, Update, Updater};

/// The first automatic check waits until the window has settled.
const FIRST_CHECK: Duration = Duration::from_secs(5);
/// Later checks while the application stays open.
const CHECK_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);

/// What the person chose in the update dialog that the application carries out.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    /// Quit through the usual save prompts, install, and start the new version.
    Restart,
    OpenPage(String),
}

#[derive(Default)]
pub struct Updates {
    updater: Option<Updater>,
    /// The update dialog is open.
    pub dialog: bool,
    /// The automatic reminder has been shown for this version.
    reminded: Option<String>,
    /// The person chose to restart into the update: install when the quit completes.
    pub restart: bool,
    next_check: Option<Instant>,
}

impl Updates {
    /// Check the project's releases from now on. Off unless the application turns it on
    /// (tests and the startup smoke run construct the app without it).
    pub fn enable(&mut self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        self.updater = Updater::new(
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_REPOSITORY"),
            Updater::default_directory(),
            move || ctx.request_repaint(),
        )
        .filter(Updater::supported);
        if let Some(updater) = &self.updater {
            updater.clean();
            self.next_check = Some(Instant::now() + FIRST_CHECK);
        }
    }

    pub fn available(&self) -> bool {
        self.updater.is_some()
    }

    fn state(&self) -> State {
        self.updater
            .as_ref()
            .map(Updater::state)
            .unwrap_or(State::Idle)
    }

    /// "Check for Updates…": show the dialog with the result, whatever it is.
    pub fn check_now(&mut self) {
        if let Some(updater) = &self.updater {
            updater.check(true);
            self.dialog = true;
        }
    }

    /// Run the automatic checks and remind the person once per new version. `quiet` holds the
    /// reminder back while another dialog or an editor's own modal is open.
    pub fn tick(&mut self, ctx: &egui::Context, automatic: bool, quiet: bool) {
        let Some(updater) = &self.updater else {
            return;
        };
        if let Some(due) = self.next_check {
            let now = Instant::now();
            if !automatic {
                // Checked again once the setting is turned back on.
                self.next_check = Some(now.max(due));
            } else if now >= due {
                updater.check(false);
                self.next_check = Some(now + CHECK_INTERVAL);
            } else {
                ctx.request_repaint_after(due - now);
            }
        }
        if let State::Available(update) = updater.state()
            && self.reminded.as_ref() != Some(&update.version)
            && !quiet
            && !self.dialog
        {
            self.reminded = Some(update.version);
            self.dialog = true;
        }
    }

    /// Text for the title-bar button while an update is waiting, if any.
    pub fn badge(&self) -> Option<String> {
        if self.dialog {
            return None;
        }
        match self.state() {
            State::Available(update)
            | State::Failed {
                update: Some(update),
                ..
            } => Some(tr("新版本 {version}").replace("{version}", &update.version)),
            State::Downloading {
                received, total, ..
            } => Some(
                tr("正在下载更新 {percent}%")
                    .replace("{percent}", &percent(received, total).to_string()),
            ),
            State::Ready(_) => Some(tr("重启以完成更新").to_owned()),
            _ => None,
        }
    }

    /// Install the downloaded update as the application quits. Returns false when a restart
    /// the person asked for could not be prepared, so the application stays open to say so.
    pub fn install_on_exit(&mut self) -> bool {
        let restart = std::mem::take(&mut self.restart);
        let Some(updater) = self.updater.as_ref().filter(|u| u.is_ready()) else {
            return true;
        };
        match updater.apply(restart) {
            Ok(()) => true,
            Err(_) if restart => {
                self.dialog = true;
                false
            }
            Err(_) => true,
        }
    }

    pub fn close(&mut self) {
        self.dialog = false;
        if let Some(updater) = &self.updater {
            updater.dismiss();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<Request> {
        if !self.dialog {
            return None;
        }
        let updater = self.updater.as_ref()?;
        let state = updater.state();
        if state == State::Idle {
            // Nothing to show: a dismissed result, or an automatic check that found nothing.
            self.dialog = false;
            return None;
        }
        let mut request = None;
        let mut close = false;
        egui::Modal::new(egui::Id::unique("weave-update"))
            .frame(dialog::frame(ctx))
            .show(ctx, |ui| {
                dialog::prepare(ui);
                match &state {
                    State::Idle | State::Checking { .. } => {
                        dialog::header(ui, tr("检查更新"));
                        ui.horizontal(|ui| {
                            ui.spinner();
                            dialog::body(ui, tr("正在检查更新…"));
                        });
                        dialog::footer(ui, |ui| {
                            close = dialog::button(ui, tr("关闭"), ButtonKind::Secondary).clicked();
                        });
                    }
                    State::UpToDate => {
                        dialog::header(ui, tr("已是最新版本"));
                        dialog::body(
                            ui,
                            &tr("Weave Office {version} 是目前的最新版本。")
                                .replace("{version}", env!("CARGO_PKG_VERSION")),
                        );
                        dialog::footer(ui, |ui| {
                            close = dialog::button(ui, tr("确定"), ButtonKind::Primary).clicked();
                        });
                    }
                    State::Available(update) => {
                        dialog::header(ui, tr("发现新版本"));
                        describe(ui, update);
                        if update.manual_reason.is_some() {
                            dialog::body(ui, tr("这个版本需要从下载页手动安装。"));
                        }
                        dialog::footer(ui, |ui| {
                            if update.manual_reason.is_none() {
                                if dialog::button(ui, tr("更新"), ButtonKind::Primary).clicked() {
                                    updater.download();
                                }
                            } else if dialog::button(ui, tr("打开下载页"), ButtonKind::Primary)
                                .clicked()
                            {
                                request = Some(Request::OpenPage(update.page.clone()));
                                close = true;
                            }
                            close |= dialog::button(ui, tr("稍后"), ButtonKind::Secondary).clicked();
                        });
                    }
                    State::Downloading {
                        update,
                        received,
                        total,
                    } => {
                        dialog::header(ui, tr("正在下载更新"));
                        dialog::body(
                            ui,
                            &tr("Weave Office {version}").replace("{version}", &update.version),
                        );
                        let fraction = if *total > 0 {
                            *received as f32 / *total as f32
                        } else {
                            0.0
                        };
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .text(format!(
                                    "{:.1} / {:.1} MB",
                                    *received as f64 / 1_048_576.0,
                                    *total as f64 / 1_048_576.0
                                ))
                                .animate(*total == 0),
                        );
                        dialog::body(ui, tr("可以继续工作，下载完成后会提醒你重启。"));
                        dialog::split_footer(
                            ui,
                            |ui| {
                                if dialog::button(ui, tr("取消下载"), ButtonKind::Destructive)
                                    .clicked()
                                {
                                    updater.cancel();
                                }
                            },
                            |ui| {
                                // The download continues; the title bar shows its progress.
                                if dialog::button(ui, tr("后台下载"), ButtonKind::Secondary)
                                    .clicked()
                                {
                                    self.dialog = false;
                                }
                            },
                        );
                    }
                    State::Ready(update) => {
                        dialog::header(ui, tr("更新已就绪"));
                        dialog::body(
                            ui,
                            &tr("重启 Weave Office 即可更新到 {version}。有未保存的文档时，会先询问是否保存。")
                                .replace("{version}", &update.version),
                        );
                        dialog::body(ui, tr("选择“稍后”时，会在下次退出时自动安装。"));
                        dialog::footer(ui, |ui| {
                            if dialog::button(ui, tr("立即重启"), ButtonKind::Primary).clicked() {
                                request = Some(Request::Restart);
                                self.dialog = false;
                            }
                            if dialog::button(ui, tr("稍后"), ButtonKind::Secondary).clicked() {
                                self.dialog = false;
                            }
                        });
                    }
                    State::Failed {
                        update,
                        failure,
                        detail,
                    } => {
                        let (title, message) = match failure {
                            Failure::Check => ("无法检查更新", "无法连接更新服务器，请检查网络后重试。"),
                            Failure::Download => ("下载更新失败", "请检查网络后重试。"),
                            Failure::Verify => (
                                "更新未通过校验",
                                "下载的安装包与发布签名不符，已删除，不会安装。",
                            ),
                            Failure::Install => ("无法安装更新", "可以从下载页手动安装新版本。"),
                        };
                        dialog::header(ui, tr(title));
                        dialog::body(ui, tr(message));
                        ui.label(RichText::new(detail).small().weak());
                        dialog::footer(ui, |ui| {
                            let retry = match failure {
                                Failure::Check => Some(true),
                                Failure::Download | Failure::Verify => Some(false),
                                Failure::Install => None,
                            };
                            match retry {
                                Some(manual_check) => {
                                    if dialog::button(ui, tr("重试"), ButtonKind::Primary).clicked()
                                    {
                                        if manual_check {
                                            updater.check(true);
                                        } else {
                                            updater.download();
                                        }
                                    }
                                }
                                None => {
                                    if let Some(update) = update
                                        && dialog::button(ui, tr("打开下载页"), ButtonKind::Primary)
                                            .clicked()
                                    {
                                        request = Some(Request::OpenPage(update.page.clone()));
                                        close = true;
                                    }
                                }
                            }
                            close |= dialog::button(ui, tr("关闭"), ButtonKind::Secondary).clicked();
                        });
                    }
                }
            });
        if close {
            self.close();
        }
        request
    }
}

/// The title-bar button that brings the update dialog back.
pub fn badge_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let accent = if ui.visuals().dark_mode {
        egui::Color32::from_rgb(89, 149, 225)
    } else {
        egui::Color32::from_rgb(35, 103, 184)
    };
    ui.add(
        egui::Button::new(RichText::new(text).size(12.0).color(egui::Color32::WHITE))
            .fill(accent)
            .stroke(egui::Stroke::new(1.0, accent))
            .corner_radius(11)
            .min_size(egui::vec2(0.0, 22.0)),
    )
    .on_hover_text(tr("Weave Office 更新"))
}

fn describe(ui: &mut egui::Ui, update: &Update) {
    dialog::body(
        ui,
        &tr("Weave Office {version} 已发布，当前版本是 {current}。")
            .replace("{version}", &update.version)
            .replace("{current}", env!("CARGO_PKG_VERSION")),
    );
    if !update.notes.is_empty() {
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(&update.notes).size(12.0)).wrap());
            });
    }
}

fn percent(received: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        (received.saturating_mul(100) / total).min(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_an_updater_nothing_is_shown_or_installed() {
        let mut updates = Updates::default();
        assert!(!updates.available());
        assert_eq!(updates.badge(), None);
        updates.check_now();
        assert!(!updates.dialog);
        updates.restart = true;
        assert!(updates.install_on_exit());
        assert!(!updates.restart);
    }

    #[test]
    fn percent_is_bounded() {
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(50, 200), 25);
        assert_eq!(percent(300, 200), 100);
    }
}
