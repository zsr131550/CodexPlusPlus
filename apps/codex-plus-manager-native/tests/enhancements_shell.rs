use std::sync::Arc;

use codex_plus_core::settings::BackendSettings;
use codex_plus_manager_native::i18n::{Locale, TextKey, ThemeMode, text};
use codex_plus_manager_native::state::Route;
use codex_plus_manager_native::state::enhancements::EnhancementViewState;
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::shell::{ShellAction, ShellFeatureStates, render_shell};
use codex_plus_manager_service::{EnhancementSettingsEnvironment, EnhancementSettingsService};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

mod common;

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

struct ShellState {
    model: codex_plus_manager_native::views::shell::ShellViewModel,
    enhancements: EnhancementViewState,
    emitted: Vec<ShellAction>,
}

#[test]
fn enhancement_route_has_bilingual_navigation_header_and_ready_status() {
    for locale in [Locale::ZhCn, Locale::En] {
        let mut model = common::model(locale, ThemeMode::Dark);
        model.route = Route::Enhancements;
        let harness = harness(model);

        let tools = harness
            .get_by_label(text(locale, TextKey::ToolsPlugins))
            .rect();
        let enhancements = harness
            .get_by_role_and_label(
                egui::accesskit::Role::Button,
                text(locale, TextKey::Enhancements),
            )
            .rect();
        let zed = harness
            .get_by_label(text(locale, TextKey::ZedRemote))
            .rect();
        assert!(
            tools.max.y <= enhancements.min.y,
            "{tools:?} {enhancements:?}"
        );
        assert!(enhancements.max.y <= zed.min.y, "{enhancements:?} {zed:?}");

        for label in [
            format!(
                "{} {}",
                text(locale, TextKey::AppName),
                text(locale, TextKey::Enhancements)
            ),
            text(locale, TextKey::EnhancementsSubtitle).to_owned(),
            format!(
                "{}: {}",
                text(locale, TextKey::Status),
                text(locale, TextKey::Ready)
            ),
        ] {
            assert!(harness.get_by_label(&label).rect().is_positive(), "{label}");
        }
    }
}

#[test]
fn sidebar_emits_the_enhancement_route() {
    let mut model = common::model(Locale::En, ThemeMode::Dark);
    model.route = Route::Overview;
    let mut harness = harness(model);

    harness.get_by_label("Enhancements").click();
    harness.run();

    assert!(
        harness
            .state()
            .emitted
            .contains(&ShellAction::Navigate(Route::Enhancements))
    );
}

fn harness(
    model: codex_plus_manager_native::views::shell::ShellViewModel,
) -> Harness<'static, ShellState> {
    let workspace = EnhancementSettingsService::new(StaticEnvironment(BackendSettings::default()))
        .load()
        .unwrap();
    Harness::builder()
        .with_size(egui::vec2(1180.0, 820.0))
        .build_ui_state(
            render,
            ShellState {
                model,
                enhancements: EnhancementViewState::from_workspace(Arc::new(workspace)),
                emitted: Vec::new(),
            },
        )
}

fn render(ui: &mut egui::Ui, state: &mut ShellState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), state.model.theme);
    let states = ShellFeatureStates {
        enhancements: Some(&state.enhancements),
        ..ShellFeatureStates::default()
    };
    for action in render_shell(ui, &state.model, states) {
        if let ShellAction::Navigate(route) = action {
            state.model.route = route;
        }
        state.emitted.push(action);
    }
}
