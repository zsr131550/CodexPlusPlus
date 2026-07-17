use eframe::egui;

use crate::i18n::ThemeMode;

pub const SUCCESS_COLOR: egui::Color32 = egui::Color32::from_rgb(45, 160, 92);
pub const WARNING_COLOR: egui::Color32 = egui::Color32::from_rgb(210, 142, 38);
pub const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(210, 66, 66);
pub const ACCENT_COLOR: egui::Color32 = egui::Color32::from_rgb(205, 58, 66);

pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    let theme = match mode {
        ThemeMode::Dark => egui::Theme::Dark,
        ThemeMode::Light => egui::Theme::Light,
    };
    let mut style = (*ctx.style_of(theme)).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.interact_size = egui::vec2(30.0, 30.0);

    let mut visuals = match mode {
        ThemeMode::Dark => egui::Visuals::dark(),
        ThemeMode::Light => egui::Visuals::light(),
    };
    match mode {
        ThemeMode::Dark => {
            visuals.panel_fill = egui::Color32::from_rgb(22, 24, 29);
            visuals.window_fill = egui::Color32::from_rgb(29, 32, 38);
            visuals.extreme_bg_color = egui::Color32::from_rgb(17, 19, 23);
            visuals.faint_bg_color = egui::Color32::from_rgb(35, 38, 45);
            visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(39, 42, 50);
            visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(51, 54, 63);
            visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(62, 48, 52);
        }
        ThemeMode::Light => {
            visuals.panel_fill = egui::Color32::from_rgb(244, 245, 247);
            visuals.window_fill = egui::Color32::WHITE;
            visuals.extreme_bg_color = egui::Color32::from_rgb(235, 237, 240);
            visuals.faint_bg_color = egui::Color32::from_rgb(248, 249, 250);
            visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(232, 234, 237);
            visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(222, 225, 229);
            visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(246, 225, 227);
        }
    }

    let radius = egui::CornerRadius::same(6);
    visuals.window_corner_radius = radius;
    visuals.menu_corner_radius = radius;
    visuals.widgets.noninteractive.corner_radius = radius;
    visuals.widgets.inactive.corner_radius = radius;
    visuals.widgets.hovered.corner_radius = radius;
    visuals.widgets.active.corner_radius = radius;
    visuals.widgets.open.corner_radius = radius;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT_COLOR);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT_COLOR);
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, ACCENT_COLOR);
    visuals.selection.bg_fill = ACCENT_COLOR;
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.warn_fg_color = WARNING_COLOR;
    visuals.error_fg_color = ERROR_COLOR;
    style.visuals = visuals;
    ctx.set_style_of(theme, style);
    ctx.set_theme(theme);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_applies_explicit_mode_and_stable_control_height() {
        let ctx = egui::Context::default();

        apply(&ctx, ThemeMode::Dark);
        let dark = ctx.style_of(egui::Theme::Dark);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        assert!(dark.visuals.dark_mode);
        assert_eq!(dark.spacing.interact_size.y, 30.0);

        apply(&ctx, ThemeMode::Light);
        let light = ctx.style_of(egui::Theme::Light);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert!(!light.visuals.dark_mode);
        assert_eq!(light.spacing.interact_size.y, 30.0);
        assert_ne!(dark.visuals.panel_fill, light.visuals.panel_fill);
    }
}
