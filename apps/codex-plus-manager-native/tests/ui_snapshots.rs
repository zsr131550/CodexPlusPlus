#![cfg(target_os = "windows")]

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use codex_plus_core::context_ownership::ContextOwnershipRevision;
use codex_plus_core::env_conflicts::{EnvConflict, EnvConflictSource};
use codex_plus_core::relay_config::CodexContextEntries;
use codex_plus_core::relay_environment::{
    ClashVergeTunCheck, CodexEnvFileCheck, ProxyEnvironmentCheck, RelayEnvironmentReport,
};
use codex_plus_core::settings::RelayProtocol;
use codex_plus_manager_native::fonts;
use codex_plus_manager_native::i18n::{Locale, TextKey, ThemeMode, text};
use codex_plus_manager_native::state::Route;
use codex_plus_manager_native::state::context::{ContextFailureKind, ContextViewState};
use codex_plus_manager_native::state::environment::EnvironmentViewState;
use codex_plus_manager_native::state::import::ImportViewState;
use codex_plus_manager_native::state::marketplace::{MarketplaceFailureKind, MarketplaceViewState};
use codex_plus_manager_native::state::provider::ProviderViewState;
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::shell::{ShellViewModel, render_shell};
use codex_plus_manager_service::{
    CcsDiscovery, CcsProviderSummary, ContextBundle, ContextEntryKey, ContextEntryLiveState,
    ContextEntrySummary, ContextKind, ContextOwnershipOutcome, ContextSyncDiffSummary,
    ContextSyncGuard, ContextSyncKeys, ContextSyncOutcome, ContextSyncPreview,
    ContextToolsErrorKind, ContextWorkspace, PluginMarketplaceErrorKind, PluginMarketplaceKind,
    PluginMarketplaceRevision, PluginMarketplaceStatus, PluginMarketplaceWorkspace,
    ProviderActivationSummary, ProviderDocument, ProviderLiveRevision, ProviderRevision,
    ProviderWorkspace, RelayEnvironmentWorkspace,
};
use eframe::egui;
use egui_kittest::{Harness, SnapshotOptions, SnapshotResults, kittest::Queryable};

mod common;

struct SnapshotState {
    model: ShellViewModel,
    provider: Option<ProviderViewState>,
    provider_import: Option<ImportViewState>,
    environment: Option<EnvironmentViewState>,
    context: Option<ContextViewState>,
    marketplace: Option<MarketplaceViewState>,
    cjk_font: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy)]
enum ContextSnapshotScenario {
    Loading,
    SafeList,
    Editor,
    Delete,
    Preview,
    Conflict,
    PartialOwnership,
}

#[derive(Debug, Clone, Copy)]
enum MarketplaceSnapshotScenario {
    Healthy,
    Confirmation,
    Running,
    Failure,
}

const CASES: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "overview_1180_zh_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "overview_1180_zh_light",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Dark,
        "overview_1180_en_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Light,
        "overview_1180_en_light",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "overview_960_zh_dark",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "overview_960_zh_light",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Dark,
        "overview_960_en_dark",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Light,
        "overview_960_en_light",
    ),
];

const PROVIDER_CASES: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "providers_1180_zh_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "providers_1180_zh_light",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Dark,
        "providers_1180_en_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Light,
        "providers_1180_en_light",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "providers_960_zh_dark",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "providers_960_zh_light",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Dark,
        "providers_960_en_dark",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Light,
        "providers_960_en_light",
    ),
];

const ENVIRONMENT_CASES: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "environment_1180_zh_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Light,
        "environment_1180_en_light",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "environment_960_zh_light",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Dark,
        "environment_960_en_dark",
    ),
];

const IMPORT_CASES: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "import_1180_zh_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Light,
        "import_1180_en_light",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "import_960_zh_light",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Dark,
        "import_960_en_dark",
    ),
];

const CONTEXT_VIEWPORTS: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (1180.0, 820.0, Locale::ZhCn, ThemeMode::Dark, "1180_zh_dark"),
    (1180.0, 820.0, Locale::En, ThemeMode::Light, "1180_en_light"),
    (960.0, 720.0, Locale::ZhCn, ThemeMode::Light, "960_zh_light"),
    (960.0, 720.0, Locale::En, ThemeMode::Dark, "960_en_dark"),
];

const CONTEXT_SCENARIOS: &[(ContextSnapshotScenario, &str)] = &[
    (ContextSnapshotScenario::Loading, "loading"),
    (ContextSnapshotScenario::SafeList, "list"),
    (ContextSnapshotScenario::Editor, "editor"),
    (ContextSnapshotScenario::Delete, "delete"),
    (ContextSnapshotScenario::Preview, "preview"),
    (ContextSnapshotScenario::Conflict, "conflict"),
    (ContextSnapshotScenario::PartialOwnership, "partial"),
];

const MARKETPLACE_VIEWPORTS: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (1180.0, 820.0, Locale::ZhCn, ThemeMode::Dark, "1180_zh_dark"),
    (1180.0, 820.0, Locale::En, ThemeMode::Light, "1180_en_light"),
    (960.0, 720.0, Locale::ZhCn, ThemeMode::Light, "960_zh_light"),
    (960.0, 720.0, Locale::En, ThemeMode::Dark, "960_en_dark"),
];

const MARKETPLACE_SCENARIOS: &[(MarketplaceSnapshotScenario, &str)] = &[
    (MarketplaceSnapshotScenario::Healthy, "healthy"),
    (MarketplaceSnapshotScenario::Confirmation, "confirmation"),
    (MarketplaceSnapshotScenario::Running, "running"),
    (MarketplaceSnapshotScenario::Failure, "failure"),
];

#[test]
fn overview_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_snapshot_matrix(CASES, Route::Overview, false);
}

#[test]
fn provider_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_snapshot_matrix(PROVIDER_CASES, Route::Providers, false);
}

#[test]
fn environment_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_snapshot_matrix(ENVIRONMENT_CASES, Route::Environment, false);
}

#[test]
fn provider_import_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_snapshot_matrix(IMPORT_CASES, Route::Providers, true);
}

#[test]
fn context_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_context_snapshot_matrix();
}

#[test]
fn marketplace_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_marketplace_snapshot_matrix();
}

fn snapshot_test_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn run_snapshot_matrix(
    cases: &[(f32, f32, Locale, ThemeMode, &str)],
    route: Route,
    import_modal: bool,
) {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(width, height, locale, mode, name) in cases {
        let options = SnapshotOptions::new().output_path(&snapshots);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width, height))
            .with_theme(match mode {
                ThemeMode::Dark => egui::Theme::Dark,
                ThemeMode::Light => egui::Theme::Light,
            })
            .with_os(egui::os::OperatingSystem::Windows)
            .with_options(options)
            .wgpu()
            .build_ui_state(
                |ui, state: &mut SnapshotState| {
                    if let Some(bytes) = state.cjk_font.take() {
                        egui_extras::install_image_loaders(ui.ctx());
                        fonts::install_cjk_font(ui.ctx(), bytes);
                        theme::apply(ui.ctx(), state.model.theme);
                    }
                    let _ = render_shell(
                        ui,
                        &state.model,
                        state.provider.as_ref(),
                        state.provider_import.as_ref(),
                        state.environment.as_ref(),
                        state.context.as_ref(),
                        state.marketplace.as_ref(),
                    );
                },
                SnapshotState {
                    model: {
                        let mut model = common::model(locale, mode);
                        model.route = route;
                        model
                    },
                    provider: (route == Route::Providers).then(common::provider_state),
                    provider_import: import_modal.then(import_state),
                    environment: (route == Route::Environment).then(environment_state),
                    context: None,
                    marketplace: None,
                    cjk_font: Some(font.clone()),
                },
            );

        harness.remove_cursor();
        harness.run();
        harness.snapshot(name);
        results.extend_harness(&mut harness);
    }

    results.unwrap();
}

fn run_context_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in CONTEXT_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in CONTEXT_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut SnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(
                            ui,
                            &state.model,
                            None,
                            None,
                            None,
                            state.context.as_ref(),
                            state.marketplace.as_ref(),
                        );
                    },
                    SnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::Context;
                            model
                        },
                        provider: None,
                        provider_import: None,
                        environment: None,
                        context: Some(context_snapshot_state(scenario)),
                        marketplace: Some(marketplace_snapshot_state(
                            MarketplaceSnapshotScenario::Healthy,
                        )),
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            if matches!(scenario, ContextSnapshotScenario::Loading) {
                harness.run_steps(2);
            } else {
                harness.run();
            }
            assert_context_layout(&harness, scenario, locale, width, height);
            let image = harness.render().expect("context snapshot should render");
            let distinct = image
                .pixels()
                .map(|pixel| pixel.0)
                .collect::<std::collections::BTreeSet<_>>();
            assert!(distinct.len() > 8, "context snapshot rendered blank");
            harness.snapshot(format!("context_{scenario_name}_{viewport_name}"));
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn run_marketplace_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in MARKETPLACE_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in MARKETPLACE_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut SnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(
                            ui,
                            &state.model,
                            None,
                            None,
                            None,
                            state.context.as_ref(),
                            state.marketplace.as_ref(),
                        );
                    },
                    SnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::Context;
                            model
                        },
                        provider: None,
                        provider_import: None,
                        environment: None,
                        context: Some(context_snapshot_state(ContextSnapshotScenario::SafeList)),
                        marketplace: Some(marketplace_snapshot_state(scenario)),
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            if matches!(scenario, MarketplaceSnapshotScenario::Running) {
                harness.run_steps(2);
            } else {
                harness.run();
            }
            assert_marketplace_layout(&harness, scenario, locale, width, height);
            let image = harness
                .render()
                .expect("marketplace snapshot should render");
            let distinct = image
                .pixels()
                .map(|pixel| pixel.0)
                .collect::<std::collections::BTreeSet<_>>();
            assert!(distinct.len() > 8, "marketplace snapshot rendered blank");
            harness.snapshot(format!("marketplace_{scenario_name}_{viewport_name}"));
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn assert_context_layout(
    harness: &Harness<'_, SnapshotState>,
    scenario: ContextSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let header = format!(
        "{} {}",
        text(locale, TextKey::AppName),
        text(locale, TextKey::ToolsPlugins)
    );
    assert_inside(harness.get_by_label(&header).rect(), width, height, &header);
    let marketplace_title = match locale {
        Locale::ZhCn => "插件市场",
        Locale::En => "Plugin marketplaces",
    };
    assert_inside(
        harness.get_by_label(marketplace_title).rect(),
        width,
        height,
        marketplace_title,
    );
    let label = match scenario {
        ContextSnapshotScenario::Loading => text(locale, TextKey::ToolsPluginsSubtitle),
        ContextSnapshotScenario::SafeList => {
            "beta-with-a-very-long-context-entry-id-that-must-truncate-safely"
        }
        ContextSnapshotScenario::Editor => match locale {
            Locale::ZhCn => "新建技能条目",
            Locale::En => "Create Skill entry",
        },
        ContextSnapshotScenario::Delete => text(locale, TextKey::DeleteContextEntry),
        ContextSnapshotScenario::Preview => text(locale, TextKey::PreviewLiveSync),
        ContextSnapshotScenario::Conflict => text(locale, TextKey::ContextProviderConflict),
        ContextSnapshotScenario::PartialOwnership => {
            "C:/isolated/context/backups/context-backup-with-a-very-long-file-name.toml"
        }
    };
    assert_inside(harness.get_by_label(label).rect(), width, height, label);
}

fn assert_marketplace_layout(
    harness: &Harness<'_, SnapshotState>,
    scenario: MarketplaceSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let labels = match locale {
        Locale::ZhCn => ["插件市场", "OpenAI 插件", "官方远端缓存"],
        Locale::En => [
            "Plugin marketplaces",
            "OpenAI plugins",
            "Official remote cache",
        ],
    };
    for label in labels {
        assert_inside(harness.get_by_label(label).rect(), width, height, label);
    }
    let scenario_label = match (locale, scenario) {
        (_, MarketplaceSnapshotScenario::Healthy) => return,
        (Locale::ZhCn, MarketplaceSnapshotScenario::Confirmation) => "修复 OpenAI 插件？",
        (Locale::En, MarketplaceSnapshotScenario::Confirmation) => "Repair OpenAI plugins?",
        (Locale::ZhCn, MarketplaceSnapshotScenario::Running) => "正在修复",
        (Locale::En, MarketplaceSnapshotScenario::Running) => "Repairing",
        (Locale::ZhCn, MarketplaceSnapshotScenario::Failure) => "写入失败",
        (Locale::En, MarketplaceSnapshotScenario::Failure) => "Write failed",
    };
    assert_inside(
        harness.get_by_label(scenario_label).rect(),
        width,
        height,
        scenario_label,
    );
}

fn assert_inside(rect: egui::Rect, width: f32, height: f32, label: &str) {
    assert!(rect.is_positive(), "{label}: {rect:?}");
    assert!(rect.min.x >= 0.0 && rect.min.y >= 0.0, "{label}: {rect:?}");
    assert!(
        rect.max.x <= width && rect.max.y <= height,
        "{label}: {rect:?}"
    );
}

fn context_snapshot_state(scenario: ContextSnapshotScenario) -> ContextViewState {
    let mut state = ContextViewState::default();
    if matches!(scenario, ContextSnapshotScenario::Loading) {
        state.begin_workspace_refresh();
        return state;
    }

    let request_id = state.begin_workspace_refresh();
    state.apply_workspace_response(request_id, Ok(context_bundle()));
    match scenario {
        ContextSnapshotScenario::Loading | ContextSnapshotScenario::SafeList => {}
        ContextSnapshotScenario::Editor => {
            state.open_create(ContextKind::Skill);
            state.set_editor_id("new-skill".to_owned());
            state.set_editor_body("token = \"snapshot-secret-sentinel\"\n".to_owned());
        }
        ContextSnapshotScenario::Delete => {
            state.request_delete(context_key(ContextKind::Mcp, "alpha"));
        }
        ContextSnapshotScenario::Preview => install_context_preview(&mut state),
        ContextSnapshotScenario::Conflict => {
            state.open_create(ContextKind::Skill);
            state.set_editor_id("conflicting-skill".to_owned());
            let (mutation_id, _) = state.begin_save().unwrap();
            state.apply_stored_mutation_response(
                mutation_id,
                Err(ContextFailureKind::Service(
                    ContextToolsErrorKind::ProviderConflict,
                )),
            );
        }
        ContextSnapshotScenario::PartialOwnership => {
            install_context_preview(&mut state);
            let (sync_id, _) = state.begin_sync().unwrap();
            state.apply_sync_response(
                sync_id,
                Ok(Arc::new(ContextSyncOutcome {
                    bundle: (*context_bundle()).clone(),
                    backup_path: Some(
                        "C:/isolated/context/backups/context-backup-with-a-very-long-file-name.toml"
                            .to_owned(),
                    ),
                    ownership: ContextOwnershipOutcome::PartialFailure,
                    diff: ContextSyncDiffSummary::default(),
                })),
            );
        }
    }
    state
}

fn marketplace_snapshot_state(scenario: MarketplaceSnapshotScenario) -> MarketplaceViewState {
    let healthy = matches!(scenario, MarketplaceSnapshotScenario::Healthy);
    let mut state = MarketplaceViewState::default();
    let request_id = state.begin_inspection().unwrap();
    state.apply_inspection_response(
        request_id,
        Ok(Arc::new(PluginMarketplaceWorkspace {
            revision: PluginMarketplaceRevision::from_digest([7; 32]),
            local: marketplace_status(healthy, 12, 34),
            remote: marketplace_status(healthy, 8, 21),
        })),
    );
    match scenario {
        MarketplaceSnapshotScenario::Healthy => {}
        MarketplaceSnapshotScenario::Confirmation => {
            state.request_repair_confirmation(PluginMarketplaceKind::Local);
        }
        MarketplaceSnapshotScenario::Running => {
            state.request_repair_confirmation(PluginMarketplaceKind::Local);
            state.confirm_repair().unwrap();
        }
        MarketplaceSnapshotScenario::Failure => {
            state.request_repair_confirmation(PluginMarketplaceKind::Remote);
            let (repair_id, _) = state.confirm_repair().unwrap();
            state.apply_repair_response(
                repair_id,
                PluginMarketplaceKind::Remote,
                Err(MarketplaceFailureKind::Service(
                    PluginMarketplaceErrorKind::WriteFailed,
                )),
            );
        }
    }
    state
}

fn marketplace_status(
    healthy: bool,
    plugin_count: usize,
    skill_count: usize,
) -> PluginMarketplaceStatus {
    PluginMarketplaceStatus {
        available: healthy,
        config_registered: healthy,
        needs_repair: !healthy,
        plugin_count: if healthy { plugin_count } else { 0 },
        skill_count: if healthy { skill_count } else { 0 },
    }
}

fn install_context_preview(state: &mut ContextViewState) {
    let (request_id, _) = state.begin_preview().unwrap();
    state.apply_preview_response(
        request_id,
        Ok(Arc::new(ContextSyncPreview {
            guard: ContextSyncGuard {
                expected_provider_revision: ProviderRevision::parse("a".repeat(64)).unwrap(),
                expected_live_revision: ProviderLiveRevision::parse("b".repeat(64)).unwrap(),
                expected_ownership_revision: ContextOwnershipRevision::parse("c".repeat(64))
                    .unwrap(),
            },
            active_provider_id: Some("snapshot-provider".to_owned()),
            diff: ContextSyncDiffSummary {
                added: 1,
                updated: 1,
                removed: 1,
                unchanged: 2,
            },
            keys: ContextSyncKeys {
                added: vec![context_key(ContextKind::Plugin, "lint")],
                updated: vec![context_key(ContextKind::Mcp, "alpha")],
                removed: vec![context_key(ContextKind::Plugin, "old-plugin")],
                unchanged: vec![
                    context_key(ContextKind::Mcp, "stable"),
                    context_key(ContextKind::Skill, "review"),
                ],
            },
        })),
    );
}

fn context_bundle() -> Arc<ContextBundle> {
    let provider_revision = ProviderRevision::parse("a".repeat(64)).unwrap();
    Arc::new(ContextBundle {
        context: ContextWorkspace {
            provider_revision: provider_revision.clone(),
            live_revision: ProviderLiveRevision::parse("b".repeat(64)).unwrap(),
            ownership_revision: ContextOwnershipRevision::parse("c".repeat(64)).unwrap(),
            active_provider_id: Some("snapshot-provider".to_owned()),
            active_provider_name: Some("Snapshot provider".to_owned()),
            entries: vec![
                ContextEntrySummary {
                    key: context_key(ContextKind::Mcp, "alpha"),
                    display_name: "alpha".to_owned(),
                    enabled: true,
                    live_state: ContextEntryLiveState::Matching,
                },
                ContextEntrySummary {
                    key: context_key(
                        ContextKind::Mcp,
                        "beta-with-a-very-long-context-entry-id-that-must-truncate-safely",
                    ),
                    display_name:
                        "beta-with-a-very-long-context-entry-id-that-must-truncate-safely"
                            .to_owned(),
                    enabled: false,
                    live_state: ContextEntryLiveState::Different,
                },
                ContextEntrySummary {
                    key: context_key(ContextKind::Skill, "review"),
                    display_name: "review".to_owned(),
                    enabled: true,
                    live_state: ContextEntryLiveState::StoredOnly,
                },
                ContextEntrySummary {
                    key: context_key(ContextKind::Plugin, "lint"),
                    display_name: "lint".to_owned(),
                    enabled: true,
                    live_state: ContextEntryLiveState::PendingRemoval,
                },
            ],
            unmanaged_live_count: 2,
            sync_needed: true,
        },
        provider: ProviderWorkspace {
            revision: provider_revision,
            document: ProviderDocument {
                profiles: Vec::new(),
                common_config_contents: String::new(),
                context_config_contents: String::new(),
                default_test_model: String::new(),
            },
            activation: ProviderActivationSummary {
                enabled: true,
                active_profile_id: Some("snapshot-provider".to_owned()),
                active_profile_kind: None,
            },
            context_options: CodexContextEntries {
                mcp_servers: Vec::new(),
                skills: Vec::new(),
                plugins: Vec::new(),
            },
        },
    })
}

fn context_key(kind: ContextKind, id: &str) -> ContextEntryKey {
    ContextEntryKey {
        kind,
        id: id.to_owned(),
    }
}

fn environment_state() -> EnvironmentViewState {
    let mut state = EnvironmentViewState::default();
    let request_id = state.begin_inspection();
    state.apply_inspection_response(
        request_id,
        Ok(Arc::new(RelayEnvironmentWorkspace {
            report: RelayEnvironmentReport {
                clash_verge_tun: ClashVergeTunCheck {
                    enabled: false,
                    config_path: None,
                },
                proxy_environment: ProxyEnvironmentCheck {
                    variables: Vec::new(),
                },
                codex_env_file: CodexEnvFileCheck {
                    exists: false,
                    path: "fixture/.env".to_owned(),
                },
            },
            conflicts: vec![EnvConflict {
                name: "OPENAI_API_KEY".to_owned(),
                source: EnvConflictSource::Process,
                value_present: true,
            }],
            revision: "a".repeat(64),
        })),
    );
    state
}

fn import_state() -> ImportViewState {
    let mut state = ImportViewState::default();
    let request_id = state.begin_discovery();
    state.apply_discovery_response(
        request_id,
        Ok(Arc::new(CcsDiscovery {
            source_path: "fixture/cc-switch.db".to_owned(),
            source_revision: "b".repeat(64),
            provider_revision: ProviderRevision::parse("c".repeat(64)).unwrap(),
            providers: vec![CcsProviderSummary {
                source_id: "fixture".to_owned(),
                name: "Snapshot provider".to_owned(),
                base_url: "https://snapshot.invalid/v1".to_owned(),
                protocol: RelayProtocol::Responses,
                duplicate: false,
            }],
            importable_count: 1,
            duplicate_count: 0,
        })),
    );
    state
}
