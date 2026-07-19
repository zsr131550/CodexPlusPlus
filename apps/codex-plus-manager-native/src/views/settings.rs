use std::fmt;

use codex_plus_manager_service::{ImageOverlayFitMode, SafeSettingsGroup, SecretReplacement};
use eframe::egui;

use crate::i18n::Locale;
use crate::state::settings::{
    SettingsFailureKind, SettingsOperationPhase, SettingsTab, SettingsViewState,
};
use crate::{icons, theme};

#[derive(Clone, PartialEq, Eq)]
pub enum SettingsAction {
    Refresh,
    SetTab(SettingsTab),
    EditStepwiseEnabled(bool),
    EditStepwiseDirectSend(bool),
    EditStepwiseUrl(String),
    EditStepwiseEnvironment(String),
    EditStepwiseModel(String),
    EditStepwiseMaxItems(u8),
    EditStepwiseMaxInputChars(u32),
    EditStepwiseMaxOutputTokens(u32),
    EditStepwiseTimeoutMs(u64),
    EditSecretReplacement(SecretReplacement),
    SetPasswordVisible(bool),
    RequestSecretClear,
    ConfirmSecretClear,
    CancelSecretClear,
    TestStepwise,
    SaveStepwise,
    RequestReset(SafeSettingsGroup),
    ConfirmReset,
    CancelReset,
    EditImageEnabled(bool),
    EditImagePath(String),
    PickImage,
    EditImageOpacity(u8),
    EditImageFitMode(ImageOverlayFitMode),
    SaveImage,
    EditExtraArgs(String),
    SaveExtraArgs,
    ConfirmDiscard,
    CancelDiscard,
}

impl fmt::Debug for SettingsAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EditStepwiseUrl(_) => formatter.write_str("EditStepwiseUrl([redacted])"),
            Self::EditStepwiseEnvironment(_) => {
                formatter.write_str("EditStepwiseEnvironment([redacted])")
            }
            Self::EditStepwiseModel(_) => formatter.write_str("EditStepwiseModel([redacted])"),
            Self::EditSecretReplacement(_) => {
                formatter.write_str("EditSecretReplacement([redacted])")
            }
            Self::EditImagePath(_) => formatter.write_str("EditImagePath([redacted])"),
            Self::EditExtraArgs(_) => formatter.write_str("EditExtraArgs([redacted])"),
            Self::SetTab(value) => formatter.debug_tuple("SetTab").field(value).finish(),
            Self::EditStepwiseEnabled(value) => formatter
                .debug_tuple("EditStepwiseEnabled")
                .field(value)
                .finish(),
            Self::EditStepwiseDirectSend(value) => formatter
                .debug_tuple("EditStepwiseDirectSend")
                .field(value)
                .finish(),
            Self::EditStepwiseMaxItems(value) => formatter
                .debug_tuple("EditStepwiseMaxItems")
                .field(value)
                .finish(),
            Self::EditStepwiseMaxInputChars(value) => formatter
                .debug_tuple("EditStepwiseMaxInputChars")
                .field(value)
                .finish(),
            Self::EditStepwiseMaxOutputTokens(value) => formatter
                .debug_tuple("EditStepwiseMaxOutputTokens")
                .field(value)
                .finish(),
            Self::EditStepwiseTimeoutMs(value) => formatter
                .debug_tuple("EditStepwiseTimeoutMs")
                .field(value)
                .finish(),
            Self::SetPasswordVisible(value) => formatter
                .debug_tuple("SetPasswordVisible")
                .field(value)
                .finish(),
            Self::RequestReset(value) => {
                formatter.debug_tuple("RequestReset").field(value).finish()
            }
            Self::EditImageEnabled(value) => formatter
                .debug_tuple("EditImageEnabled")
                .field(value)
                .finish(),
            Self::EditImageOpacity(value) => formatter
                .debug_tuple("EditImageOpacity")
                .field(value)
                .finish(),
            Self::EditImageFitMode(value) => formatter
                .debug_tuple("EditImageFitMode")
                .field(value)
                .finish(),
            Self::Refresh => formatter.write_str("Refresh"),
            Self::RequestSecretClear => formatter.write_str("RequestSecretClear"),
            Self::ConfirmSecretClear => formatter.write_str("ConfirmSecretClear"),
            Self::CancelSecretClear => formatter.write_str("CancelSecretClear"),
            Self::TestStepwise => formatter.write_str("TestStepwise"),
            Self::SaveStepwise => formatter.write_str("SaveStepwise"),
            Self::ConfirmReset => formatter.write_str("ConfirmReset"),
            Self::CancelReset => formatter.write_str("CancelReset"),
            Self::PickImage => formatter.write_str("PickImage"),
            Self::SaveImage => formatter.write_str("SaveImage"),
            Self::SaveExtraArgs => formatter.write_str("SaveExtraArgs"),
            Self::ConfirmDiscard => formatter.write_str("ConfirmDiscard"),
            Self::CancelDiscard => formatter.write_str("CancelDiscard"),
        }
    }
}

pub fn render(
    ui: &mut egui::Ui,
    state: &SettingsViewState,
    locale: Locale,
    actions: &mut Vec<SettingsAction>,
) {
    render_feedback(ui, state, locale);
    render_tabs(ui, state.tab, locale, actions);
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    egui::ScrollArea::vertical()
        .id_salt("settings_page_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| match state.tab {
            SettingsTab::Stepwise => render_stepwise(ui, state, locale, actions),
            SettingsTab::ImageOverlay => render_image_overlay(ui, state, locale, actions),
            SettingsTab::LaunchArguments => render_extra_args(ui, state, locale, actions),
        });

    if state.secret_clear_confirmation_visible() {
        render_secret_clear_confirmation(ui.ctx(), locale, actions);
    }
    if let Some(group) = state.reset_confirmation_group() {
        render_reset_confirmation(ui.ctx(), locale, group, actions);
    }
    if state.discard_confirmation_visible() {
        render_discard_confirmation(ui.ctx(), state, locale, actions);
    }
}

fn render_feedback(ui: &mut egui::Ui, state: &SettingsViewState, locale: Locale) {
    let (conflict, error, ready) = match state.tab {
        SettingsTab::Stepwise => (
            state.stepwise.conflict_visible(),
            state
                .stepwise
                .operation
                .error
                .or(state.stepwise.test.error)
                .or(state.load_error),
            state.stepwise.operation.phase == SettingsOperationPhase::Ready,
        ),
        SettingsTab::ImageOverlay => (
            state.image_overlay.conflict_visible(),
            state.image_overlay.operation.error.or(state.load_error),
            state.image_overlay.operation.phase == SettingsOperationPhase::Ready,
        ),
        SettingsTab::LaunchArguments => (
            state.extra_args.conflict_visible(),
            state.extra_args.operation.error.or(state.load_error),
            state.extra_args.operation.phase == SettingsOperationPhase::Ready,
        ),
    };

    if conflict {
        ui.colored_label(theme::WARNING_COLOR, stext(locale, SText::SettingsChanged));
    } else if let Some(error) = error {
        ui.colored_label(theme::ERROR_COLOR, failure_text(locale, error));
    } else if state.picker_error.is_some() {
        ui.colored_label(theme::ERROR_COLOR, stext(locale, SText::PickerFailed));
    } else if active_group_dirty(state) {
        ui.colored_label(theme::WARNING_COLOR, stext(locale, SText::UnsavedChanges));
    } else if ready {
        ui.colored_label(theme::SUCCESS_COLOR, stext(locale, SText::Saved));
    }
}

fn render_tabs(
    ui: &mut egui::Ui,
    selected: SettingsTab,
    locale: Locale,
    actions: &mut Vec<SettingsAction>,
) {
    ui.horizontal(|ui| {
        for (tab, label) in [
            (SettingsTab::Stepwise, SText::Stepwise),
            (SettingsTab::ImageOverlay, SText::ImageOverlay),
            (SettingsTab::LaunchArguments, SText::LaunchArguments),
        ] {
            if ui
                .add_sized(
                    [148.0, 32.0],
                    egui::Button::new(stext(locale, label)).selected(tab == selected),
                )
                .clicked()
            {
                actions.push(SettingsAction::SetTab(tab));
            }
        }
    });
}

fn render_stepwise(
    ui: &mut egui::Ui,
    state: &SettingsViewState,
    locale: Locale,
    actions: &mut Vec<SettingsAction>,
) {
    let draft = state.stepwise.draft();
    let busy = stepwise_busy(state);

    ui.horizontal(|ui| {
        let mut enabled = draft.enabled;
        if ui
            .checkbox(&mut enabled, stext(locale, SText::EnableStepwise))
            .changed()
        {
            actions.push(SettingsAction::EditStepwiseEnabled(enabled));
        }
        ui.add_space(16.0);
        let mut direct_send = draft.direct_send;
        if ui
            .checkbox(&mut direct_send, stext(locale, SText::DirectSend))
            .changed()
        {
            actions.push(SettingsAction::EditStepwiseDirectSend(direct_send));
        }
    });
    ui.add_space(10.0);

    string_field(
        ui,
        stext(locale, SText::BaseUrl),
        &draft.base_url,
        SettingsAction::EditStepwiseUrl,
        actions,
    );
    string_field(
        ui,
        stext(locale, SText::Model),
        &draft.model,
        SettingsAction::EditStepwiseModel,
        actions,
    );
    string_field(
        ui,
        stext(locale, SText::ApiKeyEnvironment),
        &draft.api_key_env,
        SettingsAction::EditStepwiseEnvironment,
        actions,
    );

    ui.horizontal(|ui| {
        ui.add_sized(
            [FIELD_LABEL_WIDTH, 24.0],
            egui::Label::new(stext(locale, SText::StoredCredential)),
        );
        ui.label(if state.stepwise.api_key_configured {
            stext(locale, SText::Configured)
        } else {
            stext(locale, SText::NotConfigured)
        });
        if state.stepwise.api_key_env_configured {
            ui.label(egui::RichText::new(stext(locale, SText::EnvironmentAvailable)).weak());
        }
    });

    ui.horizontal(|ui| {
        let label = stext(locale, SText::ReplacementApiKey);
        let label_response = ui.add_sized([FIELD_LABEL_WIDTH, 24.0], egui::Label::new(label));
        let mut replacement = draft.secret_replacement().clone();
        let reveal_replacement = state.stepwise.password_visible && !replacement.is_empty();
        let button_space = 34.0 * 2.0 + ui.spacing().item_spacing.x * 2.0;
        let edit_text_color = if reveal_replacement {
            egui::Color32::TRANSPARENT
        } else {
            ui.visuals().text_color()
        };
        let response = ui
            .add_sized(
                [(ui.available_width() - button_space).max(100.0), 32.0],
                egui::TextEdit::singleline(replacement.expose_mut())
                    .password(true)
                    .text_color(edit_text_color)
                    .hint_text(stext(locale, SText::Unchanged)),
            )
            .labelled_by(label_response.id);
        if reveal_replacement {
            let clip_rect = response.rect.shrink2(egui::vec2(8.0, 4.0));
            let font_id = egui::TextStyle::Body.resolve(ui.style());
            let color = ui.visuals().text_color();
            ui.painter().with_clip_rect(clip_rect).text(
                egui::pos2(clip_rect.left(), response.rect.center().y),
                egui::Align2::LEFT_CENTER,
                replacement.expose_mut().as_str(),
                font_id,
                color,
            );
        }
        if response.changed() {
            actions.push(SettingsAction::EditSecretReplacement(replacement));
        }

        let (icon, visibility_label) = if state.stepwise.password_visible {
            (icons::eye_off(), stext(locale, SText::HideReplacementKey))
        } else {
            (icons::eye(), stext(locale, SText::ShowReplacementKey))
        };
        if tool_button(ui, icon, visibility_label, true).clicked() {
            actions.push(SettingsAction::SetPasswordVisible(
                !state.stepwise.password_visible,
            ));
        }
        if tool_button(
            ui,
            icons::trash_2(),
            stext(locale, SText::ClearStoredCredential),
            state.stepwise.api_key_configured && !busy,
        )
        .clicked()
        {
            actions.push(SettingsAction::RequestSecretClear);
        }
    });

    ui.add_space(12.0);
    numeric_field_u8(
        ui,
        stext(locale, SText::MaximumItems),
        draft.max_items,
        0..=6,
        SettingsAction::EditStepwiseMaxItems,
        actions,
    );
    numeric_field_u32(
        ui,
        stext(locale, SText::MaximumInputCharacters),
        draft.max_input_chars,
        1_000..=24_000,
        SettingsAction::EditStepwiseMaxInputChars,
        actions,
    );
    numeric_field_u32(
        ui,
        stext(locale, SText::MaximumOutputTokens),
        draft.max_output_tokens,
        100..=4_000,
        SettingsAction::EditStepwiseMaxOutputTokens,
        actions,
    );
    numeric_field_u64(
        ui,
        stext(locale, SText::TimeoutMs),
        draft.timeout_ms,
        1_000..=60_000,
        SettingsAction::EditStepwiseTimeoutMs,
        actions,
    );

    if let Some(outcome) = state.stepwise.test_outcome {
        ui.add_space(8.0);
        ui.colored_label(
            theme::SUCCESS_COLOR,
            format!(
                "{}: {}",
                stext(locale, SText::ConnectionSucceeded),
                outcome.item_count
            ),
        );
    }
    ui.add_space(14.0);
    render_action_row(
        ui,
        [
            (
                icons::stethoscope(),
                stext(locale, SText::TestConnection),
                !busy && state.stepwise.revision().is_some(),
                SettingsAction::TestStepwise,
            ),
            (
                icons::save(),
                stext(locale, SText::SaveStepwise),
                !busy && state.stepwise.is_dirty(),
                SettingsAction::SaveStepwise,
            ),
            (
                icons::rotate_ccw(),
                stext(locale, SText::ResetStepwise),
                !busy && state.stepwise.revision().is_some(),
                SettingsAction::RequestReset(SafeSettingsGroup::Stepwise),
            ),
        ],
        actions,
    );
}

fn render_image_overlay(
    ui: &mut egui::Ui,
    state: &SettingsViewState,
    locale: Locale,
    actions: &mut Vec<SettingsAction>,
) {
    let draft = state.image_overlay.draft();
    let busy = state.image_overlay.operation.phase == SettingsOperationPhase::Running;

    let mut enabled = draft.enabled;
    if ui
        .checkbox(&mut enabled, stext(locale, SText::EnableImageOverlay))
        .changed()
    {
        actions.push(SettingsAction::EditImageEnabled(enabled));
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let label = stext(locale, SText::ImagePath);
        let label_response = ui.add_sized([FIELD_LABEL_WIDTH, 24.0], egui::Label::new(label));
        let mut path = draft.path.clone();
        let response = ui
            .add_sized(
                [(ui.available_width() - 40.0).max(100.0), 32.0],
                egui::TextEdit::singleline(&mut path),
            )
            .labelled_by(label_response.id);
        if response.changed() {
            actions.push(SettingsAction::EditImagePath(path));
        }
        if tool_button(
            ui,
            icons::folder_open(),
            stext(locale, SText::SelectImage),
            !state.picker_pending(),
        )
        .clicked()
        {
            actions.push(SettingsAction::PickImage);
        }
    });

    ui.horizontal(|ui| {
        let label = stext(locale, SText::Opacity);
        ui.add_sized([FIELD_LABEL_WIDTH, 24.0], egui::Label::new(label));
        let mut opacity = draft.opacity;
        let response = ui.add_sized(
            [ui.available_width().max(120.0), 24.0],
            egui::Slider::new(&mut opacity, 1..=100).suffix("%"),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Slider, ui.is_enabled(), label)
        });
        if response.changed() {
            actions.push(SettingsAction::EditImageOpacity(opacity));
        }
    });

    ui.horizontal(|ui| {
        let label = stext(locale, SText::FitMode);
        ui.add_sized([FIELD_LABEL_WIDTH, 24.0], egui::Label::new(label));
        let response = egui::ComboBox::from_id_salt("settings_image_fit_mode")
            .selected_text(fit_mode_text(locale, draft.fit_mode))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for mode in [
                    ImageOverlayFitMode::Fill,
                    ImageOverlayFitMode::Fit,
                    ImageOverlayFitMode::Stretch,
                    ImageOverlayFitMode::Tile,
                    ImageOverlayFitMode::Center,
                ] {
                    if ui
                        .selectable_label(draft.fit_mode == mode, fit_mode_text(locale, mode))
                        .clicked()
                    {
                        actions.push(SettingsAction::EditImageFitMode(mode));
                    }
                }
            });
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), label)
        });
    });

    ui.add_space(18.0);
    render_action_row(
        ui,
        [
            (
                icons::save(),
                stext(locale, SText::SaveImageOverlay),
                !busy && state.image_overlay.is_dirty(),
                SettingsAction::SaveImage,
            ),
            (
                icons::rotate_ccw(),
                stext(locale, SText::ResetImageOverlay),
                !busy && state.image_overlay.revision().is_some(),
                SettingsAction::RequestReset(SafeSettingsGroup::ImageOverlay),
            ),
            (
                icons::refresh_cw(),
                stext(locale, SText::RefreshSettings),
                !busy,
                SettingsAction::Refresh,
            ),
        ],
        actions,
    );
}

fn render_extra_args(
    ui: &mut egui::Ui,
    state: &SettingsViewState,
    locale: Locale,
    actions: &mut Vec<SettingsAction>,
) {
    let draft = state.extra_args.draft();
    let busy = state.extra_args.operation.phase == SettingsOperationPhase::Running;
    let label_response = ui.label(
        egui::RichText::new(stext(locale, SText::CodexLaunchArguments))
            .strong()
            .size(15.0),
    );
    ui.add_space(6.0);
    let mut value = draft.text.clone();
    let response = ui
        .add_sized(
            [ui.available_width(), 260.0],
            egui::TextEdit::multiline(&mut value)
                .font(egui::TextStyle::Monospace)
                .hint_text("--flag\n--option=value"),
        )
        .labelled_by(label_response.id);
    if response.changed() {
        actions.push(SettingsAction::EditExtraArgs(value));
    }
    ui.add_space(6.0);
    ui.label(format!(
        "{}: {}",
        stext(locale, SText::ArgumentCount),
        draft.argument_count()
    ));
    ui.add_space(14.0);
    render_action_row(
        ui,
        [
            (
                icons::save(),
                stext(locale, SText::SaveLaunchArguments),
                !busy && state.extra_args.is_dirty(),
                SettingsAction::SaveExtraArgs,
            ),
            (
                icons::rotate_ccw(),
                stext(locale, SText::ResetLaunchArguments),
                !busy && state.extra_args.revision().is_some(),
                SettingsAction::RequestReset(SafeSettingsGroup::ExtraArgs),
            ),
            (
                icons::refresh_cw(),
                stext(locale, SText::RefreshSettings),
                !busy,
                SettingsAction::Refresh,
            ),
        ],
        actions,
    );
}

fn render_secret_clear_confirmation(
    ctx: &egui::Context,
    locale: Locale,
    actions: &mut Vec<SettingsAction>,
) {
    egui::Window::new(stext(locale, SText::ClearCredentialTitle))
        .id(egui::Id::new("settings_clear_credential_confirmation"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_min_width(360.0);
            ui.label(stext(locale, SText::ClearCredentialMessage));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(stext(locale, SText::ClearCredential)).clicked() {
                    actions.push(SettingsAction::ConfirmSecretClear);
                }
                if ui.button(stext(locale, SText::Cancel)).clicked() {
                    actions.push(SettingsAction::CancelSecretClear);
                }
            });
        });
}

fn render_reset_confirmation(
    ctx: &egui::Context,
    locale: Locale,
    group: SafeSettingsGroup,
    actions: &mut Vec<SettingsAction>,
) {
    egui::Window::new(reset_title(locale, group))
        .id(egui::Id::new("settings_group_reset_confirmation"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_min_width(360.0);
            ui.label(stext(locale, SText::ResetMessage));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(reset_button(locale, group)).clicked() {
                    actions.push(SettingsAction::ConfirmReset);
                }
                if ui.button(stext(locale, SText::Cancel)).clicked() {
                    actions.push(SettingsAction::CancelReset);
                }
            });
        });
}

fn render_discard_confirmation(
    ctx: &egui::Context,
    state: &SettingsViewState,
    locale: Locale,
    actions: &mut Vec<SettingsAction>,
) {
    egui::Window::new(stext(locale, SText::DiscardTitle))
        .id(egui::Id::new("settings_discard_confirmation"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_min_width(380.0);
            ui.label(stext(locale, SText::DiscardMessage));
            if let Some(groups) = state.pending_dirty_groups() {
                for group in [
                    SafeSettingsGroup::Stepwise,
                    SafeSettingsGroup::ImageOverlay,
                    SafeSettingsGroup::ExtraArgs,
                ] {
                    if groups.contains(group) {
                        ui.label(format!("• {}", group_text(locale, group)));
                    }
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(stext(locale, SText::DiscardChanges)).clicked() {
                    actions.push(SettingsAction::ConfirmDiscard);
                }
                if ui.button(stext(locale, SText::KeepEditing)).clicked() {
                    actions.push(SettingsAction::CancelDiscard);
                }
            });
        });
}

const FIELD_LABEL_WIDTH: f32 = 188.0;
const COMMAND_WIDTH: f32 = 172.0;

fn string_field(
    ui: &mut egui::Ui,
    label: &str,
    current: &str,
    action: impl FnOnce(String) -> SettingsAction,
    actions: &mut Vec<SettingsAction>,
) {
    ui.horizontal(|ui| {
        let label_response = ui.add_sized([FIELD_LABEL_WIDTH, 24.0], egui::Label::new(label));
        let mut value = current.to_owned();
        let response = ui
            .add_sized(
                [ui.available_width().max(100.0), 32.0],
                egui::TextEdit::singleline(&mut value),
            )
            .labelled_by(label_response.id);
        if response.changed() {
            actions.push(action(value));
        }
    });
}

fn numeric_field_u8(
    ui: &mut egui::Ui,
    label: &str,
    current: u8,
    range: std::ops::RangeInclusive<u8>,
    action: impl FnOnce(u8) -> SettingsAction,
    actions: &mut Vec<SettingsAction>,
) {
    ui.horizontal(|ui| {
        ui.add_sized([FIELD_LABEL_WIDTH, 24.0], egui::Label::new(label));
        let mut value = current;
        let response = ui.add(egui::DragValue::new(&mut value).range(range).speed(1));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::DragValue, ui.is_enabled(), label)
        });
        if response.changed() {
            actions.push(action(value));
        }
    });
}

fn numeric_field_u32(
    ui: &mut egui::Ui,
    label: &str,
    current: u32,
    range: std::ops::RangeInclusive<u32>,
    action: impl FnOnce(u32) -> SettingsAction,
    actions: &mut Vec<SettingsAction>,
) {
    ui.horizontal(|ui| {
        ui.add_sized([FIELD_LABEL_WIDTH, 24.0], egui::Label::new(label));
        let mut value = current;
        let response = ui.add(egui::DragValue::new(&mut value).range(range).speed(100));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::DragValue, ui.is_enabled(), label)
        });
        if response.changed() {
            actions.push(action(value));
        }
    });
}

fn numeric_field_u64(
    ui: &mut egui::Ui,
    label: &str,
    current: u64,
    range: std::ops::RangeInclusive<u64>,
    action: impl FnOnce(u64) -> SettingsAction,
    actions: &mut Vec<SettingsAction>,
) {
    ui.horizontal(|ui| {
        ui.add_sized([FIELD_LABEL_WIDTH, 24.0], egui::Label::new(label));
        let mut value = current;
        let response = ui.add(egui::DragValue::new(&mut value).range(range).speed(100));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::DragValue, ui.is_enabled(), label)
        });
        if response.changed() {
            actions.push(action(value));
        }
    });
}

fn render_action_row<const N: usize>(
    ui: &mut egui::Ui,
    commands: [(egui::ImageSource<'static>, &str, bool, SettingsAction); N],
    actions: &mut Vec<SettingsAction>,
) {
    ui.horizontal(|ui| {
        for (icon, label, enabled, action) in commands {
            let image = egui::Image::new(icon).fit_to_exact_size(egui::vec2(16.0, 16.0));
            let response = ui
                .add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [COMMAND_WIDTH, 34.0],
                        egui::Button::image_and_text(image, label),
                    )
                })
                .inner;
            if response.clicked() {
                actions.push(action);
            }
        }
    });
}

fn tool_button(
    ui: &mut egui::Ui,
    icon: egui::ImageSource<'static>,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::image(egui::Image::new(icon).fit_to_exact_size(egui::vec2(17.0, 17.0)))
            .min_size(egui::vec2(34.0, 34.0)),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response.on_hover_text(label)
}

fn active_group_dirty(state: &SettingsViewState) -> bool {
    match state.tab {
        SettingsTab::Stepwise => state.stepwise.is_dirty(),
        SettingsTab::ImageOverlay => state.image_overlay.is_dirty(),
        SettingsTab::LaunchArguments => state.extra_args.is_dirty(),
    }
}

fn stepwise_busy(state: &SettingsViewState) -> bool {
    state.stepwise.operation.phase == SettingsOperationPhase::Running
        || state.stepwise.test.phase == SettingsOperationPhase::Running
}

fn fit_mode_text(locale: Locale, mode: ImageOverlayFitMode) -> &'static str {
    stext(
        locale,
        match mode {
            ImageOverlayFitMode::Fill => SText::Fill,
            ImageOverlayFitMode::Fit => SText::Fit,
            ImageOverlayFitMode::Stretch => SText::Stretch,
            ImageOverlayFitMode::Tile => SText::Tile,
            ImageOverlayFitMode::Center => SText::Center,
        },
    )
}

fn reset_title(locale: Locale, group: SafeSettingsGroup) -> &'static str {
    stext(
        locale,
        match group {
            SafeSettingsGroup::Stepwise => SText::ResetStepwiseTitle,
            SafeSettingsGroup::ImageOverlay => SText::ResetImageTitle,
            SafeSettingsGroup::ExtraArgs => SText::ResetArgumentsTitle,
        },
    )
}

fn reset_button(locale: Locale, group: SafeSettingsGroup) -> &'static str {
    stext(
        locale,
        match group {
            SafeSettingsGroup::Stepwise => SText::ResetStepwise,
            SafeSettingsGroup::ImageOverlay => SText::ResetImageOverlay,
            SafeSettingsGroup::ExtraArgs => SText::ResetLaunchArguments,
        },
    )
}

fn group_text(locale: Locale, group: SafeSettingsGroup) -> &'static str {
    stext(
        locale,
        match group {
            SafeSettingsGroup::Stepwise => SText::Stepwise,
            SafeSettingsGroup::ImageOverlay => SText::ImageOverlay,
            SafeSettingsGroup::ExtraArgs => SText::LaunchArguments,
        },
    )
}

pub fn failure_text(locale: Locale, kind: SettingsFailureKind) -> &'static str {
    stext(
        locale,
        match kind {
            SettingsFailureKind::SettingsReadFailed => SText::ReadFailed,
            SettingsFailureKind::SettingsWriteFailed => SText::WriteFailed,
            SettingsFailureKind::SettingsConflict => SText::SettingsChanged,
            SettingsFailureKind::InvalidRevision => SText::InvalidRevision,
            SettingsFailureKind::InvalidUrl => SText::InvalidUrl,
            SettingsFailureKind::InvalidEnvironmentVariable => SText::InvalidEnvironment,
            SettingsFailureKind::InvalidModel => SText::InvalidModel,
            SettingsFailureKind::InvalidNumericField => SText::InvalidNumeric,
            SettingsFailureKind::InvalidPath => SText::InvalidPath,
            SettingsFailureKind::InvalidFitMode => SText::InvalidFitMode,
            SettingsFailureKind::InvalidArgument => SText::InvalidArgument,
            SettingsFailureKind::InvalidSecret => SText::InvalidSecret,
            SettingsFailureKind::ConfirmationMismatch => SText::ConfirmationMismatch,
            SettingsFailureKind::StepwiseUnauthorized => SText::Unauthorized,
            SettingsFailureKind::StepwiseTimeout => SText::TimedOut,
            SettingsFailureKind::StepwiseRejected => SText::Rejected,
            SettingsFailureKind::StepwiseNetwork => SText::NetworkFailed,
            SettingsFailureKind::WorkerStopped => SText::WorkerStopped,
        },
    )
}

#[derive(Clone, Copy)]
enum SText {
    Stepwise,
    ImageOverlay,
    LaunchArguments,
    EnableStepwise,
    DirectSend,
    BaseUrl,
    Model,
    ApiKeyEnvironment,
    StoredCredential,
    Configured,
    NotConfigured,
    EnvironmentAvailable,
    ReplacementApiKey,
    Unchanged,
    ShowReplacementKey,
    HideReplacementKey,
    ClearStoredCredential,
    MaximumItems,
    MaximumInputCharacters,
    MaximumOutputTokens,
    TimeoutMs,
    TestConnection,
    SaveStepwise,
    ResetStepwise,
    ConnectionSucceeded,
    EnableImageOverlay,
    ImagePath,
    SelectImage,
    Opacity,
    FitMode,
    Fill,
    Fit,
    Stretch,
    Tile,
    Center,
    SaveImageOverlay,
    ResetImageOverlay,
    CodexLaunchArguments,
    ArgumentCount,
    SaveLaunchArguments,
    ResetLaunchArguments,
    RefreshSettings,
    SettingsChanged,
    PickerFailed,
    UnsavedChanges,
    Saved,
    ClearCredentialTitle,
    ClearCredentialMessage,
    ClearCredential,
    ResetStepwiseTitle,
    ResetImageTitle,
    ResetArgumentsTitle,
    ResetMessage,
    DiscardTitle,
    DiscardMessage,
    DiscardChanges,
    KeepEditing,
    Cancel,
    ReadFailed,
    WriteFailed,
    InvalidRevision,
    InvalidUrl,
    InvalidEnvironment,
    InvalidModel,
    InvalidNumeric,
    InvalidPath,
    InvalidFitMode,
    InvalidArgument,
    InvalidSecret,
    ConfirmationMismatch,
    Unauthorized,
    TimedOut,
    Rejected,
    NetworkFailed,
    WorkerStopped,
}

fn stext(locale: Locale, key: SText) -> &'static str {
    match (locale, key) {
        (_, SText::Stepwise) => "Stepwise",
        (Locale::ZhCn, SText::ImageOverlay) => "图片覆盖",
        (Locale::En, SText::ImageOverlay) => "Image overlay",
        (Locale::ZhCn, SText::LaunchArguments) => "启动参数",
        (Locale::En, SText::LaunchArguments) => "Launch arguments",
        (Locale::ZhCn, SText::EnableStepwise) => "启用 Stepwise",
        (Locale::En, SText::EnableStepwise) => "Enable Stepwise",
        (Locale::ZhCn, SText::DirectSend) => "直接发送",
        (Locale::En, SText::DirectSend) => "Direct send",
        (Locale::ZhCn, SText::BaseUrl) => "基础 URL",
        (Locale::En, SText::BaseUrl) => "Base URL",
        (Locale::ZhCn, SText::Model) => "模型",
        (Locale::En, SText::Model) => "Model",
        (Locale::ZhCn, SText::ApiKeyEnvironment) => "API 密钥环境变量",
        (Locale::En, SText::ApiKeyEnvironment) => "API key environment",
        (Locale::ZhCn, SText::StoredCredential) => "已保存凭据",
        (Locale::En, SText::StoredCredential) => "Stored credential",
        (Locale::ZhCn, SText::Configured) => "已配置",
        (Locale::En, SText::Configured) => "Configured",
        (Locale::ZhCn, SText::NotConfigured) => "未配置",
        (Locale::En, SText::NotConfigured) => "Not configured",
        (Locale::ZhCn, SText::EnvironmentAvailable) => "环境变量可用",
        (Locale::En, SText::EnvironmentAvailable) => "Environment available",
        (Locale::ZhCn, SText::ReplacementApiKey) => "替换 API 密钥",
        (Locale::En, SText::ReplacementApiKey) => "Replacement API key",
        (Locale::ZhCn, SText::Unchanged) => "留空则保持不变",
        (Locale::En, SText::Unchanged) => "Leave empty to keep unchanged",
        (Locale::ZhCn, SText::ShowReplacementKey) => "显示替换密钥",
        (Locale::En, SText::ShowReplacementKey) => "Show replacement key",
        (Locale::ZhCn, SText::HideReplacementKey) => "隐藏替换密钥",
        (Locale::En, SText::HideReplacementKey) => "Hide replacement key",
        (Locale::ZhCn, SText::ClearStoredCredential) => "清除已保存凭据",
        (Locale::En, SText::ClearStoredCredential) => "Clear stored credential",
        (Locale::ZhCn, SText::MaximumItems) => "最大条目数",
        (Locale::En, SText::MaximumItems) => "Maximum items",
        (Locale::ZhCn, SText::MaximumInputCharacters) => "最大输入字符数",
        (Locale::En, SText::MaximumInputCharacters) => "Maximum input characters",
        (Locale::ZhCn, SText::MaximumOutputTokens) => "最大输出令牌数",
        (Locale::En, SText::MaximumOutputTokens) => "Maximum output tokens",
        (Locale::ZhCn, SText::TimeoutMs) => "超时（毫秒）",
        (Locale::En, SText::TimeoutMs) => "Timeout (ms)",
        (Locale::ZhCn, SText::TestConnection) => "测试连接",
        (Locale::En, SText::TestConnection) => "Test connection",
        (Locale::ZhCn, SText::SaveStepwise) => "保存 Stepwise",
        (Locale::En, SText::SaveStepwise) => "Save Stepwise",
        (Locale::ZhCn, SText::ResetStepwise) => "重置 Stepwise",
        (Locale::En, SText::ResetStepwise) => "Reset Stepwise",
        (Locale::ZhCn, SText::ConnectionSucceeded) => "连接测试通过，条目数",
        (Locale::En, SText::ConnectionSucceeded) => "Connection succeeded, items",
        (Locale::ZhCn, SText::EnableImageOverlay) => "启用图片覆盖",
        (Locale::En, SText::EnableImageOverlay) => "Enable image overlay",
        (Locale::ZhCn, SText::ImagePath) => "图片路径",
        (Locale::En, SText::ImagePath) => "Image path",
        (Locale::ZhCn, SText::SelectImage) => "选择图片",
        (Locale::En, SText::SelectImage) => "Select image",
        (Locale::ZhCn, SText::Opacity) => "不透明度",
        (Locale::En, SText::Opacity) => "Opacity",
        (Locale::ZhCn, SText::FitMode) => "适应模式",
        (Locale::En, SText::FitMode) => "Fit mode",
        (Locale::ZhCn, SText::Fill) => "填充",
        (Locale::En, SText::Fill) => "Fill",
        (Locale::ZhCn, SText::Fit) => "适应",
        (Locale::En, SText::Fit) => "Fit",
        (Locale::ZhCn, SText::Stretch) => "拉伸",
        (Locale::En, SText::Stretch) => "Stretch",
        (Locale::ZhCn, SText::Tile) => "平铺",
        (Locale::En, SText::Tile) => "Tile",
        (Locale::ZhCn, SText::Center) => "居中",
        (Locale::En, SText::Center) => "Center",
        (Locale::ZhCn, SText::SaveImageOverlay) => "保存图片覆盖",
        (Locale::En, SText::SaveImageOverlay) => "Save image overlay",
        (Locale::ZhCn, SText::ResetImageOverlay) => "重置图片覆盖",
        (Locale::En, SText::ResetImageOverlay) => "Reset image overlay",
        (Locale::ZhCn, SText::CodexLaunchArguments) => "Codex 启动参数",
        (Locale::En, SText::CodexLaunchArguments) => "Codex launch arguments",
        (Locale::ZhCn, SText::ArgumentCount) => "参数数量",
        (Locale::En, SText::ArgumentCount) => "Argument count",
        (Locale::ZhCn, SText::SaveLaunchArguments) => "保存启动参数",
        (Locale::En, SText::SaveLaunchArguments) => "Save launch arguments",
        (Locale::ZhCn, SText::ResetLaunchArguments) => "重置启动参数",
        (Locale::En, SText::ResetLaunchArguments) => "Reset launch arguments",
        (Locale::ZhCn, SText::RefreshSettings) => "刷新设置",
        (Locale::En, SText::RefreshSettings) => "Refresh settings",
        (Locale::ZhCn, SText::SettingsChanged) => "设置已在其他位置更改",
        (Locale::En, SText::SettingsChanged) => "Settings changed elsewhere",
        (Locale::ZhCn, SText::PickerFailed) => "图片选择器失败",
        (Locale::En, SText::PickerFailed) => "Image picker failed",
        (Locale::ZhCn, SText::UnsavedChanges) => "当前分组有未保存更改",
        (Locale::En, SText::UnsavedChanges) => "This group has unsaved changes",
        (Locale::ZhCn, SText::Saved) => "设置已保存",
        (Locale::En, SText::Saved) => "Settings saved",
        (Locale::ZhCn, SText::ClearCredentialTitle) => "清除已保存的 Stepwise 凭据？",
        (Locale::En, SText::ClearCredentialTitle) => "Clear stored Stepwise credential?",
        (Locale::ZhCn, SText::ClearCredentialMessage) => {
            "此操作只清除已保存的密钥，不会使用空替换值。"
        }
        (Locale::En, SText::ClearCredentialMessage) => {
            "This clears the stored key and is distinct from an empty replacement."
        }
        (Locale::ZhCn, SText::ClearCredential) => "清除凭据",
        (Locale::En, SText::ClearCredential) => "Clear credential",
        (Locale::ZhCn, SText::ResetStepwiseTitle) => "重置 Stepwise 设置？",
        (Locale::En, SText::ResetStepwiseTitle) => "Reset Stepwise settings?",
        (Locale::ZhCn, SText::ResetImageTitle) => "重置图片覆盖设置？",
        (Locale::En, SText::ResetImageTitle) => "Reset image overlay settings?",
        (Locale::ZhCn, SText::ResetArgumentsTitle) => "重置启动参数？",
        (Locale::En, SText::ResetArgumentsTitle) => "Reset launch arguments?",
        (Locale::ZhCn, SText::ResetMessage) => "将仅把此分组恢复为默认值。",
        (Locale::En, SText::ResetMessage) => "Only this settings group will return to defaults.",
        (Locale::ZhCn, SText::DiscardTitle) => "丢弃设置更改？",
        (Locale::En, SText::DiscardTitle) => "Discard settings changes?",
        (Locale::ZhCn, SText::DiscardMessage) => "以下分组的未保存更改将丢失：",
        (Locale::En, SText::DiscardMessage) => "Unsaved changes in these groups will be lost:",
        (Locale::ZhCn, SText::DiscardChanges) => "丢弃更改",
        (Locale::En, SText::DiscardChanges) => "Discard changes",
        (Locale::ZhCn, SText::KeepEditing) => "继续编辑",
        (Locale::En, SText::KeepEditing) => "Keep editing",
        (Locale::ZhCn, SText::Cancel) => "取消",
        (Locale::En, SText::Cancel) => "Cancel",
        (Locale::ZhCn, SText::ReadFailed) => "设置读取失败",
        (Locale::En, SText::ReadFailed) => "Settings read failed",
        (Locale::ZhCn, SText::WriteFailed) => "设置保存失败",
        (Locale::En, SText::WriteFailed) => "Settings save failed",
        (Locale::ZhCn, SText::InvalidRevision) => "设置修订无效",
        (Locale::En, SText::InvalidRevision) => "Settings revision is invalid",
        (Locale::ZhCn, SText::InvalidUrl) => "Stepwise URL 无效",
        (Locale::En, SText::InvalidUrl) => "Stepwise URL is invalid",
        (Locale::ZhCn, SText::InvalidEnvironment) => "环境变量名称无效",
        (Locale::En, SText::InvalidEnvironment) => "Environment variable name is invalid",
        (Locale::ZhCn, SText::InvalidModel) => "Stepwise 模型无效",
        (Locale::En, SText::InvalidModel) => "Stepwise model is invalid",
        (Locale::ZhCn, SText::InvalidNumeric) => "数值设置无效",
        (Locale::En, SText::InvalidNumeric) => "A numeric setting is invalid",
        (Locale::ZhCn, SText::InvalidPath) => "图片路径无效",
        (Locale::En, SText::InvalidPath) => "Image path is invalid",
        (Locale::ZhCn, SText::InvalidFitMode) => "图片适应模式无效",
        (Locale::En, SText::InvalidFitMode) => "Image fit mode is invalid",
        (Locale::ZhCn, SText::InvalidArgument) => "启动参数无效",
        (Locale::En, SText::InvalidArgument) => "A launch argument is invalid",
        (Locale::ZhCn, SText::InvalidSecret) => "替换密钥无效",
        (Locale::En, SText::InvalidSecret) => "Replacement key is invalid",
        (Locale::ZhCn, SText::ConfirmationMismatch) => "确认内容不匹配",
        (Locale::En, SText::ConfirmationMismatch) => "Confirmation does not match",
        (Locale::ZhCn, SText::Unauthorized) => "Stepwise 凭据未获授权",
        (Locale::En, SText::Unauthorized) => "Stepwise credential was not authorized",
        (Locale::ZhCn, SText::TimedOut) => "Stepwise 请求超时",
        (Locale::En, SText::TimedOut) => "Stepwise request timed out",
        (Locale::ZhCn, SText::Rejected) => "Stepwise 请求被拒绝",
        (Locale::En, SText::Rejected) => "Stepwise request was rejected",
        (Locale::ZhCn, SText::NetworkFailed) => "Stepwise 网络请求失败",
        (Locale::En, SText::NetworkFailed) => "Stepwise network request failed",
        (Locale::ZhCn, SText::WorkerStopped) => "设置后台服务已停止",
        (Locale::En, SText::WorkerStopped) => "The settings worker has stopped",
    }
}
