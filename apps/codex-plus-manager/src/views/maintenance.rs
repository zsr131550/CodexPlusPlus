use std::fmt;

use codex_plus_core::desktop_integration::{
    DesktopIntegrationHealth, DesktopIntegrationItemKind, DesktopIntegrationItemState,
};
use codex_plus_core::startup_registration::StartAtSignInHealth;
use codex_plus_manager_service::LaunchState;
use eframe::egui;

use crate::i18n::{Locale, TextKey, text};
use crate::state::desktop_integration::{
    DesktopIntegrationFailureKind, DesktopIntegrationOperationPhase, DesktopIntegrationViewState,
};
use crate::state::maintenance::{
    MaintenanceDocumentTab, MaintenanceFailureKind, MaintenanceOperationPhase, MaintenanceViewState,
};
use crate::{icons, theme};

#[derive(Clone, PartialEq, Eq)]
pub enum MaintenanceAction {
    Refresh,
    SetAppPath(String),
    PickExecutable,
    PickDirectory,
    SaveAppPath,
    RequestClear,
    ConfirmClear,
    CancelClear,
    SetDebugPort(u16),
    SetHelperPort(u16),
    Launch,
    SetDocumentTab(MaintenanceDocumentTab),
    SetLogLimit(usize),
    CopyDocument(String),
    ConfirmDiscard,
    CancelDiscard,
    RequestRepair,
    ConfirmRepair,
    CancelRepair,
    MigrateSignIn,
    SetStartAtSignIn(bool),
}

impl fmt::Debug for MaintenanceAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SetAppPath(_) => formatter.write_str("SetAppPath([redacted])"),
            Self::CopyDocument(_) => formatter.write_str("CopyDocument([safe-document])"),
            Self::SetDebugPort(port) => formatter.debug_tuple("SetDebugPort").field(port).finish(),
            Self::SetHelperPort(port) => {
                formatter.debug_tuple("SetHelperPort").field(port).finish()
            }
            Self::SetDocumentTab(tab) => {
                formatter.debug_tuple("SetDocumentTab").field(tab).finish()
            }
            Self::SetLogLimit(limit) => formatter.debug_tuple("SetLogLimit").field(limit).finish(),
            Self::Refresh => formatter.write_str("Refresh"),
            Self::PickExecutable => formatter.write_str("PickExecutable"),
            Self::PickDirectory => formatter.write_str("PickDirectory"),
            Self::SaveAppPath => formatter.write_str("SaveAppPath"),
            Self::RequestClear => formatter.write_str("RequestClear"),
            Self::ConfirmClear => formatter.write_str("ConfirmClear"),
            Self::CancelClear => formatter.write_str("CancelClear"),
            Self::Launch => formatter.write_str("Launch"),
            Self::ConfirmDiscard => formatter.write_str("ConfirmDiscard"),
            Self::CancelDiscard => formatter.write_str("CancelDiscard"),
            Self::RequestRepair => formatter.write_str("RequestRepair"),
            Self::ConfirmRepair => formatter.write_str("ConfirmRepair"),
            Self::CancelRepair => formatter.write_str("CancelRepair"),
            Self::MigrateSignIn => formatter.write_str("MigrateSignIn"),
            Self::SetStartAtSignIn(enabled) => formatter
                .debug_tuple("SetStartAtSignIn")
                .field(enabled)
                .finish(),
        }
    }
}

pub fn render(
    ui: &mut egui::Ui,
    state: &MaintenanceViewState,
    desktop_integration: &DesktopIntegrationViewState,
    locale: Locale,
    actions: &mut Vec<MaintenanceAction>,
) {
    render_feedback(ui, state, desktop_integration, locale);
    egui::ScrollArea::vertical()
        .id_salt("maintenance_page_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if ui.available_width() >= 720.0 {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(360.0, 610.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| render_application(ui, state, desktop_integration, locale, actions),
                    );
                    ui.separator();
                    let width = ui.available_width().max(320.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, 610.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| render_diagnostics(ui, state, locale, actions),
                    );
                });
            } else {
                render_application(ui, state, desktop_integration, locale, actions);
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(12.0);
                render_diagnostics(ui, state, locale, actions);
            }
        });

    if state.clear_confirmation_visible() {
        render_clear_confirmation(ui.ctx(), locale, actions);
    }
    if state.discard_confirmation_visible() {
        render_discard_confirmation(ui.ctx(), locale, actions);
    }
    if desktop_integration.repair_confirmation_visible() {
        render_repair_confirmation(ui.ctx(), desktop_integration, locale, actions);
    }
}

fn render_feedback(
    ui: &mut egui::Ui,
    state: &MaintenanceViewState,
    desktop_integration: &DesktopIntegrationViewState,
    locale: Locale,
) {
    let error = state.save.error.or(state.launch.error).or(state.load_error);
    if state.conflict_visible() {
        ui.colored_label(
            theme::WARNING_COLOR,
            text(locale, TextKey::MaintenanceSettingsConflict),
        );
    } else if let Some(kind) = error {
        ui.colored_label(theme::ERROR_COLOR, failure_text(locale, kind));
    } else if state.picker_error.is_some() {
        ui.colored_label(theme::ERROR_COLOR, text(locale, TextKey::PathPickerFailed));
    } else if state.path_dirty() {
        ui.colored_label(
            theme::WARNING_COLOR,
            text(locale, TextKey::UnsavedPathChanges),
        );
    } else if state.launch.phase == MaintenanceOperationPhase::Ready
        && state.launch_outcome.is_some_and(|outcome| outcome.accepted)
    {
        ui.colored_label(theme::SUCCESS_COLOR, text(locale, TextKey::LaunchAccepted));
    } else if let Some(error) = desktop_integration
        .operation
        .error
        .or(desktop_integration.load_error)
    {
        ui.colored_label(
            theme::ERROR_COLOR,
            desktop_integration_failure_text(locale, error),
        );
    }
}

fn render_application(
    ui: &mut egui::Ui,
    state: &MaintenanceViewState,
    desktop_integration: &DesktopIntegrationViewState,
    locale: Locale,
    actions: &mut Vec<MaintenanceAction>,
) {
    ui.heading(text(locale, TextKey::CodexApplication));
    ui.add_space(4.0);

    let workspace = state.workspace.as_deref();
    let app_summary = workspace.and_then(|workspace| workspace.codex_app.value());
    status_row(
        ui,
        text(locale, TextKey::Status),
        match app_summary {
            Some(summary) if summary.found => text(locale, TextKey::Found),
            Some(_) => text(locale, TextKey::Missing),
            None => text(locale, TextKey::Unknown),
        },
    );
    status_row(
        ui,
        text(locale, TextKey::Version),
        app_summary
            .and_then(|summary| summary.version.as_deref())
            .unwrap_or_else(|| text(locale, TextKey::Unknown)),
    );

    ui.add_space(10.0);
    ui.label(egui::RichText::new(text(locale, TextKey::ApplicationPath)).strong());
    ui.horizontal(|ui| {
        let mut path = state.app_path_draft.expose().to_owned();
        let button_width = 34.0 * 4.0 + ui.spacing().item_spacing.x * 4.0;
        let response = ui.add_sized(
            [(ui.available_width() - button_width).max(90.0), 34.0],
            egui::TextEdit::singleline(&mut path)
                .hint_text(text(locale, TextKey::AutomaticDiscovery)),
        );
        if response.changed() {
            actions.push(MaintenanceAction::SetAppPath(path));
        }

        if icon_button(
            ui,
            icons::file_search(),
            text(locale, TextKey::SelectExecutable),
            !state.picker_pending(),
        )
        .clicked()
        {
            actions.push(MaintenanceAction::PickExecutable);
        }
        if icon_button(
            ui,
            icons::folder_open(),
            text(locale, TextKey::SelectDirectory),
            !state.picker_pending(),
        )
        .clicked()
        {
            actions.push(MaintenanceAction::PickDirectory);
        }
        if icon_button(
            ui,
            icons::save(),
            text(locale, TextKey::SaveApplicationPath),
            state.path_dirty() && state.save.phase != MaintenanceOperationPhase::Running,
        )
        .clicked()
        {
            actions.push(MaintenanceAction::SaveAppPath);
        }
        let configured = workspace
            .and_then(|workspace| workspace.app_path.as_ref())
            .is_some_and(|path| path.configured);
        if icon_button(
            ui,
            icons::rotate_ccw(),
            text(locale, TextKey::ClearApplicationPath),
            configured
                && !state.path_dirty()
                && state.save.phase != MaintenanceOperationPhase::Running,
        )
        .clicked()
        {
            actions.push(MaintenanceAction::RequestClear);
        }
    });

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(text(locale, TextKey::DebugPort));
            let mut port = state.debug_port;
            let response = ui.add(egui::DragValue::new(&mut port).range(1..=u16::MAX).speed(1));
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::DragValue,
                    ui.is_enabled(),
                    text(locale, TextKey::DebugPort),
                )
            });
            if response.changed() {
                actions.push(MaintenanceAction::SetDebugPort(port));
            }
        });
        ui.vertical(|ui| {
            ui.label(text(locale, TextKey::HelperPort));
            let mut port = state.helper_port;
            let response = ui.add(egui::DragValue::new(&mut port).range(1..=u16::MAX).speed(1));
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::DragValue,
                    ui.is_enabled(),
                    text(locale, TextKey::HelperPort),
                )
            });
            if response.changed() {
                actions.push(MaintenanceAction::SetHelperPort(port));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
            if icon_button(
                ui,
                icons::play(),
                text(locale, TextKey::LaunchCodex),
                state.launch.phase != MaintenanceOperationPhase::Running,
            )
            .clicked()
            {
                actions.push(MaintenanceAction::Launch);
            }
        });
    });

    ui.add_space(14.0);
    ui.separator();
    ui.add_space(8.0);
    render_desktop_integration(ui, desktop_integration, locale, actions);
    ui.add_space(8.0);
    let launch = workspace
        .and_then(|workspace| workspace.latest_launch.value())
        .and_then(Option::as_ref);
    status_row(
        ui,
        text(locale, TextKey::LatestLaunch),
        launch
            .map(|launch| launch_state_text(locale, launch.status))
            .unwrap_or_else(|| text(locale, TextKey::NoLaunch)),
    );
}

fn render_desktop_integration(
    ui: &mut egui::Ui,
    state: &DesktopIntegrationViewState,
    locale: Locale,
    actions: &mut Vec<MaintenanceAction>,
) {
    ui.heading(text(locale, TextKey::DesktopIntegration));
    ui.add_space(4.0);
    if let Some(workspace) = state.workspace.as_deref() {
        if workspace.repair_items.is_empty() {
            status_row(
                ui,
                text(locale, TextKey::Status),
                repair_health_text(locale, workspace.repair_health),
            );
        } else {
            for item in &workspace.repair_items {
                status_row(
                    ui,
                    desktop_item_text(locale, item.kind),
                    desktop_item_state_text(locale, item.state),
                );
            }
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if icon_button(
                ui,
                icons::wrench(),
                text(locale, TextKey::RepairDesktopIntegration),
                workspace.repair_health == DesktopIntegrationHealth::NeedsRepair
                    && state.operation.phase != DesktopIntegrationOperationPhase::Running,
            )
            .clicked()
            {
                actions.push(MaintenanceAction::RequestRepair);
            }
            if let Some(sign_in) = &workspace.sign_in {
                let mut enabled = sign_in.effective_enabled;
                let response = ui.add_enabled(
                    state.operation.phase != DesktopIntegrationOperationPhase::Running,
                    egui::Checkbox::new(&mut enabled, text(locale, TextKey::StartAtSignIn)),
                );
                if response.changed() {
                    actions.push(MaintenanceAction::SetStartAtSignIn(enabled));
                }
            }
        });

        if workspace
            .sign_in
            .as_ref()
            .is_some_and(|status| status.health == StartAtSignInHealth::NeedsMigration)
        {
            ui.colored_label(
                theme::WARNING_COLOR,
                text(locale, TextKey::LegacySignInActive),
            );
            if ui
                .add_enabled(
                    state.migrate_visible(),
                    egui::Button::new(text(locale, TextKey::MigrateSignIn)),
                )
                .clicked()
            {
                actions.push(MaintenanceAction::MigrateSignIn);
            }
        }
    } else {
        status_row(
            ui,
            text(locale, TextKey::Status),
            if matches!(
                state.load_phase,
                crate::state::desktop_integration::DesktopIntegrationLoadPhase::Loading
                    | crate::state::desktop_integration::DesktopIntegrationLoadPhase::Refreshing
            ) {
                text(locale, TextKey::Loading)
            } else {
                text(locale, TextKey::Unknown)
            },
        );
    }
}

fn render_repair_confirmation(
    ctx: &egui::Context,
    state: &DesktopIntegrationViewState,
    locale: Locale,
    actions: &mut Vec<MaintenanceAction>,
) {
    egui::Window::new(text(locale, TextKey::RepairDesktopIntegrationTitle))
        .id(egui::Id::new("maintenance_desktop_repair_confirmation"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            for kind in state.repair_confirmation_item_kinds() {
                ui.label(desktop_item_text(locale, *kind));
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(text(locale, TextKey::Repair)).clicked() {
                    actions.push(MaintenanceAction::ConfirmRepair);
                }
                if ui.button(text(locale, TextKey::Cancel)).clicked() {
                    actions.push(MaintenanceAction::CancelRepair);
                }
            });
        });
}

fn desktop_item_text(locale: Locale, kind: DesktopIntegrationItemKind) -> &'static str {
    text(
        locale,
        match kind {
            DesktopIntegrationItemKind::DesktopManagerShortcut => TextKey::DesktopShortcut,
            DesktopIntegrationItemKind::StartMenuLauncherShortcut => TextKey::StartMenuLauncher,
            DesktopIntegrationItemKind::StartMenuManagerShortcut => TextKey::StartMenuManager,
            DesktopIntegrationItemKind::UrlProtocol => TextKey::UrlProtocol,
            DesktopIntegrationItemKind::MacosBundleRegistration => TextKey::MacosRegistration,
        },
    )
}

fn desktop_item_state_text(locale: Locale, state: DesktopIntegrationItemState) -> &'static str {
    text(
        locale,
        match state {
            DesktopIntegrationItemState::Current => TextKey::Current,
            DesktopIntegrationItemState::NeedsRepair => TextKey::NeedsRepair,
        },
    )
}

fn repair_health_text(locale: Locale, health: DesktopIntegrationHealth) -> &'static str {
    text(
        locale,
        match health {
            DesktopIntegrationHealth::Current => TextKey::Current,
            DesktopIntegrationHealth::NeedsRepair => TextKey::NeedsRepair,
            DesktopIntegrationHealth::ReinstallRequired => TextKey::ReinstallRequired,
            DesktopIntegrationHealth::Unavailable => TextKey::Unavailable,
        },
    )
}

fn render_diagnostics(
    ui: &mut egui::Ui,
    state: &MaintenanceViewState,
    locale: Locale,
    actions: &mut Vec<MaintenanceAction>,
) {
    ui.heading(text(locale, TextKey::Diagnostics));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        tab_button(
            ui,
            text(locale, TextKey::Logs),
            state.document_tab == MaintenanceDocumentTab::Logs,
            MaintenanceDocumentTab::Logs,
            actions,
        );
        tab_button(
            ui,
            text(locale, TextKey::Report),
            state.document_tab == MaintenanceDocumentTab::Report,
            MaintenanceDocumentTab::Report,
            actions,
        );
        if state.document_tab == MaintenanceDocumentTab::Logs {
            for (limit, key) in [
                (50, TextKey::Lines50),
                (100, TextKey::Lines100),
                (200, TextKey::Lines200),
            ] {
                if ui
                    .add(egui::Button::new(text(locale, key)).selected(state.log_limit == limit))
                    .clicked()
                {
                    actions.push(MaintenanceAction::SetLogLimit(limit));
                }
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(
                ui,
                icons::refresh_cw(),
                text(locale, TextKey::RefreshDiagnostics),
                true,
            )
            .clicked()
            {
                actions.push(MaintenanceAction::Refresh);
            }
            let document = state.active_document_text();
            if icon_button(
                ui,
                icons::copy(),
                text(locale, TextKey::CopyDocument),
                document.is_some(),
            )
            .clicked()
                && let Some(document) = document
            {
                actions.push(MaintenanceAction::CopyDocument(document.to_owned()));
            }
        });
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("maintenance_safe_document")
        .max_height(520.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let document = state
                .active_document_text()
                .unwrap_or_else(|| text(locale, TextKey::SafeDocumentUnavailable));
            ui.add(
                egui::Label::new(egui::RichText::new(document).monospace())
                    .selectable(true)
                    .wrap(),
            );
        });
}

fn render_clear_confirmation(
    ctx: &egui::Context,
    locale: Locale,
    actions: &mut Vec<MaintenanceAction>,
) {
    egui::Window::new(text(locale, TextKey::ClearApplicationPathTitle))
        .id(egui::Id::new("maintenance_clear_path_confirmation"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            ui.label(text(locale, TextKey::ClearApplicationPathMessage));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(text(locale, TextKey::ClearPath)).clicked() {
                    actions.push(MaintenanceAction::ConfirmClear);
                }
                if ui.button(text(locale, TextKey::Cancel)).clicked() {
                    actions.push(MaintenanceAction::CancelClear);
                }
            });
        });
}

fn render_discard_confirmation(
    ctx: &egui::Context,
    locale: Locale,
    actions: &mut Vec<MaintenanceAction>,
) {
    egui::Window::new(text(locale, TextKey::DiscardApplicationPathTitle))
        .id(egui::Id::new("maintenance_discard_path_confirmation"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            ui.label(text(locale, TextKey::DiscardApplicationPathMessage));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(text(locale, TextKey::DiscardChanges)).clicked() {
                    actions.push(MaintenanceAction::ConfirmDiscard);
                }
                if ui.button(text(locale, TextKey::KeepEditing)).clicked() {
                    actions.push(MaintenanceAction::CancelDiscard);
                }
            });
        });
}

fn status_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).weak());
        });
    });
}

fn tab_button(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    tab: MaintenanceDocumentTab,
    actions: &mut Vec<MaintenanceAction>,
) {
    if ui
        .add_sized([68.0, 30.0], egui::Button::new(label).selected(selected))
        .clicked()
    {
        actions.push(MaintenanceAction::SetDocumentTab(tab));
    }
}

fn icon_button(
    ui: &mut egui::Ui,
    icon: egui::ImageSource<'static>,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::image(egui::Image::new(icon).fit_to_exact_size(egui::vec2(17.0, 17.0)))
            .min_size(egui::vec2(34.0, 34.0)),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response.on_hover_text(label)
}

fn launch_state_text(locale: Locale, state: LaunchState) -> &'static str {
    text(
        locale,
        match state {
            LaunchState::Starting => TextKey::Starting,
            LaunchState::Running => TextKey::Running,
            LaunchState::Ready => TextKey::Ready,
            LaunchState::Failed => TextKey::Failed,
            LaunchState::Stopped => TextKey::Stopped,
            LaunchState::Unknown => TextKey::Unknown,
        },
    )
}

fn desktop_integration_failure_text(
    locale: Locale,
    kind: DesktopIntegrationFailureKind,
) -> &'static str {
    text(
        locale,
        match kind {
            DesktopIntegrationFailureKind::InspectFailed => TextKey::DesktopIntegrationLoadFailed,
            DesktopIntegrationFailureKind::WorkerStopped => TextKey::WorkerStopped,
            DesktopIntegrationFailureKind::Service(kind) => match kind {
                codex_plus_manager_service::DesktopIntegrationErrorKind::InspectFailed => {
                    TextKey::DesktopIntegrationLoadFailed
                }
                codex_plus_manager_service::DesktopIntegrationErrorKind::InvalidRevision => {
                    TextKey::DesktopIntegrationInvalidRevision
                }
                codex_plus_manager_service::DesktopIntegrationErrorKind::Conflict => {
                    TextKey::DesktopIntegrationConflict
                }
                codex_plus_manager_service::DesktopIntegrationErrorKind::ConfirmationRequired => {
                    TextKey::DesktopIntegrationConfirmationRequired
                }
                codex_plus_manager_service::DesktopIntegrationErrorKind::RepairUnavailable => {
                    TextKey::DesktopIntegrationRepairUnavailable
                }
                codex_plus_manager_service::DesktopIntegrationErrorKind::MigrationUnavailable => {
                    TextKey::DesktopIntegrationMigrationUnavailable
                }
                codex_plus_manager_service::DesktopIntegrationErrorKind::SignInUnavailable => {
                    TextKey::DesktopIntegrationSignInUnavailable
                }
                codex_plus_manager_service::DesktopIntegrationErrorKind::EffectFailed => {
                    TextKey::DesktopIntegrationEffectFailed
                }
                codex_plus_manager_service::DesktopIntegrationErrorKind::WorkerStopped => {
                    TextKey::WorkerStopped
                }
            },
        },
    )
}

pub fn failure_text(locale: Locale, kind: MaintenanceFailureKind) -> &'static str {
    text(
        locale,
        match kind {
            MaintenanceFailureKind::SettingsReadFailed => TextKey::MaintenanceSettingsReadFailed,
            MaintenanceFailureKind::SettingsWriteFailed => TextKey::MaintenanceSettingsWriteFailed,
            MaintenanceFailureKind::SettingsConflict => TextKey::MaintenanceSettingsConflict,
            MaintenanceFailureKind::InvalidRevision => TextKey::MaintenanceInvalidRevision,
            MaintenanceFailureKind::InvalidPath => TextKey::MaintenanceInvalidPath,
            MaintenanceFailureKind::InvalidPort => TextKey::MaintenanceInvalidPort,
            MaintenanceFailureKind::EntrypointReadFailed => {
                TextKey::MaintenanceEntrypointReadFailed
            }
            MaintenanceFailureKind::StatusReadFailed => TextKey::MaintenanceStatusReadFailed,
            MaintenanceFailureKind::LogReadFailed => TextKey::MaintenanceLogReadFailed,
            MaintenanceFailureKind::LaunchFailed => TextKey::MaintenanceLaunchFailed,
            MaintenanceFailureKind::WorkerStopped => TextKey::WorkerStopped,
        },
    )
}
