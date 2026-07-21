use codex_plus_manager::i18n::{Locale, ThemeMode};
use codex_plus_manager::state::settings::{SettingsTab, SettingsViewState};
use codex_plus_manager::theme;
use codex_plus_manager::views::settings::{self, SettingsAction};
use codex_plus_manager_service::{SafeSettingsGroup, SecretReplacement};
use eframe::egui;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

mod common;

struct ViewState {
    settings: SettingsViewState,
    locale: Locale,
    emitted: Vec<SettingsAction>,
}

#[test]
fn stepwise_tab_exposes_complete_bilingual_settings_semantics() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            [
                "Stepwise",
                "图片覆盖",
                "启动参数",
                "启用 Stepwise",
                "直接发送",
                "基础 URL",
                "模型",
                "API 密钥环境变量",
                "已保存凭据",
                "替换 API 密钥",
                "显示替换密钥",
                "清除已保存凭据",
                "最大条目数",
                "最大输入字符数",
                "最大输出令牌数",
                "超时（毫秒）",
                "测试连接",
                "保存 Stepwise",
                "重置 Stepwise",
            ],
        ),
        (
            Locale::En,
            [
                "Stepwise",
                "Image overlay",
                "Launch arguments",
                "Enable Stepwise",
                "Direct send",
                "Base URL",
                "Model",
                "API key environment",
                "Stored credential",
                "Replacement API key",
                "Show replacement key",
                "Clear stored credential",
                "Maximum items",
                "Maximum input characters",
                "Maximum output tokens",
                "Timeout (ms)",
                "Test connection",
                "Save Stepwise",
                "Reset Stepwise",
            ],
        ),
    ] {
        let harness = harness(900.0, loaded_state(SettingsTab::Stepwise, locale));
        for label in labels {
            assert!(has_label_or_value(&harness, label), "{locale:?}: {label}");
        }
    }
}

#[test]
fn image_and_argument_tabs_expose_complete_controls_without_duplicate_settings() {
    for (locale, image_labels, argument_labels) in [
        (
            Locale::ZhCn,
            [
                "启用图片覆盖",
                "图片路径",
                "选择图片",
                "不透明度",
                "适应模式",
                "适应",
                "保存图片覆盖",
                "重置图片覆盖",
            ],
            [
                "Codex 启动参数",
                "参数数量: 2",
                "保存启动参数",
                "重置启动参数",
            ],
        ),
        (
            Locale::En,
            [
                "Enable image overlay",
                "Image path",
                "Select image",
                "Opacity",
                "Fit mode",
                "Fit",
                "Save image overlay",
                "Reset image overlay",
            ],
            [
                "Codex launch arguments",
                "Argument count: 2",
                "Save launch arguments",
                "Reset launch arguments",
            ],
        ),
    ] {
        let image = harness(900.0, loaded_state(SettingsTab::ImageOverlay, locale));
        for label in image_labels {
            assert!(has_label_or_value(&image, label), "{locale:?}: {label}");
        }

        let arguments = harness(900.0, loaded_state(SettingsTab::LaunchArguments, locale));
        for label in argument_labels {
            assert!(has_label_or_value(&arguments, label), "{locale:?}: {label}");
        }
        assert_eq!(
            arguments
                .query_all_by_role(egui::accesskit::Role::MultilineTextInput)
                .count(),
            1
        );

        for forbidden in [
            "Theme",
            "Language",
            "Provider test model",
            "Reset all settings",
        ] {
            assert!(image.query_by_label(forbidden).is_none(), "{forbidden}");
            assert!(arguments.query_by_label(forbidden).is_none(), "{forbidden}");
        }
    }
}

#[test]
fn replacement_editor_is_a_password_and_no_secret_or_private_value_leaks() {
    let mut state = loaded_state(SettingsTab::Stepwise, Locale::En);
    state
        .settings
        .edit_secret_replacement(SecretReplacement::new("replacement-key-sentinel"));
    state
        .settings
        .edit_stepwise_url("https://private-url-sentinel.invalid".to_owned());
    let mut harness = harness(900.0, state);

    assert!(
        harness
            .get_by_role_and_label(egui::accesskit::Role::PasswordInput, "Replacement API key",)
            .rect()
            .is_positive()
    );
    let tree = format!("{:#?}", harness.root());
    assert!(!tree.contains("replacement-key-sentinel"), "{tree}");

    harness.get_by_label("Show replacement key").click();
    harness.run();
    assert!(harness.state().settings.stepwise.password_visible);
    assert!(
        harness
            .get_by_label("Hide replacement key")
            .rect()
            .is_positive()
    );
    assert!(
        harness
            .get_by_role_and_label(egui::accesskit::Role::PasswordInput, "Replacement API key",)
            .rect()
            .is_positive()
    );
    let visible_tree = format!("{:#?}", harness.root());
    assert!(
        !visible_tree.contains("replacement-key-sentinel"),
        "{visible_tree}"
    );

    let state_debug = format!("{:?}", harness.state().settings);
    assert!(!state_debug.contains("replacement-key-sentinel"));
    assert!(!state_debug.contains("private-url-sentinel"));

    let secret_action =
        SettingsAction::EditSecretReplacement(SecretReplacement::new("action-secret-sentinel"));
    let path_action =
        SettingsAction::EditImagePath("C:/private/action-path-sentinel.png".to_owned());
    assert!(!format!("{secret_action:?}").contains("action-secret-sentinel"));
    assert!(!format!("{path_action:?}").contains("action-path-sentinel"));
}

#[test]
fn clear_secret_and_each_group_reset_use_distinct_frozen_confirmations() {
    for (locale, request_label, labels) in [
        (
            Locale::ZhCn,
            "清除已保存凭据",
            ["清除已保存的 Stepwise 凭据？", "清除凭据", "取消"],
        ),
        (
            Locale::En,
            "Clear stored credential",
            [
                "Clear stored Stepwise credential?",
                "Clear credential",
                "Cancel",
            ],
        ),
    ] {
        let mut clear = harness(900.0, loaded_state(SettingsTab::Stepwise, locale));
        clear.get_by_label(request_label).click();
        clear.run();
        for label in labels {
            assert!(has_label_or_value(&clear, label), "{locale:?}: {label}");
        }
        assert!(!clear.state().settings.stepwise.is_dirty());
        assert!(
            clear
                .state()
                .emitted
                .contains(&SettingsAction::RequestSecretClear)
        );
    }

    for (tab, button, title, group) in [
        (
            SettingsTab::Stepwise,
            "Reset Stepwise",
            "Reset Stepwise settings?",
            SafeSettingsGroup::Stepwise,
        ),
        (
            SettingsTab::ImageOverlay,
            "Reset image overlay",
            "Reset image overlay settings?",
            SafeSettingsGroup::ImageOverlay,
        ),
        (
            SettingsTab::LaunchArguments,
            "Reset launch arguments",
            "Reset launch arguments?",
            SafeSettingsGroup::ExtraArgs,
        ),
    ] {
        let mut reset = harness(900.0, loaded_state(tab, Locale::En));
        reset.get_by_label(button).click();
        reset.run();
        assert!(has_label_or_value(&reset, title), "{title}");
        assert_eq!(
            reset.state().settings.reset_confirmation_group(),
            Some(group)
        );
    }

    for (tab, button, title) in [
        (
            SettingsTab::Stepwise,
            "重置 Stepwise",
            "重置 Stepwise 设置？",
        ),
        (
            SettingsTab::ImageOverlay,
            "重置图片覆盖",
            "重置图片覆盖设置？",
        ),
        (
            SettingsTab::LaunchArguments,
            "重置启动参数",
            "重置启动参数？",
        ),
    ] {
        let mut reset = harness(900.0, loaded_state(tab, Locale::ZhCn));
        reset.get_by_label(button).click();
        reset.run();
        assert!(has_label_or_value(&reset, title), "{title}");
    }
}

#[test]
fn long_private_values_never_become_buttons_and_command_widths_stay_stable() {
    let mut state = loaded_state(SettingsTab::ImageOverlay, Locale::En);
    let sentinel =
        "C:/private/a-very-long-image-overlay-path-sentinel-that-must-remain-an-editor-value.png";
    state.settings.edit_image_path(sentinel.to_owned());
    let compact = harness(720.0, state);
    assert!(compact.query_by_label(sentinel).is_none());

    let save = compact.get_by_label("Save image overlay").rect();
    let reset = compact.get_by_label("Reset image overlay").rect();
    assert!(
        (save.width() - reset.width()).abs() < 2.0,
        "{save:?} {reset:?}"
    );
    assert!(save.width() >= 120.0, "{save:?}");
}

#[test]
fn stepwise_numeric_controls_match_the_service_contract() {
    let harness = harness(900.0, loaded_state(SettingsTab::Stepwise, Locale::En));

    for (label, minimum, maximum) in [
        ("Maximum items", 0.0, 6.0),
        ("Maximum input characters", 1_000.0, 24_000.0),
        ("Maximum output tokens", 100.0, 4_000.0),
        ("Timeout (ms)", 1_000.0, 60_000.0),
    ] {
        let node = harness.get_by_role_and_label(egui::accesskit::Role::SpinButton, label);
        let accesskit_node = node.accesskit_node();
        assert_eq!(accesskit_node.min_numeric_value(), Some(minimum), "{label}");
        assert_eq!(accesskit_node.max_numeric_value(), Some(maximum), "{label}");
    }
}

fn loaded_state(tab: SettingsTab, locale: Locale) -> ViewState {
    let mut settings = SettingsViewState::from_workspace(common::manager_settings_workspace(1));
    settings.set_tab(tab);
    ViewState {
        settings,
        locale,
        emitted: Vec::new(),
    }
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
    settings::render(ui, &state.settings, state.locale, &mut actions);
    for action in actions {
        match &action {
            SettingsAction::SetTab(tab) => state.settings.set_tab(*tab),
            SettingsAction::EditStepwiseEnabled(value) => {
                state.settings.edit_stepwise_enabled(*value);
            }
            SettingsAction::EditStepwiseDirectSend(value) => {
                state.settings.edit_stepwise_direct_send(*value);
            }
            SettingsAction::EditStepwiseUrl(value) => {
                state.settings.edit_stepwise_url(value.clone());
            }
            SettingsAction::EditStepwiseEnvironment(value) => {
                state.settings.edit_stepwise_environment(value.clone());
            }
            SettingsAction::EditStepwiseModel(value) => {
                state.settings.edit_stepwise_model(value.clone());
            }
            SettingsAction::EditStepwiseMaxItems(value) => {
                state.settings.edit_stepwise_max_items(*value);
            }
            SettingsAction::EditStepwiseMaxInputChars(value) => {
                state.settings.edit_stepwise_max_input_chars(*value);
            }
            SettingsAction::EditStepwiseMaxOutputTokens(value) => {
                state.settings.edit_stepwise_max_output_tokens(*value);
            }
            SettingsAction::EditStepwiseTimeoutMs(value) => {
                state.settings.edit_stepwise_timeout_ms(*value);
            }
            SettingsAction::EditSecretReplacement(value) => {
                state.settings.edit_secret_replacement(value.clone());
            }
            SettingsAction::SetPasswordVisible(value) => {
                state.settings.set_password_visible(*value);
            }
            SettingsAction::EditImageEnabled(value) => {
                state.settings.edit_image_enabled(*value);
            }
            SettingsAction::EditImagePath(value) => {
                state.settings.edit_image_path(value.clone());
            }
            SettingsAction::EditImageOpacity(value) => {
                state.settings.edit_image_opacity(*value);
            }
            SettingsAction::EditImageFitMode(value) => {
                state.settings.edit_image_fit_mode(*value);
            }
            SettingsAction::EditExtraArgs(value) => {
                state.settings.edit_extra_args(value.clone());
            }
            SettingsAction::RequestSecretClear => {
                state.settings.request_secret_clear();
            }
            SettingsAction::CancelSecretClear => state.settings.cancel_secret_clear(),
            SettingsAction::RequestReset(group) => {
                state.settings.request_reset(*group);
            }
            SettingsAction::CancelReset => state.settings.cancel_reset(),
            SettingsAction::CancelDiscard => state.settings.cancel_transition(),
            SettingsAction::Refresh
            | SettingsAction::TestStepwise
            | SettingsAction::SaveStepwise
            | SettingsAction::ConfirmSecretClear
            | SettingsAction::SaveImage
            | SettingsAction::PickImage
            | SettingsAction::SaveExtraArgs
            | SettingsAction::ConfirmReset
            | SettingsAction::ConfirmDiscard => {}
        }
        state.emitted.push(action);
    }
}

fn has_label_or_value(harness: &Harness<'_, ViewState>, label: &str) -> bool {
    harness
        .query_all_by(|node| {
            node.label().as_deref() == Some(label) || node.value().as_deref() == Some(label)
        })
        .count()
        > 0
}
