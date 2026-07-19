use std::sync::Arc;

use codex_plus_manager_service::OverviewSnapshot;
use eframe::egui;

use crate::i18n::{Locale, TextKey, ThemeMode, text};
use crate::state::context::ContextViewState;
use crate::state::enhancements::{EnhancementLoadPhase, EnhancementViewState};
use crate::state::environment::EnvironmentViewState;
use crate::state::import::ImportViewState;
use crate::state::maintenance::{MaintenanceLoadPhase, MaintenanceViewState};
use crate::state::marketplace::MarketplaceViewState;
use crate::state::provider::OperationPhase;
use crate::state::provider::{ProviderLoadPhase, ProviderViewState};
use crate::state::sessions::SessionViewState;
use crate::state::settings::{SettingsLoadPhase, SettingsViewState};
use crate::state::user_scripts::{ScriptsTab, UserScriptViewState};
use crate::state::zed_remote::{ZedRemoteLoadPhase, ZedRemoteViewState};
use crate::state::{OverviewFailureKind, OverviewPhase, Route};
use crate::{icons, theme};

use super::{
    about, context, enhancements, environment, import, maintenance, marketplace, overview,
    provider, sessions, settings, user_scripts, zed_remote,
};

pub const SIDEBAR_WIDTH: f32 = 176.0;
pub const HEADER_HEIGHT: f32 = 58.0;
pub const STATUS_HEIGHT: f32 = 28.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellAction {
    Navigate(Route),
    Refresh,
    SetLocale(Locale),
    SetTheme(ThemeMode),
    Retry,
    Provider(provider::ProviderAction),
    Import(import::ImportAction),
    Environment(environment::EnvironmentAction),
    Sessions(sessions::SessionAction),
    UserScripts(user_scripts::UserScriptAction),
    Context(context::ContextAction),
    Marketplace(marketplace::MarketplaceAction),
    Enhancements(enhancements::EnhancementAction),
    ZedRemote(zed_remote::ZedRemoteAction),
    Maintenance(maintenance::MaintenanceAction),
    Settings(settings::SettingsAction),
}

#[derive(Debug, Clone)]
pub struct ShellViewModel {
    pub route: Route,
    pub locale: Locale,
    pub theme: ThemeMode,
    pub overview_phase: OverviewPhase,
    pub overview_snapshot: Option<Arc<OverviewSnapshot>>,
    pub overview_error: Option<OverviewFailureKind>,
    pub last_updated: Option<String>,
    pub renderer: String,
}

#[derive(Clone, Copy, Default)]
pub struct ShellFeatureStates<'a> {
    pub provider: Option<&'a ProviderViewState>,
    pub provider_import: Option<&'a ImportViewState>,
    pub environment: Option<&'a EnvironmentViewState>,
    pub context: Option<&'a ContextViewState>,
    pub enhancements: Option<&'a EnhancementViewState>,
    pub marketplace: Option<&'a MarketplaceViewState>,
    pub sessions: Option<&'a SessionViewState>,
    pub user_scripts: Option<&'a UserScriptViewState>,
    pub zed_remote: Option<&'a ZedRemoteViewState>,
    pub maintenance: Option<&'a MaintenanceViewState>,
    pub settings: Option<&'a SettingsViewState>,
}

pub fn render_shell(
    ui: &mut egui::Ui,
    model: &ShellViewModel,
    states: ShellFeatureStates<'_>,
) -> Vec<ShellAction> {
    let ShellFeatureStates {
        provider: provider_state,
        provider_import: import_state,
        environment: environment_state,
        context: context_state,
        enhancements: enhancements_state,
        marketplace: marketplace_state,
        sessions: sessions_state,
        user_scripts: user_script_state,
        zed_remote: zed_remote_state,
        maintenance: maintenance_state,
        settings: settings_state,
    } = states;
    let mut actions = Vec::new();

    egui::Panel::left("native_manager_sidebar")
        .exact_size(SIDEBAR_WIDTH)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().window_fill)
                .inner_margin(egui::Margin::symmetric(12, 14)),
        )
        .show(ui, |ui| render_sidebar(ui, model, &mut actions));

    egui::Panel::top("native_manager_header")
        .exact_size(HEADER_HEIGHT)
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::symmetric(16, 8)),
        )
        .show(ui, |ui| render_header(ui, model, &mut actions));

    egui::Panel::bottom("native_manager_status")
        .exact_size(STATUS_HEIGHT)
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().window_fill)
                .inner_margin(egui::Margin::symmetric(16, 4)),
        )
        .show(ui, |ui| render_status(ui, model, states));

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::same(16)),
        )
        .show(ui, |ui| match model.route {
            Route::Overview => overview::render(ui, model, &mut actions),
            Route::Providers => {
                if let Some(state) = provider_state {
                    if let Some(import_state) = import_state {
                        let mut import_actions = Vec::new();
                        import::render_provider_toolbar(
                            ui,
                            import_state,
                            model.locale,
                            &mut import_actions,
                        );
                        actions.extend(import_actions.into_iter().map(ShellAction::Import));
                        ui.separator();
                    }
                    let mut provider_actions = Vec::new();
                    provider::render(ui, state, model.locale, &mut provider_actions);
                    actions.extend(provider_actions.into_iter().map(ShellAction::Provider));
                }
            }
            Route::Environment => {
                if let Some(state) = environment_state {
                    let mut environment_actions = Vec::new();
                    environment::render(ui, state, model.locale, &mut environment_actions);
                    actions.extend(
                        environment_actions
                            .into_iter()
                            .map(ShellAction::Environment),
                    );
                }
            }
            Route::Sessions => {
                if let Some(state) = sessions_state {
                    let mut session_actions = Vec::new();
                    sessions::render(ui, state, model.locale, &mut session_actions);
                    actions.extend(session_actions.into_iter().map(ShellAction::Sessions));
                }
            }
            Route::Scripts => {
                if let Some(state) = user_script_state {
                    let mut user_script_actions = Vec::new();
                    user_scripts::render(ui, state, model.locale, &mut user_script_actions);
                    actions.extend(
                        user_script_actions
                            .into_iter()
                            .map(ShellAction::UserScripts),
                    );
                }
            }
            Route::Context => {
                let marketplace_height = if marketplace_state.is_some() {
                    marketplace::MARKETPLACE_BAND_HEIGHT
                } else {
                    0.0
                };
                if let Some(state) = context_state {
                    let mut context_actions = Vec::new();
                    let context_height =
                        (ui.available_height() - marketplace_height - 8.0).max(340.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), context_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            context::render(
                                ui,
                                state,
                                provider_state.is_some_and(ProviderViewState::is_dirty),
                                model.locale,
                                &mut context_actions,
                            );
                        },
                    );
                    actions.extend(context_actions.into_iter().map(ShellAction::Context));
                }
                if let Some(state) = marketplace_state {
                    ui.add_space(4.0);
                    ui.separator();
                    let mut marketplace_actions = Vec::new();
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), marketplace_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            marketplace::render(ui, state, model.locale, &mut marketplace_actions);
                        },
                    );
                    actions.extend(
                        marketplace_actions
                            .into_iter()
                            .map(ShellAction::Marketplace),
                    );
                }
            }
            Route::Enhancements => {
                if let Some(state) = enhancements_state {
                    let mut enhancement_actions = Vec::new();
                    enhancements::render(ui, state, model.locale, &mut enhancement_actions);
                    actions.extend(
                        enhancement_actions
                            .into_iter()
                            .map(ShellAction::Enhancements),
                    );
                }
            }
            Route::ZedRemote => {
                if let Some(state) = zed_remote_state {
                    let mut zed_actions = Vec::new();
                    zed_remote::render(ui, state, model.locale, &mut zed_actions);
                    actions.extend(zed_actions.into_iter().map(ShellAction::ZedRemote));
                }
            }
            Route::Maintenance => {
                if let Some(state) = maintenance_state {
                    let mut maintenance_actions = Vec::new();
                    maintenance::render(ui, state, model.locale, &mut maintenance_actions);
                    actions.extend(
                        maintenance_actions
                            .into_iter()
                            .map(ShellAction::Maintenance),
                    );
                }
            }
            Route::Settings => {
                if let Some(state) = settings_state {
                    let mut settings_actions = Vec::new();
                    settings::render(ui, state, model.locale, &mut settings_actions);
                    actions.extend(settings_actions.into_iter().map(ShellAction::Settings));
                }
            }
            Route::About => about::render(ui, model),
        });

    if let (Some(import_state), Some(provider_state)) = (import_state, provider_state) {
        let mut import_actions = Vec::new();
        import::render_modals(
            ui.ctx(),
            import_state,
            provider_state,
            model.locale,
            &mut import_actions,
        );
        actions.extend(import_actions.into_iter().map(ShellAction::Import));
    }

    actions
}

fn render_sidebar(ui: &mut egui::Ui, model: &ShellViewModel, actions: &mut Vec<ShellAction>) {
    ui.label(
        egui::RichText::new(text(model.locale, TextKey::AppName))
            .strong()
            .size(20.0),
    );
    ui.label(egui::RichText::new("Native Manager").weak().size(11.0));
    ui.add_space(18.0);

    navigation_button(
        ui,
        icons::layout_dashboard(),
        text(model.locale, TextKey::Overview),
        model.route == Route::Overview,
        Route::Overview,
        actions,
    );
    navigation_button(
        ui,
        icons::server_cog(),
        text(model.locale, TextKey::Providers),
        model.route == Route::Providers,
        Route::Providers,
        actions,
    );
    navigation_button(
        ui,
        icons::triangle_alert(),
        text(model.locale, TextKey::Environment),
        model.route == Route::Environment,
        Route::Environment,
        actions,
    );
    navigation_button(
        ui,
        icons::message_circle(),
        text(model.locale, TextKey::Sessions),
        model.route == Route::Sessions,
        Route::Sessions,
        actions,
    );
    navigation_button(
        ui,
        icons::file_code_2(),
        text(model.locale, TextKey::Scripts),
        model.route == Route::Scripts,
        Route::Scripts,
        actions,
    );
    navigation_button(
        ui,
        icons::wrench(),
        text(model.locale, TextKey::ToolsPlugins),
        model.route == Route::Context,
        Route::Context,
        actions,
    );
    navigation_button(
        ui,
        icons::circle_check(),
        text(model.locale, TextKey::Enhancements),
        model.route == Route::Enhancements,
        Route::Enhancements,
        actions,
    );
    navigation_button(
        ui,
        icons::folder_git_2(),
        text(model.locale, TextKey::ZedRemote),
        model.route == Route::ZedRemote,
        Route::ZedRemote,
        actions,
    );
    navigation_button(
        ui,
        icons::file_search(),
        text(model.locale, TextKey::Maintenance),
        model.route == Route::Maintenance,
        Route::Maintenance,
        actions,
    );
    navigation_button(
        ui,
        icons::settings(),
        text(model.locale, TextKey::Settings),
        model.route == Route::Settings,
        Route::Settings,
        actions,
    );
    navigation_button(
        ui,
        icons::info(),
        text(model.locale, TextKey::About),
        model.route == Route::About,
        Route::About,
        actions,
    );

    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.horizontal(|ui| {
            segmented_button(
                ui,
                text(model.locale, TextKey::Dark),
                model.theme == ThemeMode::Dark,
                ShellAction::SetTheme(ThemeMode::Dark),
                actions,
            );
            segmented_button(
                ui,
                text(model.locale, TextKey::Light),
                model.theme == ThemeMode::Light,
                ShellAction::SetTheme(ThemeMode::Light),
                actions,
            );
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(icons::sun())
                    .fit_to_exact_size(egui::vec2(14.0, 14.0))
                    .tint(ui.visuals().weak_text_color()),
            );
            ui.label(egui::RichText::new(text(model.locale, TextKey::Theme)).weak());
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            segmented_button(
                ui,
                text(model.locale, TextKey::Chinese),
                model.locale == Locale::ZhCn,
                ShellAction::SetLocale(Locale::ZhCn),
                actions,
            );
            segmented_button(
                ui,
                text(model.locale, TextKey::English),
                model.locale == Locale::En,
                ShellAction::SetLocale(Locale::En),
                actions,
            );
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(icons::languages())
                    .fit_to_exact_size(egui::vec2(14.0, 14.0))
                    .tint(ui.visuals().weak_text_color()),
            );
            ui.label(egui::RichText::new(text(model.locale, TextKey::Language)).weak());
        });
    });
}

fn navigation_button(
    ui: &mut egui::Ui,
    icon: egui::ImageSource<'static>,
    label: &str,
    selected: bool,
    route: Route,
    actions: &mut Vec<ShellAction>,
) {
    let image = egui::Image::new(icon).fit_to_exact_size(egui::vec2(16.0, 16.0));
    if ui
        .add_sized(
            [ui.available_width(), 36.0],
            egui::Button::image_and_text(image, label).selected(selected),
        )
        .clicked()
    {
        actions.push(ShellAction::Navigate(route));
    }
}

fn segmented_button(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    action: ShellAction,
    actions: &mut Vec<ShellAction>,
) {
    let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
    if ui
        .add_sized(
            [width.max(56.0), 30.0],
            egui::Button::new(label).selected(selected),
        )
        .clicked()
    {
        actions.push(action);
    }
}

fn render_header(ui: &mut egui::Ui, model: &ShellViewModel, actions: &mut Vec<ShellAction>) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            let title = match model.route {
                Route::Overview => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::Overview)
                ),
                Route::Providers => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::Providers)
                ),
                Route::Environment => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::Environment)
                ),
                Route::Sessions => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::Sessions)
                ),
                Route::Scripts => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::Scripts)
                ),
                Route::Context => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::ToolsPlugins)
                ),
                Route::Enhancements => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::Enhancements)
                ),
                Route::ZedRemote => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::ZedRemote)
                ),
                Route::Maintenance => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::Maintenance)
                ),
                Route::Settings => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::Settings)
                ),
                Route::About => format!(
                    "{} {}",
                    text(model.locale, TextKey::About),
                    text(model.locale, TextKey::AppName)
                ),
            };
            ui.label(egui::RichText::new(title).strong().size(17.0));
            let subtitle = match model.route {
                Route::Overview => TextKey::OverviewSubtitle,
                Route::Providers => TextKey::ProvidersSubtitle,
                Route::Environment => TextKey::EnvironmentSubtitle,
                Route::Sessions => TextKey::SessionsSubtitle,
                Route::Scripts => TextKey::ScriptsSubtitle,
                Route::Context => TextKey::ToolsPluginsSubtitle,
                Route::Enhancements => TextKey::EnhancementsSubtitle,
                Route::ZedRemote => TextKey::ZedRemoteSubtitle,
                Route::Maintenance => TextKey::MaintenanceSubtitle,
                Route::Settings => TextKey::SettingsSubtitle,
                Route::About => TextKey::AboutSubtitle,
            };
            ui.label(
                egui::RichText::new(text(model.locale, subtitle))
                    .weak()
                    .size(11.0),
            );
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = text(model.locale, TextKey::Refresh);
            let response = icon_button(ui, icons::refresh_cw(), label);
            if response.clicked() {
                actions.push(ShellAction::Refresh);
            }
            if let Some(updated) = &model.last_updated {
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {updated}",
                        text(model.locale, TextKey::LastUpdated)
                    ))
                    .weak()
                    .size(11.0),
                );
            }
        });
    });
}

fn icon_button(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, label: &str) -> egui::Response {
    let response = ui.add_sized(
        [34.0, 34.0],
        egui::Button::image(egui::Image::new(icon).fit_to_exact_size(egui::vec2(17.0, 17.0))),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response.on_hover_text(label)
}

fn render_status(ui: &mut egui::Ui, model: &ShellViewModel, states: ShellFeatureStates<'_>) {
    let ShellFeatureStates {
        provider: provider_state,
        environment: environment_state,
        context: context_state,
        enhancements: enhancements_state,
        sessions: sessions_state,
        user_scripts: user_script_state,
        zed_remote: zed_remote_state,
        maintenance: maintenance_state,
        settings: settings_state,
        ..
    } = states;
    if model.route == Route::Providers {
        let phase = provider_state.map_or(ProviderLoadPhase::Idle, |state| state.load_phase);
        let (status, color) = match phase {
            ProviderLoadPhase::Idle | ProviderLoadPhase::Loading => {
                (text(model.locale, TextKey::Loading), theme::WARNING_COLOR)
            }
            ProviderLoadPhase::Ready => (text(model.locale, TextKey::Ready), theme::SUCCESS_COLOR),
            ProviderLoadPhase::Refreshing => (
                text(model.locale, TextKey::Refreshing),
                theme::WARNING_COLOR,
            ),
            ProviderLoadPhase::Error => (
                match model.locale {
                    Locale::ZhCn => "供应商加载失败",
                    Locale::En => "Provider load failed",
                },
                theme::ERROR_COLOR,
            ),
        };
        ui.horizontal(|ui| {
            ui.colored_label(
                color,
                format!("{}: {status}", text(model.locale, TextKey::Status)),
            );
            render_status_metadata(ui, model);
        });
        return;
    }

    if model.route == Route::Environment {
        let phase = environment_state.map_or(OperationPhase::Idle, |state| state.inspection_phase);
        let (status, color) = match phase {
            OperationPhase::Idle | OperationPhase::Running => {
                (text(model.locale, TextKey::Loading), theme::WARNING_COLOR)
            }
            OperationPhase::Ready => (text(model.locale, TextKey::Ready), theme::SUCCESS_COLOR),
            OperationPhase::Error => (
                text(model.locale, TextKey::InspectionFailed),
                theme::ERROR_COLOR,
            ),
        };
        ui.horizontal(|ui| {
            ui.colored_label(
                color,
                format!("{}: {status}", text(model.locale, TextKey::Status)),
            );
            render_status_metadata(ui, model);
        });
        return;
    }

    if model.route == Route::Context {
        let phase = context_state.map_or(OperationPhase::Idle, |state| state.workspace_phase);
        let (status, color) = match phase {
            OperationPhase::Idle | OperationPhase::Running => {
                (text(model.locale, TextKey::Loading), theme::WARNING_COLOR)
            }
            OperationPhase::Ready => (text(model.locale, TextKey::Ready), theme::SUCCESS_COLOR),
            OperationPhase::Error => (
                text(model.locale, TextKey::ContextLoadFailed),
                theme::ERROR_COLOR,
            ),
        };
        ui.horizontal(|ui| {
            ui.colored_label(
                color,
                format!("{}: {status}", text(model.locale, TextKey::Status)),
            );
            render_status_metadata(ui, model);
        });
        return;
    }

    if model.route == Route::Enhancements {
        let phase = enhancements_state.map_or(EnhancementLoadPhase::Idle, |state| state.load_phase);
        let (status, color) = match phase {
            EnhancementLoadPhase::Idle | EnhancementLoadPhase::Loading => {
                (text(model.locale, TextKey::Loading), theme::WARNING_COLOR)
            }
            EnhancementLoadPhase::Ready => {
                (text(model.locale, TextKey::Ready), theme::SUCCESS_COLOR)
            }
            EnhancementLoadPhase::Refreshing => (
                text(model.locale, TextKey::Refreshing),
                theme::WARNING_COLOR,
            ),
            EnhancementLoadPhase::Error => (
                text(model.locale, TextKey::EnhancementsLoadFailed),
                theme::ERROR_COLOR,
            ),
        };
        ui.horizontal(|ui| {
            ui.colored_label(
                color,
                format!("{}: {status}", text(model.locale, TextKey::Status)),
            );
            render_status_metadata(ui, model);
        });
        return;
    }

    if model.route == Route::Sessions {
        let phase = sessions_state.map_or(OperationPhase::Idle, |state| state.workspace_phase);
        let (status, color) = match phase {
            OperationPhase::Idle | OperationPhase::Running => {
                (text(model.locale, TextKey::Loading), theme::WARNING_COLOR)
            }
            OperationPhase::Ready => (text(model.locale, TextKey::Ready), theme::SUCCESS_COLOR),
            OperationPhase::Error => (
                text(model.locale, TextKey::SessionLoadFailed),
                theme::ERROR_COLOR,
            ),
        };
        ui.horizontal(|ui| {
            ui.colored_label(
                color,
                format!("{}: {status}", text(model.locale, TextKey::Status)),
            );
            render_status_metadata(ui, model);
        });
        return;
    }

    if model.route == Route::Scripts {
        let tab = user_script_state.map_or(ScriptsTab::Market, |state| state.tab);
        let phase = user_script_state.map_or(OperationPhase::Idle, |state| match tab {
            ScriptsTab::Market => state.market_phase,
            ScriptsTab::Local => state.local_phase,
        });
        let (status, color) = match phase {
            OperationPhase::Idle | OperationPhase::Running => {
                (text(model.locale, TextKey::Loading), theme::WARNING_COLOR)
            }
            OperationPhase::Ready => (text(model.locale, TextKey::Ready), theme::SUCCESS_COLOR),
            OperationPhase::Error => {
                let key = match tab {
                    ScriptsTab::Market => TextKey::ScriptMarketLoadFailed,
                    ScriptsTab::Local => TextKey::ScriptLocalLoadFailed,
                };
                (text(model.locale, key), theme::ERROR_COLOR)
            }
        };
        ui.horizontal(|ui| {
            ui.colored_label(
                color,
                format!("{}: {status}", text(model.locale, TextKey::Status)),
            );
            render_status_metadata(ui, model);
        });
        return;
    }

    if model.route == Route::ZedRemote {
        let phase = zed_remote_state.map_or(ZedRemoteLoadPhase::Idle, |state| state.load_phase);
        let (status, color) = match phase {
            ZedRemoteLoadPhase::Idle | ZedRemoteLoadPhase::Loading => {
                (text(model.locale, TextKey::Loading), theme::WARNING_COLOR)
            }
            ZedRemoteLoadPhase::Ready => (text(model.locale, TextKey::Ready), theme::SUCCESS_COLOR),
            ZedRemoteLoadPhase::Refreshing => (
                text(model.locale, TextKey::Refreshing),
                theme::WARNING_COLOR,
            ),
            ZedRemoteLoadPhase::Error => (
                text(model.locale, TextKey::ZedLoadFailed),
                theme::ERROR_COLOR,
            ),
        };
        ui.horizontal(|ui| {
            ui.colored_label(
                color,
                format!("{}: {status}", text(model.locale, TextKey::Status)),
            );
            render_status_metadata(ui, model);
        });
        return;
    }

    if model.route == Route::Maintenance {
        let phase = maintenance_state.map_or(MaintenanceLoadPhase::Idle, |state| state.load_phase);
        let (status, color) = match phase {
            MaintenanceLoadPhase::Idle | MaintenanceLoadPhase::Loading => {
                (text(model.locale, TextKey::Loading), theme::WARNING_COLOR)
            }
            MaintenanceLoadPhase::Ready => {
                (text(model.locale, TextKey::Ready), theme::SUCCESS_COLOR)
            }
            MaintenanceLoadPhase::Refreshing => (
                text(model.locale, TextKey::Refreshing),
                theme::WARNING_COLOR,
            ),
            MaintenanceLoadPhase::Error => (
                text(model.locale, TextKey::MaintenanceLoadFailed),
                theme::ERROR_COLOR,
            ),
        };
        ui.horizontal(|ui| {
            ui.colored_label(
                color,
                format!("{}: {status}", text(model.locale, TextKey::Status)),
            );
            render_status_metadata(ui, model);
        });
        return;
    }

    if model.route == Route::Settings {
        let phase = settings_state.map_or(SettingsLoadPhase::Idle, |state| state.load_phase);
        let (status, color) = match phase {
            SettingsLoadPhase::Idle | SettingsLoadPhase::Loading => {
                (text(model.locale, TextKey::Loading), theme::WARNING_COLOR)
            }
            SettingsLoadPhase::Ready => (text(model.locale, TextKey::Ready), theme::SUCCESS_COLOR),
            SettingsLoadPhase::Refreshing => (
                text(model.locale, TextKey::Refreshing),
                theme::WARNING_COLOR,
            ),
            SettingsLoadPhase::Error => (
                text(model.locale, TextKey::SettingsLoadFailed),
                theme::ERROR_COLOR,
            ),
        };
        ui.horizontal(|ui| {
            ui.colored_label(
                color,
                format!("{}: {status}", text(model.locale, TextKey::Status)),
            );
            render_status_metadata(ui, model);
        });
        return;
    }

    let phase = match model.overview_phase {
        OverviewPhase::Idle | OverviewPhase::Loading => TextKey::Loading,
        OverviewPhase::Ready => TextKey::Ready,
        OverviewPhase::Refreshing => TextKey::Refreshing,
        OverviewPhase::Error => match model.overview_error {
            Some(OverviewFailureKind::WorkerStopped) => TextKey::WorkerStopped,
            Some(OverviewFailureKind::LoadFailed) | None => TextKey::LoadFailed,
        },
    };
    ui.horizontal(|ui| {
        let color = match model.overview_phase {
            OverviewPhase::Ready => theme::SUCCESS_COLOR,
            OverviewPhase::Error => theme::ERROR_COLOR,
            _ => theme::WARNING_COLOR,
        };
        ui.colored_label(
            color,
            format!(
                "{}: {}",
                text(model.locale, TextKey::Status),
                text(model.locale, phase)
            ),
        );
        render_status_metadata(ui, model);
    });
}

fn render_status_metadata(ui: &mut egui::Ui, model: &ShellViewModel) {
    ui.separator();
    ui.label(format!(
        "{}: {}",
        text(model.locale, TextKey::Renderer),
        model.renderer
    ));
    ui.separator();
    ui.label(format!(
        "{}: {}",
        text(model.locale, TextKey::Language),
        match model.locale {
            Locale::ZhCn => text(model.locale, TextKey::Chinese),
            Locale::En => text(model.locale, TextKey::English),
        }
    ));
}
