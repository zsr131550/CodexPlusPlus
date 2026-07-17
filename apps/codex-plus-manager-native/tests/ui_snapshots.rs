#![cfg(target_os = "windows")]

use std::path::Path;

use codex_plus_manager_native::fonts;
use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::Route;
use codex_plus_manager_native::state::provider::ProviderViewState;
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::shell::{ShellViewModel, render_shell};
use eframe::egui;
use egui_kittest::{Harness, SnapshotOptions, SnapshotResults};

mod common;

struct SnapshotState {
    model: ShellViewModel,
    provider: Option<ProviderViewState>,
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

#[test]
fn overview_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }

    run_snapshot_matrix(CASES, false);
}

#[test]
fn provider_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }

    run_snapshot_matrix(PROVIDER_CASES, true);
}

fn run_snapshot_matrix(cases: &[(f32, f32, Locale, ThemeMode, &str)], providers: bool) {
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
                    let _ = render_shell(ui, &state.model, state.provider.as_ref());
                },
                SnapshotState {
                    model: {
                        let mut model = common::model(locale, mode);
                        if providers {
                            model.route = Route::Providers;
                        }
                        model
                    },
                    provider: providers.then(common::provider_state),
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
