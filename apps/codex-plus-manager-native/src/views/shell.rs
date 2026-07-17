use std::sync::Arc;

use codex_plus_manager_service::OverviewSnapshot;
use eframe::egui;

use crate::i18n::{Locale, TextKey, ThemeMode, text};
use crate::state::provider::{ProviderLoadPhase, ProviderViewState};
use crate::state::{OverviewFailureKind, OverviewPhase, Route};
use crate::{icons, theme};

use super::{about, overview, provider};

pub const SIDEBAR_WIDTH: f32 = 176.0;
pub const HEADER_HEIGHT: f32 = 58.0;
pub const STATUS_HEIGHT: f32 = 28.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellAction {
    Navigate(Route),
    Refresh,
    SetLocale(Locale),
    SetTheme(ThemeMode),
    Retry,
    Provider(provider::ProviderAction),
}

#[derive(Debug, Clone)]
pub struct ShellViewModel {
    pub route: Route,
    pub locale: Locale,
    pub theme: ThemeMode,
    pub overview_phase: OverviewPhase,
    pub overview_snapshot: Option<Arc<OverviewSnapshot>>,
    pub overview_error: Option<OverviewFailureKind>,
    pub last_updated: Option<String>,
    pub renderer: String,
}

pub fn render_shell(
    ui: &mut egui::Ui,
    model: &ShellViewModel,
    provider_state: Option<&ProviderViewState>,
) -> Vec<ShellAction> {
    let mut actions = Vec::new();

    egui::Panel::left("native_manager_sidebar")
        .exact_size(SIDEBAR_WIDTH)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().window_fill)
                .inner_margin(egui::Margin::symmetric(12, 14)),
        )
        .show(ui, |ui| render_sidebar(ui, model, &mut actions));

    egui::Panel::top("native_manager_header")
        .exact_size(HEADER_HEIGHT)
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::symmetric(16, 8)),
        )
        .show(ui, |ui| render_header(ui, model, &mut actions));

    egui::Panel::bottom("native_manager_status")
        .exact_size(STATUS_HEIGHT)
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().window_fill)
                .inner_margin(egui::Margin::symmetric(16, 4)),
        )
        .show(ui, |ui| render_status(ui, model, provider_state));

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::same(16)),
        )
        .show(ui, |ui| match model.route {
            Route::Overview => overview::render(ui, model, &mut actions),
            Route::Providers => {
                if let Some(state) = provider_state {
                    let mut provider_actions = Vec::new();
                    provider::render(ui, state, model.locale, &mut provider_actions);
                    actions.extend(provider_actions.into_iter().map(ShellAction::Provider));
                }
            }
            Route::About => about::render(ui, model),
        });

    actions
}

fn render_sidebar(ui: &mut egui::Ui, model: &ShellViewModel, actions: &mut Vec<ShellAction>) {
    ui.label(
        egui::RichText::new(text(model.locale, TextKey::AppName))
            .strong()
            .size(20.0),
    );
    ui.label(egui::RichText::new("Native Manager").weak().size(11.0));
    ui.add_space(18.0);

    navigation_button(
        ui,
        icons::layout_dashboard(),
        text(model.locale, TextKey::Overview),
        model.route == Route::Overview,
        Route::Overview,
        actions,
    );
    navigation_button(
        ui,
        icons::server_cog(),
        text(model.locale, TextKey::Providers),
        model.route == Route::Providers,
        Route::Providers,
        actions,
    );
    navigation_button(
        ui,
        icons::info(),
        text(model.locale, TextKey::About),
        model.route == Route::About,
        Route::About,
        actions,
    );

    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.horizontal(|ui| {
            segmented_button(
                ui,
                text(model.locale, TextKey::Dark),
                model.theme == ThemeMode::Dark,
                ShellAction::SetTheme(ThemeMode::Dark),
                actions,
            );
            segmented_button(
                ui,
                text(model.locale, TextKey::Light),
                model.theme == ThemeMode::Light,
                ShellAction::SetTheme(ThemeMode::Light),
                actions,
            );
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(icons::sun())
                    .fit_to_exact_size(egui::vec2(14.0, 14.0))
                    .tint(ui.visuals().weak_text_color()),
            );
            ui.label(egui::RichText::new(text(model.locale, TextKey::Theme)).weak());
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            segmented_button(
                ui,
                text(model.locale, TextKey::Chinese),
                model.locale == Locale::ZhCn,
                ShellAction::SetLocale(Locale::ZhCn),
                actions,
            );
            segmented_button(
                ui,
                text(model.locale, TextKey::English),
                model.locale == Locale::En,
                ShellAction::SetLocale(Locale::En),
                actions,
            );
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(icons::languages())
                    .fit_to_exact_size(egui::vec2(14.0, 14.0))
                    .tint(ui.visuals().weak_text_color()),
            );
            ui.label(egui::RichText::new(text(model.locale, TextKey::Language)).weak());
        });
    });
}

fn navigation_button(
    ui: &mut egui::Ui,
    icon: egui::ImageSource<'static>,
    label: &str,
    selected: bool,
    route: Route,
    actions: &mut Vec<ShellAction>,
) {
    let image = egui::Image::new(icon).fit_to_exact_size(egui::vec2(16.0, 16.0));
    if ui
        .add_sized(
            [ui.available_width(), 36.0],
            egui::Button::image_and_text(image, label).selected(selected),
        )
        .clicked()
    {
        actions.push(ShellAction::Navigate(route));
    }
}

fn segmented_button(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    action: ShellAction,
    actions: &mut Vec<ShellAction>,
) {
    let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
    if ui
        .add_sized(
            [width.max(56.0), 30.0],
            egui::Button::new(label).selected(selected),
        )
        .clicked()
    {
        actions.push(action);
    }
}

fn render_header(ui: &mut egui::Ui, model: &ShellViewModel, actions: &mut Vec<ShellAction>) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            let title = match model.route {
                Route::Overview => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::Overview)
                ),
                Route::Providers => format!(
                    "{} {}",
                    text(model.locale, TextKey::AppName),
                    text(model.locale, TextKey::Providers)
                ),
                Route::About => format!(
                    "{} {}",
                    text(model.locale, TextKey::About),
                    text(model.locale, TextKey::AppName)
                ),
            };
            ui.label(egui::RichText::new(title).strong().size(17.0));
            let subtitle = match model.route {
                Route::Overview => TextKey::OverviewSubtitle,
                Route::Providers => TextKey::ProvidersSubtitle,
                Route::About => TextKey::AboutSubtitle,
            };
            ui.label(
                egui::RichText::new(text(model.locale, subtitle))
                    .weak()
                    .size(11.0),
            );
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = text(model.locale, TextKey::Refresh);
            let response = icon_button(ui, icons::refresh_cw(), label);
            if response.clicked() {
                actions.push(ShellAction::Refresh);
            }
            if let Some(updated) = &model.last_updated {
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {updated}",
                        text(model.locale, TextKey::LastUpdated)
                    ))
                    .weak()
                    .size(11.0),
                );
            }
        });
    });
}

fn icon_button(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, label: &str) -> egui::Response {
    let response = ui.add_sized(
        [34.0, 34.0],
        egui::Button::image(egui::Image::new(icon).fit_to_exact_size(egui::vec2(17.0, 17.0))),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response.on_hover_text(label)
}

fn render_status(
    ui: &mut egui::Ui,
    model: &ShellViewModel,
    provider_state: Option<&ProviderViewState>,
) {
    if model.route == Route::Providers {
        let phase = provider_state.map_or(ProviderLoadPhase::Idle, |state| state.load_phase);
        let (status, color) = match phase {
            ProviderLoadPhase::Idle | ProviderLoadPhase::Loading => {
                (text(model.locale, TextKey::Loading), theme::WARNING_COLOR)
            }
            ProviderLoadPhase::Ready => (text(model.locale, TextKey::Ready), theme::SUCCESS_COLOR),
            ProviderLoadPhase::Refreshing => (
                text(model.locale, TextKey::Refreshing),
                theme::WARNING_COLOR,
            ),
            ProviderLoadPhase::Error => (
                match model.locale {
                    Locale::ZhCn => "供应商加载失败",
                    Locale::En => "Provider load failed",
                },
                theme::ERROR_COLOR,
            ),
        };
        ui.horizontal(|ui| {
            ui.colored_label(
                color,
                format!("{}: {status}", text(model.locale, TextKey::Status)),
            );
            render_status_metadata(ui, model);
        });
        return;
    }

    let phase = match model.overview_phase {
        OverviewPhase::Idle | OverviewPhase::Loading => TextKey::Loading,
        OverviewPhase::Ready => TextKey::Ready,
        OverviewPhase::Refreshing => TextKey::Refreshing,
        OverviewPhase::Error => match model.overview_error {
            Some(OverviewFailureKind::WorkerStopped) => TextKey::WorkerStopped,
            Some(OverviewFailureKind::LoadFailed) | None => TextKey::LoadFailed,
        },
    };
    ui.horizontal(|ui| {
        let color = match model.overview_phase {
            OverviewPhase::Ready => theme::SUCCESS_COLOR,
            OverviewPhase::Error => theme::ERROR_COLOR,
            _ => theme::WARNING_COLOR,
        };
        ui.colored_label(
            color,
            format!(
                "{}: {}",
                text(model.locale, TextKey::Status),
                text(model.locale, phase)
            ),
        );
        render_status_metadata(ui, model);
    });
}

fn render_status_metadata(ui: &mut egui::Ui, model: &ShellViewModel) {
    ui.separator();
    ui.label(format!(
        "{}: {}",
        text(model.locale, TextKey::Renderer),
        model.renderer
    ));
    ui.separator();
    ui.label(format!(
        "{}: {}",
        text(model.locale, TextKey::Language),
        match model.locale {
            Locale::ZhCn => text(model.locale, TextKey::Chinese),
            Locale::En => text(model.locale, TextKey::English),
        }
    ));
}
