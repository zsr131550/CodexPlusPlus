use std::sync::Arc;

use codex_plus_manager_service::OverviewSnapshot;

pub mod context;
pub mod environment;
pub mod import;
pub mod marketplace;
pub mod provider;
pub mod sessions;
pub mod user_scripts;
pub mod zed_remote;

use context::{ContextFailureKind, ContextViewState};
use environment::EnvironmentViewState;
use import::ImportViewState;
use provider::ProviderViewState;
use sessions::SessionViewState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    #[default]
    Overview,
    Providers,
    Environment,
    Sessions,
    Scripts,
    Context,
    ZedRemote,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverviewPhase {
    #[default]
    Idle,
    Loading,
    Ready,
    Refreshing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewFailureKind {
    LoadFailed,
    WorkerStopped,
}

#[derive(Debug, Default)]
pub struct OverviewViewState {
    pub phase: OverviewPhase,
    pub current_request_id: u64,
    pub snapshot: Option<Arc<OverviewSnapshot>>,
    pub error: Option<OverviewFailureKind>,
}

impl OverviewViewState {
    pub fn begin_refresh(&mut self) -> u64 {
        self.current_request_id = self
            .current_request_id
            .checked_add(1)
            .expect("overview request id overflow");
        self.phase = if self.snapshot.is_some() {
            OverviewPhase::Refreshing
        } else {
            OverviewPhase::Loading
        };
        self.current_request_id
    }

    pub fn apply_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<OverviewSnapshot>, OverviewFailureKind>,
    ) -> bool {
        if request_id != self.current_request_id {
            return false;
        }

        match result {
            Ok(snapshot) => {
                self.snapshot = Some(snapshot);
                self.error = None;
                self.phase = OverviewPhase::Ready;
            }
            Err(error) => {
                self.error = Some(error);
                self.phase = OverviewPhase::Error;
            }
        }
        true
    }
}

#[derive(Debug, Default)]
pub struct AppState {
    pub route: Route,
    pub overview: OverviewViewState,
    pub provider: ProviderViewState,
    pub provider_import: ImportViewState,
    pub environment: EnvironmentViewState,
    pub sessions: SessionViewState,
    pub context: ContextViewState,
    pub marketplace: marketplace::MarketplaceViewState,
    pub user_scripts: user_scripts::UserScriptViewState,
    pub zed_remote: zed_remote::ZedRemoteViewState,
}

impl AppState {
    pub fn apply_context_workspace_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<codex_plus_manager_service::ContextBundle>, ContextFailureKind>,
    ) -> bool {
        let result = reject_context_success_when_provider_is_dirty(&self.provider, result);
        let provider = result
            .as_ref()
            .ok()
            .map(|bundle| Arc::new(bundle.provider.clone()));
        let accepted = self.context.apply_workspace_response(request_id, result);
        if accepted && let Some(provider) = provider {
            self.install_context_provider_workspace(provider);
        }
        accepted
    }

    pub fn apply_context_stored_mutation_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<codex_plus_manager_service::ContextBundle>, ContextFailureKind>,
    ) -> bool {
        let result = reject_context_success_when_provider_is_dirty(&self.provider, result);
        let provider = result
            .as_ref()
            .ok()
            .map(|bundle| Arc::new(bundle.provider.clone()));
        let accepted = self
            .context
            .apply_stored_mutation_response(request_id, result);
        if accepted && let Some(provider) = provider {
            self.install_context_provider_workspace(provider);
        }
        accepted
    }

    pub fn apply_context_sync_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<codex_plus_manager_service::ContextSyncOutcome>, ContextFailureKind>,
    ) -> bool {
        let result = reject_context_success_when_provider_is_dirty(&self.provider, result);
        let provider = result
            .as_ref()
            .ok()
            .map(|outcome| Arc::new(outcome.bundle.provider.clone()));
        let accepted = self.context.apply_sync_response(request_id, result);
        if accepted && let Some(provider) = provider {
            self.install_context_provider_workspace(provider);
        }
        accepted
    }

    pub fn apply_imported_provider_workspace(
        &mut self,
        workspace: Arc<codex_plus_manager_service::ProviderWorkspace>,
    ) -> bool {
        if self.provider.is_dirty() {
            return false;
        }
        let request_id = self.provider.begin_load();
        self.provider.apply_load_response(request_id, Ok(workspace))
    }

    fn install_context_provider_workspace(
        &mut self,
        workspace: Arc<codex_plus_manager_service::ProviderWorkspace>,
    ) {
        let request_id = self.provider.begin_load();
        self.provider.apply_load_response(request_id, Ok(workspace));
    }
}

fn reject_context_success_when_provider_is_dirty<T>(
    provider: &ProviderViewState,
    result: Result<T, ContextFailureKind>,
) -> Result<T, ContextFailureKind> {
    if provider.is_dirty() && result.is_ok() {
        Err(ContextFailureKind::Service(
            codex_plus_manager_service::ContextToolsErrorKind::ProviderConflict,
        ))
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use codex_plus_manager_service::{
        LocatedResource, OverviewSnapshot, ResourcePresence, ShortcutSnapshot, UpdateCheckState,
    };

    use super::*;

    fn snapshot(version: &str) -> OverviewSnapshot {
        OverviewSnapshot {
            codex_app: LocatedResource {
                presence: ResourcePresence::Found,
                path: Some(PathBuf::from("C:/Codex")),
            },
            codex_version: Some(version.to_owned()),
            silent_shortcut: ShortcutSnapshot {
                installed: true,
                path: Some(PathBuf::from("C:/Desktop/Codex++.lnk")),
            },
            management_shortcut: ShortcutSnapshot {
                installed: true,
                path: Some(PathBuf::from("C:/Desktop/Codex++ Manager.lnk")),
            },
            latest_launch: None,
            current_version: "1.2.36".to_owned(),
            update_status: UpdateCheckState::NotChecked,
            settings_path: PathBuf::from("C:/state/settings.json"),
            logs_path: PathBuf::from("C:/state/diagnostic.log"),
        }
    }

    #[test]
    fn refresh_failure_keeps_last_good_snapshot_and_ignores_stale_results() {
        let first = Arc::new(snapshot("first"));
        let replacement = Arc::new(snapshot("replacement"));
        let mut state = OverviewViewState::default();

        let first_id = state.begin_refresh();
        assert_eq!(state.phase, OverviewPhase::Loading);
        assert!(state.apply_response(first_id, Ok(Arc::clone(&first))));
        assert_eq!(state.phase, OverviewPhase::Ready);

        let refresh_id = state.begin_refresh();
        assert_eq!(state.phase, OverviewPhase::Refreshing);
        assert!(Arc::ptr_eq(state.snapshot.as_ref().unwrap(), &first));

        assert!(!state.apply_response(first_id, Ok(replacement)));
        assert!(Arc::ptr_eq(state.snapshot.as_ref().unwrap(), &first));

        assert!(state.apply_response(refresh_id, Err(OverviewFailureKind::LoadFailed)));
        assert_eq!(state.phase, OverviewPhase::Error);
        assert_eq!(state.error, Some(OverviewFailureKind::LoadFailed));
        assert!(Arc::ptr_eq(state.snapshot.as_ref().unwrap(), &first));
    }

    #[test]
    fn initial_failure_has_no_snapshot_and_current_success_clears_error() {
        let mut state = OverviewViewState::default();

        let failed_id = state.begin_refresh();
        assert!(state.apply_response(failed_id, Err(OverviewFailureKind::WorkerStopped)));
        assert_eq!(state.phase, OverviewPhase::Error);
        assert_eq!(state.snapshot, None);
        assert_eq!(state.error, Some(OverviewFailureKind::WorkerStopped));

        let retry_id = state.begin_refresh();
        assert_eq!(state.phase, OverviewPhase::Loading);
        assert_eq!(state.error, Some(OverviewFailureKind::WorkerStopped));

        assert!(state.apply_response(retry_id, Ok(Arc::new(snapshot("recovered")))));
        assert_eq!(state.phase, OverviewPhase::Ready);
        assert_eq!(state.error, None);
        assert_eq!(
            state.snapshot.as_ref().unwrap().codex_version.as_deref(),
            Some("recovered")
        );
    }

    #[test]
    fn app_state_defaults_to_overview_and_supports_native_routes() {
        let mut app = AppState::default();
        assert_eq!(app.route, Route::Overview);

        app.route = Route::Environment;
        assert_eq!(app.route, Route::Environment);
        app.route = Route::About;
        assert_eq!(app.route, Route::About);
    }
}
