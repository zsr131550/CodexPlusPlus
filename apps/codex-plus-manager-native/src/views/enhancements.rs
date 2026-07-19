use codex_plus_core::settings::LaunchMode;
use codex_plus_manager_service::EnhancementSettings;
use eframe::egui;

use crate::i18n::Locale;
use crate::state::enhancements::{
    EnhancementFailureKind, EnhancementOperationPhase, EnhancementViewState,
};
use crate::{icons, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnhancementAction {
    Refresh,
    Edit(EnhancementSettings),
    Save,
    RequestReset,
    ConfirmReset,
    CancelReset,
    ReloadConflict,
    DiscardChanges,
}

pub fn render(
    ui: &mut egui::Ui,
    state: &EnhancementViewState,
    locale: Locale,
    actions: &mut Vec<EnhancementAction>,
) {
    render_feedback(ui, state, locale, actions);

    if !state.initialized {
        ui.centered_and_justified(|ui| {
            ui.label(etext(locale, EText::Loading));
        });
        return;
    }

    let busy = matches!(
        state.operation_phase,
        EnhancementOperationPhase::Saving | EnhancementOperationPhase::Resetting
    );
    let mut draft = *state.draft();
    let original = draft;
    let content_height = (ui.available_height() - 52.0).max(240.0);

    egui::ScrollArea::vertical()
        .id_salt("enhancements_page_scroll")
        .auto_shrink([false, false])
        .max_height(content_height)
        .show(ui, |ui| {
            ui.heading(etext(locale, EText::Enhancements));
            ui.add_space(6.0);
            ui.add_enabled_ui(!busy, |ui| {
                ui.checkbox(&mut draft.enabled, etext(locale, EText::EnableEnhancements));
            });

            ui.add_space(8.0);
            ui.label(egui::RichText::new(etext(locale, EText::LaunchMode)).strong());
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    let width = 144.0;
                    if ui
                        .add_sized(
                            [width, 30.0],
                            egui::Button::new(etext(locale, EText::FullEnhancements))
                                .selected(draft.launch_mode == LaunchMode::Patch),
                        )
                        .clicked()
                    {
                        draft.launch_mode = LaunchMode::Patch;
                    }
                    if ui
                        .add_sized(
                            [width, 30.0],
                            egui::Button::new(etext(locale, EText::Compatibility))
                                .selected(draft.launch_mode == LaunchMode::Relay),
                        )
                        .clicked()
                    {
                        draft.launch_mode = LaunchMode::Relay;
                    }
                });
            });

            ui.add_space(10.0);
            let controls_enabled = draft.enabled && !busy;
            if ui.available_width() >= 760.0 {
                ui.columns(2, |columns| {
                    render_plugin_and_conversation(
                        &mut columns[0],
                        &mut draft,
                        locale,
                        controls_enabled,
                    );
                    render_interface_and_remote(
                        &mut columns[1],
                        &mut draft,
                        locale,
                        controls_enabled,
                    );
                });
            } else {
                render_plugin_and_conversation(ui, &mut draft, locale, controls_enabled);
                ui.add_space(12.0);
                render_interface_and_remote(ui, &mut draft, locale, controls_enabled);
            }
        });

    ui.separator();
    ui.add_space(6.0);
    render_command_bar(ui, state, locale, busy, actions);

    if draft != original {
        actions.push(EnhancementAction::Edit(draft));
    }
    if state.reset_confirmation_pending() {
        render_reset_confirmation(ui.ctx(), locale, actions);
    }
}

fn render_command_bar(
    ui: &mut egui::Ui,
    state: &EnhancementViewState,
    locale: Locale,
    busy: bool,
    actions: &mut Vec<EnhancementAction>,
) {
    ui.horizontal(|ui| {
        if ui
            .add_enabled_ui(state.is_dirty() && !busy, |ui| {
                ui.add_sized(
                    [160.0, 34.0],
                    egui::Button::image_and_text(
                        egui::Image::new(icons::save()).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                        etext(locale, EText::Save),
                    ),
                )
            })
            .inner
            .clicked()
        {
            actions.push(EnhancementAction::Save);
        }
        if ui
            .add_enabled_ui(!busy, |ui| {
                ui.add_sized(
                    [160.0, 34.0],
                    egui::Button::image_and_text(
                        egui::Image::new(icons::rotate_ccw())
                            .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                        etext(locale, EText::Reset),
                    ),
                )
            })
            .inner
            .clicked()
        {
            actions.push(EnhancementAction::RequestReset);
        }
        if state.is_dirty()
            && ui
                .add_sized(
                    [160.0, 34.0],
                    egui::Button::new(etext(locale, EText::DiscardChanges)),
                )
                .clicked()
        {
            actions.push(EnhancementAction::DiscardChanges);
        }
    });
}

fn render_plugin_and_conversation(
    ui: &mut egui::Ui,
    draft: &mut EnhancementSettings,
    locale: Locale,
    enabled: bool,
) {
    group_heading(ui, etext(locale, EText::PluginsAndModels));
    ui.add_enabled_ui(enabled, |ui| {
        ui.checkbox(
            &mut draft.computer_use_guard,
            etext(locale, EText::ComputerUseGuard),
        );
        ui.add_enabled_ui(draft.launch_mode == LaunchMode::Patch, |ui| {
            ui.checkbox(
                &mut draft.plugin_marketplace_unlock,
                etext(locale, EText::PluginMarketplaceUnlock),
            );
            ui.checkbox(
                &mut draft.plugin_auto_expand,
                etext(locale, EText::PluginAutoExpand),
            );
        });
        ui.checkbox(
            &mut draft.model_whitelist_unlock,
            etext(locale, EText::ModelWhitelistUnlock),
        );
        ui.checkbox(
            &mut draft.service_tier_controls,
            etext(locale, EText::ServiceTierControls),
        );
    });

    ui.add_space(12.0);
    group_heading(ui, etext(locale, EText::ConversationAndInput));
    ui.add_enabled_ui(enabled, |ui| {
        ui.checkbox(
            &mut draft.session_delete,
            etext(locale, EText::SessionDelete),
        );
        ui.checkbox(
            &mut draft.markdown_export,
            etext(locale, EText::MarkdownExport),
        );
        ui.checkbox(&mut draft.paste_fix, etext(locale, EText::PasteFix));
        ui.checkbox(&mut draft.project_move, etext(locale, EText::ProjectMove));
        ui.checkbox(
            &mut draft.thread_id_badge,
            etext(locale, EText::ThreadIdBadge),
        );
        ui.checkbox(
            &mut draft.conversation_view,
            etext(locale, EText::ConversationView),
        );
        ui.checkbox(
            &mut draft.thread_scroll_restore,
            etext(locale, EText::ThreadScrollRestore),
        );
    });
}

fn render_interface_and_remote(
    ui: &mut egui::Ui,
    draft: &mut EnhancementSettings,
    locale: Locale,
    enabled: bool,
) {
    group_heading(ui, etext(locale, EText::InterfaceAndStartup));
    ui.add_enabled_ui(enabled, |ui| {
        #[cfg(windows)]
        ui.checkbox(
            &mut draft.pet_real_mouse_look,
            etext(locale, EText::PetRealMouseLook),
        );
        ui.checkbox(
            &mut draft.force_chinese_locale,
            etext(locale, EText::ForceChineseLocale),
        );
        ui.checkbox(&mut draft.fast_startup, etext(locale, EText::FastStartup));
        ui.checkbox(
            &mut draft.native_menu_placement,
            etext(locale, EText::NativeMenuPlacement),
        );
        ui.checkbox(
            &mut draft.native_menu_localization,
            etext(locale, EText::NativeMenuLocalization),
        );
    });

    ui.add_space(12.0);
    group_heading(ui, etext(locale, EText::RemoteProjects));
    ui.add_enabled_ui(enabled, |ui| {
        ui.checkbox(
            &mut draft.zed_remote_open,
            etext(locale, EText::ZedRemoteOpen),
        );
        ui.checkbox(
            &mut draft.upstream_worktree_create,
            etext(locale, EText::UpstreamWorktreeCreate),
        );
    });
}

fn group_heading(ui: &mut egui::Ui, label: &str) {
    ui.label(egui::RichText::new(label).strong().size(14.0));
    ui.separator();
    ui.add_space(3.0);
}

fn render_feedback(
    ui: &mut egui::Ui,
    state: &EnhancementViewState,
    locale: Locale,
    actions: &mut Vec<EnhancementAction>,
) {
    if state.error == Some(EnhancementFailureKind::SettingsConflict) {
        ui.horizontal(|ui| {
            ui.colored_label(theme::WARNING_COLOR, etext(locale, EText::Conflict));
            if ui.button(etext(locale, EText::ReloadCurrent)).clicked() {
                actions.push(EnhancementAction::ReloadConflict);
            }
        });
    } else if let Some(error) = state.error.or(state.load_error) {
        ui.colored_label(theme::ERROR_COLOR, failure_text(locale, error));
    } else if state.operation_phase == EnhancementOperationPhase::Ready {
        ui.colored_label(theme::SUCCESS_COLOR, etext(locale, EText::Saved));
    } else if state.is_dirty() {
        ui.colored_label(theme::WARNING_COLOR, etext(locale, EText::Unsaved));
    }
}

fn render_reset_confirmation(
    context: &egui::Context,
    locale: Locale,
    actions: &mut Vec<EnhancementAction>,
) {
    egui::Window::new(etext(locale, EText::ResetTitle))
        .id(egui::Id::new("enhancements_reset_confirmation"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(context, |ui| {
            ui.label(etext(locale, EText::ResetMessage));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(etext(locale, EText::ConfirmReset)).clicked() {
                    actions.push(EnhancementAction::ConfirmReset);
                }
                if ui.button(etext(locale, EText::Cancel)).clicked() {
                    actions.push(EnhancementAction::CancelReset);
                }
            });
        });
}

fn failure_text(locale: Locale, kind: EnhancementFailureKind) -> &'static str {
    match kind {
        EnhancementFailureKind::SettingsReadFailed => etext(locale, EText::ReadFailed),
        EnhancementFailureKind::SettingsWriteFailed => etext(locale, EText::WriteFailed),
        EnhancementFailureKind::SettingsConflict => etext(locale, EText::Conflict),
        EnhancementFailureKind::InvalidRevision => etext(locale, EText::InvalidRevision),
        EnhancementFailureKind::ConfirmationRequired => etext(locale, EText::ConfirmationRequired),
        EnhancementFailureKind::WorkerStopped => etext(locale, EText::WorkerStopped),
    }
}

#[derive(Clone, Copy)]
enum EText {
    Enhancements,
    EnableEnhancements,
    ComputerUseGuard,
    LaunchMode,
    FullEnhancements,
    Compatibility,
    PluginsAndModels,
    PluginMarketplaceUnlock,
    PluginAutoExpand,
    ModelWhitelistUnlock,
    ServiceTierControls,
    ConversationAndInput,
    SessionDelete,
    MarkdownExport,
    PasteFix,
    ProjectMove,
    ThreadIdBadge,
    ConversationView,
    ThreadScrollRestore,
    InterfaceAndStartup,
    PetRealMouseLook,
    ForceChineseLocale,
    FastStartup,
    NativeMenuPlacement,
    NativeMenuLocalization,
    RemoteProjects,
    ZedRemoteOpen,
    UpstreamWorktreeCreate,
    Save,
    Reset,
    DiscardChanges,
    Loading,
    Conflict,
    ReloadCurrent,
    Saved,
    Unsaved,
    ResetTitle,
    ResetMessage,
    ConfirmReset,
    Cancel,
    ReadFailed,
    WriteFailed,
    InvalidRevision,
    ConfirmationRequired,
    WorkerStopped,
}

fn etext(locale: Locale, key: EText) -> &'static str {
    match (locale, key) {
        (Locale::ZhCn, EText::Enhancements) => "增强功能",
        (Locale::En, EText::Enhancements) => "Enhancements",
        (Locale::ZhCn, EText::EnableEnhancements) => "启用增强功能",
        (Locale::En, EText::EnableEnhancements) => "Enable enhancements",
        (_, EText::ComputerUseGuard) => "Computer Use Guard",
        (Locale::ZhCn, EText::LaunchMode) => "启动模式",
        (Locale::En, EText::LaunchMode) => "Launch mode",
        (Locale::ZhCn, EText::FullEnhancements) => "完整增强",
        (Locale::En, EText::FullEnhancements) => "Full enhancements",
        (Locale::ZhCn, EText::Compatibility) => "兼容增强",
        (Locale::En, EText::Compatibility) => "Compatibility",
        (Locale::ZhCn, EText::PluginsAndModels) => "插件与模型",
        (Locale::En, EText::PluginsAndModels) => "Plugins and models",
        (Locale::ZhCn, EText::PluginMarketplaceUnlock) => "插件市场解锁",
        (Locale::En, EText::PluginMarketplaceUnlock) => "Plugin marketplace unlock",
        (Locale::ZhCn, EText::PluginAutoExpand) => "自动展开插件列表",
        (Locale::En, EText::PluginAutoExpand) => "Auto-expand plugin list",
        (Locale::ZhCn, EText::ModelWhitelistUnlock) => "模型白名单解锁",
        (Locale::En, EText::ModelWhitelistUnlock) => "Model whitelist unlock",
        (Locale::ZhCn, EText::ServiceTierControls) => "服务档位控件",
        (Locale::En, EText::ServiceTierControls) => "Service tier controls",
        (Locale::ZhCn, EText::ConversationAndInput) => "对话与输入",
        (Locale::En, EText::ConversationAndInput) => "Conversation and input",
        (Locale::ZhCn, EText::SessionDelete) => "会话删除",
        (Locale::En, EText::SessionDelete) => "Session delete",
        (Locale::ZhCn, EText::MarkdownExport) => "Markdown 导出",
        (Locale::En, EText::MarkdownExport) => "Markdown export",
        (Locale::ZhCn, EText::PasteFix) => "纯文本粘贴修复",
        (Locale::En, EText::PasteFix) => "Plain-text paste fix",
        (Locale::ZhCn, EText::ProjectMove) => "跨项目移动会话",
        (Locale::En, EText::ProjectMove) => "Move conversations between projects",
        (Locale::ZhCn, EText::ThreadIdBadge) => "会话 ID 标识",
        (Locale::En, EText::ThreadIdBadge) => "Thread ID badge",
        (Locale::ZhCn, EText::ConversationView) => "对话居中宽度",
        (Locale::En, EText::ConversationView) => "Centered conversation view",
        (Locale::ZhCn, EText::ThreadScrollRestore) => "恢复会话滚动位置",
        (Locale::En, EText::ThreadScrollRestore) => "Restore thread scroll position",
        (Locale::ZhCn, EText::InterfaceAndStartup) => "界面与启动",
        (Locale::En, EText::InterfaceAndStartup) => "Interface and startup",
        (Locale::ZhCn, EText::PetRealMouseLook) => "宠物跟随真实鼠标",
        (Locale::En, EText::PetRealMouseLook) => "Pet follows real mouse",
        (Locale::ZhCn, EText::ForceChineseLocale) => "强制中文界面",
        (Locale::En, EText::ForceChineseLocale) => "Force Chinese locale",
        (Locale::ZhCn, EText::FastStartup) => "快速启动",
        (Locale::En, EText::FastStartup) => "Fast startup",
        (Locale::ZhCn, EText::NativeMenuPlacement) => "原生菜单位置",
        (Locale::En, EText::NativeMenuPlacement) => "Native menu placement",
        (Locale::ZhCn, EText::NativeMenuLocalization) => "原生菜单本地化",
        (Locale::En, EText::NativeMenuLocalization) => "Native menu localization",
        (Locale::ZhCn, EText::RemoteProjects) => "远程项目",
        (Locale::En, EText::RemoteProjects) => "Remote projects",
        (Locale::ZhCn, EText::ZedRemoteOpen) => "使用 Zed Remote 打开",
        (Locale::En, EText::ZedRemoteOpen) => "Open files in Zed Remote",
        (Locale::ZhCn, EText::UpstreamWorktreeCreate) => "创建 upstream worktree",
        (Locale::En, EText::UpstreamWorktreeCreate) => "Create upstream worktree",
        (Locale::ZhCn, EText::Save) => "保存增强设置",
        (Locale::En, EText::Save) => "Save enhancements",
        (Locale::ZhCn, EText::Reset) => "重置增强设置",
        (Locale::En, EText::Reset) => "Reset enhancements",
        (Locale::ZhCn, EText::DiscardChanges) => "放弃更改",
        (Locale::En, EText::DiscardChanges) => "Discard changes",
        (Locale::ZhCn, EText::Loading) => "正在加载增强设置",
        (Locale::En, EText::Loading) => "Loading enhancement settings",
        (Locale::ZhCn, EText::Conflict) => "增强设置已在磁盘上更改",
        (Locale::En, EText::Conflict) => "Enhancement settings changed on disk",
        (Locale::ZhCn, EText::ReloadCurrent) => "重新加载当前设置",
        (Locale::En, EText::ReloadCurrent) => "Reload current settings",
        (Locale::ZhCn, EText::Saved) => "增强设置已保存",
        (Locale::En, EText::Saved) => "Enhancement settings saved",
        (Locale::ZhCn, EText::Unsaved) => "增强设置有未保存更改",
        (Locale::En, EText::Unsaved) => "Enhancement settings have unsaved changes",
        (Locale::ZhCn, EText::ResetTitle) => "重置增强设置？",
        (Locale::En, EText::ResetTitle) => "Reset enhancement settings?",
        (Locale::ZhCn, EText::ResetMessage) => "仅增强功能组会恢复默认值。",
        (Locale::En, EText::ResetMessage) => {
            "Only the enhancement settings group will return to defaults."
        }
        (Locale::ZhCn, EText::ConfirmReset) => "重置",
        (Locale::En, EText::ConfirmReset) => "Reset",
        (Locale::ZhCn, EText::Cancel) => "取消",
        (Locale::En, EText::Cancel) => "Cancel",
        (Locale::ZhCn, EText::ReadFailed) => "无法读取增强设置",
        (Locale::En, EText::ReadFailed) => "Could not read enhancement settings",
        (Locale::ZhCn, EText::WriteFailed) => "无法保存增强设置",
        (Locale::En, EText::WriteFailed) => "Could not save enhancement settings",
        (Locale::ZhCn, EText::InvalidRevision) => "增强设置版本已失效，请重新加载",
        (Locale::En, EText::InvalidRevision) => {
            "Enhancement settings revision expired; reload before saving"
        }
        (Locale::ZhCn, EText::ConfirmationRequired) => "重置需要明确确认",
        (Locale::En, EText::ConfirmationRequired) => "Reset requires confirmation",
        (Locale::ZhCn, EText::WorkerStopped) => "增强设置后台服务已停止",
        (Locale::En, EText::WorkerStopped) => "The enhancement settings worker has stopped",
    }
}
