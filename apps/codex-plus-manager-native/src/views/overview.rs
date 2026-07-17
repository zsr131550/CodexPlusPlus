use std::path::Path;

use codex_plus_manager_service::{OverviewSnapshot, ResourcePresence};
use eframe::egui;

use crate::i18n::{TextKey, text};
use crate::state::{OverviewFailureKind, OverviewPhase};
use crate::{icons, theme};

use super::shell::{ShellAction, ShellViewModel};

pub fn render(ui: &mut egui::Ui, model: &ShellViewModel, actions: &mut Vec<ShellAction>) {
    if model.overview_phase == OverviewPhase::Error {
        render_error(ui, model, actions);
        ui.add_space(8.0);
    } else if model.overview_phase == OverviewPhase::Refreshing {
        ui.colored_label(
            theme::WARNING_COLOR,
            text(model.locale, TextKey::Refreshing),
        );
        ui.add_space(6.0);
    }

    match &model.overview_snapshot {
        Some(snapshot) => render_snapshot(ui, model, snapshot),
        None if model.overview_phase == OverviewPhase::Error => {}
        None => render_loading(ui, model),
    }
}

fn render_error(ui: &mut egui::Ui, model: &ShellViewModel, actions: &mut Vec<ShellAction>) {
    let key = match model.overview_error {
        Some(OverviewFailureKind::WorkerStopped) => TextKey::WorkerStopped,
        Some(OverviewFailureKind::LoadFailed) | None => TextKey::LoadFailed,
    };
    egui::Frame::new()
        .fill(theme::ERROR_COLOR.gamma_multiply(0.12))
        .stroke(egui::Stroke::new(
            1.0,
            theme::ERROR_COLOR.gamma_multiply(0.65),
        ))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::Image::new(icons::triangle_alert())
                        .fit_to_exact_size(egui::vec2(16.0, 16.0))
                        .tint(theme::ERROR_COLOR),
                );
                ui.colored_label(theme::ERROR_COLOR, text(model.locale, key));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(text(model.locale, TextKey::Retry)).clicked() {
                        actions.push(ShellAction::Retry);
                    }
                });
            });
        });
}

fn render_loading(ui: &mut egui::Ui, model: &ShellViewModel) {
    ui.label(
        egui::RichText::new(text(model.locale, TextKey::Loading))
            .strong()
            .size(14.0),
    );
    ui.add_space(8.0);
    for _ in 0..5 {
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(egui::CornerRadius::same(4))
            .show(ui, |ui| {
                ui.set_min_height(54.0);
                ui.allocate_space(egui::vec2(ui.available_width(), 54.0));
            });
        ui.add_space(6.0);
    }
}

fn render_snapshot(ui: &mut egui::Ui, model: &ShellViewModel, snapshot: &OverviewSnapshot) {
    let gap = 22.0;
    let available = ui.available_width();
    let left_width = ((available - gap) * 0.60).max(360.0);
    let right_width = (available - gap - left_width).max(280.0);
    let height = ui.available_height();

    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(left_width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| render_operational(ui, model, snapshot),
        );
        ui.add_space(gap);
        ui.allocate_ui_with_layout(
            egui::vec2(right_width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| render_versions_and_paths(ui, model, snapshot),
        );
    });
}

fn render_operational(ui: &mut egui::Ui, model: &ShellViewModel, snapshot: &OverviewSnapshot) {
    let codex_state = match snapshot.codex_app.presence {
        ResourcePresence::Found => TextKey::Found,
        ResourcePresence::Missing => TextKey::Missing,
    };
    resource_row(
        ui,
        text(model.locale, TextKey::CodexApplication),
        text(model.locale, codex_state),
        snapshot.codex_app.path.as_deref(),
        snapshot.codex_app.presence == ResourcePresence::Found,
    );
    resource_row(
        ui,
        text(model.locale, TextKey::SilentEntrypoint),
        text(
            model.locale,
            if snapshot.silent_shortcut.installed {
                TextKey::Installed
            } else {
                TextKey::Missing
            },
        ),
        snapshot.silent_shortcut.path.as_deref(),
        snapshot.silent_shortcut.installed,
    );
    resource_row(
        ui,
        text(model.locale, TextKey::ManagementEntrypoint),
        text(
            model.locale,
            if snapshot.management_shortcut.installed {
                TextKey::Installed
            } else {
                TextKey::Missing
            },
        ),
        snapshot.management_shortcut.path.as_deref(),
        snapshot.management_shortcut.installed,
    );

    ui.add_space(10.0);
    section_title(ui, text(model.locale, TextKey::LatestLaunch));
    match &snapshot.latest_launch {
        Some(launch) => {
            key_value_row(ui, text(model.locale, TextKey::Status), &launch.status);
            key_value_row(
                ui,
                text(model.locale, TextKey::StartedAt),
                &launch.started_at_ms.to_string(),
            );
            key_value_row(
                ui,
                text(model.locale, TextKey::DebugPort),
                &optional_port(launch.debug_port),
            );
            key_value_row(
                ui,
                text(model.locale, TextKey::HelperPort),
                &optional_port(launch.helper_port),
            );
        }
        None => {
            ui.label(egui::RichText::new(text(model.locale, TextKey::NoLaunch)).weak());
        }
    }
}

fn render_versions_and_paths(
    ui: &mut egui::Ui,
    model: &ShellViewModel,
    snapshot: &OverviewSnapshot,
) {
    section_title(ui, text(model.locale, TextKey::Version));
    key_value_row(
        ui,
        text(model.locale, TextKey::CodexPlusVersion),
        &snapshot.current_version,
    );
    key_value_row(
        ui,
        text(model.locale, TextKey::CodexVersion),
        snapshot.codex_version.as_deref().unwrap_or("-"),
    );

    ui.add_space(14.0);
    section_title(ui, text(model.locale, TextKey::LocalPaths));
    path_row(
        ui,
        text(model.locale, TextKey::SettingsPath),
        &snapshot.settings_path,
    );
    path_row(
        ui,
        text(model.locale, TextKey::LogsPath),
        &snapshot.logs_path,
    );
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).strong().size(14.0));
    ui.add_space(4.0);
}

fn resource_row(ui: &mut egui::Ui, title: &str, status: &str, path: Option<&Path>, ok: bool) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_min_height(54.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::Image::new(if ok {
                        icons::circle_check()
                    } else {
                        icons::triangle_alert()
                    })
                    .fit_to_exact_size(egui::vec2(16.0, 16.0))
                    .tint(if ok {
                        theme::SUCCESS_COLOR
                    } else {
                        theme::WARNING_COLOR
                    }),
                );
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(title).strong());
                        ui.label(egui::RichText::new(status).weak());
                    });
                    if let Some(path) = path {
                        let text = path.to_string_lossy();
                        ui.add(egui::Label::new(text.as_ref()).truncate())
                            .on_hover_text(text.as_ref());
                    }
                });
            });
        });
    ui.add_space(6.0);
}

fn key_value_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        ui.label(egui::RichText::new(label).weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(value);
        });
    });
    ui.separator();
}

fn path_row(ui: &mut egui::Ui, label: &str, path: &Path) {
    let path = path.to_string_lossy();
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).weak());
        ui.add(egui::Label::new(path.as_ref()).truncate())
            .on_hover_text(path.as_ref());
        ui.add_space(4.0);
    });
    ui.separator();
}

fn optional_port(port: Option<u16>) -> String {
    port.map_or_else(|| "-".to_owned(), |port| port.to_string())
}
