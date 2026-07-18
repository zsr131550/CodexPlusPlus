use std::fmt;

use codex_plus_manager_service::{
    ContextEntryKey, ContextEntryLiveState, ContextEntrySummary, ContextKind,
    ContextOwnershipOutcome, ContextToolsErrorKind,
};
use eframe::egui;

use crate::i18n::{Locale, TextKey, text};
use crate::state::context::{ContextEditorMode, ContextFailureKind, ContextViewState};
use crate::state::provider::OperationPhase;
use crate::{icons, theme};

#[derive(Clone, PartialEq, Eq)]
pub enum ContextAction {
    RetryWorkspace,
    SelectKind(ContextKind),
    OpenCreate(ContextKind),
    OpenEdit(ContextEntryKey),
    SetEditorId(String),
    SetEditorBody(String),
    SetTomlRevealed(bool),
    CancelEditor,
    SaveEditor,
    SetEnabled { key: ContextEntryKey, enabled: bool },
    RequestDelete(ContextEntryKey),
    CancelDelete,
    ConfirmDelete,
    PreviewSync,
    CancelSyncPreview,
    ConfirmSync,
}

impl fmt::Debug for ContextAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RetryWorkspace => "RetryWorkspace",
            Self::SelectKind(_) => "SelectKind",
            Self::OpenCreate(_) => "OpenCreate",
            Self::OpenEdit(_) => "OpenEdit",
            Self::SetEditorId(_) => "SetEditorId",
            Self::SetEditorBody(_) => "SetEditorBody",
            Self::SetTomlRevealed(_) => "SetTomlRevealed",
            Self::CancelEditor => "CancelEditor",
            Self::SaveEditor => "SaveEditor",
            Self::SetEnabled { .. } => "SetEnabled",
            Self::RequestDelete(_) => "RequestDelete",
            Self::CancelDelete => "CancelDelete",
            Self::ConfirmDelete => "ConfirmDelete",
            Self::PreviewSync => "PreviewSync",
            Self::CancelSyncPreview => "CancelSyncPreview",
            Self::ConfirmSync => "ConfirmSync",
        })
    }
}

pub fn render(
    ui: &mut egui::Ui,
    state: &ContextViewState,
    provider_dirty: bool,
    locale: Locale,
    actions: &mut Vec<ContextAction>,
) {
    if state.bundle.is_none() && state.workspace_phase == OperationPhase::Running {
        ui.centered_and_justified(|ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text(locale, TextKey::Loading));
            });
        });
        return;
    }
    if state.bundle.is_none() && state.workspace_phase == OperationPhase::Error {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.colored_label(
                    theme::ERROR_COLOR,
                    context_error_text(locale, state.workspace_error),
                );
                if ui.button(text(locale, TextKey::Retry)).clicked() {
                    actions.push(ContextAction::RetryWorkspace);
                }
            });
        });
        return;
    }

    let Some(bundle) = state.bundle.as_ref() else {
        return;
    };
    let workspace = &bundle.context;
    render_workspace_header(ui, state, provider_dirty, locale, actions);
    ui.add_space(8.0);

    render_kind_tabs(ui, state, locale, actions);
    ui.separator();

    let available_for_rows = (ui.available_height() - 64.0).max(112.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), available_for_rows),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            egui::ScrollArea::vertical()
                .id_salt("context_tools_entries")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut matching = workspace
                        .entries
                        .iter()
                        .filter(|entry| entry.key.kind == state.selected_kind)
                        .peekable();
                    if matching.peek().is_none() {
                        ui.add_sized(
                            [ui.available_width(), 44.0],
                            egui::Label::new(
                                egui::RichText::new(text(locale, TextKey::NoContextEntries)).weak(),
                            ),
                        );
                    } else {
                        for entry in matching {
                            render_entry_row(
                                ui,
                                entry,
                                provider_dirty || context_operation_running(state),
                                locale,
                                actions,
                            );
                        }
                    }
                });
        },
    );

    ui.separator();
    render_footer(ui, state, provider_dirty, locale, actions);
    render_editor(ui.ctx(), state, provider_dirty, locale, actions);
    render_delete_confirmation(ui.ctx(), state, provider_dirty, locale, actions);
    render_sync_preview(ui.ctx(), state, provider_dirty, locale, actions);
}

fn render_workspace_header(
    ui: &mut egui::Ui,
    state: &ContextViewState,
    provider_dirty: bool,
    locale: Locale,
    actions: &mut Vec<ContextAction>,
) {
    let workspace = &state
        .bundle
        .as_ref()
        .expect("loaded context bundle")
        .context;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 36.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let provider = workspace
                .active_provider_name
                .as_deref()
                .or(workspace.active_provider_id.as_deref());
            ui.label(
                egui::RichText::new(match provider {
                    Some(provider) => {
                        format!("{}: {provider}", text(locale, TextKey::ActiveProvider))
                    }
                    None => text(locale, TextKey::NoActiveProvider).to_owned(),
                })
                .strong(),
            );
            ui.separator();
            ui.colored_label(
                if workspace.sync_needed {
                    theme::WARNING_COLOR
                } else {
                    theme::SUCCESS_COLOR
                },
                text(
                    locale,
                    if workspace.sync_needed {
                        TextKey::LiveSyncNeeded
                    } else {
                        TextKey::LiveUpToDate
                    },
                ),
            );
            if state.workspace_phase == OperationPhase::Running {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = text(locale, TextKey::SyncContext);
                let enabled = !provider_dirty
                    && workspace.active_provider_id.is_some()
                    && workspace.sync_needed
                    && !context_operation_running(state)
                    && state.editor.is_none();
                let response = ui.add_enabled(
                    enabled,
                    egui::Button::image(
                        egui::Image::new(icons::save()).fit_to_exact_size(egui::vec2(17.0, 17.0)),
                    ),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label)
                });
                if response.on_hover_text(label).clicked() {
                    actions.push(ContextAction::PreviewSync);
                }
            });
        },
    );
    if provider_dirty {
        ui.colored_label(
            theme::WARNING_COLOR,
            text(locale, TextKey::ProviderDraftDirty),
        );
    }
}

fn render_kind_tabs(
    ui: &mut egui::Ui,
    state: &ContextViewState,
    locale: Locale,
    actions: &mut Vec<ContextAction>,
) {
    let entries = &state
        .bundle
        .as_ref()
        .expect("loaded context bundle")
        .context
        .entries;
    ui.horizontal(|ui| {
        for kind in [ContextKind::Mcp, ContextKind::Skill, ContextKind::Plugin] {
            let count = entries
                .iter()
                .filter(|entry| entry.key.kind == kind)
                .count();
            let label = format!("{} ({count})", kind_text(locale, kind));
            let width = (ui.available_width() - ui.spacing().item_spacing.x * 2.0) / 3.0;
            if ui
                .add_sized(
                    [width.max(92.0), 32.0],
                    egui::Button::new(label).selected(state.selected_kind == kind),
                )
                .clicked()
            {
                actions.push(ContextAction::SelectKind(kind));
            }
        }
    });
}

fn render_entry_row(
    ui: &mut egui::Ui,
    entry: &ContextEntrySummary,
    disabled: bool,
    locale: Locale,
    actions: &mut Vec<ContextAction>,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 44.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let id_response = ui.add_sized(
                [ui.available_width().min(260.0), 28.0],
                egui::Label::new(egui::RichText::new(&entry.display_name).strong()).truncate(),
            );
            id_response.on_hover_text(&entry.display_name);
            ui.colored_label(
                live_state_color(entry.live_state),
                live_state_text(locale, entry.live_state),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let delete_label = entry_action_label(locale, "delete", &entry.key);
                if labeled_icon_button(ui, icons::trash_2(), &delete_label, !disabled).clicked() {
                    actions.push(ContextAction::RequestDelete(entry.key.clone()));
                }
                let edit_label = entry_action_label(locale, "edit", &entry.key);
                if labeled_icon_button(ui, icons::pencil(), &edit_label, !disabled).clicked() {
                    actions.push(ContextAction::OpenEdit(entry.key.clone()));
                }
                let mut enabled = entry.enabled;
                let toggle_action = if entry.enabled { "disable" } else { "enable" };
                let enable_label = format!(
                    "{} {} {}",
                    localized_action_word(locale, toggle_action),
                    kind_noun(locale, entry.key.kind),
                    entry.key.id
                );
                let response = ui.add_enabled(!disabled, egui::Checkbox::new(&mut enabled, ""));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Checkbox, !disabled, &enable_label)
                });
                if response.on_hover_text(&enable_label).changed() {
                    actions.push(ContextAction::SetEnabled {
                        key: entry.key.clone(),
                        enabled,
                    });
                }
            });
        },
    );
    ui.separator();
}

fn render_footer(
    ui: &mut egui::Ui,
    state: &ContextViewState,
    provider_dirty: bool,
    locale: Locale,
    actions: &mut Vec<ContextAction>,
) {
    let workspace = &state
        .bundle
        .as_ref()
        .expect("loaded context bundle")
        .context;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 36.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(format!(
                "{}: {}",
                text(locale, TextKey::UnmanagedLiveEntries),
                workspace.unmanaged_live_count
            ));
            render_operation_result(ui, state, locale);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = create_label(locale, state.selected_kind);
                if labeled_icon_button(
                    ui,
                    icons::plus(),
                    &label,
                    !provider_dirty && !context_operation_running(state) && state.editor.is_none(),
                )
                .clicked()
                {
                    actions.push(ContextAction::OpenCreate(state.selected_kind));
                }
            });
        },
    );
}

fn render_operation_result(ui: &mut egui::Ui, state: &ContextViewState, locale: Locale) {
    let error = if state.editor.is_some() || state.delete_confirmation.is_some() {
        None
    } else {
        state.mutation_error.or(state.draft_error)
    }
    .or_else(|| {
        (state.sync_preview.is_none())
            .then_some(state.preview_error.or(state.sync_error))
            .flatten()
    });
    if let Some(error) = error {
        ui.colored_label(theme::ERROR_COLOR, context_error_text(locale, Some(error)));
        return;
    }
    let Some(outcome) = state.sync_outcome.as_ref() else {
        return;
    };
    ui.colored_label(
        if outcome.ownership == ContextOwnershipOutcome::PartialFailure {
            theme::WARNING_COLOR
        } else {
            theme::SUCCESS_COLOR
        },
        text(
            locale,
            if outcome.ownership == ContextOwnershipOutcome::PartialFailure {
                TextKey::ContextSyncPartial
            } else if outcome.diff.added + outcome.diff.updated + outcome.diff.removed == 0 {
                TextKey::ContextNoChanges
            } else {
                TextKey::ContextSynced
            },
        ),
    );
    if let Some(path) = &outcome.backup_path {
        ui.add(egui::Label::new(path).truncate())
            .on_hover_text(path);
    }
}

fn render_editor(
    ctx: &egui::Context,
    state: &ContextViewState,
    provider_dirty: bool,
    locale: Locale,
    actions: &mut Vec<ContextAction>,
) {
    let Some(editor) = state.editor.as_ref() else {
        return;
    };
    let title = editor_title(locale, editor.mode, editor.kind);
    egui::Window::new(title)
        .id(egui::Id::new("context_entry_editor"))
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .min_width(420.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(format!(
                "{}: {}",
                text(locale, TextKey::ContextKindLabel),
                kind_noun(locale, editor.kind)
            ));
            let id_label = ui.label(text(locale, TextKey::ContextId));
            let mut id = editor.id.clone();
            let id_enabled = editor.mode == ContextEditorMode::Create
                && state.mutation_phase != OperationPhase::Running
                && !provider_dirty;
            let id_response = ui
                .add_enabled(
                    id_enabled,
                    egui::TextEdit::singleline(&mut id).desired_width(f32::INFINITY),
                )
                .labelled_by(id_label.id);
            if id_response.changed() {
                actions.push(ContextAction::SetEditorId(id));
            }

            let body_label_id = ui
                .horizontal(|ui| {
                    let body_label_id = ui.label(text(locale, TextKey::TomlBody)).id;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (icon, label) = if editor.toml_revealed {
                            (icons::eye_off(), text(locale, TextKey::HideToml))
                        } else {
                            (icons::eye(), text(locale, TextKey::RevealToml))
                        };
                        if labeled_icon_button(ui, icon, label, true).clicked() {
                            actions.push(ContextAction::SetTomlRevealed(!editor.toml_revealed));
                        }
                    });
                    body_label_id
                })
                .inner;
            if editor.toml_revealed {
                let mut body = editor.toml_body.clone();
                let response = ui
                    .add_enabled(
                        state.mutation_phase != OperationPhase::Running && !provider_dirty,
                        egui::TextEdit::multiline(&mut body)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(12)
                            .desired_width(f32::INFINITY),
                    )
                    .labelled_by(body_label_id);
                if response.changed() {
                    actions.push(ContextAction::SetEditorBody(body));
                }
            } else {
                let mut hidden = if editor.toml_body.is_empty() {
                    String::new()
                } else {
                    "********".to_owned()
                };
                ui.add_enabled(
                    false,
                    egui::TextEdit::multiline(&mut hidden)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(12)
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(body_label_id);
            }
            if let Some(error) = state.mutation_error.or(state.draft_error) {
                ui.colored_label(theme::ERROR_COLOR, context_error_text(locale, Some(error)));
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        state.mutation_phase != OperationPhase::Running,
                        egui::Button::new(text(locale, TextKey::Cancel)),
                    )
                    .clicked()
                {
                    actions.push(ContextAction::CancelEditor);
                }
                if ui
                    .add_enabled(
                        !provider_dirty
                            && state.mutation_phase != OperationPhase::Running
                            && !editor.id.trim().is_empty(),
                        egui::Button::new(text(locale, TextKey::SaveEntry)),
                    )
                    .clicked()
                {
                    actions.push(ContextAction::SaveEditor);
                }
                if state.mutation_phase == OperationPhase::Running {
                    ui.spinner();
                }
            });
        });
}

fn render_delete_confirmation(
    ctx: &egui::Context,
    state: &ContextViewState,
    provider_dirty: bool,
    locale: Locale,
    actions: &mut Vec<ContextAction>,
) {
    let Some(key) = state.delete_confirmation.as_ref() else {
        return;
    };
    egui::Window::new(text(locale, TextKey::DeleteContextEntry))
        .id(egui::Id::new("context_delete_confirmation"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(format!("{} / {}", kind_noun(locale, key.kind), key.id));
            if let Some(error) = state.mutation_error {
                ui.colored_label(theme::ERROR_COLOR, context_error_text(locale, Some(error)));
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        state.mutation_phase != OperationPhase::Running,
                        egui::Button::new(text(locale, TextKey::Cancel)),
                    )
                    .clicked()
                {
                    actions.push(ContextAction::CancelDelete);
                }
                if ui
                    .add_enabled(
                        !provider_dirty && state.mutation_phase != OperationPhase::Running,
                        egui::Button::new(text(locale, TextKey::ConfirmDelete)),
                    )
                    .clicked()
                {
                    actions.push(ContextAction::ConfirmDelete);
                }
            });
        });
}

fn render_sync_preview(
    ctx: &egui::Context,
    state: &ContextViewState,
    provider_dirty: bool,
    locale: Locale,
    actions: &mut Vec<ContextAction>,
) {
    if state.preview_phase == OperationPhase::Running && state.sync_preview.is_none() {
        egui::Window::new(text(locale, TextKey::PreviewLiveSync))
            .id(egui::Id::new("context_sync_preview_loading"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.spinner();
                ui.label(text(locale, TextKey::InProgress));
            });
        return;
    }
    let Some(preview) = state.sync_preview.as_ref() else {
        return;
    };
    egui::Window::new(text(locale, TextKey::PreviewLiveSync))
        .id(egui::Id::new("context_sync_preview"))
        .collapsible(false)
        .resizable(false)
        .default_width(480.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(format!(
                "{}: {}",
                text(locale, TextKey::ActiveProvider),
                preview.active_provider_id.as_deref().unwrap_or("-")
            ));
            for (key, count) in [
                (TextKey::Added, preview.diff.added),
                (TextKey::Updated, preview.diff.updated),
                (TextKey::Removed, preview.diff.removed),
                (TextKey::Unchanged, preview.diff.unchanged),
            ] {
                ui.label(format!("{}: {count}", text(locale, key)));
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("context_sync_preview_keys")
                .max_height(180.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (heading, keys) in [
                        (TextKey::Added, &preview.keys.added),
                        (TextKey::Updated, &preview.keys.updated),
                        (TextKey::Removed, &preview.keys.removed),
                        (TextKey::Unchanged, &preview.keys.unchanged),
                    ] {
                        if keys.is_empty() {
                            continue;
                        }
                        ui.label(egui::RichText::new(text(locale, heading)).strong());
                        for key in keys {
                            let label = format!("{} / {}", kind_noun(locale, key.kind), key.id);
                            ui.add(egui::Label::new(&label).truncate())
                                .on_hover_text(label);
                        }
                    }
                });
            if let Some(error) = state.sync_error.or(state.preview_error) {
                ui.colored_label(theme::ERROR_COLOR, context_error_text(locale, Some(error)));
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        state.sync_phase != OperationPhase::Running,
                        egui::Button::new(text(locale, TextKey::Cancel)),
                    )
                    .clicked()
                {
                    actions.push(ContextAction::CancelSyncPreview);
                }
                if ui
                    .add_enabled(
                        !provider_dirty && state.sync_phase != OperationPhase::Running,
                        egui::Button::new(text(locale, TextKey::ConfirmSync)),
                    )
                    .clicked()
                {
                    actions.push(ContextAction::ConfirmSync);
                }
                if state.sync_phase == OperationPhase::Running {
                    ui.spinner();
                }
            });
        });
}

fn labeled_icon_button(
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

fn context_operation_running(state: &ContextViewState) -> bool {
    state.draft_phase == OperationPhase::Running
        || state.mutation_phase == OperationPhase::Running
        || state.preview_phase == OperationPhase::Running
        || state.sync_phase == OperationPhase::Running
}

fn kind_text(locale: Locale, kind: ContextKind) -> &'static str {
    text(
        locale,
        match kind {
            ContextKind::Mcp => TextKey::McpServers,
            ContextKind::Skill => TextKey::Skills,
            ContextKind::Plugin => TextKey::Plugins,
        },
    )
}

fn live_state_text(locale: Locale, state: ContextEntryLiveState) -> &'static str {
    text(
        locale,
        match state {
            ContextEntryLiveState::StoredOnly => TextKey::StoredOnly,
            ContextEntryLiveState::Matching => TextKey::LiveMatching,
            ContextEntryLiveState::Different => TextKey::LiveDifferent,
            ContextEntryLiveState::PendingRemoval => TextKey::PendingRemoval,
        },
    )
}

fn live_state_color(state: ContextEntryLiveState) -> egui::Color32 {
    match state {
        ContextEntryLiveState::Matching => theme::SUCCESS_COLOR,
        ContextEntryLiveState::StoredOnly | ContextEntryLiveState::Different => {
            theme::WARNING_COLOR
        }
        ContextEntryLiveState::PendingRemoval => theme::ERROR_COLOR,
    }
}

fn create_label(locale: Locale, kind: ContextKind) -> String {
    match locale {
        Locale::ZhCn => chinese_editor_title(ContextEditorMode::Create, kind),
        Locale::En => format!("Create {} entry", kind_noun(locale, kind)),
    }
}

fn editor_title(locale: Locale, mode: ContextEditorMode, kind: ContextKind) -> String {
    match (locale, mode) {
        (Locale::ZhCn, mode) => chinese_editor_title(mode, kind),
        (Locale::En, ContextEditorMode::Create) => {
            format!("Create {} entry", kind_noun(locale, kind))
        }
        (Locale::En, ContextEditorMode::Edit) => {
            format!("Edit {} entry", kind_noun(locale, kind))
        }
    }
}

fn chinese_editor_title(mode: ContextEditorMode, kind: ContextKind) -> String {
    let action = match mode {
        ContextEditorMode::Create => "新建",
        ContextEditorMode::Edit => "编辑",
    };
    match kind {
        ContextKind::Mcp => format!("{action} MCP 条目"),
        ContextKind::Skill => format!("{action}技能条目"),
        ContextKind::Plugin => format!("{action}插件条目"),
    }
}

fn localized_action_word(locale: Locale, action: &str) -> &'static str {
    match (locale, action) {
        (Locale::ZhCn, "enable") => "启用",
        (Locale::ZhCn, "disable") => "禁用",
        (Locale::ZhCn, "edit") => "编辑",
        (Locale::ZhCn, "delete") => "删除",
        (Locale::En, "enable") => "Enable",
        (Locale::En, "disable") => "Disable",
        (Locale::En, "edit") => "Edit",
        (Locale::En, "delete") => "Delete",
        _ => text(locale, TextKey::ContextGenericFailure),
    }
}

fn entry_action_label(locale: Locale, action: &str, key: &ContextEntryKey) -> String {
    format!(
        "{} {} {}",
        localized_action_word(locale, action),
        kind_noun(locale, key.kind),
        key.id
    )
}

fn kind_noun(locale: Locale, kind: ContextKind) -> &'static str {
    match (locale, kind) {
        (_, ContextKind::Mcp) => "MCP",
        (Locale::ZhCn, ContextKind::Skill) => "技能",
        (Locale::ZhCn, ContextKind::Plugin) => "插件",
        (Locale::En, ContextKind::Skill) => "Skill",
        (Locale::En, ContextKind::Plugin) => "Plugin",
    }
}

fn context_error_text(locale: Locale, error: Option<ContextFailureKind>) -> &'static str {
    let key = match error {
        Some(ContextFailureKind::WorkerStopped) => TextKey::ContextWorkerStopped,
        Some(ContextFailureKind::Service(ContextToolsErrorKind::ProviderConflict)) => {
            TextKey::ContextProviderConflict
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::LiveConflict)) => {
            TextKey::ContextLiveConflict
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::OwnershipConflict)) => {
            TextKey::ContextOwnershipConflict
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::InvalidId)) => {
            TextKey::ContextInvalidId
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::InvalidToml)) => {
            TextKey::ContextInvalidToml
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::EntryNotFound)) => {
            TextKey::ContextEntryNotFound
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::EntryAlreadyExists)) => {
            TextKey::ContextEntryExists
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::ConfirmationMismatch)) => {
            TextKey::ContextConfirmationMismatch
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::ActiveProviderMissing)) => {
            TextKey::ContextNoActiveProvider
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::ActiveProviderInvalid)) => {
            TextKey::ContextActiveProviderInvalid
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::LockFailed)) => {
            TextKey::ContextLockFailed
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::SaveFailed)) => {
            TextKey::ContextSaveFailed
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::LiveWriteFailed)) => {
            TextKey::ContextLiveWriteFailed
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::OwnershipWriteFailed)) => {
            TextKey::ContextOwnershipWriteFailed
        }
        Some(ContextFailureKind::Service(ContextToolsErrorKind::LoadFailed)) | None => {
            TextKey::ContextLoadFailed
        }
    };
    text(locale, key)
}
