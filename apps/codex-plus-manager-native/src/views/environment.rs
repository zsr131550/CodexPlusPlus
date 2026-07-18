use std::collections::BTreeMap;

use codex_plus_core::env_conflicts::{
    EnvConflictRemoval, EnvConflictRemovalFailure, EnvConflictSource,
};
use codex_plus_core::relay_environment::ProxyEnvironmentSource;
use eframe::egui;

use crate::i18n::{Locale, TextKey, text};
use crate::state::environment::{EnvironmentFailureKind, EnvironmentViewState};
use crate::state::provider::OperationPhase;
use crate::theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentAction {
    RetryInspection,
    SetSelected { name: String, selected: bool },
    RequestCleanup,
    CancelCleanup,
    ConfirmCleanup,
}

pub fn render(
    ui: &mut egui::Ui,
    state: &EnvironmentViewState,
    locale: Locale,
    actions: &mut Vec<EnvironmentAction>,
) {
    if state.workspace.is_none() && state.inspection_phase == OperationPhase::Running {
        ui.centered_and_justified(|ui| {
            ui.spinner();
            ui.label(text(locale, TextKey::Loading));
        });
        return;
    }
    if state.workspace.is_none() && state.inspection_phase == OperationPhase::Error {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.colored_label(
                    theme::ERROR_COLOR,
                    environment_error_text(locale, state.inspection_error),
                );
                if ui.button(text(locale, TextKey::RetryInspection)).clicked() {
                    actions.push(EnvironmentAction::RetryInspection);
                }
            });
        });
        return;
    }

    let Some(workspace) = &state.workspace else {
        return;
    };
    let healthy = workspace.report.all_passed() && workspace.conflicts.is_empty();
    ui.horizontal(|ui| {
        ui.colored_label(
            if healthy {
                theme::SUCCESS_COLOR
            } else {
                theme::WARNING_COLOR
            },
            text(
                locale,
                if healthy {
                    TextKey::EnvironmentHealthy
                } else {
                    TextKey::EnvironmentIssues
                },
            ),
        );
        if state.inspection_phase == OperationPhase::Running {
            ui.spinner();
            ui.label(text(locale, TextKey::Refreshing));
        }
    });
    ui.add_space(8.0);

    let scroll_height = (ui.available_height() - 48.0).max(120.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), scroll_height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            egui::ScrollArea::vertical()
                .id_salt("relay_environment_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    section_title(ui, text(locale, TextKey::RelayDiagnostics));
                    diagnostic_row(
                        ui,
                        text(locale, TextKey::TunMode),
                        if workspace.report.clash_verge_tun.enabled {
                            text(locale, TextKey::Enabled)
                        } else {
                            text(locale, TextKey::Disabled)
                        },
                        workspace.report.clash_verge_tun.enabled,
                    );
                    diagnostic_row(
                        ui,
                        text(locale, TextKey::CodexEnvFile),
                        if workspace.report.codex_env_file.exists {
                            text(locale, TextKey::Present)
                        } else {
                            text(locale, TextKey::NotPresent)
                        },
                        workspace.report.codex_env_file.exists,
                    );

                    ui.add_space(10.0);
                    section_title(ui, text(locale, TextKey::ProxyEnvironment));
                    if workspace.report.proxy_environment.variables.is_empty() {
                        muted_row(ui, text(locale, TextKey::NoConflicts));
                    } else {
                        for variable in &workspace.report.proxy_environment.variables {
                            named_source_row(
                                ui,
                                &variable.name,
                                proxy_source_text(locale, variable.source),
                            );
                        }
                    }

                    ui.add_space(10.0);
                    section_title(ui, text(locale, TextKey::OpenAiConflicts));
                    let grouped = grouped_conflicts(state);
                    if grouped.is_empty() {
                        muted_row(ui, text(locale, TextKey::NoConflicts));
                    } else {
                        for (name, sources) in grouped {
                            let mut selected = state.is_selected(&name);
                            let source = sources
                                .iter()
                                .map(|source| conflict_source_text(locale, *source))
                                .collect::<Vec<_>>()
                                .join(" / ");
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), 34.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    if ui.checkbox(&mut selected, &name).changed() {
                                        actions.push(EnvironmentAction::SetSelected {
                                            name: name.clone(),
                                            selected,
                                        });
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(egui::RichText::new(&source).weak());
                                        },
                                    );
                                },
                            );
                        }
                    }

                    ui.add_space(10.0);
                    render_cleanup_result(ui, state, locale);
                });
        },
    );

    ui.separator();
    ui.horizontal(|ui| {
        let selected_count = state.selected_names().count();
        ui.label(format!(
            "{}: {selected_count}",
            text(locale, TextKey::CleanupSelected)
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    selected_count > 0 && state.cleanup_phase != OperationPhase::Running,
                    egui::Button::new(text(locale, TextKey::CleanupSelected)),
                )
                .clicked()
            {
                actions.push(EnvironmentAction::RequestCleanup);
            }
            if state.cleanup_phase == OperationPhase::Running {
                ui.spinner();
                ui.label(text(locale, TextKey::InProgress));
            }
        });
    });

    if state.cleanup_confirmation {
        render_cleanup_confirmation(ui.ctx(), state, locale, actions);
    }
}

fn grouped_conflicts(state: &EnvironmentViewState) -> BTreeMap<String, Vec<EnvConflictSource>> {
    let mut grouped = BTreeMap::<String, Vec<EnvConflictSource>>::new();
    if let Some(workspace) = &state.workspace {
        for conflict in &workspace.conflicts {
            let sources = grouped.entry(conflict.name.clone()).or_default();
            if !sources.contains(&conflict.source) {
                sources.push(conflict.source);
            }
        }
    }
    grouped
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).strong().size(13.0));
    ui.separator();
}

fn diagnostic_row(ui: &mut egui::Ui, label: &str, value: &str, warning: bool) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 34.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(
                    if warning {
                        theme::WARNING_COLOR
                    } else {
                        theme::SUCCESS_COLOR
                    },
                    value,
                );
            });
        },
    );
}

fn named_source_row(ui: &mut egui::Ui, name: &str, source: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 34.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(source).weak());
            });
        },
    );
}

fn muted_row(ui: &mut egui::Ui, text: &str) {
    ui.add_sized(
        [ui.available_width(), 34.0],
        egui::Label::new(egui::RichText::new(text).weak()),
    );
}

fn render_cleanup_result(ui: &mut egui::Ui, state: &EnvironmentViewState, locale: Locale) {
    if state.cleanup_phase == OperationPhase::Error {
        ui.colored_label(
            theme::ERROR_COLOR,
            cleanup_error_text(locale, state.cleanup_error),
        );
    }
    let Some(outcome) = &state.cleanup_outcome else {
        return;
    };
    let partial = !outcome.failures.is_empty();
    ui.colored_label(
        if partial {
            theme::WARNING_COLOR
        } else {
            theme::SUCCESS_COLOR
        },
        text(
            locale,
            if partial {
                TextKey::CleanupPartial
            } else {
                TextKey::CleanupSucceeded
            },
        ),
    );
    ui.label(format!(
        "{}: {}",
        text(locale, TextKey::FailureCount),
        outcome.failures.len()
    ));
    ui.label(format!(
        "{}: {}",
        text(locale, TextKey::RemainingConflicts),
        outcome.remaining.len()
    ));
    if let Some(path) = &outcome.backup_path {
        evidence_path_row(ui, text(locale, TextKey::BackupCreated), path);
    } else {
        muted_row(ui, text(locale, TextKey::NoBackup));
    }
    for removal in &outcome.removed {
        named_source_row(
            ui,
            &removal.name,
            &removal_status(locale, removal, &outcome.failures),
        );
    }
    for EnvConflictRemovalFailure { name, source } in &outcome.failures {
        if !outcome.removed.iter().any(|removal| removal.name == *name) {
            named_source_row(
                ui,
                name,
                &format!(
                    "{}: {}",
                    conflict_source_text(locale, *source),
                    text(locale, TextKey::Failed)
                ),
            );
        }
    }
}

fn evidence_path_row(ui: &mut egui::Ui, label: &str, path: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 34.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(path).truncate())
                    .on_hover_text(path);
            });
        },
    );
}

fn removal_status(
    locale: Locale,
    removal: &EnvConflictRemoval,
    failures: &[EnvConflictRemovalFailure],
) -> String {
    let mut statuses = Vec::new();
    if removal.removed_process {
        statuses.push(format!(
            "{}: {}",
            text(locale, TextKey::Process),
            text(locale, TextKey::Removed)
        ));
    }
    if removal.removed_user {
        statuses.push(format!(
            "{}: {}",
            text(locale, TextKey::User),
            text(locale, TextKey::Removed)
        ));
    }
    statuses.extend(
        failures
            .iter()
            .filter(|failure| failure.name == removal.name)
            .map(|failure| {
                format!(
                    "{}: {}",
                    conflict_source_text(locale, failure.source),
                    text(locale, TextKey::Failed)
                )
            }),
    );
    if statuses.is_empty() {
        statuses.push(text(locale, TextKey::Failed).to_owned());
    }
    statuses.join(" / ")
}

fn render_cleanup_confirmation(
    ctx: &egui::Context,
    state: &EnvironmentViewState,
    locale: Locale,
    actions: &mut Vec<EnvironmentAction>,
) {
    egui::Window::new(text(locale, TextKey::CleanupConfirmationTitle))
        .id(egui::Id::new("environment_cleanup_confirmation"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            for name in state.selected_names() {
                ui.label(name);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(text(locale, TextKey::Cancel)).clicked() {
                    actions.push(EnvironmentAction::CancelCleanup);
                }
                if ui.button(text(locale, TextKey::ConfirmCleanup)).clicked() {
                    actions.push(EnvironmentAction::ConfirmCleanup);
                }
            });
        });
}

fn environment_error_text(locale: Locale, error: Option<EnvironmentFailureKind>) -> &'static str {
    match error {
        Some(EnvironmentFailureKind::WorkerStopped) => {
            text(locale, TextKey::EnvironmentWorkerStopped)
        }
        Some(EnvironmentFailureKind::Service(
            codex_plus_manager_service::RelayEnvironmentErrorKind::Conflict,
        )) => text(locale, TextKey::EnvironmentChanged),
        Some(EnvironmentFailureKind::Service(_)) | None => text(locale, TextKey::InspectionFailed),
    }
}

fn cleanup_error_text(locale: Locale, error: Option<EnvironmentFailureKind>) -> &'static str {
    match error {
        Some(EnvironmentFailureKind::WorkerStopped) => {
            text(locale, TextKey::EnvironmentWorkerStopped)
        }
        Some(EnvironmentFailureKind::Service(
            codex_plus_manager_service::RelayEnvironmentErrorKind::Conflict,
        )) => text(locale, TextKey::EnvironmentChanged),
        Some(EnvironmentFailureKind::Service(_)) | None => text(locale, TextKey::CleanupFailed),
    }
}

fn conflict_source_text(locale: Locale, source: EnvConflictSource) -> &'static str {
    text(
        locale,
        match source {
            EnvConflictSource::Process => TextKey::Process,
            EnvConflictSource::User => TextKey::User,
        },
    )
}

fn proxy_source_text(locale: Locale, source: ProxyEnvironmentSource) -> &'static str {
    text(
        locale,
        match source {
            ProxyEnvironmentSource::Process => TextKey::Process,
            ProxyEnvironmentSource::User => TextKey::User,
            ProxyEnvironmentSource::System => TextKey::System,
        },
    )
}
