use eframe::egui;

use crate::i18n::{Locale, TextKey, text};
use crate::state::import::{ImportFailureKind, ImportViewState};
use crate::state::provider::{OperationPhase, ProviderViewState};
use crate::{icons, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportAction {
    DiscoverCcs,
    CloseCcs,
    ConfirmCcs,
    ConfirmPending,
    DismissPending,
    RefreshPending,
}

pub fn render_provider_toolbar(
    ui: &mut egui::Ui,
    state: &ImportViewState,
    locale: Locale,
    actions: &mut Vec<ImportAction>,
) {
    ui.horizontal(|ui| {
        let response = ui.add_enabled(
            state.pending.is_none(),
            egui::Button::image(
                egui::Image::new(icons::server_cog()).fit_to_exact_size(egui::vec2(17.0, 17.0)),
            )
            .min_size(egui::vec2(34.0, 34.0)),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                text(locale, TextKey::ImportProviders),
            )
        });
        if response
            .on_hover_text(text(locale, TextKey::ImportFromCcs))
            .clicked()
        {
            actions.push(ImportAction::DiscoverCcs);
        }
        if state.pending.is_some() {
            ui.colored_label(theme::WARNING_COLOR, text(locale, TextKey::PendingImport));
        }
        if let Some(outcome) = state.batch_outcome.or(state.pending_outcome) {
            ui.colored_label(
                theme::SUCCESS_COLOR,
                format!(
                    "{}: {}",
                    text(locale, TextKey::ImportedCount),
                    outcome.imported
                ),
            );
        }
    });
}

pub fn render_modals(
    ctx: &egui::Context,
    state: &ImportViewState,
    provider: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ImportAction>,
) {
    if state.pending.is_some() {
        render_pending_modal(ctx, state, provider, locale, actions);
    } else if state.discovery_open {
        render_ccs_modal(ctx, state, provider, locale, actions);
    }
}

fn render_ccs_modal(
    ctx: &egui::Context,
    state: &ImportViewState,
    provider: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ImportAction>,
) {
    egui::Window::new(text(locale, TextKey::CcsImportTitle))
        .id(egui::Id::new("ccs_provider_import"))
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .max_width(680.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            if state.discovery.phase == OperationPhase::Running {
                ui.spinner();
                ui.label(text(locale, TextKey::Loading));
            }
            if let Some(error) = state.discovery.error.or(state.batch_import.error) {
                ui.colored_label(theme::ERROR_COLOR, import_error_text(locale, error));
            }
            if let Some(discovery) = &state.discovery_result {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{}: {}",
                        text(locale, TextKey::Importable),
                        discovery.importable_count
                    ));
                    ui.separator();
                    ui.label(format!(
                        "{}: {}",
                        text(locale, TextKey::Duplicates),
                        discovery.duplicate_count
                    ));
                });
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {}",
                        text(locale, TextKey::ImportSource),
                        discovery.source_path
                    ))
                    .weak(),
                );
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("ccs_import_provider_list")
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for item in &discovery.providers {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), 52.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(&item.name).strong());
                                        if item.duplicate {
                                            ui.colored_label(
                                                theme::WARNING_COLOR,
                                                text(locale, TextKey::Duplicate),
                                            );
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&item.base_url).weak(),
                                            )
                                            .truncate(),
                                        );
                                        ui.label(
                                            egui::RichText::new(format!("{:?}", item.protocol))
                                                .weak(),
                                        );
                                    });
                                },
                            );
                            ui.separator();
                        }
                    });
                if discovery.importable_count == 0 {
                    ui.label(text(locale, TextKey::NoImportableProviders));
                }
                if provider.is_dirty() {
                    ui.colored_label(
                        theme::WARNING_COLOR,
                        text(locale, TextKey::ProviderDraftDirty),
                    );
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        state.batch_import.phase != OperationPhase::Running,
                        egui::Button::new(text(locale, TextKey::Cancel)),
                    )
                    .clicked()
                {
                    actions.push(ImportAction::CloseCcs);
                }
                if ui
                    .add_enabled(
                        state.can_import_ccs(provider.is_dirty()),
                        egui::Button::new(text(locale, TextKey::ImportNew)),
                    )
                    .clicked()
                {
                    actions.push(ImportAction::ConfirmCcs);
                }
                if state.batch_import.phase == OperationPhase::Running {
                    ui.spinner();
                    ui.label(text(locale, TextKey::InProgress));
                }
            });
        });
}

fn render_pending_modal(
    ctx: &egui::Context,
    state: &ImportViewState,
    provider: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ImportAction>,
) {
    let Some(pending) = &state.pending else {
        return;
    };
    egui::Window::new(text(locale, TextKey::PendingImportTitle))
        .id(egui::Id::new("pending_provider_import"))
        .collapsible(false)
        .resizable(false)
        .default_width(480.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            key_value(ui, text(locale, TextKey::Providers), &pending.name);
            key_value(ui, text(locale, TextKey::BaseUrl), &pending.base_url);
            key_value(ui, text(locale, TextKey::WireApi), &pending.wire_api);
            key_value(ui, text(locale, TextKey::RelayMode), &pending.relay_mode);
            key_value(
                ui,
                text(locale, TextKey::PendingImport),
                text(
                    locale,
                    if pending.api_key_present {
                        TextKey::ApiKeyPresent
                    } else {
                        TextKey::ApiKeyMissing
                    },
                ),
            );
            if provider.is_dirty() {
                ui.colored_label(
                    theme::WARNING_COLOR,
                    text(locale, TextKey::ProviderDraftDirty),
                );
            }
            if let Some(error) = state
                .pending_load
                .error
                .or(state.pending_confirm.error)
                .or(state.pending_dismiss.error)
            {
                ui.colored_label(theme::ERROR_COLOR, import_error_text(locale, error));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let refresh_enabled = state.pending_load.phase != OperationPhase::Running
                    && state.pending_confirm.phase != OperationPhase::Running
                    && state.pending_dismiss.phase != OperationPhase::Running;
                let refresh = ui.add_enabled(
                    refresh_enabled,
                    egui::Button::image(
                        egui::Image::new(icons::refresh_cw())
                            .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                    )
                    .min_size(egui::vec2(32.0, 32.0)),
                );
                refresh.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        refresh_enabled,
                        text(locale, TextKey::RefreshPendingImport),
                    )
                });
                if refresh
                    .on_hover_text(text(locale, TextKey::RefreshPendingImport))
                    .clicked()
                {
                    actions.push(ImportAction::RefreshPending);
                }
                if ui
                    .add_enabled(
                        state.pending_load.phase != OperationPhase::Running
                            && state.pending_confirm.phase != OperationPhase::Running
                            && state.pending_dismiss.phase != OperationPhase::Running,
                        egui::Button::new(text(locale, TextKey::DismissImport)),
                    )
                    .clicked()
                {
                    actions.push(ImportAction::DismissPending);
                }
                if ui
                    .add_enabled(
                        state.can_confirm_pending(provider.is_dirty(), provider.baseline.is_some()),
                        egui::Button::new(text(locale, TextKey::ConfirmImport)),
                    )
                    .clicked()
                {
                    actions.push(ImportAction::ConfirmPending);
                }
                if state.pending_load.phase == OperationPhase::Running
                    || state.pending_confirm.phase == OperationPhase::Running
                    || state.pending_dismiss.phase == OperationPhase::Running
                {
                    ui.spinner();
                }
            });
        });
}

fn key_value(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized([128.0, 24.0], egui::Label::new(label));
        ui.add(egui::Label::new(value).truncate());
    });
}

fn import_error_text(locale: Locale, error: ImportFailureKind) -> &'static str {
    match error {
        ImportFailureKind::DirtyProvider => text(locale, TextKey::ProviderDraftDirty),
        ImportFailureKind::MissingProviderWorkspace => text(locale, TextKey::ProviderChanged),
        ImportFailureKind::WorkerStopped => text(locale, TextKey::ImportWorkerStopped),
        ImportFailureKind::Service(kind) => match kind {
            codex_plus_manager_service::ProviderImportErrorKind::SourceChanged => {
                text(locale, TextKey::SourceChanged)
            }
            codex_plus_manager_service::ProviderImportErrorKind::PendingConflict
            | codex_plus_manager_service::ProviderImportErrorKind::PendingUnavailable => {
                text(locale, TextKey::PendingChanged)
            }
            codex_plus_manager_service::ProviderImportErrorKind::ProviderConflict => {
                text(locale, TextKey::ProviderChanged)
            }
            _ => text(locale, TextKey::LoadFailed),
        },
    }
}
