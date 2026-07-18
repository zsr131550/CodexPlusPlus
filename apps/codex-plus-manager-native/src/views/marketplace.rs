use codex_plus_manager_service::{
    PluginMarketplaceErrorKind, PluginMarketplaceKind, PluginMarketplaceRepairOutcome,
};
use eframe::egui;

use crate::i18n::Locale;
use crate::state::marketplace::{MarketplaceFailureKind, MarketplaceViewState};
use crate::state::provider::OperationPhase;
use crate::{icons, theme};

pub const MARKETPLACE_BAND_HEIGHT: f32 = 150.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketplaceAction {
    Refresh,
    RequestRepair(PluginMarketplaceKind),
    CancelRepair,
    ConfirmRepair,
}

pub fn render(
    ui: &mut egui::Ui,
    state: &MarketplaceViewState,
    locale: Locale,
    actions: &mut Vec<MarketplaceAction>,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(
                egui::RichText::new(section_title(locale))
                    .strong()
                    .size(13.0),
            );
        },
    );
    ui.separator();
    render_row(ui, state, locale, PluginMarketplaceKind::Local, actions);
    ui.separator();
    render_row(ui, state, locale, PluginMarketplaceKind::Remote, actions);
    render_confirmation(ui.ctx(), state, locale, actions);
}

fn render_row(
    ui: &mut egui::Ui,
    state: &MarketplaceViewState,
    locale: Locale,
    kind: PluginMarketplaceKind,
    actions: &mut Vec<MarketplaceAction>,
) {
    let name = marketplace_name(locale, kind);
    let running =
        state.active_repair_kind == Some(kind) && state.repair_phase == OperationPhase::Running;
    let controls_enabled = state.inspection_phase != OperationPhase::Running
        && state.repair_phase != OperationPhase::Running;
    let status = state.status(kind);
    let (status_label, status_color) = row_status(state, locale, kind);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 52.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2((ui.available_width() * 0.28).clamp(150.0, 220.0), 42.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.label(egui::RichText::new(name).strong());
                    ui.label(
                        egui::RichText::new(marketplace_source(locale, kind))
                            .weak()
                            .size(11.0),
                    );
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(116.0, 28.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.colored_label(status_color, status_label);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(18.0, 18.0),
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    if running || state.inspection_phase == OperationPhase::Running {
                        ui.spinner();
                    }
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let repair_accessible = format!("{} {name}", repair_word(locale));
                let repair_enabled = controls_enabled && state.repair_enabled(kind);
                let repair = ui.add_enabled(
                    repair_enabled,
                    egui::Button::image_and_text(
                        egui::Image::new(icons::wrench()).fit_to_exact_size(egui::vec2(15.0, 15.0)),
                        repair_word(locale),
                    ),
                );
                repair.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        repair_enabled,
                        &repair_accessible,
                    )
                });
                if repair.on_hover_text(&repair_accessible).clicked() {
                    actions.push(MarketplaceAction::RequestRepair(kind));
                }

                let refresh_accessible = format!("{} {name}", refresh_word(locale));
                let refresh = ui.add_enabled(
                    controls_enabled,
                    egui::Button::image(
                        egui::Image::new(icons::refresh_cw())
                            .fit_to_exact_size(egui::vec2(15.0, 15.0)),
                    ),
                );
                refresh.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        controls_enabled,
                        &refresh_accessible,
                    )
                });
                if refresh.on_hover_text(&refresh_accessible).clicked() {
                    actions.push(MarketplaceAction::Refresh);
                }

                ui.add_sized(
                    [76.0, 24.0],
                    egui::Label::new(format!(
                        "{}: {}",
                        skills_word(locale),
                        status.map_or(0, |status| status.skill_count)
                    )),
                );
                ui.add_sized(
                    [80.0, 24.0],
                    egui::Label::new(format!(
                        "{}: {}",
                        plugins_word(locale),
                        status.map_or(0, |status| status.plugin_count)
                    )),
                );
            });
        },
    );
}

fn render_confirmation(
    ctx: &egui::Context,
    state: &MarketplaceViewState,
    locale: Locale,
    actions: &mut Vec<MarketplaceAction>,
) {
    let Some(kind) = state.confirmation_kind else {
        return;
    };
    egui::Window::new(confirmation_title(locale, kind))
        .id(egui::Id::new("plugin_marketplace_repair_confirmation"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.add_sized(
                [420.0, 58.0],
                egui::Label::new(confirmation_body(locale, kind)).wrap(),
            );
            ui.add_space(6.0);
            ui.allocate_ui_with_layout(
                egui::vec2(420.0, 30.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if ui.button(repair_word(locale)).clicked() {
                        actions.push(MarketplaceAction::ConfirmRepair);
                    }
                    if ui.button(cancel_word(locale)).clicked() {
                        actions.push(MarketplaceAction::CancelRepair);
                    }
                },
            );
        });
}

fn row_status(
    state: &MarketplaceViewState,
    locale: Locale,
    kind: PluginMarketplaceKind,
) -> (&'static str, egui::Color32) {
    if state.active_repair_kind == Some(kind) && state.repair_phase == OperationPhase::Running {
        return (repairing_word(locale), theme::WARNING_COLOR);
    }
    if state.inspection_phase == OperationPhase::Running {
        return (checking_word(locale), theme::WARNING_COLOR);
    }
    if state.repair_phase == OperationPhase::Error
        && state.failed_repair_kind == Some(kind)
        && state.repair_error.is_some()
    {
        return (failure_text(locale, state.repair_error), theme::ERROR_COLOR);
    }
    if state.inspection_phase == OperationPhase::Error && state.workspace.is_none() {
        return (
            failure_text(locale, state.inspection_error),
            theme::ERROR_COLOR,
        );
    }
    if state
        .last_repair
        .is_some_and(|(last_kind, _)| last_kind == kind)
        && let Some((_, outcome)) = state.last_repair
    {
        return (outcome_text(locale, outcome), theme::SUCCESS_COLOR);
    }
    match state.status(kind) {
        Some(status) if !status.needs_repair => (ready_word(locale), theme::SUCCESS_COLOR),
        Some(_) => (repair_needed_word(locale), theme::WARNING_COLOR),
        None => (not_available_word(locale), theme::WARNING_COLOR),
    }
}

fn failure_text(locale: Locale, failure: Option<MarketplaceFailureKind>) -> &'static str {
    match failure {
        Some(MarketplaceFailureKind::WorkerStopped)
        | Some(MarketplaceFailureKind::Service(PluginMarketplaceErrorKind::WorkerStopped)) => {
            match locale {
                Locale::ZhCn => "工作线程已停止",
                Locale::En => "Worker stopped",
            }
        }
        Some(MarketplaceFailureKind::Service(PluginMarketplaceErrorKind::DownloadFailed)) => {
            match locale {
                Locale::ZhCn => "下载失败",
                Locale::En => "Download failed",
            }
        }
        Some(MarketplaceFailureKind::Service(PluginMarketplaceErrorKind::DownloadTooLarge)) => {
            match locale {
                Locale::ZhCn => "下载超过限制",
                Locale::En => "Download too large",
            }
        }
        Some(MarketplaceFailureKind::Service(PluginMarketplaceErrorKind::ArchiveInvalid)) => {
            match locale {
                Locale::ZhCn => "归档无效",
                Locale::En => "Invalid archive",
            }
        }
        Some(MarketplaceFailureKind::Service(PluginMarketplaceErrorKind::Conflict)) => match locale
        {
            Locale::ZhCn => "状态已变化",
            Locale::En => "State changed",
        },
        Some(MarketplaceFailureKind::Service(PluginMarketplaceErrorKind::WriteFailed)) => {
            match locale {
                Locale::ZhCn => "写入失败",
                Locale::En => "Write failed",
            }
        }
        Some(MarketplaceFailureKind::Service(PluginMarketplaceErrorKind::InspectFailed)) | None => {
            match locale {
                Locale::ZhCn => "检查失败",
                Locale::En => "Inspection failed",
            }
        }
    }
}

fn outcome_text(locale: Locale, outcome: PluginMarketplaceRepairOutcome) -> &'static str {
    match (locale, outcome) {
        (Locale::ZhCn, PluginMarketplaceRepairOutcome::Initialized) => "已初始化",
        (Locale::ZhCn, PluginMarketplaceRepairOutcome::Configured) => "已注册",
        (Locale::ZhCn, PluginMarketplaceRepairOutcome::AlreadyHealthy) => "已经可用",
        (Locale::En, PluginMarketplaceRepairOutcome::Initialized) => "Initialized",
        (Locale::En, PluginMarketplaceRepairOutcome::Configured) => "Configured",
        (Locale::En, PluginMarketplaceRepairOutcome::AlreadyHealthy) => "Already ready",
    }
}

fn section_title(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "插件市场",
        Locale::En => "Plugin marketplaces",
    }
}

fn marketplace_name(locale: Locale, kind: PluginMarketplaceKind) -> &'static str {
    match (locale, kind) {
        (Locale::ZhCn, PluginMarketplaceKind::Local) => "OpenAI 插件",
        (Locale::ZhCn, PluginMarketplaceKind::Remote) => "官方远端缓存",
        (Locale::En, PluginMarketplaceKind::Local) => "OpenAI plugins",
        (Locale::En, PluginMarketplaceKind::Remote) => "Official remote cache",
    }
}

fn marketplace_source(locale: Locale, kind: PluginMarketplaceKind) -> &'static str {
    match (locale, kind) {
        (Locale::ZhCn, PluginMarketplaceKind::Local) => "在线来源",
        (Locale::ZhCn, PluginMarketplaceKind::Remote) => "内置离线",
        (Locale::En, PluginMarketplaceKind::Local) => "Online source",
        (Locale::En, PluginMarketplaceKind::Remote) => "Embedded offline",
    }
}

fn confirmation_title(locale: Locale, kind: PluginMarketplaceKind) -> &'static str {
    match (locale, kind) {
        (Locale::ZhCn, PluginMarketplaceKind::Local) => "修复 OpenAI 插件？",
        (Locale::ZhCn, PluginMarketplaceKind::Remote) => "修复官方远端缓存？",
        (Locale::En, PluginMarketplaceKind::Local) => "Repair OpenAI plugins?",
        (Locale::En, PluginMarketplaceKind::Remote) => "Repair official remote cache?",
    }
}

fn confirmation_body(locale: Locale, kind: PluginMarketplaceKind) -> &'static str {
    match (locale, kind) {
        (Locale::ZhCn, PluginMarketplaceKind::Local) => {
            "下载在线市场（最大 128 MiB）并完成校验，然后更新本地缓存与 Codex 配置。"
        }
        (Locale::ZhCn, PluginMarketplaceKind::Remote) => {
            "校验并释放内置离线快照，然后更新本地缓存与 Codex 配置。"
        }
        (Locale::En, PluginMarketplaceKind::Local) => {
            "Downloads the online marketplace (maximum 128 MiB), validates it, then updates the local cache and Codex configuration."
        }
        (Locale::En, PluginMarketplaceKind::Remote) => {
            "Validates and releases the embedded offline snapshot, then updates the local cache and Codex configuration."
        }
    }
}

fn repair_word(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "修复",
        Locale::En => "Repair",
    }
}

fn refresh_word(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "刷新",
        Locale::En => "Refresh",
    }
}

fn cancel_word(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "取消",
        Locale::En => "Cancel",
    }
}

fn plugins_word(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "插件",
        Locale::En => "Plugins",
    }
}

fn skills_word(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "技能",
        Locale::En => "Skills",
    }
}

fn ready_word(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "可用",
        Locale::En => "Ready",
    }
}

fn repair_needed_word(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "需要修复",
        Locale::En => "Repair needed",
    }
}

fn not_available_word(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "尚不可用",
        Locale::En => "Not available",
    }
}

fn checking_word(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "正在检查",
        Locale::En => "Checking",
    }
}

fn repairing_word(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "正在修复",
        Locale::En => "Repairing",
    }
}
