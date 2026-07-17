use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use codex_plus_manager_service::{
    DiagnoseProviderProfile, FetchProviderModels, OverviewSource, ProviderErrorKind,
    ProviderNetworkFailureKind, ProviderProfile, ProviderSource, TestProviderProfile,
};
use eframe::egui;

use crate::fonts;
use crate::perf::{PerfRecorder, PerfScriptAction};
use crate::persistence::{self, PersistedUiState};
use crate::runtime::provider::{ProviderDispatcher, StoreResponse};
use crate::runtime::{DispatchError, OverviewDispatcher};
use crate::state::provider::{
    DeleteProfileError, GuardOutcome, GuardResolution, OperationPhase, ProviderLoadFailureKind,
    ProviderLoadPhase, ProviderSaveFailureKind, TransitionResult,
};
use crate::state::{AppState, OverviewFailureKind, OverviewPhase, Route};
use crate::theme;
use crate::views::provider::{ProviderAction, ProviderEdit};
use crate::views::shell::{ShellAction, ShellViewModel, render_shell};

pub struct NativeManagerApp {
    state: AppState,
    persisted: PersistedUiState,
    overview_dispatcher: OverviewDispatcher,
    provider_dispatcher: ProviderDispatcher,
    last_updated: Option<String>,
    overview_worker_stopped: bool,
    provider_store_worker_stopped: bool,
    pending_route: Option<Route>,
    pending_provider_reload: bool,
    perf: Option<PerfRecorder>,
}

impl NativeManagerApp {
    pub fn new(
        creation: &eframe::CreationContext<'_>,
        cjk_font: Option<Vec<u8>>,
        source: Arc<dyn OverviewSource>,
        provider_source: Arc<dyn ProviderSource>,
        perf: Option<PerfRecorder>,
    ) -> Self {
        egui_extras::install_image_loaders(&creation.egui_ctx);
        if let Some(bytes) = cjk_font {
            fonts::install_cjk_font(&creation.egui_ctx, bytes);
        }

        let persisted = persistence::load(creation.storage);
        theme::apply(&creation.egui_ctx, persisted.theme);
        let repaint_context = creation.egui_ctx.clone();
        let overview_dispatcher =
            OverviewDispatcher::spawn(source, Arc::new(move || repaint_context.request_repaint()));
        let provider_repaint_context = creation.egui_ctx.clone();
        let provider_dispatcher = ProviderDispatcher::spawn(
            provider_source,
            Arc::new(move || provider_repaint_context.request_repaint()),
        );
        let mut app = Self {
            state: AppState::default(),
            persisted,
            overview_dispatcher,
            provider_dispatcher,
            last_updated: None,
            overview_worker_stopped: false,
            provider_store_worker_stopped: false,
            pending_route: None,
            pending_provider_reload: false,
            perf,
        };
        app.refresh_overview();
        app.load_providers();
        app
    }

    fn refresh_overview(&mut self) {
        let request_id = self.state.overview.begin_refresh();
        if self.overview_dispatcher.request(request_id).is_err() {
            self.overview_worker_stopped = true;
            self.state
                .overview
                .apply_response(request_id, Err(OverviewFailureKind::WorkerStopped));
        }
    }

    fn load_providers(&mut self) {
        let request_id = self.state.provider.begin_load();
        if self.provider_dispatcher.request_load(request_id).is_err() {
            self.provider_store_worker_stopped = true;
            self.state
                .provider
                .apply_load_response(request_id, Err(ProviderLoadFailureKind::WorkerStopped));
        }
    }

    fn apply_action(&mut self, ctx: &egui::Context, action: ShellAction) {
        match action {
            ShellAction::Navigate(route) => self.navigate(route),
            ShellAction::Refresh => match self.state.route {
                Route::Providers => self.request_provider_reload(),
                Route::Overview | Route::About => self.refresh_overview(),
            },
            ShellAction::Retry => self.refresh_overview(),
            ShellAction::SetLocale(locale) => self.persisted.locale = locale,
            ShellAction::SetTheme(mode) => {
                self.persisted.theme = mode;
                theme::apply(ctx, mode);
            }
            ShellAction::Provider(action) => self.apply_provider_action(action),
        }
        ctx.request_repaint();
    }

    fn navigate(&mut self, route: Route) {
        if self.state.route == Route::Providers
            && route != Route::Providers
            && self.state.provider.is_dirty()
        {
            self.pending_route = Some(route);
            self.pending_provider_reload = false;
            let _ = self.state.provider.request_reload();
            return;
        }
        self.pending_route = None;
        self.state.route = route;
        if route == Route::Providers && self.state.provider.load_phase == ProviderLoadPhase::Idle {
            self.load_providers();
        }
    }

    fn request_provider_reload(&mut self) {
        match self.state.provider.request_reload() {
            TransitionResult::Applied => self.load_providers(),
            TransitionResult::GuardRequired => {
                self.pending_route = None;
                self.pending_provider_reload = true;
            }
            TransitionResult::NotFound => {}
        }
    }

    fn apply_provider_action(&mut self, action: ProviderAction) {
        match action {
            ProviderAction::RetryLoad => self.load_providers(),
            ProviderAction::ToggleList => {
                self.state.provider.list_collapsed = !self.state.provider.list_collapsed;
            }
            ProviderAction::Select(profile_id) => {
                self.pending_provider_reload = false;
                self.pending_route = None;
                let _ = self.state.provider.request_selection(&profile_id);
            }
            ProviderAction::SetTab(tab) => self.state.provider.editor_tab = tab,
            ProviderAction::AddOrdinary => {
                self.state.provider.add_ordinary();
            }
            ProviderAction::AddAggregate => {
                self.state.provider.add_aggregate();
            }
            ProviderAction::Duplicate => {
                self.state.provider.duplicate_selected();
            }
            ProviderAction::Move(direction) => {
                self.state.provider.move_selected(direction);
            }
            ProviderAction::Delete { confirmed } => {
                match self.state.provider.delete_selected(confirmed) {
                    Ok(()) => self.state.provider.set_delete_confirmation_required(false),
                    Err(DeleteProfileError::ConfirmationRequired) => {
                        self.state.provider.set_delete_confirmation_required(true)
                    }
                    Err(_) => self.state.provider.set_delete_confirmation_required(false),
                }
            }
            ProviderAction::CancelDelete => {
                self.state.provider.set_delete_confirmation_required(false)
            }
            ProviderAction::Edit(edit) => match edit {
                ProviderEdit::ModelRow {
                    index,
                    model,
                    window,
                } => {
                    self.state.provider.update_model_row(index, &model, &window);
                }
                edit => {
                    self.state
                        .provider
                        .edit_selected(|profile| apply_provider_edit(profile, edit));
                }
            },
            ProviderAction::ApplyPreset(preset_id) => {
                self.state.provider.apply_preset(&preset_id);
            }
            ProviderAction::AddModel => {
                self.state.provider.add_model_row();
            }
            ProviderAction::RemoveModel(index) => {
                self.state.provider.remove_model_row(index);
            }
            ProviderAction::MergeModels => {
                self.state.provider.merge_discovered_models();
            }
            ProviderAction::SetAggregateMember {
                profile_id,
                enabled,
            } => {
                self.state
                    .provider
                    .set_aggregate_member(&profile_id, enabled);
            }
            ProviderAction::SetAggregateWeight { profile_id, weight } => {
                self.state
                    .provider
                    .set_aggregate_weight(&profile_id, weight);
            }
            ProviderAction::SetSecretRevealed(revealed) => {
                self.state.provider.set_secret_revealed(revealed);
            }
            ProviderAction::SetConfigRevealed(revealed) => {
                self.state.provider.set_config_revealed(revealed);
            }
            ProviderAction::SetAuthRevealed(revealed) => {
                self.state.provider.set_auth_revealed(revealed);
            }
            ProviderAction::Save => self.save_providers(),
            ProviderAction::Discard => {
                self.pending_route = None;
                self.pending_provider_reload = false;
                self.state.provider.discard_draft();
            }
            ProviderAction::Test => self.test_provider(),
            ProviderAction::FetchModels => self.fetch_provider_models(),
            ProviderAction::Doctor => self.diagnose_provider(),
            ProviderAction::ResolveGuard(resolution) => self.resolve_provider_guard(resolution),
        }
    }

    fn save_providers(&mut self) {
        let Some((request_id, request)) = self.state.provider.begin_save() else {
            return;
        };
        if self
            .provider_dispatcher
            .request_save(request_id, request)
            .is_err()
        {
            self.provider_store_worker_stopped = true;
            self.state
                .provider
                .apply_save_response(request_id, Err(ProviderSaveFailureKind::WorkerStopped));
        }
    }

    fn resolve_provider_guard(&mut self, resolution: GuardResolution) {
        match self.state.provider.resolve_guard(resolution) {
            GuardOutcome::NeedsSave => self.save_providers(),
            GuardOutcome::Applied => self.complete_pending_provider_transition(),
            GuardOutcome::Stayed => {
                self.pending_route = None;
                self.pending_provider_reload = false;
            }
            GuardOutcome::NoPendingGuard => {}
        }
    }

    fn complete_pending_provider_transition(&mut self) {
        if let Some(route) = self.pending_route.take() {
            self.state.route = route;
        }
        if std::mem::take(&mut self.pending_provider_reload) {
            self.load_providers();
        }
    }

    fn selected_provider_request(&self) -> Option<(ProviderProfile, String)> {
        Some((
            self.state.provider.selected_profile()?.clone(),
            self.state.provider.draft()?.default_test_model.clone(),
        ))
    }

    fn test_provider(&mut self) {
        let Some((profile, default_test_model)) = self.selected_provider_request() else {
            return;
        };
        let Some(token) = self.state.provider.begin_test() else {
            return;
        };
        if self
            .provider_dispatcher
            .request_test(
                token.clone(),
                TestProviderProfile {
                    profile,
                    default_test_model,
                },
            )
            .is_err()
        {
            self.state
                .provider
                .apply_test_response(token, Err(ProviderNetworkFailureKind::Network));
        }
    }

    fn fetch_provider_models(&mut self) {
        let Some((profile, _)) = self.selected_provider_request() else {
            return;
        };
        let Some(token) = self.state.provider.begin_models() else {
            return;
        };
        if self
            .provider_dispatcher
            .request_models(token.clone(), FetchProviderModels { profile })
            .is_err()
        {
            self.state
                .provider
                .apply_models_failure(token, ProviderNetworkFailureKind::Network);
        }
    }

    fn diagnose_provider(&mut self) {
        let Some((profile, default_test_model)) = self.selected_provider_request() else {
            return;
        };
        let Some(token) = self.state.provider.begin_doctor() else {
            return;
        };
        if self
            .provider_dispatcher
            .request_doctor(
                token.clone(),
                DiagnoseProviderProfile {
                    profile,
                    default_test_model,
                },
            )
            .is_err()
        {
            self.state
                .provider
                .apply_doctor_failure(token, ProviderNetworkFailureKind::Network);
        }
    }

    fn reduce_overview_responses(&mut self) {
        loop {
            match self.overview_dispatcher.try_recv() {
                Ok(Some(response)) => {
                    let request_id = response.request_id;
                    let result = response.result.map_err(|_| OverviewFailureKind::LoadFailed);
                    if self.state.overview.apply_response(request_id, result)
                        && self.state.overview.phase == OverviewPhase::Ready
                    {
                        self.last_updated = Some(current_utc_time());
                        if let Some(perf) = &mut self.perf {
                            perf.record_overview_ready();
                        }
                    }
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if !self.overview_worker_stopped {
                        self.overview_worker_stopped = true;
                        self.state.overview.apply_response(
                            self.state.overview.current_request_id,
                            Err(OverviewFailureKind::WorkerStopped),
                        );
                    }
                    break;
                }
            }
        }
    }

    fn reduce_provider_responses(&mut self) {
        loop {
            match self.provider_dispatcher.try_recv_store() {
                Ok(Some(StoreResponse::Load { request_id, result })) => {
                    let accepted = self.state.provider.apply_load_response(
                        request_id,
                        result.map_err(|_| ProviderLoadFailureKind::LoadFailed),
                    );
                    if accepted && self.state.provider.load_phase == ProviderLoadPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(StoreResponse::Save { request_id, result })) => {
                    let succeeded = result.is_ok();
                    let accepted = self.state.provider.apply_save_response(
                        request_id,
                        result.map_err(|error| map_provider_save_failure(error.kind())),
                    );
                    if accepted && succeeded {
                        self.last_updated = Some(current_utc_time());
                        self.complete_pending_provider_transition();
                    }
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if !self.provider_store_worker_stopped {
                        self.provider_store_worker_stopped = true;
                        if matches!(
                            self.state.provider.load_phase,
                            ProviderLoadPhase::Loading | ProviderLoadPhase::Refreshing
                        ) {
                            self.state.provider.apply_load_response(
                                self.state.provider.current_load_request_id,
                                Err(ProviderLoadFailureKind::WorkerStopped),
                            );
                        }
                        if self.state.provider.save.phase == OperationPhase::Running {
                            self.state.provider.apply_save_response(
                                self.state.provider.save.current_request_id,
                                Err(ProviderSaveFailureKind::WorkerStopped),
                            );
                        }
                    }
                    break;
                }
            }
        }

        loop {
            match self.provider_dispatcher.try_recv_test() {
                Ok(Some(response)) => {
                    self.state.provider.apply_test_response(
                        response.token,
                        response.result.map_err(|error| error.kind()),
                    );
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if self.state.provider.test.phase == OperationPhase::Running {
                        self.state.provider.test.phase = OperationPhase::Error;
                        self.state.provider.test.error = Some(ProviderNetworkFailureKind::Network);
                    }
                    break;
                }
            }
        }

        loop {
            match self.provider_dispatcher.try_recv_models() {
                Ok(Some(response)) => {
                    self.state.provider.apply_models_response(
                        response.token,
                        response.result.map_err(|error| error.kind()),
                    );
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if self.state.provider.models.phase == OperationPhase::Running {
                        self.state.provider.models.phase = OperationPhase::Error;
                        self.state.provider.models.error =
                            Some(ProviderNetworkFailureKind::Network);
                    }
                    break;
                }
            }
        }

        loop {
            match self.provider_dispatcher.try_recv_doctor() {
                Ok(Some(response)) => {
                    self.state.provider.apply_doctor_response(
                        response.token,
                        response.result.map_err(|error| error.kind()),
                    );
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if self.state.provider.doctor.phase == OperationPhase::Running {
                        self.state.provider.doctor.phase = OperationPhase::Error;
                        self.state.provider.doctor.error =
                            Some(ProviderNetworkFailureKind::Network);
                    }
                    break;
                }
            }
        }
    }

    fn view_model(&self) -> ShellViewModel {
        ShellViewModel {
            route: self.state.route,
            locale: self.persisted.locale,
            theme: self.persisted.theme,
            overview_phase: self.state.overview.phase,
            overview_snapshot: self.state.overview.snapshot.clone(),
            overview_error: self.state.overview.error,
            last_updated: self.last_updated.clone(),
            renderer: "WGPU".to_owned(),
        }
    }

    fn apply_perf_action(&mut self, ctx: &egui::Context, action: PerfScriptAction) {
        let shell_action = match action {
            PerfScriptAction::NavigateProviders => Some(ShellAction::Navigate(Route::Providers)),
            PerfScriptAction::NavigateOverview => Some(ShellAction::Navigate(Route::Overview)),
            PerfScriptAction::SelectNextProvider => {
                let next_id = self.state.provider.draft().and_then(|document| {
                    let selected = self.state.provider.selected_profile_id.as_deref();
                    let current = document
                        .profiles
                        .iter()
                        .position(|profile| Some(profile.id()) == selected)
                        .unwrap_or_default();
                    document
                        .profiles
                        .get((current + 1) % document.profiles.len().max(1))
                        .map(|profile| profile.id().to_owned())
                });
                next_id.map(|profile_id| ShellAction::Provider(ProviderAction::Select(profile_id)))
            }
            PerfScriptAction::EditProviderName => Some(ShellAction::Provider(
                ProviderAction::Edit(ProviderEdit::Name("Performance provider edit".to_owned())),
            )),
            PerfScriptAction::DiscardProvider => {
                Some(ShellAction::Provider(ProviderAction::Discard))
            }
            PerfScriptAction::ToggleProviderList => {
                Some(ShellAction::Provider(ProviderAction::ToggleList))
            }
        };
        if let Some(action) = shell_action {
            self.apply_action(ctx, action);
        }
    }
}

impl eframe::App for NativeManagerApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.reduce_overview_responses();
        self.reduce_provider_responses();
        if let Some(perf) = &mut self.perf {
            perf.drive(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let scripted_action = self.perf.as_ref().and_then(|perf| perf.scripted_action(ui));
        if let Some(action) = scripted_action {
            self.apply_perf_action(ui.ctx(), action);
        }
        let model = self.view_model();
        for action in render_shell(ui, &model, Some(&self.state.provider)) {
            self.apply_action(ui.ctx(), action);
        }
        if let Some(perf) = &mut self.perf {
            perf.record_ui_frame(frame.info().cpu_usage);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        persistence::save(storage, &self.persisted);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if let Some(perf) = &mut self.perf {
            perf.raw_input_hook(ctx, input);
        }
    }

    fn on_exit(&mut self) {
        if let Some(perf) = &mut self.perf {
            perf.finish();
        }
    }
}

fn apply_provider_edit(profile: &mut ProviderProfile, edit: ProviderEdit) {
    match edit {
        ProviderEdit::Name(name) => match profile {
            ProviderProfile::Ordinary(profile) => profile.name = name,
            ProviderProfile::Aggregate { shell, routing } => {
                shell.name = name.clone();
                routing.name = name;
            }
        },
        ProviderEdit::AggregateStrategy(strategy) => {
            if let ProviderProfile::Aggregate { routing, .. } = profile {
                routing.strategy = strategy;
            }
        }
        ProviderEdit::ModelRow { .. } => {}
        edit => {
            let Some(profile) = profile.ordinary_mut() else {
                return;
            };
            match edit {
                ProviderEdit::Mode(mode) => profile.relay_mode = mode,
                ProviderEdit::Protocol(protocol) => profile.protocol = protocol,
                ProviderEdit::BaseUrl(base_url) => {
                    profile.base_url = base_url.clone();
                    profile.upstream_base_url = base_url;
                }
                ProviderEdit::ApiKey(api_key) => profile.api_key = api_key,
                ProviderEdit::Model(model) => profile.model = model,
                ProviderEdit::TestModel(model) => profile.test_model = model,
                ProviderEdit::UseCommonConfig(enabled) => profile.use_common_config = enabled,
                ProviderEdit::ContextWindow(window) => profile.context_window = digits_only(window),
                ProviderEdit::AutoCompactLimit(limit) => {
                    profile.auto_compact_limit = digits_only(limit);
                }
                ProviderEdit::InsertMode(mode) => profile.model_insert_mode = mode,
                ProviderEdit::UserAgent(user_agent) => profile.user_agent = user_agent,
                ProviderEdit::ConfigContents(contents) => profile.config_contents = contents,
                ProviderEdit::AuthContents(contents) => profile.auth_contents = contents,
                ProviderEdit::Name(_)
                | ProviderEdit::ModelRow { .. }
                | ProviderEdit::AggregateStrategy(_) => unreachable!("handled above"),
            }
        }
    }
}

fn digits_only(value: String) -> String {
    value.chars().filter(char::is_ascii_digit).collect()
}

fn map_provider_save_failure(kind: ProviderErrorKind) -> ProviderSaveFailureKind {
    match kind {
        ProviderErrorKind::Conflict => ProviderSaveFailureKind::Conflict,
        ProviderErrorKind::Validation => ProviderSaveFailureKind::Validation,
        ProviderErrorKind::LoadFailed | ProviderErrorKind::SaveFailed => {
            ProviderSaveFailureKind::SaveFailed
        }
    }
}

fn current_utc_time() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_utc_time(seconds)
}

fn format_utc_time(seconds_since_epoch: u64) -> String {
    let seconds = seconds_since_epoch % 86_400;
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02} UTC")
}

#[cfg(test)]
mod tests {
    use codex_plus_core::settings::{AggregateRelayProfile, RelayMode, RelayProfile};
    use codex_plus_manager_service::{ProviderErrorKind, ProviderProfile};

    use super::{
        ProviderEdit, ProviderSaveFailureKind, apply_provider_edit, format_utc_time,
        map_provider_save_failure,
    };

    #[test]
    fn refresh_time_is_stable_and_explicitly_utc() {
        assert_eq!(format_utc_time(3_661), "01:01:01 UTC");
        assert_eq!(format_utc_time(86_400), "00:00:00 UTC");
    }

    #[test]
    fn provider_edits_keep_aggregate_names_in_sync_and_normalize_numeric_fields() {
        let mut aggregate = ProviderProfile::Aggregate {
            shell: RelayProfile {
                id: "aggregate".to_string(),
                name: "Old".to_string(),
                relay_mode: RelayMode::Aggregate,
                ..RelayProfile::default()
            },
            routing: AggregateRelayProfile {
                id: "aggregate".to_string(),
                name: "Old".to_string(),
                strategy: Default::default(),
                members: Vec::new(),
            },
        };
        apply_provider_edit(&mut aggregate, ProviderEdit::Name("New".to_string()));
        let ProviderProfile::Aggregate { shell, routing } = aggregate else {
            unreachable!()
        };
        assert_eq!(shell.name, "New");
        assert_eq!(routing.name, "New");

        let mut ordinary = ProviderProfile::Ordinary(RelayProfile::default());
        apply_provider_edit(
            &mut ordinary,
            ProviderEdit::ContextWindow("2x00 000".to_string()),
        );
        assert_eq!(ordinary.ordinary().unwrap().context_window, "200000");
    }

    #[test]
    fn provider_save_errors_map_to_stable_native_categories() {
        assert_eq!(
            map_provider_save_failure(ProviderErrorKind::Conflict),
            ProviderSaveFailureKind::Conflict
        );
        assert_eq!(
            map_provider_save_failure(ProviderErrorKind::Validation),
            ProviderSaveFailureKind::Validation
        );
        assert_eq!(
            map_provider_save_failure(ProviderErrorKind::SaveFailed),
            ProviderSaveFailureKind::SaveFailed
        );
    }
}
