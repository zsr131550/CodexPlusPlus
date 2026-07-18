use std::collections::BTreeMap;
use std::fmt;

use codex_plus_core::settings::{
    AggregateRelayStrategy, RelayMode, RelayModelInsertMode, RelayProtocol,
};
use codex_plus_manager_service::{
    ProviderKind, ProviderLiveFileKind, ProviderProfile, ProviderRollbackOutcome, provider_presets,
};
use eframe::egui;

use crate::i18n::Locale;
use crate::state::provider::{
    GuardResolution, ListDirection, LiveMutationFailureKind, LiveMutationKind, OperationPhase,
    ProviderEditorTab, ProviderLoadPhase, ProviderSaveFailureKind, ProviderViewState,
};
use crate::{icons, theme};

#[derive(Clone, PartialEq, Eq)]
pub enum ProviderEdit {
    Name(String),
    Mode(RelayMode),
    Protocol(RelayProtocol),
    BaseUrl(String),
    ApiKey(String),
    Model(String),
    TestModel(String),
    UseCommonConfig(bool),
    ContextWindow(String),
    AutoCompactLimit(String),
    InsertMode(RelayModelInsertMode),
    UserAgent(String),
    ConfigContents(String),
    AuthContents(String),
    ModelRow {
        index: usize,
        model: String,
        window: String,
    },
    AggregateStrategy(AggregateRelayStrategy),
}

impl fmt::Debug for ProviderEdit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Name(_) => "Name",
            Self::Mode(_) => "Mode",
            Self::Protocol(_) => "Protocol",
            Self::BaseUrl(_) => "BaseUrl",
            Self::ApiKey(_) => "ApiKey",
            Self::Model(_) => "Model",
            Self::TestModel(_) => "TestModel",
            Self::UseCommonConfig(_) => "UseCommonConfig",
            Self::ContextWindow(_) => "ContextWindow",
            Self::AutoCompactLimit(_) => "AutoCompactLimit",
            Self::InsertMode(_) => "InsertMode",
            Self::UserAgent(_) => "UserAgent",
            Self::ConfigContents(_) => "ConfigContents",
            Self::AuthContents(_) => "AuthContents",
            Self::ModelRow { .. } => "ModelRow",
            Self::AggregateStrategy(_) => "AggregateStrategy",
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ProviderAction {
    RetryLoad,
    ToggleList,
    Select(String),
    SetTab(ProviderEditorTab),
    AddOrdinary,
    AddAggregate,
    Duplicate,
    Move(ListDirection),
    Delete {
        confirmed: bool,
    },
    CancelDelete,
    Edit(ProviderEdit),
    ApplyPreset(String),
    AddModel,
    RemoveModel(usize),
    MergeModels,
    SetAggregateMember {
        profile_id: String,
        enabled: bool,
    },
    SetAggregateWeight {
        profile_id: String,
        weight: u32,
    },
    SetSecretRevealed(bool),
    SetConfigRevealed(bool),
    SetAuthRevealed(bool),
    Save,
    Discard,
    Test,
    FetchModels,
    Doctor,
    RefreshLive,
    RequestLiveMutation(LiveMutationKind),
    ConfirmLiveMutation,
    CancelLiveMutation,
    BeginLiveFileEdit(ProviderLiveFileKind),
    EditLiveFile {
        kind: ProviderLiveFileKind,
        contents: String,
    },
    CancelLiveFileEdit(ProviderLiveFileKind),
    SetLiveFileRevealed {
        kind: ProviderLiveFileKind,
        revealed: bool,
    },
    ResolveGuard(GuardResolution),
}

impl fmt::Debug for ProviderAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RetryLoad => "RetryLoad",
            Self::ToggleList => "ToggleList",
            Self::Select(_) => "Select",
            Self::SetTab(_) => "SetTab",
            Self::AddOrdinary => "AddOrdinary",
            Self::AddAggregate => "AddAggregate",
            Self::Duplicate => "Duplicate",
            Self::Move(_) => "Move",
            Self::Delete { .. } => "Delete",
            Self::CancelDelete => "CancelDelete",
            Self::Edit(_) => "Edit",
            Self::ApplyPreset(_) => "ApplyPreset",
            Self::AddModel => "AddModel",
            Self::RemoveModel(_) => "RemoveModel",
            Self::MergeModels => "MergeModels",
            Self::SetAggregateMember { .. } => "SetAggregateMember",
            Self::SetAggregateWeight { .. } => "SetAggregateWeight",
            Self::SetSecretRevealed(_) => "SetSecretRevealed",
            Self::SetConfigRevealed(_) => "SetConfigRevealed",
            Self::SetAuthRevealed(_) => "SetAuthRevealed",
            Self::Save => "Save",
            Self::Discard => "Discard",
            Self::Test => "Test",
            Self::FetchModels => "FetchModels",
            Self::Doctor => "Doctor",
            Self::RefreshLive => "RefreshLive",
            Self::RequestLiveMutation(_) => "RequestLiveMutation",
            Self::ConfirmLiveMutation => "ConfirmLiveMutation",
            Self::CancelLiveMutation => "CancelLiveMutation",
            Self::BeginLiveFileEdit(_) => "BeginLiveFileEdit",
            Self::EditLiveFile { .. } => "EditLiveFile",
            Self::CancelLiveFileEdit(_) => "CancelLiveFileEdit",
            Self::SetLiveFileRevealed { .. } => "SetLiveFileRevealed",
            Self::ResolveGuard(_) => "ResolveGuard",
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum PText {
    ProviderList,
    ProviderEditor,
    CollapseList,
    ExpandList,
    AddProvider,
    AddAggregate,
    Duplicate,
    MoveUp,
    MoveDown,
    DeleteProvider,
    General,
    Models,
    Config,
    Diagnostics,
    Routing,
    Live,
    LiveStatus,
    Configured,
    NotConfigured,
    Authenticated,
    NotAuthenticated,
    RefreshLive,
    ActivateProvider,
    ReapplyProvider,
    BackfillProvider,
    ClearLive,
    LiveConfigHidden,
    LiveAuthHidden,
    RevealLiveConfig,
    RevealLiveAuth,
    HideLiveConfig,
    HideLiveAuth,
    EditLiveConfig,
    EditLiveAuth,
    SaveLiveConfig,
    SaveLiveAuth,
    CancelLiveConfig,
    CancelLiveAuth,
    ConfirmLiveTitle,
    ConfirmLiveNow,
    CancelLiveChange,
    LiveMutationFailed,
    RollbackVerified,
    RollbackFailed,
    RollbackNotRequired,
    BackupPath,
    SaveChanges,
    DiscardChanges,
    LoadingProviders,
    LoadError,
    RetryProviderLoad,
    Active,
    Ordinary,
    Aggregate,
    Presets,
    PresetSearch,
    Name,
    Mode,
    Official,
    MixedApi,
    PureApi,
    Protocol,
    Responses,
    ChatCompletions,
    BaseUrl,
    ApiKey,
    ShowSecret,
    HideSecret,
    Model,
    TestModel,
    UseCommonConfig,
    ContextWindow,
    CompactLimit,
    InsertMode,
    Patch,
    ModelCatalog,
    UserAgent,
    ModelName,
    Window,
    AddModel,
    RemoveModel,
    DiscoverModels,
    MergeModels,
    ConfigToml,
    AuthJson,
    Hidden,
    Reveal,
    Hide,
    TestConnection,
    FetchModels,
    RunDiagnostics,
    Idle,
    Running,
    Success,
    Failed,
    Strategy,
    Failover,
    ConversationRoundRobin,
    RequestRoundRobin,
    WeightedRoundRobin,
    Weight,
    Unsaved,
    Saved,
    GuardTitle,
    GuardMessage,
    Stay,
    SaveAndContinue,
    DiscardAndContinue,
    NoProfile,
    DeleteTitle,
    DeleteMessage,
    Cancel,
    ConfirmDelete,
    SaveConflict,
    SaveValidation,
    SaveFailed,
    SaveWorkerStopped,
}

fn ptext(locale: Locale, key: PText) -> &'static str {
    let (zh, en) = match key {
        PText::ProviderList => ("供应商列表", "Provider list"),
        PText::ProviderEditor => ("供应商编辑器", "Provider editor"),
        PText::CollapseList => ("收起供应商列表", "Collapse provider list"),
        PText::ExpandList => ("展开供应商列表", "Expand provider list"),
        PText::AddProvider => ("添加供应商", "Add provider"),
        PText::AddAggregate => ("添加聚合配置", "Add aggregate"),
        PText::Duplicate => ("复制供应商", "Duplicate provider"),
        PText::MoveUp => ("上移", "Move up"),
        PText::MoveDown => ("下移", "Move down"),
        PText::DeleteProvider => ("删除供应商", "Delete provider"),
        PText::General => ("常规", "General"),
        PText::Models => ("模型", "Models"),
        PText::Config => ("配置", "Config"),
        PText::Diagnostics => ("诊断", "Diagnostics"),
        PText::Routing => ("路由", "Routing"),
        PText::Live => ("实时文件", "Live"),
        PText::LiveStatus => ("实时状态", "Live status"),
        PText::Configured => ("已配置", "Configured"),
        PText::NotConfigured => ("未配置", "Not configured"),
        PText::Authenticated => ("已认证", "Authenticated"),
        PText::NotAuthenticated => ("未认证", "Not authenticated"),
        PText::RefreshLive => ("刷新实时状态", "Refresh live status"),
        PText::ActivateProvider => ("启用供应商", "Activate provider"),
        PText::ReapplyProvider => ("重新应用当前供应商", "Reapply active provider"),
        PText::BackfillProvider => ("从实时文件回填", "Backfill active provider"),
        PText::ClearLive => ("清理实时配置", "Clear live configuration"),
        PText::LiveConfigHidden => ("实时 config 已隐藏", "Live config hidden"),
        PText::LiveAuthHidden => ("实时 auth 已隐藏", "Live auth hidden"),
        PText::RevealLiveConfig => ("显示实时 config", "Reveal live config"),
        PText::RevealLiveAuth => ("显示实时 auth", "Reveal live auth"),
        PText::HideLiveConfig => ("隐藏实时 config", "Hide live config"),
        PText::HideLiveAuth => ("隐藏实时 auth", "Hide live auth"),
        PText::EditLiveConfig => ("编辑实时 config", "Edit live config"),
        PText::EditLiveAuth => ("编辑实时 auth", "Edit live auth"),
        PText::SaveLiveConfig => ("保存实时 config", "Save live config"),
        PText::SaveLiveAuth => ("保存实时 auth", "Save live auth"),
        PText::CancelLiveConfig => ("取消编辑实时 config", "Cancel live config"),
        PText::CancelLiveAuth => ("取消编辑实时 auth", "Cancel live auth"),
        PText::ConfirmLiveTitle => ("确认实时变更", "Confirm live change"),
        PText::ConfirmLiveNow => ("立即确认实时变更", "Confirm live change now"),
        PText::CancelLiveChange => ("取消实时变更", "Cancel live change"),
        PText::LiveMutationFailed => ("实时变更失败", "Live mutation failed"),
        PText::RollbackVerified => ("回滚已验证", "Rollback verified"),
        PText::RollbackFailed => ("回滚失败", "Rollback failed"),
        PText::RollbackNotRequired => ("无需回滚", "Rollback not required"),
        PText::BackupPath => ("备份路径", "Backup path"),
        PText::SaveChanges => ("保存更改", "Save changes"),
        PText::DiscardChanges => ("放弃更改", "Discard changes"),
        PText::LoadingProviders => ("正在加载供应商...", "Loading providers..."),
        PText::LoadError => ("无法加载供应商。", "Unable to load providers."),
        PText::RetryProviderLoad => ("重试加载供应商", "Retry provider load"),
        PText::Active => ("当前启用", "Active"),
        PText::Ordinary => ("普通供应商", "Ordinary"),
        PText::Aggregate => ("聚合供应商", "Aggregate"),
        PText::Presets => ("预设", "Presets"),
        PText::PresetSearch => ("搜索预设", "Search presets"),
        PText::Name => ("名称", "Name"),
        PText::Mode => ("模式", "Mode"),
        PText::Official => ("官方", "Official"),
        PText::MixedApi => ("混合 API", "Mixed API"),
        PText::PureApi => ("纯 API", "Pure API"),
        PText::Protocol => ("协议", "Protocol"),
        PText::Responses => ("Responses", "Responses"),
        PText::ChatCompletions => ("Chat Completions", "Chat Completions"),
        PText::BaseUrl => ("基础 URL", "Base URL"),
        PText::ApiKey => ("API 密钥", "API key"),
        PText::ShowSecret => ("显示密钥", "Show secret"),
        PText::HideSecret => ("隐藏密钥", "Hide secret"),
        PText::Model => ("默认模型", "Model"),
        PText::TestModel => ("测试模型", "Test model"),
        PText::UseCommonConfig => ("使用公共配置", "Use common config"),
        PText::ContextWindow => ("上下文窗口", "Context window"),
        PText::CompactLimit => ("自动压缩阈值", "Auto compact limit"),
        PText::InsertMode => ("模型写入模式", "Model insert mode"),
        PText::Patch => ("补丁", "Patch"),
        PText::ModelCatalog => ("模型目录", "Model catalog"),
        PText::UserAgent => ("用户代理", "User agent"),
        PText::ModelName => ("模型名称", "Model name"),
        PText::Window => ("窗口", "Window"),
        PText::AddModel => ("添加模型", "Add model"),
        PText::RemoveModel => ("删除模型", "Remove model"),
        PText::DiscoverModels => ("获取模型", "Discover models"),
        PText::MergeModels => ("合并已获取模型", "Merge discovered models"),
        PText::ConfigToml => ("config.toml 内容", "config.toml contents"),
        PText::AuthJson => ("auth.json 内容", "auth.json contents"),
        PText::Hidden => ("内容已隐藏", "Contents hidden"),
        PText::Reveal => ("显示内容", "Reveal contents"),
        PText::Hide => ("隐藏内容", "Hide contents"),
        PText::TestConnection => ("测试连接", "Test connection"),
        PText::FetchModels => ("获取模型", "Fetch models"),
        PText::RunDiagnostics => ("运行诊断", "Run diagnostics"),
        PText::Idle => ("尚未运行", "Not run"),
        PText::Running => ("运行中", "Running"),
        PText::Success => ("成功", "Success"),
        PText::Failed => ("失败", "Failed"),
        PText::Strategy => ("策略", "Strategy"),
        PText::Failover => ("故障转移", "Failover"),
        PText::ConversationRoundRobin => ("按会话轮询", "Conversation round robin"),
        PText::RequestRoundRobin => ("按请求轮询", "Request round robin"),
        PText::WeightedRoundRobin => ("加权轮询", "Weighted round robin"),
        PText::Weight => ("权重", "Weight"),
        PText::Unsaved => ("有未保存更改", "Unsaved changes"),
        PText::Saved => ("已保存", "Saved"),
        PText::GuardTitle => ("未保存的更改", "Unsaved changes"),
        PText::GuardMessage => (
            "保存或放弃更改后再继续。",
            "Save or discard changes before continuing.",
        ),
        PText::Stay => ("留在此处", "Stay"),
        PText::SaveAndContinue => ("保存并继续", "Save and continue"),
        PText::DiscardAndContinue => ("放弃并继续", "Discard and continue"),
        PText::NoProfile => ("没有可编辑的供应商", "No provider selected"),
        PText::DeleteTitle => ("确认删除", "Confirm deletion"),
        PText::DeleteMessage => (
            "此供应商被聚合配置引用。删除后将同时移除这些引用。",
            "This provider is referenced by aggregate routes. Deleting it also removes those references.",
        ),
        PText::Cancel => ("取消", "Cancel"),
        PText::ConfirmDelete => ("确认删除", "Delete provider and references"),
        PText::SaveConflict => (
            "供应商配置已在磁盘上更改，请重新加载后再保存。",
            "Provider workspace changed on disk. Reload before saving again.",
        ),
        PText::SaveValidation => (
            "供应商配置未通过校验，请检查当前字段。",
            "Provider validation failed. Review the current fields.",
        ),
        PText::SaveFailed => ("无法保存供应商配置。", "Unable to save provider workspace."),
        PText::SaveWorkerStopped => (
            "供应商后台服务已停止。",
            "The provider background service has stopped.",
        ),
    };
    match locale {
        Locale::ZhCn => zh,
        Locale::En => en,
    }
}

pub fn render(
    ui: &mut egui::Ui,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    match state.load_phase {
        ProviderLoadPhase::Idle | ProviderLoadPhase::Loading => {
            ui.centered_and_justified(|ui| {
                ui.spinner();
                ui.label(ptext(locale, PText::LoadingProviders));
            });
        }
        ProviderLoadPhase::Error if state.draft().is_none() => {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.colored_label(theme::ERROR_COLOR, ptext(locale, PText::LoadError));
                    if ui.button(ptext(locale, PText::RetryProviderLoad)).clicked() {
                        actions.push(ProviderAction::RetryLoad);
                    }
                });
            });
        }
        ProviderLoadPhase::Ready | ProviderLoadPhase::Refreshing | ProviderLoadPhase::Error => {
            render_workspace(ui, state, locale, actions);
        }
    }

    if state.has_pending_guard() {
        render_guard(ui.ctx(), locale, actions);
    }
    if state.delete_confirmation_required {
        render_delete_confirmation(ui.ctx(), locale, actions);
    }
    if state.pending_live_confirmation().is_some() {
        render_live_confirmation(ui.ctx(), state, locale, actions);
    }
}

fn render_workspace(
    ui: &mut egui::Ui,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    let available_width = ui.available_width();
    let list_width = if state.list_collapsed {
        42.0
    } else if available_width >= 860.0 {
        244.0
    } else {
        196.0
    };
    let height = ui.available_height();

    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(list_width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| render_list(ui, state, locale, actions),
        );
        ui.separator();
        let editor_width = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(editor_width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| render_editor(ui, state, locale, actions),
        );
    });
}

fn render_list(
    ui: &mut egui::Ui,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    ui.horizontal(|ui| {
        if !state.list_collapsed {
            ui.label(egui::RichText::new(ptext(locale, PText::ProviderList)).strong());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (icon, label) = if state.list_collapsed {
                (icons::panel_left_open(), ptext(locale, PText::ExpandList))
            } else {
                (
                    icons::panel_left_close(),
                    ptext(locale, PText::CollapseList),
                )
            };
            if tool_button(ui, icon, label, true).clicked() {
                actions.push(ProviderAction::ToggleList);
            }
        });
    });
    if state.list_collapsed {
        return;
    }

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        let tools = [
            (
                icons::plus(),
                PText::AddProvider,
                ProviderAction::AddOrdinary,
                true,
            ),
            (
                icons::server_cog(),
                PText::AddAggregate,
                ProviderAction::AddAggregate,
                true,
            ),
            (
                icons::copy(),
                PText::Duplicate,
                ProviderAction::Duplicate,
                state.selected_profile().is_some(),
            ),
            (
                icons::chevron_up(),
                PText::MoveUp,
                ProviderAction::Move(ListDirection::Up),
                state.selected_profile().is_some(),
            ),
            (
                icons::chevron_down(),
                PText::MoveDown,
                ProviderAction::Move(ListDirection::Down),
                state.selected_profile().is_some(),
            ),
            (
                icons::trash_2(),
                PText::DeleteProvider,
                ProviderAction::Delete { confirmed: false },
                state.selected_profile().is_some() && !state.selected_is_active(),
            ),
        ];
        for (icon, label, action, enabled) in tools {
            if tool_button(ui, icon, ptext(locale, label), enabled).clicked() {
                actions.push(action);
            }
        }
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("provider_profile_list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(document) = state.draft() {
                for profile in &document.profiles {
                    let selected = state.selected_profile_id.as_deref() == Some(profile.id());
                    if ui
                        .add_sized(
                            [ui.available_width(), 34.0],
                            egui::Button::new(profile.name()).selected(selected),
                        )
                        .clicked()
                    {
                        actions.push(ProviderAction::Select(profile.id().to_owned()));
                    }
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(match profile.kind() {
                                ProviderKind::Ordinary => ptext(locale, PText::Ordinary),
                                ProviderKind::Aggregate => ptext(locale, PText::Aggregate),
                            })
                            .weak()
                            .size(10.0),
                        );
                        if state
                            .baseline
                            .as_ref()
                            .and_then(|workspace| workspace.activation.active_profile_id.as_deref())
                            == Some(profile.id())
                        {
                            ui.colored_label(
                                theme::SUCCESS_COLOR,
                                egui::RichText::new(ptext(locale, PText::Active)).size(10.0),
                            );
                        }
                    });
                    ui.add_space(4.0);
                }
            }
        });
}

fn render_editor(
    ui: &mut egui::Ui,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    ui.label(egui::RichText::new(ptext(locale, PText::ProviderEditor)).strong());
    let Some(profile) = state.selected_profile() else {
        ui.centered_and_justified(|ui| {
            ui.label(ptext(locale, PText::NoProfile));
        });
        return;
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(profile.name()).strong().size(16.0));
        ui.label(
            egui::RichText::new(match profile.kind() {
                ProviderKind::Ordinary => ptext(locale, PText::Ordinary),
                ProviderKind::Aggregate => ptext(locale, PText::Aggregate),
            })
            .weak(),
        );
    });
    render_live_strip(ui, state, locale, actions);

    let effective_tab = render_tabs(ui, profile.kind(), state.editor_tab, locale, actions);
    ui.separator();
    let footer_height = if state.save.error.is_some() {
        72.0
    } else {
        50.0
    };
    let content_height = (ui.available_height() - footer_height).max(120.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), content_height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            egui::ScrollArea::vertical()
                .id_salt("provider_editor_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| match (profile, effective_tab) {
                    (_, ProviderEditorTab::Live) => {
                        render_live_files(ui, state, locale, actions);
                    }
                    (ProviderProfile::Ordinary(profile), ProviderEditorTab::General) => {
                        render_general(ui, profile, state, locale, actions);
                    }
                    (ProviderProfile::Ordinary(profile), ProviderEditorTab::Models) => {
                        render_models(ui, profile, state, locale, actions);
                    }
                    (ProviderProfile::Ordinary(profile), ProviderEditorTab::Config) => {
                        render_config(ui, profile, state, locale, actions);
                    }
                    (ProviderProfile::Ordinary(_), ProviderEditorTab::Diagnostics)
                    | (ProviderProfile::Aggregate { .. }, ProviderEditorTab::Diagnostics) => {
                        render_diagnostics(ui, state, locale, actions);
                    }
                    (ProviderProfile::Aggregate { routing, .. }, _) => {
                        render_routing(ui, routing, state, locale, actions);
                    }
                    _ => {}
                });
        },
    );
    ui.separator();
    render_save_bar(ui, state, locale, actions);
}

fn render_live_strip(
    ui: &mut egui::Ui,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    let Some(workspace) = state.live.workspace.as_ref() else {
        return;
    };
    let running = state.live.mutation_phase == OperationPhase::Running;
    let clean = !state.has_unsaved_changes();
    let enabled = workspace.provider.activation.enabled && clean && !running;
    let selected_is_active = state.selected_is_active();
    let selected_id = state.selected_profile_id.clone();
    let selected_is_ordinary = state
        .selected_profile()
        .is_some_and(|profile| profile.kind() == ProviderKind::Ordinary);

    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(ptext(locale, PText::LiveStatus)).strong());
        let configured = if workspace.status.configured {
            PText::Configured
        } else {
            PText::NotConfigured
        };
        ui.colored_label(
            if workspace.status.configured {
                theme::SUCCESS_COLOR
            } else {
                ui.visuals().weak_text_color()
            },
            ptext(locale, configured),
        );
        let authenticated = if workspace.status.authenticated {
            PText::Authenticated
        } else {
            PText::NotAuthenticated
        };
        ui.colored_label(
            if workspace.status.authenticated {
                theme::SUCCESS_COLOR
            } else {
                ui.visuals().weak_text_color()
            },
            ptext(locale, authenticated),
        );
        if running {
            ui.spinner();
            ui.label(ptext(locale, PText::Running));
        }
        ui.separator();
        if tool_button(
            ui,
            icons::refresh_cw(),
            ptext(locale, PText::RefreshLive),
            clean && !running,
        )
        .clicked()
        {
            actions.push(ProviderAction::RefreshLive);
        }
        let activation_label = if selected_is_active {
            PText::ReapplyProvider
        } else {
            PText::ActivateProvider
        };
        if tool_button(
            ui,
            icons::circle_check(),
            ptext(locale, activation_label),
            enabled && selected_id.is_some(),
        )
        .clicked()
        {
            let kind = if selected_is_active {
                LiveMutationKind::Reapply
            } else {
                LiveMutationKind::Switch {
                    target_profile_id: selected_id.unwrap_or_default(),
                }
            };
            actions.push(ProviderAction::RequestLiveMutation(kind));
        }
        if tool_button(
            ui,
            icons::save(),
            ptext(locale, PText::BackfillProvider),
            enabled && selected_is_active && selected_is_ordinary,
        )
        .clicked()
        {
            actions.push(ProviderAction::RequestLiveMutation(
                LiveMutationKind::Backfill,
            ));
        }
        if tool_button(
            ui,
            icons::trash_2(),
            ptext(locale, PText::ClearLive),
            enabled && workspace.status.configured,
        )
        .clicked()
        {
            actions.push(ProviderAction::RequestLiveMutation(LiveMutationKind::Clear));
        }
    });
    render_live_evidence(ui, state, locale);
    ui.add_space(2.0);
}

fn render_live_evidence(ui: &mut egui::Ui, state: &ProviderViewState, locale: Locale) {
    if let Some(failure) = &state.live.failure {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(theme::ERROR_COLOR, ptext(locale, PText::LiveMutationFailed));
            let failure_kind = match failure.kind {
                LiveMutationFailureKind::Activation(kind) => format!("{kind:?}"),
                LiveMutationFailureKind::WorkerStopped => "WorkerStopped".to_string(),
            };
            ui.label(egui::RichText::new(failure_kind).weak());
            ui.label(ptext(locale, rollback_text(failure.rollback)));
        });
    } else if state.live.mutation_phase == OperationPhase::Ready {
        ui.label(ptext(locale, rollback_text(state.live.rollback)));
    }
    if let Some(path) = state.live.backup_path.as_deref() {
        ui.horizontal(|ui| {
            ui.label(ptext(locale, PText::BackupPath));
            ui.add(egui::Label::new(path).selectable(true).truncate());
        });
    }
}

fn rollback_text(rollback: ProviderRollbackOutcome) -> PText {
    match rollback {
        ProviderRollbackOutcome::NotRequired => PText::RollbackNotRequired,
        ProviderRollbackOutcome::Verified => PText::RollbackVerified,
        ProviderRollbackOutcome::Failed => PText::RollbackFailed,
    }
}

fn render_live_files(
    ui: &mut egui::Ui,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    let Some(workspace) = state.live.workspace.as_ref() else {
        return;
    };
    render_live_file(
        ui,
        state,
        locale,
        actions,
        ProviderLiveFileKind::Config,
        &workspace.files.config_path,
    );
    ui.separator();
    render_live_file(
        ui,
        state,
        locale,
        actions,
        ProviderLiveFileKind::Auth,
        &workspace.files.auth_path,
    );
}

fn render_live_file(
    ui: &mut egui::Ui,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
    kind: ProviderLiveFileKind,
    path: &str,
) {
    let is_config = kind == ProviderLiveFileKind::Config;
    ui.label(
        egui::RichText::new(if is_config {
            "config.toml"
        } else {
            "auth.json"
        })
        .strong(),
    );
    ui.add(egui::Label::new(path).selectable(true).truncate())
        .on_hover_text(path);
    let editing = state.live_file_editing(kind);
    let revealed = state.live_file_revealed(kind);
    let mut draft = state.live_file_draft(kind).unwrap_or_default().to_string();
    if editing || revealed {
        let response = ui.add_sized(
            [ui.available_width(), 112.0],
            egui::TextEdit::multiline(&mut draft)
                .code_editor()
                .interactive(editing),
        );
        if editing && response.changed() {
            actions.push(ProviderAction::EditLiveFile {
                kind,
                contents: draft,
            });
        }
    } else {
        ui.add_sized(
            [ui.available_width(), 42.0],
            egui::Label::new(ptext(
                locale,
                if is_config {
                    PText::LiveConfigHidden
                } else {
                    PText::LiveAuthHidden
                },
            )),
        );
    }
    ui.horizontal(|ui| {
        if editing {
            let save_label = if is_config {
                PText::SaveLiveConfig
            } else {
                PText::SaveLiveAuth
            };
            if ui
                .add_enabled(
                    state.live_file_dirty(kind)
                        && state.live.mutation_phase != OperationPhase::Running,
                    egui::Button::new(ptext(locale, save_label)),
                )
                .clicked()
            {
                actions.push(ProviderAction::RequestLiveMutation(
                    LiveMutationKind::SaveFile(kind),
                ));
            }
            let cancel_label = if is_config {
                PText::CancelLiveConfig
            } else {
                PText::CancelLiveAuth
            };
            if ui.button(ptext(locale, cancel_label)).clicked() {
                actions.push(ProviderAction::CancelLiveFileEdit(kind));
            }
        } else {
            let reveal_label = match (is_config, revealed) {
                (true, false) => PText::RevealLiveConfig,
                (true, true) => PText::HideLiveConfig,
                (false, false) => PText::RevealLiveAuth,
                (false, true) => PText::HideLiveAuth,
            };
            if tool_button(
                ui,
                if revealed {
                    icons::eye_off()
                } else {
                    icons::eye()
                },
                ptext(locale, reveal_label),
                state.live.mutation_phase != OperationPhase::Running,
            )
            .clicked()
            {
                actions.push(ProviderAction::SetLiveFileRevealed {
                    kind,
                    revealed: !revealed,
                });
            }
            let edit_label = if is_config {
                PText::EditLiveConfig
            } else {
                PText::EditLiveAuth
            };
            if ui
                .add_enabled(
                    !state.is_dirty() && state.live.mutation_phase != OperationPhase::Running,
                    egui::Button::new(ptext(locale, edit_label)),
                )
                .clicked()
            {
                actions.push(ProviderAction::BeginLiveFileEdit(kind));
            }
        }
    });
}

fn render_tabs(
    ui: &mut egui::Ui,
    kind: ProviderKind,
    selected: ProviderEditorTab,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) -> ProviderEditorTab {
    let tabs: &[(ProviderEditorTab, PText)] = match kind {
        ProviderKind::Ordinary => &[
            (ProviderEditorTab::General, PText::General),
            (ProviderEditorTab::Models, PText::Models),
            (ProviderEditorTab::Config, PText::Config),
            (ProviderEditorTab::Live, PText::Live),
            (ProviderEditorTab::Diagnostics, PText::Diagnostics),
        ],
        ProviderKind::Aggregate => &[
            (ProviderEditorTab::Routing, PText::Routing),
            (ProviderEditorTab::Live, PText::Live),
            (ProviderEditorTab::Diagnostics, PText::Diagnostics),
        ],
    };
    let effective = if tabs.iter().any(|(tab, _)| *tab == selected) {
        selected
    } else {
        tabs[0].0
    };
    ui.horizontal(|ui| {
        for (tab, label) in tabs {
            if ui
                .add(egui::Button::new(ptext(locale, *label)).selected(*tab == effective))
                .clicked()
            {
                actions.push(ProviderAction::SetTab(*tab));
            }
        }
    });
    effective
}

fn render_general(
    ui: &mut egui::Ui,
    profile: &codex_plus_core::settings::RelayProfile,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    render_presets(ui, locale, actions);
    ui.separator();
    text_field(
        ui,
        locale,
        PText::Name,
        &profile.name,
        false,
        ProviderEdit::Name,
        actions,
    );

    combo_field(
        ui,
        locale,
        PText::Mode,
        mode_label(locale, profile.relay_mode),
        |ui| {
            for mode in [RelayMode::Official, RelayMode::MixedApi, RelayMode::PureApi] {
                if ui
                    .selectable_label(profile.relay_mode == mode, mode_label(locale, mode))
                    .clicked()
                {
                    actions.push(ProviderAction::Edit(ProviderEdit::Mode(mode)));
                }
            }
        },
    );
    combo_field(
        ui,
        locale,
        PText::Protocol,
        protocol_label(locale, profile.protocol),
        |ui| {
            for protocol in [RelayProtocol::Responses, RelayProtocol::ChatCompletions] {
                if ui
                    .selectable_label(
                        profile.protocol == protocol,
                        protocol_label(locale, protocol),
                    )
                    .clicked()
                {
                    actions.push(ProviderAction::Edit(ProviderEdit::Protocol(protocol)));
                }
            }
        },
    );
    text_field(
        ui,
        locale,
        PText::BaseUrl,
        &profile.upstream_base_url,
        false,
        ProviderEdit::BaseUrl,
        actions,
    );

    ui.horizontal(|ui| {
        ui.add_sized(
            [128.0, 24.0],
            egui::Label::new(ptext(locale, PText::ApiKey)),
        );
        if state.secret_revealed {
            let mut value = profile.api_key.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut value)
                        .desired_width(ui.available_width() - 36.0),
                )
                .changed()
            {
                actions.push(ProviderAction::Edit(ProviderEdit::ApiKey(value)));
            }
        } else {
            let mut masked = if profile.api_key.is_empty() {
                String::new()
            } else {
                "••••••••••••".to_owned()
            };
            ui.add_enabled(
                false,
                egui::TextEdit::singleline(&mut masked)
                    .password(true)
                    .desired_width(ui.available_width() - 36.0),
            );
        }
        let (icon, label) = if state.secret_revealed {
            (icons::eye_off(), ptext(locale, PText::HideSecret))
        } else {
            (icons::eye(), ptext(locale, PText::ShowSecret))
        };
        if tool_button(ui, icon, label, true).clicked() {
            actions.push(ProviderAction::SetSecretRevealed(!state.secret_revealed));
        }
    });

    text_field(
        ui,
        locale,
        PText::Model,
        &profile.model,
        false,
        ProviderEdit::Model,
        actions,
    );
    text_field(
        ui,
        locale,
        PText::TestModel,
        &profile.test_model,
        false,
        ProviderEdit::TestModel,
        actions,
    );
    let mut common = profile.use_common_config;
    if ui
        .checkbox(&mut common, ptext(locale, PText::UseCommonConfig))
        .changed()
    {
        actions.push(ProviderAction::Edit(ProviderEdit::UseCommonConfig(common)));
    }
    text_field(
        ui,
        locale,
        PText::ContextWindow,
        &profile.context_window,
        false,
        ProviderEdit::ContextWindow,
        actions,
    );
    text_field(
        ui,
        locale,
        PText::CompactLimit,
        &profile.auto_compact_limit,
        false,
        ProviderEdit::AutoCompactLimit,
        actions,
    );
    combo_field(
        ui,
        locale,
        PText::InsertMode,
        insert_mode_label(locale, profile.model_insert_mode),
        |ui| {
            for mode in [
                RelayModelInsertMode::Patch,
                RelayModelInsertMode::ModelCatalog,
            ] {
                if ui
                    .selectable_label(
                        profile.model_insert_mode == mode,
                        insert_mode_label(locale, mode),
                    )
                    .clicked()
                {
                    actions.push(ProviderAction::Edit(ProviderEdit::InsertMode(mode)));
                }
            }
        },
    );
    text_field(
        ui,
        locale,
        PText::UserAgent,
        &profile.user_agent,
        false,
        ProviderEdit::UserAgent,
        actions,
    );
}

fn render_presets(ui: &mut egui::Ui, locale: Locale, actions: &mut Vec<ProviderAction>) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [128.0, 24.0],
            egui::Label::new(ptext(locale, PText::Presets)),
        );
        let search_id = ui.make_persistent_id("provider_preset_search");
        let mut search = ui
            .ctx()
            .data_mut(|data| data.get_temp::<String>(search_id).unwrap_or_default());
        if ui
            .add(
                egui::TextEdit::singleline(&mut search)
                    .hint_text(ptext(locale, PText::PresetSearch)),
            )
            .changed()
        {
            ui.ctx()
                .data_mut(|data| data.insert_temp(search_id, search.clone()));
        }
        egui::ComboBox::from_id_salt("provider_preset_menu")
            .selected_text(ptext(locale, PText::Presets))
            .show_ui(ui, |ui| {
                if let Ok(presets) = provider_presets() {
                    let needle = search.trim().to_lowercase();
                    for preset in presets.iter().filter(|preset| {
                        needle.is_empty()
                            || preset.name.to_lowercase().contains(&needle)
                            || preset.id.to_lowercase().contains(&needle)
                    }) {
                        if ui.selectable_label(false, &preset.name).clicked() {
                            actions.push(ProviderAction::ApplyPreset(preset.id.clone()));
                            ui.close();
                        }
                    }
                }
            });
    });
}

fn render_models(
    ui: &mut egui::Ui,
    profile: &codex_plus_core::settings::RelayProfile,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    ui.horizontal(|ui| {
        if ui.button(ptext(locale, PText::AddModel)).clicked() {
            actions.push(ProviderAction::AddModel);
        }
        if ui.button(ptext(locale, PText::DiscoverModels)).clicked() {
            actions.push(ProviderAction::FetchModels);
        }
        let can_merge = state
            .models
            .result
            .as_ref()
            .is_some_and(|result| !result.models.is_empty());
        if ui
            .add_enabled(
                can_merge,
                egui::Button::new(ptext(locale, PText::MergeModels)),
            )
            .clicked()
        {
            actions.push(ProviderAction::MergeModels);
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_sized(
            [ui.available_width() * 0.58, 20.0],
            egui::Label::new(ptext(locale, PText::ModelName)),
        );
        ui.label(ptext(locale, PText::Window));
    });
    for (index, (model, window)) in model_rows(profile).into_iter().enumerate() {
        ui.horizontal(|ui| {
            let mut next_model = model.clone();
            let mut next_window = window.clone();
            let model_changed = ui
                .add(
                    egui::TextEdit::singleline(&mut next_model)
                        .desired_width(ui.available_width() * 0.58),
                )
                .changed();
            let window_changed = ui
                .add(egui::TextEdit::singleline(&mut next_window).desired_width(100.0))
                .changed();
            if model_changed || window_changed {
                actions.push(ProviderAction::Edit(ProviderEdit::ModelRow {
                    index,
                    model: next_model,
                    window: next_window,
                }));
            }
            if tool_button(
                ui,
                icons::trash_2(),
                ptext(locale, PText::RemoveModel),
                true,
            )
            .clicked()
            {
                actions.push(ProviderAction::RemoveModel(index));
            }
        });
    }
}

fn render_config(
    ui: &mut egui::Ui,
    profile: &codex_plus_core::settings::RelayProfile,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    render_secret_document(
        ui,
        locale,
        PText::ConfigToml,
        &profile.config_contents,
        state.config_revealed,
        ProviderAction::SetConfigRevealed,
        ProviderEdit::ConfigContents,
        actions,
    );
    ui.separator();
    render_secret_document(
        ui,
        locale,
        PText::AuthJson,
        &profile.auth_contents,
        state.auth_revealed,
        ProviderAction::SetAuthRevealed,
        ProviderEdit::AuthContents,
        actions,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_secret_document(
    ui: &mut egui::Ui,
    locale: Locale,
    label: PText,
    contents: &str,
    revealed: bool,
    reveal_action: fn(bool) -> ProviderAction,
    edit_action: fn(String) -> ProviderEdit,
    actions: &mut Vec<ProviderAction>,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(ptext(locale, label)).strong());
        let (icon, action_label) = if revealed {
            (icons::eye_off(), ptext(locale, PText::Hide))
        } else {
            (icons::eye(), ptext(locale, PText::Reveal))
        };
        if tool_button(ui, icon, action_label, true).clicked() {
            actions.push(reveal_action(!revealed));
        }
    });
    if revealed {
        let mut value = contents.to_owned();
        if ui
            .add(
                egui::TextEdit::multiline(&mut value)
                    .code_editor()
                    .desired_rows(10)
                    .desired_width(f32::INFINITY),
            )
            .changed()
        {
            actions.push(ProviderAction::Edit(edit_action(value)));
        }
    } else {
        ui.add_sized(
            [ui.available_width(), 72.0],
            egui::Label::new(egui::RichText::new(ptext(locale, PText::Hidden)).weak()),
        );
    }
}

fn render_routing(
    ui: &mut egui::Ui,
    routing: &codex_plus_core::settings::AggregateRelayProfile,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    combo_field(
        ui,
        locale,
        PText::Strategy,
        strategy_label(locale, routing.strategy),
        |ui| {
            for strategy in [
                AggregateRelayStrategy::Failover,
                AggregateRelayStrategy::ConversationRoundRobin,
                AggregateRelayStrategy::RequestRoundRobin,
                AggregateRelayStrategy::WeightedRoundRobin,
            ] {
                if ui
                    .selectable_label(
                        routing.strategy == strategy,
                        strategy_label(locale, strategy),
                    )
                    .clicked()
                {
                    actions.push(ProviderAction::Edit(ProviderEdit::AggregateStrategy(
                        strategy,
                    )));
                }
            }
        },
    );
    ui.separator();
    if let Some(document) = state.draft() {
        let member_suffix = match locale {
            Locale::ZhCn => "成员",
            Locale::En => "member",
        };
        for ordinary in document
            .profiles
            .iter()
            .filter_map(ProviderProfile::ordinary)
        {
            let member = routing
                .members
                .iter()
                .find(|member| member.relay_id == ordinary.id);
            let mut enabled = member.is_some();
            let mut weight = member.map_or(1, |member| member.weight);
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut enabled, format!("{} {member_suffix}", ordinary.name))
                    .changed()
                {
                    actions.push(ProviderAction::SetAggregateMember {
                        profile_id: ordinary.id.clone(),
                        enabled,
                    });
                }
                ui.label(ptext(locale, PText::Weight));
                if ui
                    .add_enabled(enabled, egui::DragValue::new(&mut weight).range(1..=1_000))
                    .changed()
                {
                    actions.push(ProviderAction::SetAggregateWeight {
                        profile_id: ordinary.id.clone(),
                        weight,
                    });
                }
            });
        }
    }
}

fn render_diagnostics(
    ui: &mut egui::Ui,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    ui.horizontal(|ui| {
        if ui.button(ptext(locale, PText::TestConnection)).clicked() {
            actions.push(ProviderAction::Test);
        }
        if ui.button(ptext(locale, PText::FetchModels)).clicked() {
            actions.push(ProviderAction::FetchModels);
        }
        if ui
            .add(egui::Button::image_and_text(
                egui::Image::new(icons::stethoscope()).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                ptext(locale, PText::RunDiagnostics),
            ))
            .clicked()
        {
            actions.push(ProviderAction::Doctor);
        }
    });
    ui.add_space(12.0);
    operation_row(
        ui,
        ptext(locale, PText::TestConnection),
        state.test.phase,
        locale,
    );
    operation_row(
        ui,
        ptext(locale, PText::FetchModels),
        state.models.phase,
        locale,
    );
    operation_row(
        ui,
        ptext(locale, PText::RunDiagnostics),
        state.doctor.phase,
        locale,
    );
}

fn operation_row(ui: &mut egui::Ui, label: &str, phase: OperationPhase, locale: Locale) {
    let (status, color) = match phase {
        OperationPhase::Idle => (ptext(locale, PText::Idle), ui.visuals().weak_text_color()),
        OperationPhase::Running => (ptext(locale, PText::Running), theme::WARNING_COLOR),
        OperationPhase::Ready => (ptext(locale, PText::Success), theme::SUCCESS_COLOR),
        OperationPhase::Error => (ptext(locale, PText::Failed), theme::ERROR_COLOR),
    };
    ui.horizontal(|ui| {
        ui.label(label);
        ui.colored_label(color, status);
    });
}

fn render_save_bar(
    ui: &mut egui::Ui,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    if let Some(error) = state.save.error {
        let message = match error {
            ProviderSaveFailureKind::Conflict => PText::SaveConflict,
            ProviderSaveFailureKind::Validation => PText::SaveValidation,
            ProviderSaveFailureKind::SaveFailed => PText::SaveFailed,
            ProviderSaveFailureKind::WorkerStopped => PText::SaveWorkerStopped,
        };
        ui.colored_label(theme::ERROR_COLOR, ptext(locale, message));
    }
    ui.horizontal(|ui| {
        let (status, color) = if state.is_dirty() {
            (ptext(locale, PText::Unsaved), theme::WARNING_COLOR)
        } else {
            (ptext(locale, PText::Saved), theme::SUCCESS_COLOR)
        };
        ui.colored_label(color, status);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let save_enabled = state.is_dirty() && state.save.phase != OperationPhase::Running;
            if ui
                .add_enabled(
                    save_enabled,
                    egui::Button::image_and_text(
                        egui::Image::new(icons::save()).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                        ptext(locale, PText::SaveChanges),
                    ),
                )
                .clicked()
            {
                actions.push(ProviderAction::Save);
            }
            if ui
                .add_enabled(
                    state.is_dirty(),
                    egui::Button::new(ptext(locale, PText::DiscardChanges)),
                )
                .clicked()
            {
                actions.push(ProviderAction::Discard);
            }
        });
    });
}

fn render_guard(ctx: &egui::Context, locale: Locale, actions: &mut Vec<ProviderAction>) {
    egui::Window::new(ptext(locale, PText::GuardTitle))
        .id(egui::Id::new("provider_dirty_guard"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(ptext(locale, PText::GuardMessage));
            ui.horizontal(|ui| {
                if ui.button(ptext(locale, PText::Stay)).clicked() {
                    actions.push(ProviderAction::ResolveGuard(GuardResolution::Stay));
                }
                if ui
                    .button(ptext(locale, PText::DiscardAndContinue))
                    .clicked()
                {
                    actions.push(ProviderAction::ResolveGuard(GuardResolution::Discard));
                }
                if ui.button(ptext(locale, PText::SaveAndContinue)).clicked() {
                    actions.push(ProviderAction::ResolveGuard(GuardResolution::Save));
                }
            });
        });
}

fn render_live_confirmation(
    ctx: &egui::Context,
    state: &ProviderViewState,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    let Some(kind) = state.pending_live_confirmation() else {
        return;
    };
    egui::Window::new(ptext(locale, PText::ConfirmLiveTitle))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(ptext(locale, live_action_text(kind)));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(ptext(locale, PText::CancelLiveChange)).clicked() {
                    actions.push(ProviderAction::CancelLiveMutation);
                }
                if ui
                    .add_enabled(
                        state.live.mutation_phase != OperationPhase::Running,
                        egui::Button::new(ptext(locale, PText::ConfirmLiveNow)),
                    )
                    .clicked()
                {
                    actions.push(ProviderAction::ConfirmLiveMutation);
                }
            });
        });
}

fn live_action_text(kind: &LiveMutationKind) -> PText {
    match kind {
        LiveMutationKind::Switch { .. } => PText::ActivateProvider,
        LiveMutationKind::Reapply => PText::ReapplyProvider,
        LiveMutationKind::Backfill => PText::BackfillProvider,
        LiveMutationKind::Clear => PText::ClearLive,
        LiveMutationKind::SaveFile(ProviderLiveFileKind::Config) => PText::SaveLiveConfig,
        LiveMutationKind::SaveFile(ProviderLiveFileKind::Auth) => PText::SaveLiveAuth,
    }
}

fn render_delete_confirmation(
    ctx: &egui::Context,
    locale: Locale,
    actions: &mut Vec<ProviderAction>,
) {
    egui::Window::new(ptext(locale, PText::DeleteTitle))
        .id(egui::Id::new("provider_delete_confirmation"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(ptext(locale, PText::DeleteMessage));
            ui.horizontal(|ui| {
                if ui.button(ptext(locale, PText::Cancel)).clicked() {
                    actions.push(ProviderAction::CancelDelete);
                }
                if ui.button(ptext(locale, PText::ConfirmDelete)).clicked() {
                    actions.push(ProviderAction::Delete { confirmed: true });
                }
            });
        });
}

fn text_field(
    ui: &mut egui::Ui,
    locale: Locale,
    label: PText,
    current: &str,
    password: bool,
    edit: impl FnOnce(String) -> ProviderEdit,
    actions: &mut Vec<ProviderAction>,
) {
    ui.horizontal(|ui| {
        ui.add_sized([128.0, 24.0], egui::Label::new(ptext(locale, label)));
        let mut value = current.to_owned();
        if ui
            .add(
                egui::TextEdit::singleline(&mut value)
                    .password(password)
                    .desired_width(f32::INFINITY),
            )
            .changed()
        {
            actions.push(ProviderAction::Edit(edit(value)));
        }
    });
}

fn combo_field(
    ui: &mut egui::Ui,
    locale: Locale,
    label: PText,
    selected: &str,
    contents: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.add_sized([128.0, 24.0], egui::Label::new(ptext(locale, label)));
        let response = egui::ComboBox::from_id_salt(ptext(Locale::En, label))
            .selected_text(selected)
            .width(ui.available_width())
            .show_ui(ui, contents);
        response
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, selected));
    });
}

fn tool_button(
    ui: &mut egui::Ui,
    icon: egui::ImageSource<'static>,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::image(egui::Image::new(icon).fit_to_exact_size(egui::vec2(16.0, 16.0)))
            .min_size(egui::vec2(30.0, 30.0)),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response.on_hover_text(label)
}

fn model_rows(profile: &codex_plus_core::settings::RelayProfile) -> Vec<(String, String)> {
    let windows = serde_json::from_str::<BTreeMap<String, String>>(&profile.model_windows)
        .unwrap_or_default();
    profile
        .model_list
        .lines()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(|model| {
            (
                model.to_owned(),
                windows.get(model).cloned().unwrap_or_default(),
            )
        })
        .collect()
}

fn mode_label(locale: Locale, mode: RelayMode) -> &'static str {
    ptext(
        locale,
        match mode {
            RelayMode::Official => PText::Official,
            RelayMode::MixedApi => PText::MixedApi,
            RelayMode::PureApi => PText::PureApi,
            RelayMode::Aggregate => PText::Aggregate,
        },
    )
}

fn protocol_label(locale: Locale, protocol: RelayProtocol) -> &'static str {
    ptext(
        locale,
        match protocol {
            RelayProtocol::Responses => PText::Responses,
            RelayProtocol::ChatCompletions => PText::ChatCompletions,
        },
    )
}

fn insert_mode_label(locale: Locale, mode: RelayModelInsertMode) -> &'static str {
    ptext(
        locale,
        match mode {
            RelayModelInsertMode::Patch => PText::Patch,
            RelayModelInsertMode::ModelCatalog => PText::ModelCatalog,
        },
    )
}

fn strategy_label(locale: Locale, strategy: AggregateRelayStrategy) -> &'static str {
    ptext(
        locale,
        match strategy {
            AggregateRelayStrategy::Failover => PText::Failover,
            AggregateRelayStrategy::ConversationRoundRobin => PText::ConversationRoundRobin,
            AggregateRelayStrategy::RequestRoundRobin => PText::RequestRoundRobin,
            AggregateRelayStrategy::WeightedRoundRobin => PText::WeightedRoundRobin,
        },
    )
}
