use eframe::egui;

use crate::external_url::ExternalUrl;
use crate::i18n::Locale;
use crate::i18n::{TextKey, text};
use crate::icons;
use crate::state::update::{UpdateFailureKind, UpdatePhase, UpdateViewState};
use crate::theme;

use super::shell::ShellViewModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    Check,
    RequestInstall,
    CancelInstall,
    ConfirmInstall,
}

pub fn render(
    ui: &mut egui::Ui,
    model: &ShellViewModel,
    update: &UpdateViewState,
    actions: &mut Vec<UpdateAction>,
) {
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(text(model.locale, TextKey::AppName))
            .strong()
            .size(26.0),
    );
    ui.label(
        egui::RichText::new(text(model.locale, TextKey::AboutSubtitle))
            .weak()
            .size(13.0),
    );
    ui.add_space(24.0);
    ui.separator();
    about_row(
        ui,
        text(model.locale, TextKey::Version),
        env!("CARGO_PKG_VERSION"),
    );
    about_row(ui, text(model.locale, TextKey::Renderer), &model.renderer);
    about_row(ui, "OS", std::env::consts::OS);
    about_row(ui, text(model.locale, TextKey::License), "AGPL-3.0-only");
    render_update(ui, model.locale, update, actions);
    ui.add_space(14.0);
    ui.horizontal_wrapped(|ui| {
        external_button(
            ui,
            icons::folder_git_2(),
            about_text(model.locale, AboutText::Repository),
            "https://github.com/BigPizzaV3/CodexPlusPlus",
        );
        external_button(
            ui,
            icons::triangle_alert(),
            about_text(model.locale, AboutText::Issues),
            "https://github.com/BigPizzaV3/CodexPlusPlus/issues",
        );
        external_button(
            ui,
            icons::message_circle(),
            "Discord",
            "https://discord.gg/y96kX7A76v",
        );
        external_button(
            ui,
            icons::message_circle(),
            "Telegram",
            "https://t.me/CodexPlusPlus",
        );
    });

    if update.confirmation_version().is_some() {
        render_update_confirmation(ui.ctx(), model.locale, update, actions);
    }
}

fn render_update(
    ui: &mut egui::Ui,
    locale: Locale,
    state: &UpdateViewState,
    actions: &mut Vec<UpdateAction>,
) {
    ui.add_space(16.0);
    ui.heading(about_text(locale, AboutText::Updates));
    ui.add_space(4.0);

    match state.phase {
        UpdatePhase::Idle => {
            ui.label(about_text(locale, AboutText::NotChecked));
            update_button(
                ui,
                icons::refresh_cw(),
                about_text(locale, AboutText::Check),
                UpdateAction::Check,
                actions,
            );
        }
        UpdatePhase::Checking => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(15.0));
                ui.label(about_text(locale, AboutText::Checking));
            });
            render_last_result(ui, locale, state);
        }
        UpdatePhase::Current => {
            ui.colored_label(theme::SUCCESS_COLOR, about_text(locale, AboutText::Current));
            render_last_result(ui, locale, state);
            update_button(
                ui,
                icons::refresh_cw(),
                about_text(locale, AboutText::CheckAgain),
                UpdateAction::Check,
                actions,
            );
        }
        UpdatePhase::Available => {
            render_last_result(ui, locale, state);
            update_button(
                ui,
                icons::play(),
                about_text(locale, AboutText::DownloadInstall),
                UpdateAction::RequestInstall,
                actions,
            );
        }
        UpdatePhase::Downloading => render_download_progress(ui, locale, state),
        UpdatePhase::Launching => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(15.0));
                ui.label(about_text(locale, AboutText::Launching));
            });
        }
        UpdatePhase::Error => {
            render_last_result(ui, locale, state);
            ui.colored_label(theme::ERROR_COLOR, update_failure_text(locale, state.error));
            update_button(
                ui,
                icons::refresh_cw(),
                about_text(locale, AboutText::Retry),
                UpdateAction::Check,
                actions,
            );
        }
    }
}

fn render_last_result(ui: &mut egui::Ui, locale: Locale, state: &UpdateViewState) {
    let Some(result) = &state.result else {
        return;
    };
    match &result.availability {
        codex_plus_manager_service::UpdateAvailability::Current => {
            ui.label(format!(
                "{}: {}",
                about_text(locale, AboutText::InstalledVersion),
                result.installed_version
            ));
        }
        codex_plus_manager_service::UpdateAvailability::Available(candidate) => {
            ui.label(format!(
                "{} {} {}",
                about_text(locale, AboutText::VersionPrefix),
                candidate.version,
                about_text(locale, AboutText::AvailableSuffix)
            ));
            ui.label(egui::RichText::new(&candidate.asset_name).monospace());
            if !result.summary.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("about_update_summary")
                    .max_height(140.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(&result.summary).selectable(true).wrap());
                    });
            }
        }
        codex_plus_manager_service::UpdateAvailability::Unavailable => {}
    }
}

fn render_download_progress(ui: &mut egui::Ui, locale: Locale, state: &UpdateViewState) {
    ui.label(about_text(locale, AboutText::Downloading));
    match state.progress {
        Some(progress) => {
            let label = match progress.total_bytes {
                Some(total) => format!("{} / {} bytes", progress.downloaded_bytes, total),
                None => format!("{} bytes", progress.downloaded_bytes),
            };
            let fraction = progress
                .total_bytes
                .filter(|total| *total > 0)
                .map_or(0.0, |total| progress.downloaded_bytes as f32 / total as f32);
            ui.add(
                egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                    .text(label)
                    .animate(progress.total_bytes.is_none()),
            );
        }
        None => {
            ui.add(egui::ProgressBar::new(0.0).animate(true));
        }
    }
}

fn render_update_confirmation(
    ctx: &egui::Context,
    locale: Locale,
    state: &UpdateViewState,
    actions: &mut Vec<UpdateAction>,
) {
    let Some(version) = state.confirmation_version() else {
        return;
    };
    egui::Window::new(about_text(locale, AboutText::ConfirmTitle))
        .id(egui::Id::new("about_update_confirmation"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            ui.label(match locale {
                Locale::ZhCn => format!("安装版本 {version}？"),
                Locale::En => format!("Install version {version}?"),
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .button(about_text(locale, AboutText::ConfirmInstall))
                    .clicked()
                {
                    actions.push(UpdateAction::ConfirmInstall);
                }
                if ui.button(text(locale, TextKey::Cancel)).clicked() {
                    actions.push(UpdateAction::CancelInstall);
                }
            });
        });
}

fn update_button(
    ui: &mut egui::Ui,
    icon: egui::ImageSource<'static>,
    label: &str,
    action: UpdateAction,
    actions: &mut Vec<UpdateAction>,
) {
    ui.add_space(8.0);
    if ui
        .add_sized(
            [184.0, 34.0],
            egui::Button::image_and_text(
                egui::Image::new(icon).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                label,
            ),
        )
        .clicked()
    {
        actions.push(action);
    }
}

fn about_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.set_min_height(42.0);
        ui.label(egui::RichText::new(label).weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).strong());
        });
    });
    ui.separator();
}

fn external_button(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, label: &str, value: &str) {
    let url = ExternalUrl::parse(value).expect("built-in external URL is valid");
    if ui
        .add_sized(
            [156.0, 34.0],
            egui::Button::image_and_text(
                egui::Image::new(icon).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                label,
            ),
        )
        .clicked()
    {
        url.emit(ui.ctx());
    }
}

#[derive(Clone, Copy)]
enum AboutText {
    Repository,
    Issues,
    Updates,
    NotChecked,
    Check,
    Checking,
    Current,
    CheckAgain,
    InstalledVersion,
    VersionPrefix,
    AvailableSuffix,
    DownloadInstall,
    Downloading,
    Launching,
    Retry,
    ConfirmTitle,
    ConfirmInstall,
}

fn about_text(locale: Locale, key: AboutText) -> &'static str {
    match (locale, key) {
        (Locale::ZhCn, AboutText::Repository) => "项目仓库",
        (Locale::En, AboutText::Repository) => "Project repository",
        (Locale::ZhCn, AboutText::Issues) => "提交问题",
        (Locale::En, AboutText::Issues) => "Report an issue",
        (Locale::ZhCn, AboutText::Updates) => "应用更新",
        (Locale::En, AboutText::Updates) => "Updates",
        (Locale::ZhCn, AboutText::NotChecked) => "尚未检查更新",
        (Locale::En, AboutText::NotChecked) => "Updates have not been checked",
        (Locale::ZhCn, AboutText::Check) => "检查更新",
        (Locale::En, AboutText::Check) => "Check for updates",
        (Locale::ZhCn, AboutText::Checking) => "正在检查更新...",
        (Locale::En, AboutText::Checking) => "Checking for updates...",
        (Locale::ZhCn, AboutText::Current) => "Codex++ 已是最新版本",
        (Locale::En, AboutText::Current) => "Codex++ is up to date",
        (Locale::ZhCn, AboutText::CheckAgain) => "重新检查",
        (Locale::En, AboutText::CheckAgain) => "Check again",
        (Locale::ZhCn, AboutText::InstalledVersion) => "已安装版本",
        (Locale::En, AboutText::InstalledVersion) => "Installed version",
        (Locale::ZhCn, AboutText::VersionPrefix) => "版本",
        (Locale::En, AboutText::VersionPrefix) => "Version",
        (Locale::ZhCn, AboutText::AvailableSuffix) => "可以安装",
        (Locale::En, AboutText::AvailableSuffix) => "is available",
        (Locale::ZhCn, AboutText::DownloadInstall) => "下载并安装",
        (Locale::En, AboutText::DownloadInstall) => "Download and install",
        (Locale::ZhCn, AboutText::Downloading) => "正在下载更新",
        (Locale::En, AboutText::Downloading) => "Downloading update",
        (Locale::ZhCn, AboutText::Launching) => "安装器已打开，正在退出 Codex++...",
        (Locale::En, AboutText::Launching) => "Installer opened. Exiting Codex++...",
        (Locale::ZhCn, AboutText::Retry) => "重试更新检查",
        (Locale::En, AboutText::Retry) => "Retry update check",
        (Locale::ZhCn, AboutText::ConfirmTitle) => "确认更新",
        (Locale::En, AboutText::ConfirmTitle) => "Confirm update",
        (Locale::ZhCn, AboutText::ConfirmInstall) => "安装",
        (Locale::En, AboutText::ConfirmInstall) => "Install",
    }
}

fn update_failure_text(locale: Locale, failure: Option<UpdateFailureKind>) -> &'static str {
    match (locale, failure) {
        (Locale::ZhCn, Some(UpdateFailureKind::NoCompatibleAsset)) => "没有适用于此平台的安装包",
        (Locale::En, Some(UpdateFailureKind::NoCompatibleAsset)) => {
            "No compatible installer is available"
        }
        (Locale::ZhCn, Some(UpdateFailureKind::DownloadTooLarge)) => "安装包超过允许的大小",
        (Locale::En, Some(UpdateFailureKind::DownloadTooLarge)) => {
            "The installer exceeds the allowed size"
        }
        (Locale::ZhCn, Some(UpdateFailureKind::InsecureAsset)) => "更新地址未通过安全验证",
        (Locale::En, Some(UpdateFailureKind::InsecureAsset)) => {
            "The update address failed security validation"
        }
        (Locale::ZhCn, Some(UpdateFailureKind::WorkerStopped)) => "更新后台任务已停止",
        (Locale::En, Some(UpdateFailureKind::WorkerStopped)) => "The update worker stopped",
        (Locale::ZhCn, _) => "更新操作失败，请重试",
        (Locale::En, _) => "The update operation failed. Try again.",
    }
}
