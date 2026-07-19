use codex_plus_core::zed_remote::{ZedOpenStrategy, ZedRemoteProjectSource};
use codex_plus_manager_service::ZedRemoteErrorKind;
use eframe::egui;

use crate::i18n::{Locale, TextKey, text};
use crate::state::provider::OperationPhase;
use crate::state::zed_remote::{ZedRemoteFailureKind, ZedRemoteLoadPhase, ZedRemoteViewState};
use crate::{icons, theme};

#[derive(Clone, PartialEq, Eq)]
pub enum ZedRemoteAction {
    Refresh,
    SetSearch(String),
    SetRecentPage(usize),
    SetDiscoveredPage(usize),
    SetStrategy(ZedOpenStrategy),
    SetRegistryEnabled(bool),
    SavePreferences,
    RequestOpen {
        project_id: String,
        strategy: ZedOpenStrategy,
        remember: bool,
    },
    ConfirmOpen,
    CancelOpen,
    SetOpenStrategy(ZedOpenStrategy),
    SetOpenRemember(bool),
    CopyUrl(String),
    RequestForget(String),
    ConfirmForget,
    CancelForget,
}

impl std::fmt::Debug for ZedRemoteAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Refresh => "Refresh",
            Self::SetSearch(_) => "SetSearch",
            Self::SetRecentPage(_) => "SetRecentPage",
            Self::SetDiscoveredPage(_) => "SetDiscoveredPage",
            Self::SetStrategy(_) => "SetStrategy",
            Self::SetRegistryEnabled(_) => "SetRegistryEnabled",
            Self::SavePreferences => "SavePreferences",
            Self::RequestOpen { .. } => "RequestOpen",
            Self::ConfirmOpen => "ConfirmOpen",
            Self::CancelOpen => "CancelOpen",
            Self::SetOpenStrategy(_) => "SetOpenStrategy",
            Self::SetOpenRemember(_) => "SetOpenRemember",
            Self::CopyUrl(_) => "CopyUrl",
            Self::RequestForget(_) => "RequestForget",
            Self::ConfirmForget => "ConfirmForget",
            Self::CancelForget => "CancelForget",
        })
    }
}

pub fn render(
    ui: &mut egui::Ui,
    state: &ZedRemoteViewState,
    locale: Locale,
    actions: &mut Vec<ZedRemoteAction>,
) {
    render_toolbar(ui, state, locale, actions);
    ui.add_space(8.0);

    if state.workspace.is_none() && state.load_phase == ZedRemoteLoadPhase::Loading {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.spinner();
                ui.label(text(locale, TextKey::Loading));
            });
        });
        return;
    }
    if state.workspace.is_none() && state.load_phase == ZedRemoteLoadPhase::Error {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.colored_label(theme::ERROR_COLOR, error_text(locale, state.load_error));
                if ui.button(text(locale, TextKey::Retry)).clicked() {
                    actions.push(ZedRemoteAction::Refresh);
                }
            });
        });
        return;
    }

    let Some(workspace) = &state.workspace else {
        ui.centered_and_justified(|ui| ui.label(text(locale, TextKey::ZedNoProjects)));
        return;
    };

    ui.horizontal_wrapped(|ui| {
        let availability = workspace.availability;
        let available =
            availability.platform_supported && (availability.cli_found || availability.app_found);
        ui.colored_label(
            if available {
                theme::SUCCESS_COLOR
            } else {
                theme::WARNING_COLOR
            },
            if available {
                format!(
                    "{}: {}",
                    text(locale, TextKey::Status),
                    text(locale, TextKey::Ready)
                )
            } else {
                text(locale, TextKey::ZedUnavailable).to_owned()
            },
        );
        ui.label(format!(
            "CLI: {} / App: {}",
            yes_no(locale, availability.cli_found),
            yes_no(locale, availability.app_found)
        ));
        if state.load_phase == ZedRemoteLoadPhase::Refreshing {
            ui.spinner();
            ui.label(text(locale, TextKey::Refreshing));
        }
    });
    ui.add_space(6.0);
    render_operation_error(ui, state, locale);

    let list_height = (ui.available_height() - 12.0).max(180.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), list_height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            egui::ScrollArea::vertical()
                .id_salt("zed_remote_projects")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    render_group(
                        ui,
                        state,
                        locale,
                        actions,
                        ZedRemoteProjectSource::CurrentThread,
                        TextKey::ZedCurrentProject,
                    );
                    render_group(
                        ui,
                        state,
                        locale,
                        actions,
                        ZedRemoteProjectSource::Recent,
                        TextKey::ZedRecentProjects,
                    );
                    render_group(
                        ui,
                        state,
                        locale,
                        actions,
                        ZedRemoteProjectSource::SqliteThreadCwd,
                        TextKey::ZedDiscoveredProjects,
                    );
                });
        },
    );

    if state.pending_open.is_some() {
        render_open_confirmation(ui.ctx(), state, locale, actions);
    }
    if state.pending_forget.is_some() {
        render_forget_confirmation(ui.ctx(), state, locale, actions);
    }
}

fn render_toolbar(
    ui: &mut egui::Ui,
    state: &ZedRemoteViewState,
    locale: Locale,
    actions: &mut Vec<ZedRemoteAction>,
) {
    let mutation_running = state.mutation_running();
    ui.horizontal(|ui| {
        ui.label(text(locale, TextKey::ZedSearchProjects));
        let mut query = state.search_query.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut query)
                    .desired_width(250.0)
                    .hint_text(text(locale, TextKey::ZedSearchProjects)),
            )
            .changed()
        {
            actions.push(ZedRemoteAction::SetSearch(query));
        }
        let refresh = ui.add_enabled(
            state.load_phase != ZedRemoteLoadPhase::Loading,
            egui::Button::image(
                egui::Image::new(icons::refresh_cw()).fit_to_exact_size(egui::vec2(15.0, 15.0)),
            ),
        );
        refresh.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                state.load_phase != ZedRemoteLoadPhase::Loading,
                text(locale, TextKey::Refresh),
            )
        });
        if refresh.clicked() {
            actions.push(ZedRemoteAction::Refresh);
        }
        refresh.on_hover_text(text(locale, TextKey::Refresh));
    });
    ui.add_space(5.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(text(locale, TextKey::ZedOpenStrategy));
        let mut strategy = state.draft_strategy;
        ui.add_enabled_ui(!mutation_running, |ui| {
            egui::ComboBox::from_id_salt("zed_remote_strategy")
                .selected_text(strategy_label(locale, strategy))
                .show_ui(ui, |ui| {
                    for candidate in strategy_options() {
                        if ui
                            .selectable_value(
                                &mut strategy,
                                candidate,
                                strategy_label(locale, candidate),
                            )
                            .changed()
                        {
                            actions.push(ZedRemoteAction::SetStrategy(strategy));
                        }
                    }
                });
        });
        let mut enabled = state.draft_registry_enabled;
        if ui
            .add_enabled(
                !mutation_running,
                egui::Checkbox::new(&mut enabled, text(locale, TextKey::ZedRegistry)),
            )
            .changed()
        {
            actions.push(ZedRemoteAction::SetRegistryEnabled(enabled));
        }
        let save_enabled = state.preferences_dirty && !mutation_running;
        let save = ui.add_enabled(
            save_enabled,
            egui::Button::image(
                egui::Image::new(icons::save()).fit_to_exact_size(egui::vec2(15.0, 15.0)),
            ),
        );
        save.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                save_enabled,
                text(locale, TextKey::ZedSavePreferences),
            )
        });
        if save
            .on_hover_text(text(locale, TextKey::ZedSavePreferences))
            .clicked()
        {
            actions.push(ZedRemoteAction::SavePreferences);
        }
        if state.save_phase == OperationPhase::Running {
            ui.spinner();
        }
    });
}

fn render_group(
    ui: &mut egui::Ui,
    state: &ZedRemoteViewState,
    locale: Locale,
    actions: &mut Vec<ZedRemoteAction>,
    source: ZedRemoteProjectSource,
    title: TextKey,
) {
    let Some(workspace) = &state.workspace else {
        return;
    };
    let ids = state.visible_project_ids(source);
    let page_count = state.page_count(source);
    let page = match source {
        ZedRemoteProjectSource::Recent => state.recent_page,
        ZedRemoteProjectSource::CurrentThread => 0,
        _ => state.discovered_page,
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(text(locale, title)).strong().size(13.0));
        if page_count > 1 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let next = ui.add_enabled(
                    page + 1 < page_count,
                    egui::Button::new(text(locale, TextKey::NextPage)),
                );
                if next.clicked() {
                    push_page_action(actions, source, page + 1);
                }
                ui.label(format!(
                    "{} {}/{}",
                    text(locale, TextKey::Page),
                    page + 1,
                    page_count
                ));
                let previous = ui.add_enabled(
                    page > 0,
                    egui::Button::new(text(locale, TextKey::PreviousPage)),
                );
                if previous.clicked() {
                    push_page_action(actions, source, page.saturating_sub(1));
                }
            });
        }
    });
    ui.separator();
    for id in &ids {
        if let Some(project) = workspace.projects.iter().find(|project| &project.id == id) {
            render_project_row(ui, state, locale, actions, project);
        }
    }
    if ids.is_empty() {
        ui.label(egui::RichText::new(text(locale, TextKey::ZedNoProjects)).weak());
    }
    ui.add_space(10.0);
}

fn push_page_action(
    actions: &mut Vec<ZedRemoteAction>,
    source: ZedRemoteProjectSource,
    page: usize,
) {
    if source == ZedRemoteProjectSource::Recent {
        actions.push(ZedRemoteAction::SetRecentPage(page));
    } else {
        actions.push(ZedRemoteAction::SetDiscoveredPage(page));
    }
}

fn render_project_row(
    ui: &mut egui::Ui,
    state: &ZedRemoteViewState,
    locale: Locale,
    actions: &mut Vec<ZedRemoteAction>,
    project: &codex_plus_manager_service::ZedRemoteProjectSummary,
) {
    const ACTION_WIDTH: f32 = 128.0;
    const ROW_HEIGHT: f32 = 78.0;
    let available = state.availability().is_some_and(|availability| {
        availability.platform_supported && (availability.cli_found || availability.app_found)
    });
    let mutate_enabled = !state.mutation_running();
    let details_width = (ui.available_width() - ACTION_WIDTH - 12.0).max(180.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(details_width, ROW_HEIGHT),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&project.label).strong()).truncate(),
                    )
                    .on_hover_text(&project.label);
                    let target = format!("{}  {}", ssh_authority(project), project.remote_path);
                    ui.add(
                        egui::Label::new(egui::RichText::new(&target).weak().size(11.0)).truncate(),
                    )
                    .on_hover_text(&target);
                    let metadata =
                        format!("{}  {}", source_label(locale, project.source), project.url);
                    ui.add(
                        egui::Label::new(egui::RichText::new(&metadata).weak().size(11.0))
                            .truncate(),
                    )
                    .on_hover_text(&metadata);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(ACTION_WIDTH, ROW_HEIGHT),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if project.source == ZedRemoteProjectSource::Recent
                        && icon_command_button(
                            ui,
                            icons::trash_2(),
                            text(locale, TextKey::ZedForget),
                            mutate_enabled,
                        )
                        .clicked()
                    {
                        actions.push(ZedRemoteAction::RequestForget(project.id.clone()));
                    }
                    if icon_command_button(
                        ui,
                        icons::copy(),
                        text(locale, TextKey::ZedCopyUrl),
                        true,
                    )
                    .clicked()
                    {
                        actions.push(ZedRemoteAction::CopyUrl(project.id.clone()));
                    }
                    if icon_command_button(
                        ui,
                        icons::folder_git_2(),
                        text(locale, TextKey::ZedOpen),
                        mutate_enabled && available,
                    )
                    .clicked()
                    {
                        actions.push(ZedRemoteAction::RequestOpen {
                            project_id: project.id.clone(),
                            strategy: state.draft_strategy,
                            remember: state.draft_registry_enabled,
                        });
                    }
                },
            );
        },
    );
    ui.separator();
}

fn render_open_confirmation(
    ctx: &egui::Context,
    state: &ZedRemoteViewState,
    locale: Locale,
    actions: &mut Vec<ZedRemoteAction>,
) {
    let confirmation = state.pending_open.as_ref().expect("checked by caller");
    let available = state.availability().is_some_and(|availability| {
        availability.platform_supported && (availability.cli_found || availability.app_found)
    });
    let mutation_running = state.mutation_running();
    egui::Window::new(text(locale, TextKey::ZedOpenConfirmation))
        .id(egui::Id::new("zed_remote_open_confirmation"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.add_sized(
                [460.0, 22.0],
                egui::Label::new(egui::RichText::new(&confirmation.label).strong()).truncate(),
            )
            .on_hover_text(&confirmation.label);
            ui.add(egui::Label::new(&confirmation.authority).truncate())
                .on_hover_text(&confirmation.authority);
            ui.add(egui::Label::new(&confirmation.remote_path).truncate())
                .on_hover_text(&confirmation.remote_path);
            ui.add_space(6.0);
            ui.label(text(locale, TextKey::ZedOpenStrategy));
            let mut strategy = confirmation.strategy;
            ui.add_enabled_ui(!mutation_running, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for candidate in strategy_options() {
                        if ui
                            .selectable_value(
                                &mut strategy,
                                candidate,
                                strategy_label(locale, candidate),
                            )
                            .changed()
                        {
                            actions.push(ZedRemoteAction::SetOpenStrategy(strategy));
                        }
                    }
                });
            });
            let mut remember = confirmation.remember;
            if ui
                .add_enabled(
                    !mutation_running && state.draft_registry_enabled,
                    egui::Checkbox::new(&mut remember, text(locale, TextKey::ZedRemember)),
                )
                .changed()
            {
                actions.push(ZedRemoteAction::SetOpenRemember(remember));
            }
            if !available {
                ui.colored_label(theme::WARNING_COLOR, text(locale, TextKey::ZedUnavailable));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !mutation_running,
                        egui::Button::new(text(locale, TextKey::Cancel)),
                    )
                    .clicked()
                {
                    actions.push(ZedRemoteAction::CancelOpen);
                }
                if ui
                    .add_enabled(
                        available && !mutation_running,
                        egui::Button::new(text(locale, TextKey::ZedOpenNow)),
                    )
                    .clicked()
                {
                    actions.push(ZedRemoteAction::ConfirmOpen);
                }
                if mutation_running {
                    ui.spinner();
                }
            });
        });
}

fn render_forget_confirmation(
    ctx: &egui::Context,
    state: &ZedRemoteViewState,
    locale: Locale,
    actions: &mut Vec<ZedRemoteAction>,
) {
    let confirmation = state.pending_forget.as_ref().expect("checked by caller");
    let mutation_running = state.mutation_running();
    egui::Window::new(text(locale, TextKey::ZedForgetConfirmation))
        .id(egui::Id::new("zed_remote_forget_confirmation"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_min_width(440.0);
            ui.add_sized(
                [440.0, 22.0],
                egui::Label::new(egui::RichText::new(&confirmation.label).strong()).truncate(),
            )
            .on_hover_text(&confirmation.label);
            ui.add(egui::Label::new(&confirmation.authority).truncate())
                .on_hover_text(&confirmation.authority);
            ui.add(egui::Label::new(&confirmation.remote_path).truncate())
                .on_hover_text(&confirmation.remote_path);
            ui.label(egui::RichText::new(forget_scope_text(locale)).weak());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !mutation_running,
                        egui::Button::new(text(locale, TextKey::Cancel)),
                    )
                    .clicked()
                {
                    actions.push(ZedRemoteAction::CancelForget);
                }
                if ui
                    .add_enabled(
                        !mutation_running,
                        egui::Button::new(text(locale, TextKey::ZedForgetNow)),
                    )
                    .clicked()
                {
                    actions.push(ZedRemoteAction::ConfirmForget);
                }
                if mutation_running {
                    ui.spinner();
                }
            });
        });
}

fn render_operation_error(ui: &mut egui::Ui, state: &ZedRemoteViewState, locale: Locale) {
    if state.save_phase == OperationPhase::Error {
        ui.colored_label(theme::ERROR_COLOR, error_text(locale, state.save_error));
    }
    if state.open_phase == OperationPhase::Error {
        ui.colored_label(theme::ERROR_COLOR, error_text(locale, state.open_error));
    }
    if state.forget_phase == OperationPhase::Error {
        ui.colored_label(theme::ERROR_COLOR, error_text(locale, state.forget_error));
    }
    if let Some(outcome) = &state.open_outcome {
        ui.colored_label(
            theme::SUCCESS_COLOR,
            text(locale, TextKey::ZedLaunchSucceeded),
        );
        if matches!(
            outcome.remember,
            codex_plus_manager_service::ZedRememberOutcome::Failed(_)
        ) {
            ui.colored_label(
                theme::WARNING_COLOR,
                text(locale, TextKey::ZedRememberFailed),
            );
        }
    }
}

fn icon_command_button(
    ui: &mut egui::Ui,
    icon: egui::ImageSource<'static>,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::image(egui::Image::new(icon).fit_to_exact_size(egui::vec2(16.0, 16.0))),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response.on_hover_text(label)
}

fn ssh_authority(project: &codex_plus_manager_service::ZedRemoteProjectSummary) -> String {
    let host = if project.ssh.host.contains(':') && !project.ssh.host.starts_with('[') {
        format!("[{}]", project.ssh.host)
    } else {
        project.ssh.host.clone()
    };
    match project.ssh.port {
        Some(port) => format!("{}@{}:{}", project.ssh.user, host, port),
        None => format!("{}@{}", project.ssh.user, host),
    }
}

fn source_label(locale: Locale, source: ZedRemoteProjectSource) -> &'static str {
    match (locale, source) {
        (Locale::ZhCn, ZedRemoteProjectSource::CurrentThread) => "当前线程",
        (Locale::ZhCn, ZedRemoteProjectSource::CodexRemoteProject) => "Codex 远程项目",
        (Locale::ZhCn, ZedRemoteProjectSource::ThreadWorkspaceHint) => "线程工作区",
        (Locale::ZhCn, ZedRemoteProjectSource::SqliteThreadCwd) => "会话数据库",
        (Locale::ZhCn, ZedRemoteProjectSource::Recent) => "最近记录",
        (Locale::En, ZedRemoteProjectSource::CurrentThread) => "Current thread",
        (Locale::En, ZedRemoteProjectSource::CodexRemoteProject) => "Codex remote project",
        (Locale::En, ZedRemoteProjectSource::ThreadWorkspaceHint) => "Thread workspace",
        (Locale::En, ZedRemoteProjectSource::SqliteThreadCwd) => "Session database",
        (Locale::En, ZedRemoteProjectSource::Recent) => "Recent record",
    }
}

fn forget_scope_text(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "仅移除 Codex++ 最近项目记录，不会删除远程文件。",
        Locale::En => "Only the Codex++ recent record is removed; remote files are unchanged.",
    }
}

fn strategy_options() -> [ZedOpenStrategy; 4] {
    [
        ZedOpenStrategy::Default,
        ZedOpenStrategy::AddToFocusedWorkspace,
        ZedOpenStrategy::ReuseWindow,
        ZedOpenStrategy::NewWindow,
    ]
}

fn strategy_label(locale: Locale, strategy: ZedOpenStrategy) -> &'static str {
    match (locale, strategy) {
        (Locale::ZhCn, ZedOpenStrategy::Default) => "默认",
        (Locale::ZhCn, ZedOpenStrategy::AddToFocusedWorkspace) => "加入当前工作区",
        (Locale::ZhCn, ZedOpenStrategy::ReuseWindow) => "复用窗口",
        (Locale::ZhCn, ZedOpenStrategy::NewWindow) => "新窗口",
        (Locale::En, ZedOpenStrategy::Default) => "Default",
        (Locale::En, ZedOpenStrategy::AddToFocusedWorkspace) => "Add to focused workspace",
        (Locale::En, ZedOpenStrategy::ReuseWindow) => "Reuse window",
        (Locale::En, ZedOpenStrategy::NewWindow) => "New window",
    }
}

fn yes_no(locale: Locale, value: bool) -> &'static str {
    match (locale, value) {
        (Locale::ZhCn, true) => "是",
        (Locale::ZhCn, false) => "否",
        (Locale::En, true) => "yes",
        (Locale::En, false) => "no",
    }
}

fn error_text(locale: Locale, error: Option<ZedRemoteFailureKind>) -> &'static str {
    match error {
        Some(ZedRemoteFailureKind::WorkerStopped) => text(locale, TextKey::ZedWorkerStopped),
        Some(ZedRemoteFailureKind::Service(ZedRemoteErrorKind::SettingsConflict)) => {
            text(locale, TextKey::ZedSettingsConflict)
        }
        Some(ZedRemoteFailureKind::Service(ZedRemoteErrorKind::RegistryConflict)) => {
            text(locale, TextKey::ZedRegistryConflict)
        }
        Some(ZedRemoteFailureKind::Service(ZedRemoteErrorKind::ProjectConflict)) => {
            text(locale, TextKey::ZedProjectConflict)
        }
        Some(ZedRemoteFailureKind::Service(ZedRemoteErrorKind::ZedUnavailable)) => {
            text(locale, TextKey::ZedUnavailable)
        }
        Some(ZedRemoteFailureKind::Service(ZedRemoteErrorKind::LaunchFailed)) => {
            text(locale, TextKey::ZedLaunchFailed)
        }
        Some(ZedRemoteFailureKind::Service(_)) | None => text(locale, TextKey::ZedLoadFailed),
    }
}
