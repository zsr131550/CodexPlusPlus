use std::sync::Arc;

use codex_plus_core::settings::BackendSettings;
use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::enhancements::EnhancementViewState;
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::enhancements::{self, EnhancementAction};
use codex_plus_manager_service::{
    EnhancementSettingsEnvironment, EnhancementSettingsService, EnhancementWorkspace,
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

#[derive(Clone)]
struct StaticEnvironment(BackendSettings);

impl EnhancementSettingsEnvironment for StaticEnvironment {
    fn load_enhancement_settings(&self) -> anyhow::Result<BackendSettings> {
        Ok(self.0.clone())
    }

    fn update_enhancement_settings_if<F>(
        &self,
        _payload: serde_json::Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool,
    {
        Ok(predicate(&self.0).then(|| self.0.clone()))
    }
}

struct ViewState {
    enhancements: EnhancementViewState,
    locale: Locale,
    emitted: Vec<EnhancementAction>,
}

#[test]
fn route_exposes_all_owned_controls_in_both_locales_without_duplicate_owners() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            vec![
                "增强功能",
                "启用增强功能",
                "Computer Use Guard",
                "启动模式",
                "完整增强",
                "兼容增强",
                "插件与模型",
                "插件市场解锁",
                "自动展开插件列表",
                "模型白名单解锁",
                "服务档位控件",
                "对话与输入",
                "会话删除",
                "Markdown 导出",
                "纯文本粘贴修复",
                "跨项目移动会话",
                "会话 ID 标识",
                "对话居中宽度",
                "恢复会话滚动位置",
                "界面与启动",
                "强制中文界面",
                "快速启动",
                "原生菜单位置",
                "原生菜单本地化",
                "远程项目",
                "使用 Zed Remote 打开",
                "创建 upstream worktree",
                "保存增强设置",
                "重置增强设置",
            ],
        ),
        (
            Locale::En,
            vec![
                "Enhancements",
                "Enable enhancements",
                "Computer Use Guard",
                "Launch mode",
                "Full enhancements",
                "Compatibility",
                "Plugins and models",
                "Plugin marketplace unlock",
                "Auto-expand plugin list",
                "Model whitelist unlock",
                "Service tier controls",
                "Conversation and input",
                "Session delete",
                "Markdown export",
                "Plain-text paste fix",
                "Move conversations between projects",
                "Thread ID badge",
                "Centered conversation view",
                "Restore thread scroll position",
                "Interface and startup",
                "Force Chinese locale",
                "Fast startup",
                "Native menu placement",
                "Native menu localization",
                "Remote projects",
                "Open files in Zed Remote",
                "Create upstream worktree",
                "Save enhancements",
                "Reset enhancements",
            ],
        ),
    ] {
        let harness = harness(960.0, loaded_state(locale, true));
        for label in labels {
            assert!(has_label_or_value(&harness, label), "{locale:?}: {label}");
        }
        #[cfg(windows)]
        assert!(has_label_or_value(
            &harness,
            match locale {
                Locale::ZhCn => "宠物跟随真实鼠标",
                Locale::En => "Pet follows real mouse",
            }
        ));

        for forbidden in [
            "Stepwise",
            "Zed project registry",
            "Sync Zed settings",
            "Zed open strategy",
            "Repair plugin marketplace",
            "Zed 项目记录",
            "同步 Zed settings",
            "Zed 默认打开策略",
            "修复插件市场",
        ] {
            assert!(harness.query_by_label(forbidden).is_none(), "{forbidden}");
        }
    }
}

#[test]
fn master_off_disables_subcontrols_without_erasing_checked_values() {
    let harness = harness(960.0, loaded_state(Locale::En, false));

    for label in [
        "Computer Use Guard",
        "Plugin marketplace unlock",
        "Session delete",
        "Fast startup",
        "Open files in Zed Remote",
    ] {
        assert!(
            harness
                .query_by(|node| node.label().as_deref() == Some(label) && node.is_disabled())
                .is_some(),
            "{label}"
        );
    }
    assert!(harness.state().enhancements.draft().session_delete);
    assert!(
        harness
            .state()
            .enhancements
            .draft()
            .plugin_marketplace_unlock
    );
}

#[test]
fn reset_uses_a_distinct_confirmation_and_commands_keep_stable_widths() {
    let mut harness = harness(760.0, loaded_state(Locale::En, true));
    let save = harness.get_by_label("Save enhancements").rect();
    let reset = harness.get_by_label("Reset enhancements").rect();
    assert!(
        (save.width() - reset.width()).abs() < 2.0,
        "{save:?} {reset:?}"
    );
    assert!(save.width() >= 120.0, "{save:?}");

    harness.get_by_label("Reset enhancements").click();
    harness.run();
    harness.run();
    for label in ["Reset enhancement settings?", "Reset", "Cancel"] {
        assert!(has_label_or_value(&harness, label), "{label}");
    }
    assert!(harness.state().enhancements.reset_confirmation_pending());
    assert!(
        harness
            .state()
            .emitted
            .contains(&EnhancementAction::RequestReset)
    );
}

fn loaded_state(locale: Locale, enabled: bool) -> ViewState {
    let settings = BackendSettings {
        enhancements_enabled: enabled,
        computer_use_guard_enabled: true,
        codex_app_fast_startup: true,
        ..BackendSettings::default()
    };
    ViewState {
        enhancements: EnhancementViewState::from_workspace(workspace(settings)),
        locale,
        emitted: Vec::new(),
    }
}

fn workspace(settings: BackendSettings) -> Arc<EnhancementWorkspace> {
    Arc::new(
        EnhancementSettingsService::new(StaticEnvironment(settings))
            .load()
            .unwrap(),
    )
}

fn harness(width: f32, state: ViewState) -> Harness<'static, ViewState> {
    Harness::builder()
        .with_size(egui::vec2(width, 760.0))
        .build_ui_state(render, state)
}

fn render(ui: &mut egui::Ui, state: &mut ViewState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), ThemeMode::Dark);
    let mut actions = Vec::new();
    enhancements::render(ui, &state.enhancements, state.locale, &mut actions);
    for action in actions {
        match action {
            EnhancementAction::Edit(settings) => state.enhancements.edit(settings),
            EnhancementAction::RequestReset => {
                state.enhancements.request_reset();
            }
            EnhancementAction::CancelReset => state.enhancements.cancel_reset(),
            EnhancementAction::ReloadConflict => {
                state.enhancements.reload_conflict();
            }
            EnhancementAction::DiscardChanges => state.enhancements.discard_changes(),
            EnhancementAction::Refresh
            | EnhancementAction::Save
            | EnhancementAction::ConfirmReset => {}
        }
        state.emitted.push(action);
    }
}

fn has_label_or_value(harness: &Harness<'_, ViewState>, label: &str) -> bool {
    harness.query_by_label(label).is_some()
        || harness
            .query_all_by(|node| node.value().as_deref() == Some(label))
            .next()
            .is_some()
}
