use codex_plus_manager_service::{
    ScriptIntegrity, UserScriptErrorKind, UserScriptOrigin, UserScriptStatus,
};
use eframe::egui;

use crate::external_url::ExternalUrl;
use crate::i18n::{Locale, TextKey, text};
use crate::state::provider::OperationPhase;
use crate::state::user_scripts::{
    LocalScriptFilter, MarketScriptFilter, ScriptsTab, UserScriptFailureKind,
    UserScriptMutationKind, UserScriptViewState,
};
use crate::{icons, theme};

const SCRIPT_ROW_HEIGHT: f32 = 66.0;
const RESULT_BAND_HEIGHT: f32 = 34.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserScriptAction {
    RefreshMarket,
    RefreshLocal,
    SetTab(ScriptsTab),
    SetMarketQuery(String),
    SetMarketFilter(MarketScriptFilter),
    SetMarketPage(usize),
    SetLocalQuery(String),
    SetLocalFilter(LocalScriptFilter),
    SetLocalPage(usize),
    RequestInstall(String),
    SetUnverifiedAcknowledgement(bool),
    CancelInstall,
    ConfirmInstall,
    SetGlobalEnabled(bool),
    SetScriptEnabled { key: String, enabled: bool },
    RequestDelete(String),
    CancelDelete,
    ConfirmDelete,
    Retry,
}

pub fn render(
    ui: &mut egui::Ui,
    state: &UserScriptViewState,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    render_tabs(ui, state, locale, actions);
    ui.add_space(6.0);

    match state.tab {
        ScriptsTab::Market => render_market(ui, state, locale, actions),
        ScriptsTab::Local => render_local(ui, state, locale, actions),
    }

    render_install_confirmation(ui.ctx(), state, locale, actions);
    render_delete_confirmation(ui.ctx(), state, locale, actions);
}

fn render_tabs(
    ui: &mut egui::Ui,
    state: &UserScriptViewState,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    ui.horizontal(|ui| {
        let width = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(120.0);
        for (tab, key) in [
            (ScriptsTab::Market, TextKey::ScriptMarket),
            (ScriptsTab::Local, TextKey::LocalScripts),
        ] {
            if ui
                .add_sized(
                    [width, 30.0],
                    egui::Button::new(text(locale, key)).selected(state.tab == tab),
                )
                .clicked()
            {
                actions.push(UserScriptAction::SetTab(tab));
            }
        }
    });
}

fn render_market(
    ui: &mut egui::Ui,
    state: &UserScriptViewState,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    render_market_toolbar(ui, state, locale, actions);
    render_read_error(
        ui,
        state.market_phase,
        state.market_error,
        TextKey::ScriptMarketLoadFailed,
        locale,
        actions,
    );

    let list_height = (ui.available_height() - RESULT_BAND_HEIGHT - 42.0).max(160.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), list_height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| render_market_list(ui, state, locale, actions),
    );
    render_page_footer(
        ui,
        state.market_page,
        state.market_page_count(),
        locale,
        UserScriptAction::SetMarketPage,
        actions,
    );
    render_mutation_result(ui, state, locale, actions);
}

fn render_market_toolbar(
    ui: &mut egui::Ui,
    state: &UserScriptViewState,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    ui.horizontal(|ui| {
        ui.label(text(locale, TextKey::SearchScripts));
        let mut query = state.market_query.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut query)
                    .desired_width(ui.available_width().min(230.0)),
            )
            .changed()
        {
            actions.push(UserScriptAction::SetMarketQuery(query));
        }

        let mut filter = state.market_filter;
        egui::ComboBox::from_id_salt("user_script_market_filter")
            .selected_text(market_filter_text(locale, filter))
            .width(128.0)
            .show_ui(ui, |ui| {
                for (value, key) in [
                    (MarketScriptFilter::All, TextKey::AllScripts),
                    (MarketScriptFilter::Available, TextKey::AvailableScripts),
                    (MarketScriptFilter::Installed, TextKey::InstalledScripts),
                    (MarketScriptFilter::Updates, TextKey::ScriptUpdates),
                    (MarketScriptFilter::Verified, TextKey::VerifiedScripts),
                ] {
                    ui.selectable_value(&mut filter, value, text(locale, key));
                }
            });
        if filter != state.market_filter {
            actions.push(UserScriptAction::SetMarketFilter(filter));
        }

        let refresh = labeled_icon_button(
            ui,
            icons::refresh_cw(),
            text(locale, TextKey::RefreshMarket),
            state.market_phase != OperationPhase::Running,
        );
        if refresh.clicked() {
            actions.push(UserScriptAction::RefreshMarket);
        }
        let repository =
            ExternalUrl::parse("https://github.com/BigPizzaV3/CodexPlusPlusScriptMarket")
                .expect("built-in script market URL is valid");
        if labeled_icon_button(
            ui,
            icons::folder_git_2(),
            script_link_text(locale, ScriptLinkText::MarketRepository),
            true,
        )
        .clicked()
        {
            repository.emit(ui.ctx());
        }
        if state.market_phase == OperationPhase::Running {
            ui.spinner();
        }
    });
}

fn render_market_list(
    ui: &mut egui::Ui,
    state: &UserScriptViewState,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    if state.market.is_none() && state.market_phase == OperationPhase::Running {
        ui.centered_and_justified(|ui| {
            ui.spinner();
            ui.label(text(locale, TextKey::Loading));
        });
        return;
    }

    let entries = state.market_page_entries();
    if entries.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(text(locale, TextKey::NoMarketScripts)).weak());
        });
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("user_script_market_scroll")
        .auto_shrink([false, false])
        .show_rows(ui, SCRIPT_ROW_HEIGHT, entries.len(), |ui, rows| {
            for entry in &entries[rows] {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), SCRIPT_ROW_HEIGHT - 1.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let metadata_width = 128.0;
                        let integrity_width = 122.0;
                        let action_width = 136.0;
                        let gaps = ui.spacing().item_spacing.x * 3.0;
                        let details_width = (ui.available_width()
                            - metadata_width
                            - integrity_width
                            - action_width
                            - gaps)
                            .max(180.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(details_width, 52.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_width(details_width);
                                ui.set_max_width(details_width);
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&entry.name).strong())
                                        .truncate(),
                                )
                                .on_hover_text(&entry.name);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&entry.description).weak().size(11.0),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&entry.description);
                            },
                        );

                        ui.allocate_ui_with_layout(
                            egui::vec2(metadata_width, 52.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_width(metadata_width);
                                ui.set_max_width(metadata_width);
                                ui.label(format!("v{} | {}", entry.version, entry.author));
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{}: {}",
                                        text(locale, TextKey::SourceHost),
                                        entry.source_host
                                    ))
                                    .weak()
                                    .size(11.0),
                                );
                            },
                        );

                        let (integrity_label, integrity_color) =
                            integrity_text(locale, entry.integrity);
                        ui.allocate_ui_with_layout(
                            egui::vec2(integrity_width, 28.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(integrity_width);
                                ui.set_max_width(integrity_width);
                                ui.colored_label(integrity_color, integrity_label);
                            },
                        );

                        ui.allocate_ui_with_layout(
                            egui::vec2(action_width, 32.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(action_width);
                                ui.set_max_width(action_width);
                                let installed = state.installed_version(&entry.id);
                                let update =
                                    installed.is_some_and(|version| version != entry.version);
                                let actionable = installed.is_none() || update;
                                let key = match installed {
                                    Some(_) if update => TextKey::UpdateScript,
                                    Some(_) => TextKey::Installed,
                                    None => TextKey::InstallScript,
                                };
                                let enabled = state.mutations_enabled()
                                    && actionable
                                    && entry.integrity != ScriptIntegrity::Invalid
                                    && state.local.is_some();
                                if ui
                                    .add_enabled(
                                        enabled,
                                        egui::Button::new(text(locale, key))
                                            .min_size(egui::vec2(96.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    actions
                                        .push(UserScriptAction::RequestInstall(entry.id.clone()));
                                }
                                if let Some(homepage) = entry
                                    .homepage
                                    .as_ref()
                                    .and_then(|value| ExternalUrl::parse(value.as_str()).ok())
                                    && labeled_icon_button(
                                        ui,
                                        icons::folder_git_2(),
                                        script_link_text(locale, ScriptLinkText::ProjectHomepage),
                                        true,
                                    )
                                    .clicked()
                                {
                                    homepage.emit(ui.ctx());
                                }
                            },
                        );
                    },
                );
                ui.separator();
            }
        });
}

fn render_local(
    ui: &mut egui::Ui,
    state: &UserScriptViewState,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    render_local_toolbar(ui, state, locale, actions);
    render_read_error(
        ui,
        state.local_phase,
        state.local_error,
        TextKey::ScriptLocalLoadFailed,
        locale,
        actions,
    );

    let list_height = (ui.available_height() - RESULT_BAND_HEIGHT - 42.0).max(160.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), list_height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| render_local_list(ui, state, locale, actions),
    );
    render_page_footer(
        ui,
        state.local_page,
        state.local_page_count(),
        locale,
        UserScriptAction::SetLocalPage,
        actions,
    );
    render_mutation_result(ui, state, locale, actions);
}

fn render_local_toolbar(
    ui: &mut egui::Ui,
    state: &UserScriptViewState,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    ui.horizontal(|ui| {
        let mut globally_enabled = state
            .local
            .as_ref()
            .is_some_and(|local| local.globally_enabled);
        let global_enabled = state.mutations_enabled() && state.local.is_some();
        let global = ui.add_enabled(
            global_enabled,
            egui::Checkbox::new(
                &mut globally_enabled,
                text(locale, TextKey::EnableAllScripts),
            ),
        );
        if global.changed() {
            actions.push(UserScriptAction::SetGlobalEnabled(globally_enabled));
        }

        ui.separator();
        ui.label(text(locale, TextKey::SearchScripts));
        let mut query = state.local_query.clone();
        if ui
            .add(egui::TextEdit::singleline(&mut query).desired_width(180.0))
            .changed()
        {
            actions.push(UserScriptAction::SetLocalQuery(query));
        }

        let mut filter = state.local_filter;
        egui::ComboBox::from_id_salt("user_script_local_filter")
            .selected_text(local_filter_text(locale, filter))
            .width(112.0)
            .show_ui(ui, |ui| {
                for (value, key) in [
                    (LocalScriptFilter::All, TextKey::AllScripts),
                    (LocalScriptFilter::Enabled, TextKey::EnabledScripts),
                    (LocalScriptFilter::Disabled, TextKey::DisabledScripts),
                    (LocalScriptFilter::Builtin, TextKey::BuiltinScripts),
                    (LocalScriptFilter::User, TextKey::UserScripts),
                ] {
                    ui.selectable_value(&mut filter, value, text(locale, key));
                }
            });
        if filter != state.local_filter {
            actions.push(UserScriptAction::SetLocalFilter(filter));
        }

        let refresh = labeled_icon_button(
            ui,
            icons::refresh_cw(),
            text(locale, TextKey::RefreshLocalScripts),
            state.local_phase != OperationPhase::Running,
        );
        if refresh.clicked() {
            actions.push(UserScriptAction::RefreshLocal);
        }
        if state.local_phase == OperationPhase::Running {
            ui.spinner();
        }
    });
}

fn render_local_list(
    ui: &mut egui::Ui,
    state: &UserScriptViewState,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    if state.local.is_none() && state.local_phase == OperationPhase::Running {
        ui.centered_and_justified(|ui| {
            ui.spinner();
            ui.label(text(locale, TextKey::Loading));
        });
        return;
    }

    let scripts = state.local_page_scripts();
    if scripts.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(text(locale, TextKey::NoLocalScripts)).weak());
        });
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("user_script_local_scroll")
        .auto_shrink([false, false])
        .show_rows(ui, SCRIPT_ROW_HEIGHT, scripts.len(), |ui, rows| {
            for script in &scripts[rows] {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), SCRIPT_ROW_HEIGHT - 1.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let mut enabled = script.enabled;
                        let toggle_enabled = state.mutations_enabled();
                        let toggle_label = script_toggle_label(locale, &script.name);
                        let response =
                            ui.add_enabled(toggle_enabled, egui::Checkbox::new(&mut enabled, ""));
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Checkbox,
                                toggle_enabled,
                                &toggle_label,
                            )
                        });
                        if response.on_hover_text(&toggle_label).changed() {
                            actions.push(UserScriptAction::SetScriptEnabled {
                                key: script.key.clone(),
                                enabled,
                            });
                        }

                        let origin_width = 88.0;
                        let version_width = 180.0;
                        let action_min_width = 40.0;
                        let gaps = ui.spacing().item_spacing.x * 3.0;
                        let details_width = (ui.available_width()
                            - origin_width
                            - version_width
                            - action_min_width
                            - gaps)
                            .clamp(180.0, 260.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(details_width, 52.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_width(details_width);
                                ui.set_max_width(details_width);
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&script.name).strong())
                                        .truncate(),
                                )
                                .on_hover_text(&script.name);
                                let status = if script.status == UserScriptStatus::Disabled {
                                    text(locale, TextKey::Disabled)
                                } else {
                                    text(locale, TextKey::Enabled)
                                };
                                ui.label(egui::RichText::new(status).weak().size(11.0));
                            },
                        );

                        let origin = match script.origin {
                            UserScriptOrigin::Builtin => text(locale, TextKey::BuiltinScript),
                            UserScriptOrigin::User => text(locale, TextKey::UserScript),
                        };
                        ui.allocate_ui_with_layout(
                            egui::vec2(origin_width, 24.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(origin_width);
                                ui.set_max_width(origin_width);
                                ui.label(origin);
                            },
                        );

                        ui.allocate_ui_with_layout(
                            egui::vec2(version_width, 24.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(version_width);
                                ui.set_max_width(version_width);
                                if let Some(version) = &script.version {
                                    ui.label(format!(
                                        "{}: {version}",
                                        text(locale, TextKey::InstalledVersion)
                                    ));
                                }
                            },
                        );

                        let action_width = ui.available_width().max(action_min_width);
                        ui.allocate_ui_with_layout(
                            egui::vec2(action_width, 32.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(action_width);
                                ui.set_max_width(action_width);
                                if script.origin == UserScriptOrigin::User {
                                    let delete = labeled_icon_button(
                                        ui,
                                        icons::trash_2(),
                                        text(locale, TextKey::DeleteUserScript),
                                        state.mutations_enabled(),
                                    );
                                    if delete.clicked() {
                                        actions.push(UserScriptAction::RequestDelete(
                                            script.key.clone(),
                                        ));
                                    }
                                }
                            },
                        );
                    },
                );
                ui.separator();
            }
        });
}

fn render_read_error(
    ui: &mut egui::Ui,
    phase: OperationPhase,
    error: Option<UserScriptFailureKind>,
    fallback: TextKey,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    if phase != OperationPhase::Error {
        return;
    }
    ui.horizontal(|ui| {
        ui.colored_label(theme::ERROR_COLOR, failure_text(locale, error, fallback));
        if ui.button(text(locale, TextKey::Retry)).clicked() {
            actions.push(UserScriptAction::Retry);
        }
    });
}

fn render_mutation_result(
    ui: &mut egui::Ui,
    state: &UserScriptViewState,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), RESULT_BAND_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| match state.mutation_phase {
            OperationPhase::Running => {
                ui.spinner();
                ui.label(text(locale, TextKey::InProgress));
            }
            OperationPhase::Error => {
                ui.colored_label(
                    theme::ERROR_COLOR,
                    failure_text(locale, state.mutation_error, TextKey::ScriptInstallFailed),
                );
                if ui.button(text(locale, TextKey::Retry)).clicked() {
                    actions.push(UserScriptAction::Retry);
                }
            }
            OperationPhase::Ready if state.mutation_outcome.is_some() => {
                let key = match state.mutation_kind {
                    Some(UserScriptMutationKind::Install) => TextKey::ScriptInstalled,
                    Some(UserScriptMutationKind::Update) => TextKey::ScriptUpdated,
                    Some(UserScriptMutationKind::Delete) => TextKey::ScriptDeleted,
                    Some(UserScriptMutationKind::SetGlobalEnabled)
                    | Some(UserScriptMutationKind::SetScriptEnabled)
                    | None => TextKey::ScriptSettingSaved,
                };
                ui.colored_label(theme::SUCCESS_COLOR, text(locale, key));
                if state
                    .mutation_outcome
                    .as_ref()
                    .is_some_and(|outcome| outcome.backup.created)
                {
                    ui.label(text(locale, TextKey::BackupCreated));
                }
            }
            OperationPhase::Idle | OperationPhase::Ready => {}
        },
    );
}

fn render_page_footer(
    ui: &mut egui::Ui,
    page: usize,
    page_count: usize,
    locale: Locale,
    make_action: impl Fn(usize) -> UserScriptAction,
    actions: &mut Vec<UserScriptAction>,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 34.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            let next = labeled_icon_button(
                ui,
                icons::chevron_down(),
                text(locale, TextKey::NextPage),
                page + 1 < page_count,
            );
            if next.clicked() {
                actions.push(make_action(page + 1));
            }
            ui.label(format!(
                "{} {} / {}",
                text(locale, TextKey::Page),
                page + 1,
                page_count
            ));
            let previous = labeled_icon_button(
                ui,
                icons::chevron_up(),
                text(locale, TextKey::PreviousPage),
                page > 0,
            );
            if previous.clicked() {
                actions.push(make_action(page.saturating_sub(1)));
            }
        },
    );
}

fn render_install_confirmation(
    ctx: &egui::Context,
    state: &UserScriptViewState,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    let Some(confirmation) = state.install_confirmation() else {
        return;
    };
    let title = text(
        locale,
        if confirmation.update {
            TextKey::UpdateScriptQuestion
        } else {
            TextKey::InstallScriptQuestion
        },
    );
    egui::Window::new(title)
        .id(egui::Id::new("user_script_install_confirmation"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_min_width(430.0);
            ui.label(
                egui::RichText::new(format!("{}  v{}", confirmation.name, confirmation.version))
                    .strong(),
            );
            let (integrity, color) = integrity_text(locale, confirmation.integrity);
            ui.colored_label(color, integrity);
            ui.label(format!(
                "{}: {}",
                text(locale, TextKey::SourceHost),
                confirmation.source_host
            ));
            ui.add_sized(
                [430.0, 46.0],
                egui::Label::new(text(locale, TextKey::ScriptDownloadBoundary)).wrap(),
            );

            if confirmation.integrity == ScriptIntegrity::Unverified {
                let mut acknowledged = confirmation.acknowledge_unverified();
                if ui
                    .checkbox(
                        &mut acknowledged,
                        text(locale, TextKey::AcknowledgeUnverified),
                    )
                    .changed()
                {
                    actions.push(UserScriptAction::SetUnverifiedAcknowledgement(acknowledged));
                }
            }

            ui.add_space(6.0);
            ui.allocate_ui_with_layout(
                egui::vec2(430.0, 30.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let confirm_key = if confirmation.update {
                        TextKey::UpdateScript
                    } else {
                        TextKey::InstallScript
                    };
                    let confirm_enabled = state.mutations_enabled()
                        && confirmation.integrity != ScriptIntegrity::Invalid
                        && (confirmation.integrity != ScriptIntegrity::Unverified
                            || confirmation.acknowledge_unverified());
                    if ui
                        .add_enabled(
                            confirm_enabled,
                            egui::Button::new(text(locale, confirm_key)),
                        )
                        .clicked()
                    {
                        actions.push(UserScriptAction::ConfirmInstall);
                    }
                    if ui.button(text(locale, TextKey::Cancel)).clicked() {
                        actions.push(UserScriptAction::CancelInstall);
                    }
                },
            );
        });
}

fn render_delete_confirmation(
    ctx: &egui::Context,
    state: &UserScriptViewState,
    locale: Locale,
    actions: &mut Vec<UserScriptAction>,
) {
    let Some(confirmation) = state.delete_confirmation() else {
        return;
    };
    egui::Window::new(text(locale, TextKey::ConfirmScriptDeletion))
        .id(egui::Id::new("user_script_delete_confirmation"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.label(egui::RichText::new(&confirmation.name).strong());
            ui.label(&confirmation.key);
            ui.add_sized(
                [420.0, 46.0],
                egui::Label::new(text(locale, TextKey::ScriptDeleteBackupWarning)).wrap(),
            );
            ui.allocate_ui_with_layout(
                egui::vec2(420.0, 30.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if ui
                        .add_enabled(
                            state.mutations_enabled(),
                            egui::Button::new(text(locale, TextKey::DeleteUserScript)),
                        )
                        .clicked()
                    {
                        actions.push(UserScriptAction::ConfirmDelete);
                    }
                    if ui.button(text(locale, TextKey::Cancel)).clicked() {
                        actions.push(UserScriptAction::CancelDelete);
                    }
                },
            );
        });
}

fn market_filter_text(locale: Locale, filter: MarketScriptFilter) -> &'static str {
    text(
        locale,
        match filter {
            MarketScriptFilter::All => TextKey::AllScripts,
            MarketScriptFilter::Available => TextKey::AvailableScripts,
            MarketScriptFilter::Installed => TextKey::InstalledScripts,
            MarketScriptFilter::Updates => TextKey::ScriptUpdates,
            MarketScriptFilter::Verified => TextKey::VerifiedScripts,
        },
    )
}

fn local_filter_text(locale: Locale, filter: LocalScriptFilter) -> &'static str {
    text(
        locale,
        match filter {
            LocalScriptFilter::All => TextKey::AllScripts,
            LocalScriptFilter::Enabled => TextKey::EnabledScripts,
            LocalScriptFilter::Disabled => TextKey::DisabledScripts,
            LocalScriptFilter::Builtin => TextKey::BuiltinScripts,
            LocalScriptFilter::User => TextKey::UserScripts,
        },
    )
}

fn integrity_text(locale: Locale, integrity: ScriptIntegrity) -> (&'static str, egui::Color32) {
    match integrity {
        ScriptIntegrity::Verified => (text(locale, TextKey::VerifiedScript), theme::SUCCESS_COLOR),
        ScriptIntegrity::Unverified => (
            text(locale, TextKey::UnverifiedScript),
            theme::WARNING_COLOR,
        ),
        ScriptIntegrity::Invalid => (text(locale, TextKey::InvalidScript), theme::ERROR_COLOR),
    }
}

fn failure_text(
    locale: Locale,
    failure: Option<UserScriptFailureKind>,
    fallback: TextKey,
) -> &'static str {
    let key = match failure {
        Some(UserScriptFailureKind::WorkerStopped)
        | Some(UserScriptFailureKind::Service(UserScriptErrorKind::WorkerStopped)) => {
            TextKey::WorkerStopped
        }
        Some(UserScriptFailureKind::Service(UserScriptErrorKind::InvalidIntegrity)) => {
            TextKey::ScriptInvalidIntegrity
        }
        Some(UserScriptFailureKind::Service(UserScriptErrorKind::IntegrityMismatch)) => {
            TextKey::ScriptIntegrityMismatch
        }
        Some(UserScriptFailureKind::Service(UserScriptErrorKind::UnverifiedNotAcknowledged)) => {
            TextKey::ScriptUnverifiedRequired
        }
        Some(UserScriptFailureKind::Service(
            UserScriptErrorKind::ConfirmationMismatch
            | UserScriptErrorKind::InvalidTarget
            | UserScriptErrorKind::Conflict,
        )) => TextKey::ScriptConflict,
        Some(UserScriptFailureKind::Service(UserScriptErrorKind::BackupFailed)) => {
            TextKey::ScriptBackupFailed
        }
        Some(UserScriptFailureKind::Service(UserScriptErrorKind::WriteFailed)) => {
            TextKey::ScriptWriteFailed
        }
        Some(UserScriptFailureKind::Service(UserScriptErrorKind::RollbackFailed)) => {
            TextKey::ScriptRollbackFailed
        }
        Some(UserScriptFailureKind::Service(
            UserScriptErrorKind::InspectFailed
            | UserScriptErrorKind::MarketRefreshFailed
            | UserScriptErrorKind::DownloadFailed
            | UserScriptErrorKind::DownloadTooLarge,
        ))
        | None => fallback,
    };
    text(locale, key)
}

fn script_toggle_label(locale: Locale, name: &str) -> String {
    match locale {
        Locale::ZhCn => format!("{}: {name}", text(locale, TextKey::EnabledScripts)),
        Locale::En => format!("Enable script: {name}"),
    }
}

#[derive(Clone, Copy)]
enum ScriptLinkText {
    MarketRepository,
    ProjectHomepage,
}

fn script_link_text(locale: Locale, key: ScriptLinkText) -> &'static str {
    match (locale, key) {
        (Locale::ZhCn, ScriptLinkText::MarketRepository) => "脚本市场仓库",
        (Locale::En, ScriptLinkText::MarketRepository) => "Script market repository",
        (Locale::ZhCn, ScriptLinkText::ProjectHomepage) => "打开项目主页",
        (Locale::En, ScriptLinkText::ProjectHomepage) => "Open project homepage",
    }
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
