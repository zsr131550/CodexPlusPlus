use codex_plus_core::models::DeleteStatus;
use codex_plus_manager_service::{ProviderSyncErrorKind, ProviderSyncStatus, SessionErrorKind};
use eframe::egui;

use crate::i18n::{Locale, TextKey, text};
use crate::state::provider::OperationPhase;
use crate::state::sessions::{
    ProviderSyncFailureKind, SessionFailureKind, SessionFilter, SessionViewState,
};
use crate::{icons, theme};

const SESSION_ROW_HEIGHT: f32 = 58.0;
const PROVIDER_BAND_HEIGHT: f32 = 154.0;
const DELETE_RESULT_HEIGHT: f32 = 96.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAction {
    Refresh,
    RetryWorkspace,
    SetQuery(String),
    SetFilter(SessionFilter),
    SetSelected { id: String, selected: bool },
    SelectAllFiltered,
    ClearSelection,
    SetPage(usize),
    RequestDelete,
    CancelDelete,
    ConfirmDelete,
    RetryProviderWorkspace,
    SetProviderTarget(String),
    RunProviderRepair,
    CancelProviderRepair,
    ConfirmProviderRepair,
    SetAutoRepair(bool),
}

pub fn render(
    ui: &mut egui::Ui,
    state: &SessionViewState,
    locale: Locale,
    actions: &mut Vec<SessionAction>,
) {
    render_toolbar(ui, state, locale, actions);
    ui.add_space(6.0);

    let delete_result_height = if state.delete_outcome.is_some() || state.delete_error.is_some() {
        DELETE_RESULT_HEIGHT
    } else {
        0.0
    };
    let list_height =
        (ui.available_height() - PROVIDER_BAND_HEIGHT - 56.0 - delete_result_height).max(160.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), list_height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| render_session_list(ui, state, locale, actions),
    );
    render_selection_footer(ui, state, locale, actions);
    if delete_result_height > 0.0 {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), delete_result_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| render_delete_result(ui, state, locale),
        );
    }
    ui.separator();
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), PROVIDER_BAND_HEIGHT),
        egui::Layout::top_down(egui::Align::Min),
        |ui| render_provider_repair(ui, state, locale, actions),
    );

    if state.delete_confirmation.is_some() {
        render_delete_confirmation(ui.ctx(), state, locale, actions);
    }
    if state.provider_run_confirmation.is_some() {
        render_provider_confirmation(ui.ctx(), state, locale, actions);
    }
}

fn render_toolbar(
    ui: &mut egui::Ui,
    state: &SessionViewState,
    locale: Locale,
    actions: &mut Vec<SessionAction>,
) {
    ui.horizontal(|ui| {
        ui.label(text(locale, TextKey::SearchSessions));
        let mut query = state.query.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut query)
                    .desired_width(ui.available_width().min(300.0)),
            )
            .changed()
        {
            actions.push(SessionAction::SetQuery(query));
        }
        let counts = [
            (
                SessionFilter::All,
                TextKey::AllSessions,
                state.workspace.as_ref().map_or(0, |w| w.sessions.len()),
            ),
            (
                SessionFilter::Active,
                TextKey::ActiveSessions,
                state.active_count(),
            ),
            (
                SessionFilter::Archived,
                TextKey::ArchivedSessions,
                state.archived_count(),
            ),
        ];
        for (filter, key, count) in counts {
            let label = format!("{} {count}", text(locale, key));
            if ui
                .add_sized(
                    [76.0, 28.0],
                    egui::Button::new(label).selected(state.filter == filter),
                )
                .clicked()
            {
                actions.push(SessionAction::SetFilter(filter));
            }
        }
        let refresh = labeled_icon_button(
            ui,
            icons::refresh_cw(),
            text(locale, TextKey::RefreshSessions),
            state.workspace_phase != OperationPhase::Running,
        );
        if refresh.clicked() {
            actions.push(SessionAction::Refresh);
        }
        if state.workspace_phase == OperationPhase::Running {
            ui.spinner();
        }
    });

    if let Some(issue_count) = state
        .workspace
        .as_ref()
        .map(|workspace| workspace.read_issues.len())
        .filter(|count| *count > 0)
    {
        ui.colored_label(
            theme::WARNING_COLOR,
            format!("{}: {issue_count}", text(locale, TextKey::ReadIssues)),
        );
    }
    if state.workspace_phase == OperationPhase::Error {
        render_session_error(ui, state.workspace_error, locale);
        if ui.button(text(locale, TextKey::Retry)).clicked() {
            actions.push(SessionAction::RetryWorkspace);
        }
    }
}

fn render_session_list(
    ui: &mut egui::Ui,
    state: &SessionViewState,
    locale: Locale,
    actions: &mut Vec<SessionAction>,
) {
    if state.workspace.is_none() && state.workspace_phase == OperationPhase::Running {
        ui.centered_and_justified(|ui| {
            ui.spinner();
            ui.label(text(locale, TextKey::Loading));
        });
        return;
    }

    let sessions = state.page_sessions();
    if sessions.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(text(locale, TextKey::NoSessions)).weak());
        });
        return;
    }

    let mutations_enabled = state.mutations_enabled();
    egui::ScrollArea::vertical()
        .id_salt("sessions_page_scroll")
        .auto_shrink([false, false])
        .show_rows(ui, SESSION_ROW_HEIGHT, sessions.len(), |ui, rows| {
            for session in &sessions[rows] {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), SESSION_ROW_HEIGHT - 1.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let mut selected = state.selected_ids.contains(&session.id);
                        let select_label = select_session_label(locale, &session.id);
                        let response = ui
                            .add_enabled(mutations_enabled, egui::Checkbox::new(&mut selected, ""));
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Checkbox,
                                mutations_enabled,
                                &select_label,
                            )
                        });
                        if response.on_hover_text(&select_label).changed() {
                            actions.push(SessionAction::SetSelected {
                                id: session.id.clone(),
                                selected,
                            });
                        }
                        ui.vertical(|ui| {
                            let title = if session.title.trim().is_empty() {
                                &session.id
                            } else {
                                &session.title
                            };
                            ui.add(
                                egui::Label::new(egui::RichText::new(title).strong()).truncate(),
                            )
                            .on_hover_text(title);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "{} | {} | {}: {}",
                                        session.model_provider,
                                        session.cwd,
                                        text(locale, TextKey::Databases),
                                        session.source_db_paths.len()
                                    ))
                                    .weak()
                                    .size(11.0),
                                )
                                .truncate(),
                            )
                            .on_hover_text(format!("{} | {}", session.id, session.cwd));
                        });
                        if session.archived {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(text(locale, TextKey::Archived)).weak(),
                                    );
                                },
                            );
                        }
                    },
                );
                ui.separator();
            }
        });
}

fn render_selection_footer(
    ui: &mut egui::Ui,
    state: &SessionViewState,
    locale: Locale,
    actions: &mut Vec<SessionAction>,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 42.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(format!(
                "{}: {}",
                text(locale, TextKey::SelectedSessions),
                state.selected_ids.len()
            ));
            let mutations_enabled = state.mutations_enabled();
            if ui
                .add_enabled(
                    mutations_enabled && !state.filtered_sessions().is_empty(),
                    egui::Button::new(text(locale, TextKey::SelectAllFiltered)),
                )
                .clicked()
            {
                actions.push(SessionAction::SelectAllFiltered);
            }
            if ui
                .add_enabled(
                    mutations_enabled && !state.selected_ids.is_empty(),
                    egui::Button::new(text(locale, TextKey::ClearSelection)),
                )
                .clicked()
            {
                actions.push(SessionAction::ClearSelection);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let delete = ui.add_enabled(
                    mutations_enabled && !state.selected_ids.is_empty(),
                    egui::Button::image_and_text(
                        egui::Image::new(icons::trash_2())
                            .fit_to_exact_size(egui::vec2(15.0, 15.0)),
                        text(locale, TextKey::DeleteSelectedSessions),
                    ),
                );
                if delete.clicked() {
                    actions.push(SessionAction::RequestDelete);
                }
                let next = labeled_icon_button(
                    ui,
                    icons::chevron_down(),
                    text(locale, TextKey::NextPage),
                    state.page + 1 < state.page_count(),
                );
                if next.clicked() {
                    actions.push(SessionAction::SetPage(state.page + 1));
                }
                ui.label(format!(
                    "{} {} / {}",
                    text(locale, TextKey::Page),
                    state.page + 1,
                    state.page_count()
                ));
                let previous = labeled_icon_button(
                    ui,
                    icons::chevron_up(),
                    text(locale, TextKey::PreviousPage),
                    state.page > 0,
                );
                if previous.clicked() {
                    actions.push(SessionAction::SetPage(state.page.saturating_sub(1)));
                }
            });
        },
    );
}

fn render_provider_repair(
    ui: &mut egui::Ui,
    state: &SessionViewState,
    locale: Locale,
    actions: &mut Vec<SessionAction>,
) {
    ui.label(
        egui::RichText::new(text(locale, TextKey::HistoricalSessionRepair))
            .strong()
            .size(13.0),
    );
    ui.add_space(4.0);

    if state.provider_workspace.is_none()
        && state.provider_workspace_phase == OperationPhase::Running
    {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(text(locale, TextKey::Loading));
        });
        return;
    }
    if state.provider_workspace_phase == OperationPhase::Error {
        ui.colored_label(
            theme::ERROR_COLOR,
            provider_error_text(locale, state.provider_workspace_error),
        );
        if ui.button(text(locale, TextKey::Retry)).clicked() {
            actions.push(SessionAction::RetryProviderWorkspace);
        }
        return;
    }

    let Some(workspace) = &state.provider_workspace else {
        return;
    };
    let mutation_running = state.provider_run_phase == OperationPhase::Running
        || state.auto_repair_phase == OperationPhase::Running;
    ui.horizontal(|ui| {
        ui.label(text(locale, TextKey::ProviderTarget));
        let response = egui::ComboBox::from_id_salt("session_provider_repair_target")
            .selected_text(&state.selected_provider_target)
            .width(220.0)
            .show_ui(ui, |ui| {
                for target in &workspace.targets.targets {
                    if ui
                        .add_enabled(
                            !mutation_running,
                            egui::Button::new(&target.id)
                                .selected(target.id == state.selected_provider_target),
                        )
                        .clicked()
                    {
                        actions.push(SessionAction::SetProviderTarget(target.id.clone()));
                    }
                }
            });
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::ComboBox,
                !mutation_running,
                &state.selected_provider_target,
            )
        });

        let mut auto_repair = workspace.auto_repair;
        let auto_response = ui.add_enabled(
            !mutation_running,
            egui::Checkbox::new(&mut auto_repair, text(locale, TextKey::AutomaticRepair)),
        );
        if auto_response.changed() {
            actions.push(SessionAction::SetAutoRepair(auto_repair));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let run = ui.add_enabled(
                !mutation_running && !state.selected_provider_target.trim().is_empty(),
                egui::Button::image_and_text(
                    egui::Image::new(icons::wrench()).fit_to_exact_size(egui::vec2(15.0, 15.0)),
                    text(locale, TextKey::RunProviderRepair),
                ),
            );
            if run.clicked() {
                actions.push(SessionAction::RunProviderRepair);
            }
            if mutation_running {
                ui.spinner();
                ui.label(text(locale, TextKey::InProgress));
            }
        });
    });
    render_provider_result(ui, state, locale);
}

fn render_delete_confirmation(
    ctx: &egui::Context,
    state: &SessionViewState,
    locale: Locale,
    actions: &mut Vec<SessionAction>,
) {
    let confirmation = state
        .delete_confirmation
        .as_ref()
        .expect("checked by caller");
    egui::Window::new(text(locale, TextKey::ConfirmSessionDeletion))
        .id(egui::Id::new("session_delete_confirmation"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.label(egui::RichText::new(delete_count_text(locale, confirmation.count())).strong());
            ui.add_space(6.0);
            for preview in &confirmation.previews {
                ui.label(format!("{}: {preview}", preview_prefix(locale)));
            }
            if confirmation.remaining_preview_count() > 0 {
                ui.label(
                    egui::RichText::new(more_sessions_text(
                        locale,
                        confirmation.remaining_preview_count(),
                    ))
                    .weak(),
                );
            }
            ui.add_space(6.0);
            ui.add_sized(
                [520.0, 48.0],
                egui::Label::new(text(locale, TextKey::DeleteBackupWarning)).wrap(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(text(locale, TextKey::Cancel)).clicked() {
                    actions.push(SessionAction::CancelDelete);
                }
                if ui.button(delete_now_text(locale)).clicked() {
                    actions.push(SessionAction::ConfirmDelete);
                }
            });
        });
}

fn render_provider_confirmation(
    ctx: &egui::Context,
    state: &SessionViewState,
    locale: Locale,
    actions: &mut Vec<SessionAction>,
) {
    let target = state
        .provider_run_confirmation
        .as_deref()
        .expect("checked by caller");
    egui::Window::new(text(locale, TextKey::ConfirmProviderRepair))
        .id(egui::Id::new("session_provider_repair_confirmation"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.label(provider_target_confirmation_text(locale, target));
            ui.add_space(6.0);
            ui.label(text(locale, TextKey::ProviderRepairWarning));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(text(locale, TextKey::Cancel)).clicked() {
                    actions.push(SessionAction::CancelProviderRepair);
                }
                if ui.button(repair_now_text(locale)).clicked() {
                    actions.push(SessionAction::ConfirmProviderRepair);
                }
            });
        });
}

fn render_delete_result(ui: &mut egui::Ui, state: &SessionViewState, locale: Locale) {
    if state.delete_phase == OperationPhase::Error {
        render_session_error(ui, state.delete_error, locale);
    }
    let Some(outcome) = &state.delete_outcome else {
        return;
    };
    let failed = outcome
        .outcomes
        .iter()
        .filter(|item| matches!(item.status, DeleteStatus::Failed | DeleteStatus::Partial))
        .count();
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(
            if failed == 0 {
                theme::SUCCESS_COLOR
            } else {
                theme::WARNING_COLOR
            },
            text(
                locale,
                if failed == 0 {
                    TextKey::SessionDeleteComplete
                } else {
                    TextKey::SessionDeletePartial
                },
            ),
        );
        if failed > 0 {
            ui.label(format!("{}: {failed}", text(locale, TextKey::FailureCount)));
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("session_delete_results")
        .max_height(DELETE_RESULT_HEIGHT - 26.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for item in &outcome.outcomes {
                ui.horizontal(|ui| {
                    let status = format!(
                        "{}: {}",
                        item.session_id,
                        delete_status_text(locale, &item.status)
                    );
                    ui.add_sized([220.0, 18.0], egui::Label::new(&status).truncate())
                        .on_hover_text(&status);

                    let backup = item
                        .backup_path
                        .as_deref()
                        .unwrap_or_else(|| no_backup_text(locale));
                    let evidence = format!("{}: {backup}", text(locale, TextKey::BackupEvidence));
                    ui.add(egui::Label::new(&evidence).truncate())
                        .on_hover_text(&evidence);
                });
            }
        });
}

fn render_provider_result(ui: &mut egui::Ui, state: &SessionViewState, locale: Locale) {
    if state.provider_run_phase == OperationPhase::Error {
        ui.colored_label(
            theme::ERROR_COLOR,
            provider_error_text(locale, state.provider_run_error),
        );
    }
    if state.auto_repair_phase == OperationPhase::Error {
        ui.colored_label(
            theme::ERROR_COLOR,
            provider_error_text(locale, state.auto_repair_error),
        );
    }
    let Some(outcome) = &state.provider_outcome else {
        return;
    };
    let color = if outcome.result.status == ProviderSyncStatus::Synced {
        theme::SUCCESS_COLOR
    } else {
        theme::WARNING_COLOR
    };
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(color, text(locale, TextKey::ProviderRepairComplete));
        ui.label(format!(
            "{}: {}",
            text(locale, TextKey::SessionFiles),
            outcome.result.changed_session_files
        ));
        ui.label(format!(
            "{}: {}",
            text(locale, TextKey::SqliteRows),
            outcome.result.sqlite_rows_updated
        ));
        if let Some(path) = &outcome.result.backup_dir {
            let path = path.to_string_lossy();
            ui.add(
                egui::Label::new(format!("{}: {path}", text(locale, TextKey::BackupEvidence)))
                    .truncate(),
            )
            .on_hover_text(path.as_ref());
        }
    });
}

fn render_session_error(ui: &mut egui::Ui, error: Option<SessionFailureKind>, locale: Locale) {
    let key = match error {
        Some(SessionFailureKind::WorkerStopped) => TextKey::SessionWorkerStopped,
        Some(SessionFailureKind::Service(SessionErrorKind::Conflict)) => TextKey::SessionConflict,
        Some(SessionFailureKind::Service(SessionErrorKind::ConfirmationMismatch)) => {
            TextKey::SessionConfirmationMismatch
        }
        Some(SessionFailureKind::Service(SessionErrorKind::DeleteFailed)) => {
            TextKey::SessionDeleteFailed
        }
        Some(SessionFailureKind::Service(SessionErrorKind::LoadFailed)) | None => {
            TextKey::SessionLoadFailed
        }
    };
    ui.colored_label(theme::ERROR_COLOR, text(locale, key));
}

fn provider_error_text(locale: Locale, error: Option<ProviderSyncFailureKind>) -> &'static str {
    text(
        locale,
        match error {
            Some(ProviderSyncFailureKind::WorkerStopped) => TextKey::SessionWorkerStopped,
            Some(ProviderSyncFailureKind::Service(ProviderSyncErrorKind::SettingsConflict)) => {
                TextKey::ProviderSyncConflict
            }
            Some(ProviderSyncFailureKind::Service(ProviderSyncErrorKind::ConfirmationMismatch)) => {
                TextKey::ProviderSyncConfirmationMismatch
            }
            Some(ProviderSyncFailureKind::Service(ProviderSyncErrorKind::SyncFailed)) => {
                TextKey::ProviderRepairFailed
            }
            Some(ProviderSyncFailureKind::Service(ProviderSyncErrorKind::LoadFailed)) | None => {
                TextKey::ProviderSyncLoadFailed
            }
        },
    )
}

fn labeled_icon_button(
    ui: &mut egui::Ui,
    icon: egui::ImageSource<'static>,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::image(egui::Image::new(icon).fit_to_exact_size(egui::vec2(15.0, 15.0))),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response.on_hover_text(label)
}

fn delete_count_text(locale: Locale, count: usize) -> String {
    match locale {
        Locale::ZhCn => format!("删除选中的 {count} 个会话？"),
        Locale::En => format!("Delete {count} selected sessions?"),
    }
}

fn more_sessions_text(locale: Locale, count: usize) -> String {
    match locale {
        Locale::ZhCn => format!("另有 {count} 个会话"),
        Locale::En => format!("{count} more sessions"),
    }
}

fn select_session_label(locale: Locale, id: &str) -> String {
    match locale {
        Locale::ZhCn => format!("选择会话 {id}"),
        Locale::En => format!("Select session {id}"),
    }
}

fn delete_now_text(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "立即删除",
        Locale::En => "Delete now",
    }
}

fn preview_prefix(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "预览",
        Locale::En => "Preview",
    }
}

fn provider_target_confirmation_text(locale: Locale, target: &str) -> String {
    match locale {
        Locale::ZhCn => format!("目标供应商: {target}"),
        Locale::En => format!("Target provider: {target}"),
    }
}

fn repair_now_text(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "立即修复",
        Locale::En => "Repair now",
    }
}

fn delete_status_text(locale: Locale, status: &DeleteStatus) -> &'static str {
    match (locale, status) {
        (Locale::ZhCn, DeleteStatus::ServerDeleted | DeleteStatus::LocalDeleted) => "已删除",
        (Locale::ZhCn, DeleteStatus::Partial) => "部分删除",
        (Locale::ZhCn, DeleteStatus::Failed) => "失败",
        (Locale::ZhCn, DeleteStatus::Undone) => "已恢复",
        (Locale::En, DeleteStatus::ServerDeleted | DeleteStatus::LocalDeleted) => "Deleted",
        (Locale::En, DeleteStatus::Partial) => "Partial",
        (Locale::En, DeleteStatus::Failed) => "Failed",
        (Locale::En, DeleteStatus::Undone) => "Restored",
    }
}

fn no_backup_text(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "无",
        Locale::En => "None",
    }
}
