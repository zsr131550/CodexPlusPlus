#![cfg(target_os = "windows")]

use std::path::Path;
use std::sync::Arc;

use codex_plus_core::env_conflicts::{EnvConflict, EnvConflictSource};
use codex_plus_core::relay_environment::{
    ClashVergeTunCheck, CodexEnvFileCheck, ProxyEnvironmentCheck, RelayEnvironmentReport,
};
use codex_plus_core::settings::RelayProtocol;
use codex_plus_manager_native::fonts;
use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::Route;
use codex_plus_manager_native::state::environment::EnvironmentViewState;
use codex_plus_manager_native::state::import::ImportViewState;
use codex_plus_manager_native::state::provider::ProviderViewState;
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::shell::{ShellViewModel, render_shell};
use codex_plus_manager_service::{
    CcsDiscovery, CcsProviderSummary, ProviderRevision, RelayEnvironmentWorkspace,
};
use eframe::egui;
use egui_kittest::{Harness, SnapshotOptions, SnapshotResults};

mod common;

struct SnapshotState {
    model: ShellViewModel,
    provider: Option<ProviderViewState>,
    provider_import: Option<ImportViewState>,
    environment: Option<EnvironmentViewState>,
    cjk_font: Option<Vec<u8>>,
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

#[test]
fn overview_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }

    run_snapshot_matrix(CASES, Route::Overview, false);
}

#[test]
fn provider_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }

    run_snapshot_matrix(PROVIDER_CASES, Route::Providers, false);
}

#[test]
fn environment_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }

    run_snapshot_matrix(ENVIRONMENT_CASES, Route::Environment, false);
}

#[test]
fn provider_import_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }

    run_snapshot_matrix(IMPORT_CASES, Route::Providers, true);
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
