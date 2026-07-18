use std::sync::Arc;

use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::marketplace::MarketplaceViewState;
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::marketplace::{self, MarketplaceAction};
use codex_plus_manager_service::{
    PluginMarketplaceErrorKind, PluginMarketplaceKind, PluginMarketplaceRevision,
    PluginMarketplaceStatus, PluginMarketplaceWorkspace,
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

struct ViewState {
    marketplace: MarketplaceViewState,
    locale: Locale,
    emitted: Vec<MarketplaceAction>,
}

#[test]
fn marketplace_rows_have_complete_bilingual_semantics() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            [
                "插件市场",
                "OpenAI 插件",
                "在线来源",
                "需要修复",
                "插件: 0",
                "技能: 0",
                "刷新 OpenAI 插件",
                "修复 OpenAI 插件",
                "官方远端缓存",
                "内置离线",
            ],
        ),
        (
            Locale::En,
            [
                "Plugin marketplaces",
                "OpenAI plugins",
                "Online source",
                "Repair needed",
                "Plugins: 0",
                "Skills: 0",
                "Refresh OpenAI plugins",
                "Repair OpenAI plugins",
                "Official remote cache",
                "Embedded offline",
            ],
        ),
    ] {
        let harness = harness(ViewState {
            marketplace: loaded_state(false, true),
            locale,
            emitted: Vec::new(),
        });
        for label in labels {
            let rect = harness.get_by_label(label).rect();
            assert!(rect.is_positive(), "{label}");
            assert!(
                rect.max.x <= 760.0 && rect.max.y <= 190.0,
                "{label}: {rect:?}"
            );
        }
    }
}

#[test]
fn healthy_row_stays_visible_and_disables_repair() {
    let harness = harness(ViewState {
        marketplace: loaded_state(false, true),
        locale: Locale::En,
        emitted: Vec::new(),
    });

    assert!(
        harness
            .get_by_label("Official remote cache")
            .rect()
            .is_positive()
    );
    assert!(
        harness
            .query_by(|node| {
                node.label().as_deref() == Some("Repair Official remote cache")
                    && node.is_disabled()
            })
            .is_some()
    );
    assert!(
        harness
            .query_by(|node| {
                node.label().as_deref() == Some("Repair OpenAI plugins") && !node.is_disabled()
            })
            .is_some()
    );
}

#[test]
fn row_controls_emit_typed_refresh_and_repair_actions() {
    let mut harness = harness(ViewState {
        marketplace: loaded_state(false, true),
        locale: Locale::En,
        emitted: Vec::new(),
    });

    harness.get_by_label("Refresh OpenAI plugins").click();
    harness.get_by_label("Repair OpenAI plugins").click();
    harness.run();

    assert!(
        harness
            .state()
            .emitted
            .contains(&MarketplaceAction::Refresh)
    );
    assert!(
        harness
            .state()
            .emitted
            .contains(&MarketplaceAction::RequestRepair(
                PluginMarketplaceKind::Local,
            ))
    );
}

#[test]
fn local_confirmation_names_network_limit_and_mutation_boundary() {
    let mut state = loaded_state(false, true);
    assert!(state.request_repair_confirmation(PluginMarketplaceKind::Local));
    let harness = confirmation_harness(ViewState {
        marketplace: state,
        locale: Locale::En,
        emitted: Vec::new(),
    });

    for label in [
        "Repair OpenAI plugins?",
        "Downloads the online marketplace (maximum 128 MiB), validates it, then updates the local cache and Codex configuration.",
        "Cancel",
        "Repair",
    ] {
        assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
    }
    let body = harness
        .get_by_label("Downloads the online marketplace (maximum 128 MiB), validates it, then updates the local cache and Codex configuration.")
        .rect();
    let repair = harness.get_by_label("Repair").rect();
    assert!(
        repair.center().y - body.center().y < 120.0,
        "confirmation controls should not stretch the window: body={body:?}, repair={repair:?}"
    );
}

#[test]
fn remote_confirmation_names_embedded_offline_source() {
    let mut state = loaded_state(true, false);
    assert!(state.request_repair_confirmation(PluginMarketplaceKind::Remote));
    let harness = confirmation_harness(ViewState {
        marketplace: state,
        locale: Locale::ZhCn,
        emitted: Vec::new(),
    });

    for label in [
        "修复官方远端缓存？",
        "校验并释放内置离线快照，然后更新本地缓存与 Codex 配置。",
        "取消",
        "修复",
    ] {
        assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
    }
}

#[test]
fn repair_failure_is_shown_only_on_the_target_row() {
    let mut state = loaded_state(false, false);
    state.request_repair_confirmation(PluginMarketplaceKind::Remote);
    let (request_id, _) = state.confirm_repair().unwrap();
    state.apply_repair_response(
        request_id,
        PluginMarketplaceKind::Remote,
        Err(
            codex_plus_manager_native::state::marketplace::MarketplaceFailureKind::Service(
                PluginMarketplaceErrorKind::WriteFailed,
            ),
        ),
    );
    let harness = harness(ViewState {
        marketplace: state,
        locale: Locale::En,
        emitted: Vec::new(),
    });

    assert!(harness.get_by_label("Repair needed").rect().is_positive());
    assert!(harness.get_by_label("Write failed").rect().is_positive());
}

fn harness(state: ViewState) -> Harness<'static, ViewState> {
    Harness::builder()
        .with_size(egui::vec2(760.0, 190.0))
        .build_ui_state(render, state)
}

fn confirmation_harness(state: ViewState) -> Harness<'static, ViewState> {
    Harness::builder()
        .with_size(egui::vec2(760.0, 360.0))
        .build_ui_state(render, state)
}

fn render(ui: &mut egui::Ui, state: &mut ViewState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), ThemeMode::Dark);
    let mut actions = Vec::new();
    marketplace::render(ui, &state.marketplace, state.locale, &mut actions);
    state.emitted.extend(actions);
}

fn loaded_state(local_healthy: bool, remote_healthy: bool) -> MarketplaceViewState {
    let mut state = MarketplaceViewState::default();
    let request_id = state.begin_inspection().unwrap();
    assert!(state.apply_inspection_response(
        request_id,
        Ok(Arc::new(PluginMarketplaceWorkspace {
            revision: PluginMarketplaceRevision::from_digest([1; 32]),
            local: status(local_healthy),
            remote: status(remote_healthy),
        })),
    ));
    state
}

fn status(healthy: bool) -> PluginMarketplaceStatus {
    PluginMarketplaceStatus {
        available: healthy,
        config_registered: healthy,
        needs_repair: !healthy,
        plugin_count: usize::from(healthy),
        skill_count: usize::from(healthy),
    }
}
