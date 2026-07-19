use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use codex_plus_core::zed_remote::{
    ZedOpenStrategy, ZedRemoteProjectSource, ZedRemoteRegistryRevision,
};
use codex_plus_manager_service::{
    ContextKind, ContextToolsSource, DiagnoseProviderProfile, FetchProviderModels, OverviewSource,
    PluginMarketplaceKind, PluginMarketplaceSource, ProviderActivationSource, ProviderErrorKind,
    ProviderImportSource, ProviderNetworkFailureKind, ProviderProfile, ProviderSource,
    ProviderSyncSource, RelayEnvironmentSource, ScriptIntegrity, SessionSource,
    TestProviderProfile, UserScriptOrigin, UserScriptRevision, UserScriptSource, ZedRemoteSource,
};
use eframe::egui;

use crate::fonts;
use crate::perf::{PerfRecorder, PerfScriptAction};
use crate::persistence::{self, PersistedUiState};
use crate::runtime::context::{ContextDispatcher, ContextResponse};
use crate::runtime::environment::{EnvironmentDispatcher, EnvironmentResponse};
use crate::runtime::import::{ImportDispatcher, ImportResponse};
use crate::runtime::marketplace::{MarketplaceDispatcher, MarketplaceResponse};
use crate::runtime::provider::{
    ActivationResponse, ProviderActivationDispatcher, ProviderDispatcher, StoreResponse,
};
use crate::runtime::sessions::{SessionDispatcher, SessionResponse};
use crate::runtime::user_scripts::{UserScriptDispatcher, UserScriptResponse};
use crate::runtime::zed_remote::{ZedRemoteDispatcher, ZedRemoteResponse};
use crate::runtime::{DispatchError, OverviewDispatcher};
use crate::state::context::ContextFailureKind;
use crate::state::environment::EnvironmentFailureKind;
use crate::state::import::ImportFailureKind;
use crate::state::marketplace::MarketplaceFailureKind;
use crate::state::provider::{
    DeleteProfileError, GuardOutcome, GuardResolution, LiveLoadFailureKind, LiveMutationFailure,
    LiveMutationKind, OperationPhase, ProviderEditorTab, ProviderLoadFailureKind,
    ProviderLoadPhase, ProviderSaveFailureKind, TransitionResult,
};
use crate::state::sessions::{ProviderSyncFailureKind, SessionFailureKind};
use crate::state::user_scripts::{ScriptsTab, UserScriptFailureKind};
use crate::state::zed_remote::{ZedRemoteFailureKind, ZedRemoteLoadPhase};
use crate::state::{AppState, OverviewFailureKind, OverviewPhase, Route};
use crate::theme;
use crate::views::context::ContextAction;
use crate::views::environment::EnvironmentAction;
use crate::views::import::ImportAction;
use crate::views::marketplace::MarketplaceAction;
use crate::views::provider::{ProviderAction, ProviderEdit};
use crate::views::sessions::SessionAction;
use crate::views::shell::{ShellAction, ShellFeatureStates, ShellViewModel, render_shell};
use crate::views::user_scripts::UserScriptAction;
use crate::views::zed_remote::ZedRemoteAction;

pub struct NativeManagerSources {
    pub overview: Arc<dyn OverviewSource>,
    pub provider: Arc<dyn ProviderSource>,
    pub activation: Arc<dyn ProviderActivationSource>,
    pub provider_import: Arc<dyn ProviderImportSource>,
    pub environment: Arc<dyn RelayEnvironmentSource>,
    pub context: Arc<dyn ContextToolsSource>,
    pub marketplace: Arc<dyn PluginMarketplaceSource>,
    pub sessions: Arc<dyn SessionSource>,
    pub provider_sync: Arc<dyn ProviderSyncSource>,
    pub user_scripts: Arc<dyn UserScriptSource>,
    pub zed_remote: Arc<dyn ZedRemoteSource>,
}

pub struct NativeManagerApp {
    state: AppState,
    persisted: PersistedUiState,
    overview_dispatcher: OverviewDispatcher,
    provider_dispatcher: ProviderDispatcher,
    activation_dispatcher: ProviderActivationDispatcher,
    import_dispatcher: ImportDispatcher,
    environment_dispatcher: EnvironmentDispatcher,
    context_dispatcher: ContextDispatcher,
    marketplace_dispatcher: MarketplaceDispatcher,
    session_dispatcher: SessionDispatcher,
    user_script_dispatcher: UserScriptDispatcher,
    zed_remote_dispatcher: ZedRemoteDispatcher,
    last_updated: Option<String>,
    overview_worker_stopped: bool,
    provider_store_worker_stopped: bool,
    activation_worker_stopped: bool,
    import_worker_stopped: bool,
    environment_worker_stopped: bool,
    context_worker_stopped: bool,
    marketplace_worker_stopped: bool,
    session_worker_stopped: bool,
    user_script_worker_stopped: bool,
    zed_remote_worker_stopped: bool,
    window_focused: bool,
    pending_route: Option<Route>,
    pending_provider_reload: bool,
    perf: Option<PerfRecorder>,
    perf_stale_user_script_revision: Option<UserScriptRevision>,
}

impl NativeManagerApp {
    pub fn new(
        creation: &eframe::CreationContext<'_>,
        cjk_font: Option<Vec<u8>>,
        sources: NativeManagerSources,
        perf: Option<PerfRecorder>,
    ) -> Self {
        egui_extras::install_image_loaders(&creation.egui_ctx);
        if let Some(bytes) = cjk_font {
            fonts::install_cjk_font(&creation.egui_ctx, bytes);
        }

        let persisted = persistence::load(creation.storage);
        theme::apply(&creation.egui_ctx, persisted.theme);
        let repaint_context = creation.egui_ctx.clone();
        let overview_dispatcher = OverviewDispatcher::spawn(
            sources.overview,
            Arc::new(move || repaint_context.request_repaint()),
        );
        let provider_repaint_context = creation.egui_ctx.clone();
        let provider_dispatcher = ProviderDispatcher::spawn(
            sources.provider,
            Arc::new(move || provider_repaint_context.request_repaint()),
        );
        let activation_repaint_context = creation.egui_ctx.clone();
        let activation_dispatcher = ProviderActivationDispatcher::spawn(
            sources.activation,
            Arc::new(move || activation_repaint_context.request_repaint()),
        );
        let import_repaint_context = creation.egui_ctx.clone();
        let import_dispatcher = ImportDispatcher::spawn(
            sources.provider_import,
            Arc::new(move || import_repaint_context.request_repaint()),
        );
        let environment_repaint_context = creation.egui_ctx.clone();
        let environment_dispatcher = EnvironmentDispatcher::spawn(
            sources.environment,
            Arc::new(move || environment_repaint_context.request_repaint()),
        );
        let context_repaint_context = creation.egui_ctx.clone();
        let context_dispatcher = ContextDispatcher::spawn(
            sources.context,
            Arc::new(move || context_repaint_context.request_repaint()),
        );
        let marketplace_repaint_context = creation.egui_ctx.clone();
        let marketplace_dispatcher = MarketplaceDispatcher::spawn(
            sources.marketplace,
            Arc::new(move || marketplace_repaint_context.request_repaint()),
        );
        let session_repaint_context = creation.egui_ctx.clone();
        let session_dispatcher = SessionDispatcher::spawn(
            sources.sessions,
            sources.provider_sync,
            Arc::new(move || session_repaint_context.request_repaint()),
        );
        let user_script_repaint_context = creation.egui_ctx.clone();
        let user_script_dispatcher = UserScriptDispatcher::spawn(
            sources.user_scripts,
            Arc::new(move || user_script_repaint_context.request_repaint()),
        );
        let zed_remote_repaint_context = creation.egui_ctx.clone();
        let zed_remote_dispatcher = ZedRemoteDispatcher::spawn(
            sources.zed_remote,
            Arc::new(move || zed_remote_repaint_context.request_repaint()),
        );
        let mut app = Self {
            state: AppState::default(),
            persisted,
            overview_dispatcher,
            provider_dispatcher,
            activation_dispatcher,
            import_dispatcher,
            environment_dispatcher,
            context_dispatcher,
            marketplace_dispatcher,
            session_dispatcher,
            user_script_dispatcher,
            zed_remote_dispatcher,
            last_updated: None,
            overview_worker_stopped: false,
            provider_store_worker_stopped: false,
            activation_worker_stopped: false,
            import_worker_stopped: false,
            environment_worker_stopped: false,
            context_worker_stopped: false,
            marketplace_worker_stopped: false,
            session_worker_stopped: false,
            user_script_worker_stopped: false,
            zed_remote_worker_stopped: false,
            window_focused: true,
            pending_route: None,
            pending_provider_reload: false,
            perf,
            perf_stale_user_script_revision: None,
        };
        app.refresh_overview();
        app.load_providers();
        app.load_pending_import();
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

    fn refresh_zed_remote(&mut self) {
        let request_id = self.state.zed_remote.begin_load();
        if self.zed_remote_dispatcher.request_load(request_id).is_err() {
            self.zed_remote_worker_stopped = true;
            self.state
                .zed_remote
                .apply_load_response(request_id, Err(ZedRemoteFailureKind::WorkerStopped));
        }
    }

    fn refresh_zed_remote_after_conflict(&mut self) {
        if self.state.zed_remote.take_refresh_after_conflict() {
            self.refresh_zed_remote();
        }
    }

    fn load_providers(&mut self) {
        let Some(request_id) = self.state.provider.begin_live_load() else {
            return;
        };
        if self.activation_dispatcher.request_load(request_id).is_err() {
            self.activation_worker_stopped = true;
            self.state
                .provider
                .apply_live_load_response(request_id, Err(LiveLoadFailureKind::WorkerStopped));
        }
    }

    fn apply_action(&mut self, ctx: &egui::Context, action: ShellAction) {
        match action {
            ShellAction::Navigate(route) => self.navigate(route),
            ShellAction::Refresh => match self.state.route {
                Route::Providers => self.request_provider_reload(),
                Route::Environment => self.inspect_environment(),
                Route::Sessions => self.refresh_sessions_route(),
                Route::Scripts => self.refresh_user_scripts_route(),
                Route::ZedRemote => self.refresh_zed_remote(),
                Route::Context => {
                    self.load_context_workspace();
                    self.inspect_plugin_marketplaces();
                }
                Route::Overview | Route::About => self.refresh_overview(),
            },
            ShellAction::Retry => match self.state.route {
                Route::Environment => self.inspect_environment(),
                Route::Sessions => self.refresh_sessions_route(),
                Route::Scripts => self.refresh_user_scripts_route(),
                Route::ZedRemote => self.refresh_zed_remote(),
                Route::Context => {
                    self.load_context_workspace();
                    self.inspect_plugin_marketplaces();
                }
                Route::Overview | Route::Providers | Route::About => self.refresh_overview(),
            },
            ShellAction::SetLocale(locale) => self.persisted.locale = locale,
            ShellAction::SetTheme(mode) => {
                self.persisted.theme = mode;
                theme::apply(ctx, mode);
            }
            ShellAction::Provider(action) => self.apply_provider_action(action),
            ShellAction::Import(action) => self.apply_import_action(action),
            ShellAction::Environment(action) => self.apply_environment_action(action),
            ShellAction::Sessions(action) => self.apply_session_action(action),
            ShellAction::UserScripts(action) => self.apply_user_script_action(action),
            ShellAction::Context(action) => self.apply_context_action(action),
            ShellAction::Marketplace(action) => self.apply_marketplace_action(action),
            ShellAction::ZedRemote(action) => self.apply_zed_remote_action(ctx, action),
        }
        ctx.request_repaint();
    }

    fn navigate(&mut self, route: Route) {
        if self.state.route == Route::Providers
            && route != Route::Providers
            && self.state.provider.has_unsaved_changes()
        {
            self.pending_route = Some(route);
            self.pending_provider_reload = false;
            let _ = self.state.provider.request_reload();
            return;
        }
        self.pending_route = None;
        self.state.provider_import.reset_route_transients();
        if self.state.route == Route::Providers && route != Route::Providers {
            self.state.provider.leave_provider_route();
        }
        let entering_context = self.state.route != Route::Context && route == Route::Context;
        let entering_sessions = self.state.route != Route::Sessions && route == Route::Sessions;
        let entering_scripts = self.state.route != Route::Scripts && route == Route::Scripts;
        let entering_zed_remote = self.state.route != Route::ZedRemote && route == Route::ZedRemote;
        self.state.route = route;
        if route == Route::Providers && self.state.provider.load_phase == ProviderLoadPhase::Idle {
            self.load_providers();
        }
        if route == Route::Environment
            && self.state.environment.inspection_phase == OperationPhase::Idle
        {
            self.inspect_environment();
        }
        if entering_context {
            self.load_context_workspace();
            self.inspect_plugin_marketplaces();
        }
        if entering_sessions {
            self.refresh_sessions_route();
        }
        if entering_scripts {
            self.refresh_user_scripts_route();
        }
        if entering_zed_remote {
            self.refresh_zed_remote();
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
            ProviderAction::RefreshLive => self.request_provider_reload(),
            ProviderAction::RequestLiveMutation(kind) => {
                let _ = self.state.provider.request_live_mutation(kind);
            }
            ProviderAction::ConfirmLiveMutation => self.dispatch_confirmed_live_mutation(),
            ProviderAction::CancelLiveMutation => {
                self.state.provider.cancel_live_confirmation();
            }
            ProviderAction::BeginLiveFileEdit(kind) => {
                self.state.provider.begin_live_file_edit(kind);
            }
            ProviderAction::EditLiveFile { kind, contents } => {
                self.state.provider.edit_live_file(kind, contents);
            }
            ProviderAction::CancelLiveFileEdit(kind) => {
                self.state.provider.cancel_live_file_edit(kind);
            }
            ProviderAction::SetLiveFileRevealed { kind, revealed } => {
                self.state.provider.set_live_file_revealed(kind, revealed);
            }
            ProviderAction::ResolveGuard(resolution) => self.resolve_provider_guard(resolution),
        }
    }

    fn apply_import_action(&mut self, action: ImportAction) {
        match action {
            ImportAction::DiscoverCcs => self.discover_ccs_providers(),
            ImportAction::CloseCcs => {
                self.state.provider_import.close_discovery();
            }
            ImportAction::ConfirmCcs => self.import_ccs_providers(),
            ImportAction::ConfirmPending => self.confirm_pending_import(),
            ImportAction::DismissPending => self.dismiss_pending_import(),
            ImportAction::RefreshPending => self.load_pending_import(),
        }
    }

    fn apply_environment_action(&mut self, action: EnvironmentAction) {
        match action {
            EnvironmentAction::RetryInspection => self.inspect_environment(),
            EnvironmentAction::SetSelected { name, selected } => {
                self.state.environment.toggle_selection(&name, selected);
            }
            EnvironmentAction::RequestCleanup => {
                self.state.environment.request_cleanup_confirmation();
            }
            EnvironmentAction::CancelCleanup => {
                self.state.environment.cancel_cleanup_confirmation();
            }
            EnvironmentAction::ConfirmCleanup => self.cleanup_environment(),
        }
    }

    fn apply_session_action(&mut self, action: SessionAction) {
        match action {
            SessionAction::Refresh => self.refresh_sessions_route(),
            SessionAction::RetryWorkspace => self.load_session_workspace(),
            SessionAction::SetQuery(query) => self.state.sessions.set_query(query),
            SessionAction::SetFilter(filter) => self.state.sessions.set_filter(filter),
            SessionAction::SetSelected { id, selected } => {
                self.state.sessions.set_selected(&id, selected);
            }
            SessionAction::SelectAllFiltered => {
                self.state.sessions.select_all_filtered();
            }
            SessionAction::ClearSelection => {
                self.state.sessions.clear_selection();
            }
            SessionAction::SetPage(page) => {
                self.state.sessions.set_page(page);
            }
            SessionAction::RequestDelete => {
                self.state.sessions.request_delete();
            }
            SessionAction::CancelDelete => {
                self.state.sessions.cancel_delete();
            }
            SessionAction::ConfirmDelete => self.delete_selected_sessions(),
            SessionAction::RetryProviderWorkspace => self.load_session_provider_workspace(),
            SessionAction::SetProviderTarget(target) => {
                self.state.sessions.set_provider_target(target);
            }
            SessionAction::RunProviderRepair => {
                self.state.sessions.request_provider_run_confirmation();
            }
            SessionAction::CancelProviderRepair => {
                self.state.sessions.cancel_provider_run_confirmation();
            }
            SessionAction::ConfirmProviderRepair => self.run_session_provider_repair(),
            SessionAction::SetAutoRepair(enabled) => self.set_session_auto_repair(enabled),
        }
    }

    fn refresh_sessions_route(&mut self) {
        self.load_session_workspace();
        self.load_session_provider_workspace();
    }

    fn apply_user_script_action(&mut self, action: UserScriptAction) {
        match action {
            UserScriptAction::RefreshMarket => self.load_script_market(),
            UserScriptAction::RefreshLocal => self.load_local_scripts(),
            UserScriptAction::SetTab(tab) => {
                self.state.user_scripts.set_tab(tab);
            }
            UserScriptAction::SetMarketQuery(query) => {
                self.state.user_scripts.set_market_query(query);
            }
            UserScriptAction::SetMarketFilter(filter) => {
                self.state.user_scripts.set_market_filter(filter);
            }
            UserScriptAction::SetMarketPage(page) => {
                self.state.user_scripts.set_market_page(page);
            }
            UserScriptAction::SetLocalQuery(query) => {
                self.state.user_scripts.set_local_query(query);
            }
            UserScriptAction::SetLocalFilter(filter) => {
                self.state.user_scripts.set_local_filter(filter);
            }
            UserScriptAction::SetLocalPage(page) => {
                self.state.user_scripts.set_local_page(page);
            }
            UserScriptAction::RequestInstall(script_id) => {
                self.state.user_scripts.request_install(&script_id);
            }
            UserScriptAction::SetUnverifiedAcknowledgement(acknowledged) => {
                self.state
                    .user_scripts
                    .set_unverified_acknowledgement(acknowledged);
            }
            UserScriptAction::CancelInstall => {
                self.state.user_scripts.cancel_install();
            }
            UserScriptAction::ConfirmInstall => self.install_user_script(),
            UserScriptAction::SetGlobalEnabled(enabled) => {
                self.set_all_user_scripts_enabled(enabled);
            }
            UserScriptAction::SetScriptEnabled { key, enabled } => {
                self.set_user_script_enabled(&key, enabled);
            }
            UserScriptAction::RequestDelete(key) => {
                self.state.user_scripts.request_delete(&key);
            }
            UserScriptAction::CancelDelete => {
                self.state.user_scripts.cancel_delete();
            }
            UserScriptAction::ConfirmDelete => self.delete_user_script(),
            UserScriptAction::Retry => self.refresh_user_scripts_route(),
        }
    }

    fn refresh_user_scripts_route(&mut self) {
        self.load_local_scripts();
        self.load_script_market();
    }

    fn load_local_scripts(&mut self) {
        if self.user_script_worker_stopped {
            self.state.user_scripts.mark_worker_stopped();
            return;
        }
        let request_id = self.state.user_scripts.begin_local_refresh();
        if self
            .user_script_dispatcher
            .request_local(request_id)
            .is_err()
        {
            self.stop_user_script_worker();
        }
    }

    fn load_script_market(&mut self) {
        if self.user_script_worker_stopped {
            self.state.user_scripts.mark_worker_stopped();
            return;
        }
        let request_id = self.state.user_scripts.begin_market_refresh();
        if self
            .user_script_dispatcher
            .request_market(request_id)
            .is_err()
        {
            self.stop_user_script_worker();
        }
    }

    fn install_user_script(&mut self) {
        let Some((request_id, request)) = self.state.user_scripts.confirm_install() else {
            return;
        };
        if self
            .user_script_dispatcher
            .request_install(request_id, request)
            .is_err()
        {
            self.stop_user_script_worker();
        }
    }

    fn set_all_user_scripts_enabled(&mut self, enabled: bool) {
        let Some((request_id, request)) = self.state.user_scripts.request_global_enabled(enabled)
        else {
            return;
        };
        if self
            .user_script_dispatcher
            .request_set_global(request_id, request)
            .is_err()
        {
            self.stop_user_script_worker();
        }
    }

    fn set_user_script_enabled(&mut self, key: &str, enabled: bool) {
        let Some((request_id, request)) =
            self.state.user_scripts.request_script_enabled(key, enabled)
        else {
            return;
        };
        if self
            .user_script_dispatcher
            .request_set_script(request_id, request)
            .is_err()
        {
            self.stop_user_script_worker();
        }
    }

    fn delete_user_script(&mut self) {
        let Some((request_id, request)) = self.state.user_scripts.confirm_delete() else {
            return;
        };
        if self
            .user_script_dispatcher
            .request_delete(request_id, request)
            .is_err()
        {
            self.stop_user_script_worker();
        }
    }

    fn stop_user_script_worker(&mut self) {
        self.user_script_worker_stopped = true;
        self.state.user_scripts.mark_worker_stopped();
    }

    fn request_perf_user_script_conflict(&mut self) {
        let Some(stale_revision) = self.perf_stale_user_script_revision.clone() else {
            return;
        };
        let enabled = self
            .state
            .user_scripts
            .local
            .as_ref()
            .is_some_and(|workspace| !workspace.globally_enabled);
        let Some((request_id, mut request)) =
            self.state.user_scripts.request_global_enabled(enabled)
        else {
            return;
        };
        request.expected_revision = stale_revision;
        if self
            .user_script_dispatcher
            .request_set_global(request_id, request)
            .is_err()
        {
            self.stop_user_script_worker();
        }
    }

    fn load_session_workspace(&mut self) {
        if self.session_worker_stopped {
            self.state.sessions.mark_worker_stopped();
            return;
        }
        let request_id = self.state.sessions.begin_workspace_refresh();
        if self
            .session_dispatcher
            .request_session_load(request_id)
            .is_err()
        {
            self.session_worker_stopped = true;
            self.state.sessions.mark_worker_stopped();
        }
    }

    fn load_session_provider_workspace(&mut self) {
        if self.session_worker_stopped {
            self.state.sessions.mark_worker_stopped();
            return;
        }
        let Some(request_id) = self.state.sessions.begin_provider_workspace_refresh() else {
            return;
        };
        if self
            .session_dispatcher
            .request_provider_load(request_id)
            .is_err()
        {
            self.session_worker_stopped = true;
            self.state.sessions.mark_worker_stopped();
        }
    }

    fn delete_selected_sessions(&mut self) {
        let Some((request_id, request)) = self.state.sessions.confirm_delete() else {
            return;
        };
        if self
            .session_dispatcher
            .request_delete(request_id, request)
            .is_err()
        {
            self.session_worker_stopped = true;
            self.state.sessions.mark_worker_stopped();
        }
    }

    fn run_session_provider_repair(&mut self) {
        let Some((request_id, request)) = self.state.sessions.confirm_provider_run() else {
            return;
        };
        if self
            .session_dispatcher
            .request_provider_run(request_id, request)
            .is_err()
        {
            self.session_worker_stopped = true;
            self.state.sessions.mark_worker_stopped();
        }
    }

    fn set_session_auto_repair(&mut self, enabled: bool) {
        let Some((request_id, request)) = self.state.sessions.begin_set_auto_repair(enabled) else {
            return;
        };
        if self
            .session_dispatcher
            .request_auto_repair(request_id, request)
            .is_err()
        {
            self.session_worker_stopped = true;
            self.state.sessions.mark_worker_stopped();
        }
    }

    fn apply_context_action(&mut self, action: ContextAction) {
        match action {
            ContextAction::RetryWorkspace => self.load_context_workspace(),
            ContextAction::SelectKind(kind) => self.state.context.selected_kind = kind,
            ContextAction::OpenCreate(kind) => {
                if !self.state.provider.is_dirty() {
                    self.state.context.open_create(kind);
                }
            }
            ContextAction::OpenEdit(key) => self.load_context_entry_draft(key),
            ContextAction::SetEditorId(id) => {
                self.state.context.set_editor_id(id);
            }
            ContextAction::SetEditorBody(body) => {
                self.state.context.set_editor_body(body);
            }
            ContextAction::SetTomlRevealed(revealed) => {
                self.state.context.set_editor_toml_revealed(revealed);
            }
            ContextAction::CancelEditor => {
                self.state.context.cancel_editor();
            }
            ContextAction::SaveEditor => self.save_context_entry(),
            ContextAction::SetEnabled { key, enabled } => {
                self.toggle_context_entry(key, enabled);
            }
            ContextAction::RequestDelete(key) => {
                if !self.state.provider.is_dirty() {
                    self.state.context.request_delete(key);
                }
            }
            ContextAction::CancelDelete => {
                self.state.context.cancel_delete();
            }
            ContextAction::ConfirmDelete => self.delete_context_entry(),
            ContextAction::PreviewSync => self.preview_context_sync(),
            ContextAction::CancelSyncPreview => {
                self.state.context.cancel_preview();
            }
            ContextAction::ConfirmSync => self.sync_context_to_live(),
        }
    }

    fn apply_marketplace_action(&mut self, action: MarketplaceAction) {
        match action {
            MarketplaceAction::Refresh => self.inspect_plugin_marketplaces(),
            MarketplaceAction::RequestRepair(kind) => {
                self.state.marketplace.request_repair_confirmation(kind);
            }
            MarketplaceAction::CancelRepair => {
                self.state.marketplace.cancel_repair_confirmation();
            }
            MarketplaceAction::ConfirmRepair => self.repair_plugin_marketplace(),
        }
    }

    fn apply_zed_remote_action(&mut self, ctx: &egui::Context, action: ZedRemoteAction) {
        match action {
            ZedRemoteAction::Refresh => self.refresh_zed_remote(),
            ZedRemoteAction::SetSearch(query) => self.state.zed_remote.set_search_query(query),
            ZedRemoteAction::SetRecentPage(page) => self.state.zed_remote.set_recent_page(page),
            ZedRemoteAction::SetDiscoveredPage(page) => {
                self.state.zed_remote.set_discovered_page(page)
            }
            ZedRemoteAction::SetStrategy(strategy) => self.state.zed_remote.set_strategy(strategy),
            ZedRemoteAction::SetRegistryEnabled(enabled) => {
                self.state.zed_remote.set_registry_enabled(enabled)
            }
            ZedRemoteAction::SavePreferences => {
                let Some((request_id, request)) = self.state.zed_remote.begin_save_preferences()
                else {
                    return;
                };
                if self
                    .zed_remote_dispatcher
                    .request_save_preferences(request_id, request)
                    .is_err()
                {
                    self.zed_remote_worker_stopped = true;
                    self.state
                        .zed_remote
                        .apply_save_response(request_id, Err(ZedRemoteFailureKind::WorkerStopped));
                }
            }
            ZedRemoteAction::RequestOpen {
                project_id,
                strategy,
                remember,
            } => {
                self.state
                    .zed_remote
                    .request_open(project_id, strategy, remember);
            }
            ZedRemoteAction::ConfirmOpen => {
                let Some((request_id, request)) = self.state.zed_remote.begin_open() else {
                    return;
                };
                if self
                    .zed_remote_dispatcher
                    .request_open(request_id, request)
                    .is_err()
                {
                    self.zed_remote_worker_stopped = true;
                    self.state
                        .zed_remote
                        .apply_open_response(request_id, Err(ZedRemoteFailureKind::WorkerStopped));
                }
            }
            ZedRemoteAction::CancelOpen => {
                self.state.zed_remote.cancel_open();
            }
            ZedRemoteAction::SetOpenStrategy(strategy) => {
                self.state.zed_remote.set_open_strategy(strategy);
            }
            ZedRemoteAction::SetOpenRemember(remember) => {
                self.state.zed_remote.set_open_remember(remember);
            }
            ZedRemoteAction::CopyUrl(project_id) => {
                if let Some(url) = self
                    .state
                    .zed_remote
                    .workspace
                    .as_ref()
                    .and_then(|workspace| {
                        workspace
                            .projects
                            .iter()
                            .find(|project| project.id == project_id)
                    })
                    .map(|project| project.url.clone())
                {
                    ctx.copy_text(url);
                }
            }
            ZedRemoteAction::RequestForget(project_id) => {
                self.state.zed_remote.request_forget(project_id);
            }
            ZedRemoteAction::ConfirmForget => {
                let Some((request_id, request)) = self.state.zed_remote.begin_forget() else {
                    return;
                };
                if self
                    .zed_remote_dispatcher
                    .request_forget(request_id, request)
                    .is_err()
                {
                    self.zed_remote_worker_stopped = true;
                    self.state.zed_remote.apply_forget_response(
                        request_id,
                        Err(ZedRemoteFailureKind::WorkerStopped),
                    );
                }
            }
            ZedRemoteAction::CancelForget => {
                self.state.zed_remote.cancel_forget();
            }
        }
    }

    fn inspect_plugin_marketplaces(&mut self) {
        let Some(request_id) = self.state.marketplace.begin_inspection() else {
            return;
        };
        if self
            .marketplace_dispatcher
            .request_inspection(request_id)
            .is_err()
        {
            self.marketplace_worker_stopped = true;
            self.state
                .marketplace
                .apply_inspection_response(request_id, Err(MarketplaceFailureKind::WorkerStopped));
        }
    }

    fn repair_plugin_marketplace(&mut self) {
        let Some((request_id, request)) = self.state.marketplace.confirm_repair() else {
            return;
        };
        let kind = request.kind;
        if self
            .marketplace_dispatcher
            .request_repair(request_id, request)
            .is_err()
        {
            self.marketplace_worker_stopped = true;
            self.state.marketplace.apply_repair_response(
                request_id,
                kind,
                Err(MarketplaceFailureKind::WorkerStopped),
            );
        }
    }

    fn load_context_workspace(&mut self) {
        if self.state.provider.is_dirty() {
            return;
        }
        let request_id = self.state.context.begin_workspace_refresh();
        if self
            .context_dispatcher
            .request_workspace(request_id)
            .is_err()
        {
            self.context_worker_stopped = true;
            self.state.apply_context_workspace_response(
                request_id,
                Err(ContextFailureKind::WorkerStopped),
            );
        }
    }

    fn load_context_entry_draft(&mut self, key: codex_plus_manager_service::ContextEntryKey) {
        if self.state.provider.is_dirty() {
            return;
        }
        let Some((request_id, request)) = self.state.context.begin_edit(key) else {
            return;
        };
        if self
            .context_dispatcher
            .request_draft(request_id, request)
            .is_err()
        {
            self.context_worker_stopped = true;
            self.state
                .context
                .apply_draft_response(request_id, Err(ContextFailureKind::WorkerStopped));
        }
    }

    fn save_context_entry(&mut self) {
        if self.state.provider.is_dirty() {
            return;
        }
        let Some((request_id, request)) = self.state.context.begin_save() else {
            return;
        };
        if self
            .context_dispatcher
            .request_save(request_id, request)
            .is_err()
        {
            self.context_worker_stopped = true;
            self.state.apply_context_stored_mutation_response(
                request_id,
                Err(ContextFailureKind::WorkerStopped),
            );
        }
    }

    fn toggle_context_entry(
        &mut self,
        key: codex_plus_manager_service::ContextEntryKey,
        enabled: bool,
    ) {
        if self.state.provider.is_dirty() {
            return;
        }
        let Some((request_id, request)) = self.state.context.begin_toggle(key, enabled) else {
            return;
        };
        if self
            .context_dispatcher
            .request_toggle(request_id, request)
            .is_err()
        {
            self.context_worker_stopped = true;
            self.state.apply_context_stored_mutation_response(
                request_id,
                Err(ContextFailureKind::WorkerStopped),
            );
        }
    }

    fn delete_context_entry(&mut self) {
        if self.state.provider.is_dirty() {
            return;
        }
        let Some((request_id, request)) = self.state.context.begin_delete() else {
            return;
        };
        if self
            .context_dispatcher
            .request_delete(request_id, request)
            .is_err()
        {
            self.context_worker_stopped = true;
            self.state.apply_context_stored_mutation_response(
                request_id,
                Err(ContextFailureKind::WorkerStopped),
            );
        }
    }

    fn preview_context_sync(&mut self) {
        if self.state.provider.is_dirty() {
            return;
        }
        let Some((request_id, request)) = self.state.context.begin_preview() else {
            return;
        };
        if self
            .context_dispatcher
            .request_preview(request_id, request)
            .is_err()
        {
            self.context_worker_stopped = true;
            self.state
                .context
                .apply_preview_response(request_id, Err(ContextFailureKind::WorkerStopped));
        }
    }

    fn sync_context_to_live(&mut self) {
        if self.state.provider.is_dirty() {
            return;
        }
        let Some((request_id, request)) = self.state.context.begin_sync() else {
            return;
        };
        if self
            .context_dispatcher
            .request_sync(request_id, request)
            .is_err()
        {
            self.context_worker_stopped = true;
            self.state
                .apply_context_sync_response(request_id, Err(ContextFailureKind::WorkerStopped));
        }
    }

    fn discover_ccs_providers(&mut self) {
        let request_id = self.state.provider_import.begin_discovery();
        if self
            .import_dispatcher
            .request_discovery(request_id)
            .is_err()
        {
            self.import_worker_stopped = true;
            self.state
                .provider_import
                .apply_discovery_response(request_id, Err(ImportFailureKind::WorkerStopped));
        }
    }

    fn import_ccs_providers(&mut self) {
        let Some((request_id, request)) = self
            .state
            .provider_import
            .begin_ccs_import(self.state.provider.is_dirty())
        else {
            return;
        };
        if self
            .import_dispatcher
            .request_ccs_import(request_id, request)
            .is_err()
        {
            self.import_worker_stopped = true;
            self.state
                .provider_import
                .apply_ccs_import_response(request_id, Err(ImportFailureKind::WorkerStopped));
        }
    }

    fn load_pending_import(&mut self) {
        let request_id = self.state.provider_import.begin_pending_load();
        if self
            .import_dispatcher
            .request_pending_load(request_id)
            .is_err()
        {
            self.import_worker_stopped = true;
            self.state
                .provider_import
                .apply_pending_load_response(request_id, Err(ImportFailureKind::WorkerStopped));
        }
    }

    fn confirm_pending_import(&mut self) {
        let provider_revision = self
            .state
            .provider
            .baseline
            .as_ref()
            .map(|workspace| workspace.revision.clone());
        let Some((request_id, request)) = self
            .state
            .provider_import
            .begin_pending_confirm(self.state.provider.is_dirty(), provider_revision)
        else {
            return;
        };
        if self
            .import_dispatcher
            .request_pending_confirm(request_id, request)
            .is_err()
        {
            self.import_worker_stopped = true;
            self.state
                .provider_import
                .apply_pending_confirm_response(request_id, Err(ImportFailureKind::WorkerStopped));
        }
    }

    fn dismiss_pending_import(&mut self) {
        let Some((request_id, request)) = self.state.provider_import.begin_pending_dismiss() else {
            return;
        };
        if self
            .import_dispatcher
            .request_pending_dismiss(request_id, request)
            .is_err()
        {
            self.import_worker_stopped = true;
            self.state
                .provider_import
                .apply_pending_dismiss_response(request_id, Err(ImportFailureKind::WorkerStopped));
        }
    }

    fn inspect_environment(&mut self) {
        let request_id = self.state.environment.begin_inspection();
        if self
            .environment_dispatcher
            .request_inspection(request_id)
            .is_err()
        {
            self.environment_worker_stopped = true;
            self.state
                .environment
                .apply_inspection_response(request_id, Err(EnvironmentFailureKind::WorkerStopped));
        }
    }

    fn cleanup_environment(&mut self) {
        let Some((request_id, request)) = self.state.environment.begin_cleanup() else {
            return;
        };
        if self
            .environment_dispatcher
            .request_cleanup(request_id, request)
            .is_err()
        {
            self.environment_worker_stopped = true;
            self.state
                .environment
                .apply_cleanup_response(request_id, Err(EnvironmentFailureKind::WorkerStopped));
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

    fn dispatch_confirmed_live_mutation(&mut self) {
        let Some((request_id, command)) = self.state.provider.confirm_live_mutation() else {
            return;
        };
        if self
            .activation_dispatcher
            .request_mutation(request_id, command)
            .is_err()
        {
            self.activation_worker_stopped = true;
            self.state.provider.apply_live_mutation_response(
                request_id,
                Err(LiveMutationFailure::worker_stopped()),
            );
        }
    }

    fn resolve_provider_guard(&mut self, resolution: GuardResolution) {
        match self.state.provider.resolve_guard(resolution) {
            GuardOutcome::NeedsSave => self.save_providers(),
            GuardOutcome::NeedsLiveSave(kind) => {
                let _ = self
                    .state
                    .provider
                    .request_live_mutation(LiveMutationKind::SaveFile(kind));
            }
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
            if self.state.route == Route::Providers && route != Route::Providers {
                self.state.provider.leave_provider_route();
            }
            self.state.route = route;
            if route == Route::Environment
                && self.state.environment.inspection_phase == OperationPhase::Idle
            {
                self.inspect_environment();
            }
            if route == Route::Context {
                self.load_context_workspace();
                self.inspect_plugin_marketplaces();
            }
            if route == Route::Sessions {
                self.refresh_sessions_route();
            }
            if route == Route::Scripts {
                self.refresh_user_scripts_route();
            }
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

    fn reduce_activation_responses(&mut self) {
        loop {
            match self.activation_dispatcher.try_recv() {
                Ok(Some(ActivationResponse::Load { request_id, result })) => {
                    let accepted = self.state.provider.apply_live_load_response(
                        request_id,
                        result.map_err(|error| LiveLoadFailureKind::Activation(error.kind())),
                    );
                    if accepted && self.state.provider.live.load_phase == ProviderLoadPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(ActivationResponse::Mutation { request_id, result })) => {
                    let succeeded = result.is_ok();
                    let accepted = self.state.provider.apply_live_mutation_response(
                        request_id,
                        result.map_err(|error| {
                            LiveMutationFailure::new(
                                error.kind(),
                                error.rollback(),
                                error.backup_path().map(str::to_owned),
                            )
                        }),
                    );
                    if accepted && succeeded {
                        self.last_updated = Some(current_utc_time());
                        self.complete_pending_provider_transition();
                    }
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if !self.activation_worker_stopped {
                        self.activation_worker_stopped = true;
                        if matches!(
                            self.state.provider.live.load_phase,
                            ProviderLoadPhase::Loading | ProviderLoadPhase::Refreshing
                        ) {
                            self.state.provider.apply_live_load_response(
                                self.state.provider.live.current_load_request_id,
                                Err(LiveLoadFailureKind::WorkerStopped),
                            );
                        }
                        if self.state.provider.live.mutation_phase == OperationPhase::Running {
                            self.state.provider.apply_live_mutation_response(
                                self.state.provider.live.current_mutation_request_id,
                                Err(LiveMutationFailure::worker_stopped()),
                            );
                        }
                    }
                    break;
                }
            }
        }
    }

    fn reduce_import_responses(&mut self) {
        loop {
            match self.import_dispatcher.try_recv() {
                Ok(Some(ImportResponse::Discovery { request_id, result })) => {
                    self.state.provider_import.apply_discovery_response(
                        request_id,
                        result.map_err(|error| ImportFailureKind::Service(error.kind())),
                    );
                }
                Ok(Some(ImportResponse::CcsImport { request_id, result })) => {
                    let apply = self.state.provider_import.apply_ccs_import_response(
                        request_id,
                        result.map_err(|error| ImportFailureKind::Service(error.kind())),
                    );
                    self.install_import_workspace(apply.workspace);
                }
                Ok(Some(ImportResponse::PendingLoad { request_id, result })) => {
                    self.state.provider_import.apply_pending_load_response(
                        request_id,
                        result.map_err(|error| ImportFailureKind::Service(error.kind())),
                    );
                }
                Ok(Some(ImportResponse::PendingConfirm { request_id, result })) => {
                    let apply = self.state.provider_import.apply_pending_confirm_response(
                        request_id,
                        result.map_err(|error| ImportFailureKind::Service(error.kind())),
                    );
                    self.install_import_workspace(apply.workspace);
                }
                Ok(Some(ImportResponse::PendingDismiss { request_id, result })) => {
                    self.state.provider_import.apply_pending_dismiss_response(
                        request_id,
                        result.map_err(|error| ImportFailureKind::Service(error.kind())),
                    );
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if !self.import_worker_stopped {
                        self.import_worker_stopped = true;
                        self.fail_running_import_operations();
                    }
                    break;
                }
            }
        }
    }

    fn install_import_workspace(
        &mut self,
        workspace: Option<Arc<codex_plus_manager_service::ProviderWorkspace>>,
    ) {
        let Some(workspace) = workspace else {
            return;
        };
        if self.state.apply_imported_provider_workspace(workspace) {
            self.last_updated = Some(current_utc_time());
            self.load_providers();
        }
    }

    fn fail_running_import_operations(&mut self) {
        if self.state.provider_import.discovery.phase == OperationPhase::Running {
            self.state.provider_import.apply_discovery_response(
                self.state.provider_import.discovery.current_request_id,
                Err(ImportFailureKind::WorkerStopped),
            );
        }
        if self.state.provider_import.batch_import.phase == OperationPhase::Running {
            self.state.provider_import.apply_ccs_import_response(
                self.state.provider_import.batch_import.current_request_id,
                Err(ImportFailureKind::WorkerStopped),
            );
        }
        if self.state.provider_import.pending_load.phase == OperationPhase::Running {
            self.state.provider_import.apply_pending_load_response(
                self.state.provider_import.pending_load.current_request_id,
                Err(ImportFailureKind::WorkerStopped),
            );
        }
        if self.state.provider_import.pending_confirm.phase == OperationPhase::Running {
            self.state.provider_import.apply_pending_confirm_response(
                self.state
                    .provider_import
                    .pending_confirm
                    .current_request_id,
                Err(ImportFailureKind::WorkerStopped),
            );
        }
        if self.state.provider_import.pending_dismiss.phase == OperationPhase::Running {
            self.state.provider_import.apply_pending_dismiss_response(
                self.state
                    .provider_import
                    .pending_dismiss
                    .current_request_id,
                Err(ImportFailureKind::WorkerStopped),
            );
        }
    }

    fn reduce_environment_responses(&mut self) {
        loop {
            match self.environment_dispatcher.try_recv() {
                Ok(Some(EnvironmentResponse::Inspection { request_id, result })) => {
                    let accepted = self.state.environment.apply_inspection_response(
                        request_id,
                        result.map_err(|error| EnvironmentFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.environment.inspection_phase == OperationPhase::Ready
                    {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(EnvironmentResponse::Cleanup { request_id, result })) => {
                    let accepted = self.state.environment.apply_cleanup_response(
                        request_id,
                        result.map_err(|error| EnvironmentFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.environment.cleanup_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if !self.environment_worker_stopped {
                        self.environment_worker_stopped = true;
                        if self.state.environment.inspection_phase == OperationPhase::Running {
                            self.state.environment.apply_inspection_response(
                                self.state.environment.current_inspection_request_id,
                                Err(EnvironmentFailureKind::WorkerStopped),
                            );
                        }
                        if self.state.environment.cleanup_phase == OperationPhase::Running {
                            self.state.environment.apply_cleanup_response(
                                self.state.environment.current_cleanup_request_id,
                                Err(EnvironmentFailureKind::WorkerStopped),
                            );
                        }
                    }
                    break;
                }
            }
        }
    }

    fn reduce_session_responses(&mut self) {
        loop {
            match self.session_dispatcher.try_recv() {
                Ok(Some(SessionResponse::SessionsLoaded { request_id, result })) => {
                    let accepted = self.state.sessions.apply_workspace_response(
                        request_id,
                        result.map_err(|error| SessionFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.sessions.workspace_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(SessionResponse::SessionsDeleted { request_id, result })) => {
                    let accepted = self.state.sessions.apply_delete_response(
                        request_id,
                        result.map_err(|error| SessionFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.sessions.delete_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(SessionResponse::ProviderSyncLoaded { request_id, result })) => {
                    let accepted = self.state.sessions.apply_provider_workspace_response(
                        request_id,
                        result.map_err(|error| ProviderSyncFailureKind::Service(error.kind())),
                    );
                    if accepted
                        && self.state.sessions.provider_workspace_phase == OperationPhase::Ready
                    {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(SessionResponse::ProviderSyncRan { request_id, result })) => {
                    let accepted = self.state.sessions.apply_provider_run_response(
                        request_id,
                        result.map_err(|error| ProviderSyncFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.sessions.provider_run_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(SessionResponse::AutoRepairSaved { request_id, result })) => {
                    let accepted = self.state.sessions.apply_auto_repair_response(
                        request_id,
                        result.map_err(|error| ProviderSyncFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.sessions.auto_repair_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if !self.session_worker_stopped {
                        self.session_worker_stopped = true;
                        self.state.sessions.mark_worker_stopped();
                    }
                    break;
                }
            }
        }
    }

    fn reduce_user_script_responses(&mut self) {
        loop {
            match self.user_script_dispatcher.try_recv() {
                Ok(Some(UserScriptResponse::LocalInspected { request_id, result })) => {
                    let accepted = self.state.user_scripts.apply_local_response(
                        request_id,
                        result.map_err(|error| UserScriptFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.user_scripts.local_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(UserScriptResponse::MarketRefreshed { request_id, result })) => {
                    let accepted = self.state.user_scripts.apply_market_response(
                        request_id,
                        result.map_err(|error| UserScriptFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.user_scripts.market_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(UserScriptResponse::MutationFinished { request_id, result })) => {
                    let accepted = self.state.user_scripts.apply_mutation_response(
                        request_id,
                        result.map_err(|error| UserScriptFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.user_scripts.mutation_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if !self.user_script_worker_stopped {
                        self.stop_user_script_worker();
                    }
                    break;
                }
            }
        }
    }

    fn reduce_context_responses(&mut self) {
        loop {
            match self.context_dispatcher.try_recv() {
                Ok(Some(ContextResponse::Workspace { request_id, result })) => {
                    let accepted = self.state.apply_context_workspace_response(
                        request_id,
                        result.map_err(|error| ContextFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.context.workspace_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(ContextResponse::Draft { request_id, result })) => {
                    self.state.context.apply_draft_response(
                        request_id,
                        result.map_err(|error| ContextFailureKind::Service(error.kind())),
                    );
                }
                Ok(Some(ContextResponse::StoredMutation { request_id, result })) => {
                    let accepted = self.state.apply_context_stored_mutation_response(
                        request_id,
                        result.map_err(|error| ContextFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.context.mutation_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(ContextResponse::Preview { request_id, result })) => {
                    self.state.context.apply_preview_response(
                        request_id,
                        result.map_err(|error| ContextFailureKind::Service(error.kind())),
                    );
                }
                Ok(Some(ContextResponse::Sync { request_id, result })) => {
                    let accepted = self.state.apply_context_sync_response(
                        request_id,
                        result.map_err(|error| ContextFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.context.sync_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if !self.context_worker_stopped {
                        self.context_worker_stopped = true;
                        self.fail_running_context_operations();
                    }
                    break;
                }
            }
        }
    }

    fn reduce_marketplace_responses(&mut self) {
        loop {
            match self.marketplace_dispatcher.try_recv() {
                Ok(Some(MarketplaceResponse::Inspected { request_id, result })) => {
                    let accepted = self.state.marketplace.apply_inspection_response(
                        request_id,
                        result.map_err(|error| MarketplaceFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.marketplace.inspection_phase == OperationPhase::Ready
                    {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(Some(MarketplaceResponse::Repaired {
                    request_id,
                    kind,
                    result,
                })) => {
                    let accepted = self.state.marketplace.apply_repair_response(
                        request_id,
                        kind,
                        result.map_err(|error| MarketplaceFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.marketplace.repair_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if !self.marketplace_worker_stopped {
                        self.marketplace_worker_stopped = true;
                        if self.state.marketplace.inspection_phase == OperationPhase::Running {
                            self.state.marketplace.apply_inspection_response(
                                self.state.marketplace.current_inspection_request_id,
                                Err(MarketplaceFailureKind::WorkerStopped),
                            );
                        }
                        if self.state.marketplace.repair_phase == OperationPhase::Running
                            && let Some(kind) = self.state.marketplace.active_repair_kind
                        {
                            self.state.marketplace.apply_repair_response(
                                self.state.marketplace.current_repair_request_id,
                                kind,
                                Err(MarketplaceFailureKind::WorkerStopped),
                            );
                        }
                    }
                    break;
                }
            }
        }
    }

    fn reduce_zed_remote_responses(&mut self) {
        loop {
            match self.zed_remote_dispatcher.try_recv() {
                Ok(Some(ZedRemoteResponse::Load { request_id, result })) => {
                    let accepted = self.state.zed_remote.apply_load_response(
                        request_id,
                        result.map_err(|error| ZedRemoteFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.zed_remote.load_phase == ZedRemoteLoadPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                    self.refresh_zed_remote_after_conflict();
                }
                Ok(Some(ZedRemoteResponse::SavePreferences { request_id, result })) => {
                    let accepted = self.state.zed_remote.apply_save_response(
                        request_id,
                        result.map_err(|error| ZedRemoteFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.zed_remote.save_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                    self.refresh_zed_remote_after_conflict();
                }
                Ok(Some(ZedRemoteResponse::Open { request_id, result })) => {
                    let accepted = self.state.zed_remote.apply_open_response(
                        request_id,
                        result.map_err(|error| ZedRemoteFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.zed_remote.open_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                    self.refresh_zed_remote_after_conflict();
                }
                Ok(Some(ZedRemoteResponse::Forget { request_id, result })) => {
                    let accepted = self.state.zed_remote.apply_forget_response(
                        request_id,
                        result.map_err(|error| ZedRemoteFailureKind::Service(error.kind())),
                    );
                    if accepted && self.state.zed_remote.forget_phase == OperationPhase::Ready {
                        self.last_updated = Some(current_utc_time());
                    }
                    self.refresh_zed_remote_after_conflict();
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if !self.zed_remote_worker_stopped {
                        self.zed_remote_worker_stopped = true;
                        if matches!(
                            self.state.zed_remote.load_phase,
                            ZedRemoteLoadPhase::Loading | ZedRemoteLoadPhase::Refreshing
                        ) {
                            self.state.zed_remote.apply_load_response(
                                self.state.zed_remote.current_load_request_id,
                                Err(ZedRemoteFailureKind::WorkerStopped),
                            );
                        }
                        if self.state.zed_remote.save_phase == OperationPhase::Running {
                            self.state.zed_remote.apply_save_response(
                                self.state.zed_remote.current_save_request_id,
                                Err(ZedRemoteFailureKind::WorkerStopped),
                            );
                        }
                        if self.state.zed_remote.open_phase == OperationPhase::Running {
                            self.state.zed_remote.apply_open_response(
                                self.state.zed_remote.current_open_request_id,
                                Err(ZedRemoteFailureKind::WorkerStopped),
                            );
                        }
                        if self.state.zed_remote.forget_phase == OperationPhase::Running {
                            self.state.zed_remote.apply_forget_response(
                                self.state.zed_remote.current_forget_request_id,
                                Err(ZedRemoteFailureKind::WorkerStopped),
                            );
                        }
                    }
                    break;
                }
            }
        }
    }

    fn fail_running_context_operations(&mut self) {
        if self.state.context.workspace_phase == OperationPhase::Running {
            self.state.apply_context_workspace_response(
                self.state.context.current_workspace_request_id,
                Err(ContextFailureKind::WorkerStopped),
            );
        }
        if self.state.context.draft_phase == OperationPhase::Running {
            self.state.context.apply_draft_response(
                self.state.context.current_draft_request_id,
                Err(ContextFailureKind::WorkerStopped),
            );
        }
        if self.state.context.mutation_phase == OperationPhase::Running {
            self.state.apply_context_stored_mutation_response(
                self.state.context.current_mutation_request_id,
                Err(ContextFailureKind::WorkerStopped),
            );
        }
        if self.state.context.preview_phase == OperationPhase::Running {
            self.state.context.apply_preview_response(
                self.state.context.current_preview_request_id,
                Err(ContextFailureKind::WorkerStopped),
            );
        }
        if self.state.context.sync_phase == OperationPhase::Running {
            self.state.apply_context_sync_response(
                self.state.context.current_sync_request_id,
                Err(ContextFailureKind::WorkerStopped),
            );
        }
    }

    fn refresh_pending_on_focus_regain(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(true));
        if focused && !self.window_focused {
            self.load_pending_import();
            if self.state.route == Route::Context {
                self.load_context_workspace();
                self.inspect_plugin_marketplaces();
            }
            if self.state.route == Route::Sessions {
                self.refresh_sessions_route();
            }
            if self.state.route == Route::Scripts {
                self.refresh_user_scripts_route();
            }
            if self.state.route == Route::ZedRemote {
                self.refresh_zed_remote();
            }
        }
        self.window_focused = focused;
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
            PerfScriptAction::RefreshLive => {
                Some(ShellAction::Provider(ProviderAction::RefreshLive))
            }
            PerfScriptAction::OpenLiveTab => Some(ShellAction::Provider(ProviderAction::SetTab(
                ProviderEditorTab::Live,
            ))),
            PerfScriptAction::RequestClearLive => Some(ShellAction::Provider(
                ProviderAction::RequestLiveMutation(LiveMutationKind::Clear),
            )),
            PerfScriptAction::CancelLiveConfirmation => {
                Some(ShellAction::Provider(ProviderAction::CancelLiveMutation))
            }
            PerfScriptAction::ConfirmLiveMutation => {
                Some(ShellAction::Provider(ProviderAction::ConfirmLiveMutation))
            }
            PerfScriptAction::ToggleProviderList => {
                Some(ShellAction::Provider(ProviderAction::ToggleList))
            }
            PerfScriptAction::NavigateEnvironment => {
                Some(ShellAction::Navigate(Route::Environment))
            }
            PerfScriptAction::RefreshEnvironment => Some(ShellAction::Refresh),
            PerfScriptAction::SelectFirstEnvironmentConflict => self
                .state
                .environment
                .workspace
                .as_ref()
                .and_then(|workspace| workspace.conflicts.first())
                .map(|conflict| {
                    ShellAction::Environment(EnvironmentAction::SetSelected {
                        name: conflict.name.clone(),
                        selected: true,
                    })
                }),
            PerfScriptAction::RequestEnvironmentCleanup => {
                Some(ShellAction::Environment(EnvironmentAction::RequestCleanup))
            }
            PerfScriptAction::CancelEnvironmentCleanup => {
                Some(ShellAction::Environment(EnvironmentAction::CancelCleanup))
            }
            PerfScriptAction::OpenCcsImport => Some(ShellAction::Import(ImportAction::DiscoverCcs)),
            PerfScriptAction::CloseCcsImport => Some(ShellAction::Import(ImportAction::CloseCcs)),
            PerfScriptAction::RefreshPendingImport => {
                Some(ShellAction::Import(ImportAction::RefreshPending))
            }
            PerfScriptAction::DismissPendingImport => {
                Some(ShellAction::Import(ImportAction::DismissPending))
            }
            PerfScriptAction::NavigateContext => Some(ShellAction::Navigate(Route::Context)),
            PerfScriptAction::RefreshContext => Some(ShellAction::Refresh),
            PerfScriptAction::SelectNextContextKind => {
                let kind = match self.state.context.selected_kind {
                    ContextKind::Mcp => ContextKind::Skill,
                    ContextKind::Skill => ContextKind::Plugin,
                    ContextKind::Plugin => ContextKind::Mcp,
                };
                Some(ShellAction::Context(ContextAction::SelectKind(kind)))
            }
            PerfScriptAction::CreateContextEntry => Some(ShellAction::Context(
                ContextAction::OpenCreate(self.state.context.selected_kind),
            )),
            PerfScriptAction::CancelContextEditor => {
                Some(ShellAction::Context(ContextAction::CancelEditor))
            }
            PerfScriptAction::OpenFirstContextEntry => self
                .state
                .context
                .bundle
                .as_ref()
                .and_then(|bundle| {
                    bundle
                        .context
                        .entries
                        .iter()
                        .find(|entry| entry.key.kind == self.state.context.selected_kind)
                })
                .map(|entry| ShellAction::Context(ContextAction::OpenEdit(entry.key.clone()))),
            PerfScriptAction::ToggleFirstContextEntry => self
                .state
                .context
                .bundle
                .as_ref()
                .and_then(|bundle| {
                    bundle
                        .context
                        .entries
                        .iter()
                        .find(|entry| entry.key.kind == self.state.context.selected_kind)
                })
                .map(|entry| {
                    ShellAction::Context(ContextAction::SetEnabled {
                        key: entry.key.clone(),
                        enabled: !entry.enabled,
                    })
                }),
            PerfScriptAction::RequestDeleteFirstContextEntry => self
                .state
                .context
                .bundle
                .as_ref()
                .and_then(|bundle| {
                    bundle
                        .context
                        .entries
                        .iter()
                        .find(|entry| entry.key.kind == self.state.context.selected_kind)
                })
                .map(|entry| ShellAction::Context(ContextAction::RequestDelete(entry.key.clone()))),
            PerfScriptAction::CancelContextDelete => {
                Some(ShellAction::Context(ContextAction::CancelDelete))
            }
            PerfScriptAction::PreviewContextSync => {
                Some(ShellAction::Context(ContextAction::PreviewSync))
            }
            PerfScriptAction::CancelContextSyncPreview => {
                Some(ShellAction::Context(ContextAction::CancelSyncPreview))
            }
            PerfScriptAction::ConfirmContextSync => {
                Some(ShellAction::Context(ContextAction::ConfirmSync))
            }
            PerfScriptAction::RequestLocalMarketplaceRepair => Some(ShellAction::Marketplace(
                MarketplaceAction::RequestRepair(PluginMarketplaceKind::Local),
            )),
            PerfScriptAction::ConfirmLocalMarketplaceRepair => {
                Some(ShellAction::Marketplace(MarketplaceAction::ConfirmRepair))
            }
            PerfScriptAction::RequestRemoteMarketplaceRepair => Some(ShellAction::Marketplace(
                MarketplaceAction::RequestRepair(PluginMarketplaceKind::Remote),
            )),
            PerfScriptAction::ConfirmRemoteMarketplaceRepair => {
                Some(ShellAction::Marketplace(MarketplaceAction::ConfirmRepair))
            }
            PerfScriptAction::RefreshMarketplace => {
                Some(ShellAction::Marketplace(MarketplaceAction::Refresh))
            }
            PerfScriptAction::NavigateSessions => Some(ShellAction::Navigate(Route::Sessions)),
            PerfScriptAction::RefreshSessions => {
                Some(ShellAction::Sessions(SessionAction::Refresh))
            }
            PerfScriptAction::SetSessionQuery => Some(ShellAction::Sessions(
                SessionAction::SetQuery("performance".to_owned()),
            )),
            PerfScriptAction::SelectAllFilteredSessions => {
                Some(ShellAction::Sessions(SessionAction::SelectAllFiltered))
            }
            PerfScriptAction::OpenDeleteConfirmation => {
                Some(ShellAction::Sessions(SessionAction::RequestDelete))
            }
            PerfScriptAction::CancelDeleteConfirmation => {
                Some(ShellAction::Sessions(SessionAction::CancelDelete))
            }
            PerfScriptAction::RunProviderRepair => {
                Some(ShellAction::Sessions(SessionAction::RunProviderRepair))
            }
            PerfScriptAction::CancelProviderRepair => {
                Some(ShellAction::Sessions(SessionAction::CancelProviderRepair))
            }
            PerfScriptAction::NavigateScripts => Some(ShellAction::Navigate(Route::Scripts)),
            PerfScriptAction::RefreshLocalScripts => {
                Some(ShellAction::UserScripts(UserScriptAction::RefreshLocal))
            }
            PerfScriptAction::RefreshScriptMarket => {
                Some(ShellAction::UserScripts(UserScriptAction::RefreshMarket))
            }
            PerfScriptAction::OpenLocalScripts => Some(ShellAction::UserScripts(
                UserScriptAction::SetTab(ScriptsTab::Local),
            )),
            PerfScriptAction::OpenScriptMarket => Some(ShellAction::UserScripts(
                UserScriptAction::SetTab(ScriptsTab::Market),
            )),
            PerfScriptAction::RequestVerifiedScriptInstall => self
                .state
                .user_scripts
                .market
                .as_ref()
                .and_then(|market| {
                    market.entries.iter().find(|entry| {
                        entry.integrity == ScriptIntegrity::Verified
                            && self
                                .state
                                .user_scripts
                                .installed_version(&entry.id)
                                .is_none()
                    })
                })
                .map(|entry| {
                    ShellAction::UserScripts(UserScriptAction::RequestInstall(entry.id.clone()))
                }),
            PerfScriptAction::CancelScriptInstall => {
                Some(ShellAction::UserScripts(UserScriptAction::CancelInstall))
            }
            PerfScriptAction::ConfirmVerifiedScriptInstall => {
                Some(ShellAction::UserScripts(UserScriptAction::ConfirmInstall))
            }
            PerfScriptAction::DisableAllScripts => {
                self.perf_stale_user_script_revision = self
                    .state
                    .user_scripts
                    .local
                    .as_ref()
                    .map(|workspace| workspace.revision.clone());
                Some(ShellAction::UserScripts(
                    UserScriptAction::SetGlobalEnabled(false),
                ))
            }
            PerfScriptAction::ToggleFirstUserScript => self
                .state
                .user_scripts
                .local
                .as_ref()
                .and_then(|workspace| {
                    workspace.scripts.iter().find(|script| {
                        script.origin == UserScriptOrigin::User && script.market_id.is_none()
                    })
                })
                .map(|script| {
                    ShellAction::UserScripts(UserScriptAction::SetScriptEnabled {
                        key: script.key.clone(),
                        enabled: !script.enabled,
                    })
                }),
            PerfScriptAction::RequestScriptConflict => {
                self.request_perf_user_script_conflict();
                None
            }
            PerfScriptAction::RetryScriptConflict => {
                Some(ShellAction::UserScripts(UserScriptAction::Retry))
            }
            PerfScriptAction::RequestDeleteFirstUserScript => self
                .state
                .user_scripts
                .local
                .as_ref()
                .and_then(|workspace| {
                    workspace.scripts.iter().find(|script| {
                        script.origin == UserScriptOrigin::User && script.market_id.is_none()
                    })
                })
                .map(|script| {
                    ShellAction::UserScripts(UserScriptAction::RequestDelete(script.key.clone()))
                }),
            PerfScriptAction::CancelUserScriptDelete => {
                Some(ShellAction::UserScripts(UserScriptAction::CancelDelete))
            }
            PerfScriptAction::ConfirmUserScriptDelete => {
                Some(ShellAction::UserScripts(UserScriptAction::ConfirmDelete))
            }
            PerfScriptAction::NavigateZedRemote => Some(ShellAction::Navigate(Route::ZedRemote)),
            PerfScriptAction::RefreshZedRemote => {
                Some(ShellAction::ZedRemote(ZedRemoteAction::Refresh))
            }
            PerfScriptAction::EditZedPreferences => {
                self.apply_action(
                    ctx,
                    ShellAction::ZedRemote(ZedRemoteAction::SetStrategy(
                        ZedOpenStrategy::NewWindow,
                    )),
                );
                self.apply_action(
                    ctx,
                    ShellAction::ZedRemote(ZedRemoteAction::SetRegistryEnabled(true)),
                );
                None
            }
            PerfScriptAction::SaveZedPreferences => {
                Some(ShellAction::ZedRemote(ZedRemoteAction::SavePreferences))
            }
            PerfScriptAction::RequestZedOpen => self
                .state
                .zed_remote
                .workspace
                .as_ref()
                .and_then(|workspace| workspace.projects.iter().find(|project| project.is_current))
                .map(|project| {
                    ShellAction::ZedRemote(ZedRemoteAction::RequestOpen {
                        project_id: project.id.clone(),
                        strategy: self.state.zed_remote.draft_strategy,
                        remember: self.state.zed_remote.draft_registry_enabled,
                    })
                }),
            PerfScriptAction::CancelZedOpen => {
                Some(ShellAction::ZedRemote(ZedRemoteAction::CancelOpen))
            }
            PerfScriptAction::ConfirmZedOpen => {
                Some(ShellAction::ZedRemote(ZedRemoteAction::ConfirmOpen))
            }
            PerfScriptAction::RequestZedForget => self
                .state
                .zed_remote
                .workspace
                .as_ref()
                .and_then(|workspace| {
                    workspace.projects.iter().find(|project| {
                        project.source == ZedRemoteProjectSource::Recent
                            && project.label == "Performance forget fixture"
                    })
                })
                .map(|project| {
                    ShellAction::ZedRemote(ZedRemoteAction::RequestForget(project.id.clone()))
                }),
            PerfScriptAction::CancelZedForget => {
                Some(ShellAction::ZedRemote(ZedRemoteAction::CancelForget))
            }
            PerfScriptAction::ConfirmZedForget => {
                Some(ShellAction::ZedRemote(ZedRemoteAction::ConfirmForget))
            }
            PerfScriptAction::RequestZedConflictRefresh => {
                let project_id = self
                    .state
                    .zed_remote
                    .workspace
                    .as_ref()
                    .and_then(|workspace| {
                        workspace.projects.iter().find(|project| {
                            project.source == ZedRemoteProjectSource::Recent
                                && project.label == "Performance stale fixture"
                        })
                    })
                    .map(|project| project.id.clone());
                if let Some(project_id) = project_id {
                    self.apply_action(
                        ctx,
                        ShellAction::ZedRemote(ZedRemoteAction::RequestForget(project_id)),
                    );
                    if let Some(confirmation) = self.state.zed_remote.pending_forget.as_mut() {
                        confirmation.expected_registry_revision =
                            ZedRemoteRegistryRevision::from_digest([0xa5; 32]);
                    }
                }
                None
            }
            PerfScriptAction::ConfirmZedConflictRefresh => {
                Some(ShellAction::ZedRemote(ZedRemoteAction::ConfirmForget))
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
        self.reduce_activation_responses();
        self.reduce_import_responses();
        self.reduce_environment_responses();
        self.reduce_session_responses();
        self.reduce_user_script_responses();
        self.reduce_context_responses();
        self.reduce_marketplace_responses();
        self.reduce_zed_remote_responses();
        self.refresh_pending_on_focus_regain(ctx);
        if let Some(perf) = &mut self.perf {
            perf.drive(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let scripted_action = self.perf.as_mut().and_then(|perf| perf.scripted_action(ui));
        if let Some(action) = scripted_action {
            self.apply_perf_action(ui.ctx(), action);
        }
        let model = self.view_model();
        for action in render_shell(
            ui,
            &model,
            ShellFeatureStates {
                provider: Some(&self.state.provider),
                provider_import: Some(&self.state.provider_import),
                environment: Some(&self.state.environment),
                context: Some(&self.state.context),
                marketplace: Some(&self.state.marketplace),
                sessions: Some(&self.state.sessions),
                user_scripts: Some(&self.state.user_scripts),
                zed_remote: Some(&self.state.zed_remote),
            },
        ) {
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
