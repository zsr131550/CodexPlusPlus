use std::fmt;
use std::sync::Arc;

use codex_plus_manager_service::{
    DeleteUserScript, InstallMarketScript, ScriptIntegrity, ScriptMarketSummary,
    ScriptMarketWorkspace, SetUserScriptEnabled, SetUserScriptsEnabled, UserScriptErrorKind,
    UserScriptMutationOutcome, UserScriptOrigin, UserScriptSummary, UserScriptWorkspace,
};

use super::provider::OperationPhase;

pub const USER_SCRIPT_PAGE_SIZE: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScriptsTab {
    #[default]
    Market,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarketScriptFilter {
    #[default]
    All,
    Available,
    Installed,
    Updates,
    Verified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LocalScriptFilter {
    #[default]
    All,
    Enabled,
    Disabled,
    Builtin,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserScriptFailureKind {
    Service(UserScriptErrorKind),
    WorkerStopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserScriptMutationKind {
    Install,
    Update,
    SetGlobalEnabled,
    SetScriptEnabled,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallConfirmation {
    pub script_id: String,
    pub name: String,
    pub version: String,
    pub integrity: ScriptIntegrity,
    pub source_host: String,
    pub update: bool,
    acknowledge_unverified: bool,
    expected_local_revision: codex_plus_manager_service::UserScriptRevision,
    expected_market_revision: codex_plus_manager_service::ScriptMarketRevision,
}

impl InstallConfirmation {
    pub fn acknowledge_unverified(&self) -> bool {
        self.acknowledge_unverified
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteConfirmation {
    pub key: String,
    pub name: String,
    expected_revision: codex_plus_manager_service::UserScriptRevision,
}

pub struct UserScriptViewState {
    pub tab: ScriptsTab,
    pub market_query: String,
    pub market_filter: MarketScriptFilter,
    pub market_page: usize,
    pub local_query: String,
    pub local_filter: LocalScriptFilter,
    pub local_page: usize,

    pub local_phase: OperationPhase,
    pub current_local_request_id: u64,
    pub local_error: Option<UserScriptFailureKind>,
    pub local: Option<Arc<UserScriptWorkspace>>,

    pub market_phase: OperationPhase,
    pub current_market_request_id: u64,
    pub market_error: Option<UserScriptFailureKind>,
    pub market: Option<Arc<ScriptMarketWorkspace>>,

    pub mutation_phase: OperationPhase,
    pub current_mutation_request_id: u64,
    pub mutation_kind: Option<UserScriptMutationKind>,
    pub mutation_error: Option<UserScriptFailureKind>,
    pub mutation_outcome: Option<Arc<UserScriptMutationOutcome>>,

    install_confirmation: Option<InstallConfirmation>,
    delete_confirmation: Option<DeleteConfirmation>,
    worker_stopped: bool,
}

impl Default for UserScriptViewState {
    fn default() -> Self {
        Self {
            tab: ScriptsTab::Market,
            market_query: String::new(),
            market_filter: MarketScriptFilter::All,
            market_page: 0,
            local_query: String::new(),
            local_filter: LocalScriptFilter::All,
            local_page: 0,
            local_phase: OperationPhase::Idle,
            current_local_request_id: 0,
            local_error: None,
            local: None,
            market_phase: OperationPhase::Idle,
            current_market_request_id: 0,
            market_error: None,
            market: None,
            mutation_phase: OperationPhase::Idle,
            current_mutation_request_id: 0,
            mutation_kind: None,
            mutation_error: None,
            mutation_outcome: None,
            install_confirmation: None,
            delete_confirmation: None,
            worker_stopped: false,
        }
    }
}

impl fmt::Debug for UserScriptViewState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserScriptViewState")
            .field("tab", &self.tab)
            .field("market_phase", &self.market_phase)
            .field(
                "market_count",
                &self
                    .market
                    .as_ref()
                    .map_or(0, |market| market.entries.len()),
            )
            .field("local_phase", &self.local_phase)
            .field(
                "local_count",
                &self.local.as_ref().map_or(0, |local| local.scripts.len()),
            )
            .field("mutation_phase", &self.mutation_phase)
            .field("mutation_kind", &self.mutation_kind)
            .field(
                "has_install_confirmation",
                &self.install_confirmation.is_some(),
            )
            .field(
                "has_delete_confirmation",
                &self.delete_confirmation.is_some(),
            )
            .field("worker_stopped", &self.worker_stopped)
            .finish_non_exhaustive()
    }
}

impl UserScriptViewState {
    pub fn begin_local_refresh(&mut self) -> u64 {
        self.current_local_request_id = next_id(self.current_local_request_id, "local scripts");
        self.local_phase = OperationPhase::Running;
        self.local_error = None;
        self.current_local_request_id
    }

    pub fn apply_local_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<UserScriptWorkspace>, UserScriptFailureKind>,
    ) -> bool {
        if request_id != self.current_local_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.local = Some(workspace);
                self.local_phase = OperationPhase::Ready;
                self.local_error = None;
                self.clamp_local_page();
            }
            Err(error) => {
                self.local_phase = OperationPhase::Error;
                self.local_error = Some(error);
            }
        }
        true
    }

    pub fn begin_market_refresh(&mut self) -> u64 {
        self.current_market_request_id = next_id(self.current_market_request_id, "script market");
        self.market_phase = OperationPhase::Running;
        self.market_error = None;
        self.current_market_request_id
    }

    pub fn apply_market_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ScriptMarketWorkspace>, UserScriptFailureKind>,
    ) -> bool {
        if request_id != self.current_market_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.market = Some(workspace);
                self.market_phase = OperationPhase::Ready;
                self.market_error = None;
                self.clamp_market_page();
            }
            Err(error) => {
                self.market_phase = OperationPhase::Error;
                self.market_error = Some(error);
            }
        }
        true
    }

    pub fn set_tab(&mut self, tab: ScriptsTab) -> bool {
        let changed = self.tab != tab;
        self.tab = tab;
        changed
    }

    pub fn set_market_query(&mut self, query: String) {
        self.market_query = query;
        self.market_page = 0;
    }

    pub fn set_market_filter(&mut self, filter: MarketScriptFilter) {
        self.market_filter = filter;
        self.market_page = 0;
    }

    pub fn set_market_page(&mut self, page: usize) {
        self.market_page = page.min(self.market_page_count().saturating_sub(1));
    }

    pub fn set_local_query(&mut self, query: String) {
        self.local_query = query;
        self.local_page = 0;
    }

    pub fn set_local_filter(&mut self, filter: LocalScriptFilter) {
        self.local_filter = filter;
        self.local_page = 0;
    }

    pub fn set_local_page(&mut self, page: usize) {
        self.local_page = page.min(self.local_page_count().saturating_sub(1));
    }

    pub fn filtered_market_entries(&self) -> Vec<&ScriptMarketSummary> {
        let query = self.market_query.trim().to_ascii_lowercase();
        self.market
            .as_ref()
            .into_iter()
            .flat_map(|market| &market.entries)
            .filter(|entry| {
                let installed = self.installed_version(&entry.id).is_some();
                let update = self.update_available(entry);
                match self.market_filter {
                    MarketScriptFilter::All => true,
                    MarketScriptFilter::Available => !installed,
                    MarketScriptFilter::Installed => installed,
                    MarketScriptFilter::Updates => update,
                    MarketScriptFilter::Verified => entry.integrity == ScriptIntegrity::Verified,
                }
            })
            .filter(|entry| {
                query.is_empty()
                    || entry.id.to_ascii_lowercase().contains(&query)
                    || entry.name.to_ascii_lowercase().contains(&query)
                    || entry.description.to_ascii_lowercase().contains(&query)
                    || entry.author.to_ascii_lowercase().contains(&query)
                    || entry
                        .tags
                        .iter()
                        .any(|tag| tag.to_ascii_lowercase().contains(&query))
            })
            .collect()
    }

    pub fn market_page_entries(&self) -> Vec<&ScriptMarketSummary> {
        self.filtered_market_entries()
            .into_iter()
            .skip(self.market_page * USER_SCRIPT_PAGE_SIZE)
            .take(USER_SCRIPT_PAGE_SIZE)
            .collect()
    }

    pub fn market_page_count(&self) -> usize {
        self.filtered_market_entries()
            .len()
            .div_ceil(USER_SCRIPT_PAGE_SIZE)
            .max(1)
    }

    pub fn filtered_local_scripts(&self) -> Vec<&UserScriptSummary> {
        let query = self.local_query.trim().to_ascii_lowercase();
        self.local
            .as_ref()
            .into_iter()
            .flat_map(|local| &local.scripts)
            .filter(|script| match self.local_filter {
                LocalScriptFilter::All => true,
                LocalScriptFilter::Enabled => script.enabled,
                LocalScriptFilter::Disabled => !script.enabled,
                LocalScriptFilter::Builtin => script.origin == UserScriptOrigin::Builtin,
                LocalScriptFilter::User => script.origin == UserScriptOrigin::User,
            })
            .filter(|script| {
                query.is_empty()
                    || script.key.to_ascii_lowercase().contains(&query)
                    || script.name.to_ascii_lowercase().contains(&query)
                    || script
                        .market_id
                        .as_ref()
                        .is_some_and(|id| id.to_ascii_lowercase().contains(&query))
            })
            .collect()
    }

    pub fn local_page_scripts(&self) -> Vec<&UserScriptSummary> {
        self.filtered_local_scripts()
            .into_iter()
            .skip(self.local_page * USER_SCRIPT_PAGE_SIZE)
            .take(USER_SCRIPT_PAGE_SIZE)
            .collect()
    }

    pub fn local_page_count(&self) -> usize {
        self.filtered_local_scripts()
            .len()
            .div_ceil(USER_SCRIPT_PAGE_SIZE)
            .max(1)
    }

    pub fn installed_version(&self, market_id: &str) -> Option<&str> {
        self.local
            .as_ref()?
            .scripts
            .iter()
            .find(|script| script.market_id.as_deref() == Some(market_id))
            .and_then(|script| script.version.as_deref())
    }

    pub fn update_available(&self, entry: &ScriptMarketSummary) -> bool {
        self.installed_version(&entry.id)
            .is_some_and(|version| version != entry.version)
    }

    pub fn request_install(&mut self, script_id: &str) -> bool {
        if !self.mutations_enabled()
            || self.install_confirmation.is_some()
            || self.delete_confirmation.is_some()
        {
            return false;
        }
        let Some(local) = self.local.as_ref() else {
            return false;
        };
        let Some(market) = self.market.as_ref() else {
            return false;
        };
        let mut matches = market.entries.iter().filter(|entry| entry.id == script_id);
        let Some(entry) = matches.next() else {
            return false;
        };
        if matches.next().is_some() {
            return false;
        }
        self.install_confirmation = Some(InstallConfirmation {
            script_id: entry.id.clone(),
            name: entry.name.clone(),
            version: entry.version.clone(),
            integrity: entry.integrity,
            source_host: entry.source_host.clone(),
            update: self.installed_version(&entry.id).is_some(),
            acknowledge_unverified: false,
            expected_local_revision: local.revision.clone(),
            expected_market_revision: market.revision.clone(),
        });
        self.mutation_error = None;
        true
    }

    pub fn install_confirmation(&self) -> Option<&InstallConfirmation> {
        self.install_confirmation.as_ref()
    }

    pub fn set_unverified_acknowledgement(&mut self, acknowledged: bool) -> bool {
        let Some(confirmation) = self.install_confirmation.as_mut() else {
            return false;
        };
        if confirmation.integrity != ScriptIntegrity::Unverified {
            return false;
        }
        let changed = confirmation.acknowledge_unverified != acknowledged;
        confirmation.acknowledge_unverified = acknowledged;
        changed
    }

    pub fn cancel_install(&mut self) -> bool {
        if self.mutation_phase == OperationPhase::Running {
            return false;
        }
        self.install_confirmation.take().is_some()
    }

    pub fn confirm_install(&mut self) -> Option<(u64, InstallMarketScript)> {
        if !self.mutations_enabled() {
            return None;
        }
        let confirmation = self.install_confirmation.as_ref()?;
        if confirmation.integrity == ScriptIntegrity::Invalid
            || (confirmation.integrity == ScriptIntegrity::Unverified
                && !confirmation.acknowledge_unverified)
        {
            return None;
        }
        let confirmation = self.install_confirmation.take()?;
        let mutation_kind = if confirmation.update {
            UserScriptMutationKind::Update
        } else {
            UserScriptMutationKind::Install
        };
        let request_id = self.begin_mutation(mutation_kind);
        Some((
            request_id,
            InstallMarketScript {
                expected_local_revision: confirmation.expected_local_revision,
                expected_market_revision: confirmation.expected_market_revision,
                script_id: confirmation.script_id.clone(),
                confirmed_script_id: confirmation.script_id,
                confirmed_version: confirmation.version,
                acknowledge_unverified: confirmation.acknowledge_unverified,
            },
        ))
    }

    pub fn request_global_enabled(
        &mut self,
        enabled: bool,
    ) -> Option<(u64, SetUserScriptsEnabled)> {
        if !self.mutations_enabled()
            || self.install_confirmation.is_some()
            || self.delete_confirmation.is_some()
        {
            return None;
        }
        let expected_revision = self.local.as_ref()?.revision.clone();
        let request_id = self.begin_mutation(UserScriptMutationKind::SetGlobalEnabled);
        Some((
            request_id,
            SetUserScriptsEnabled {
                expected_revision,
                enabled,
            },
        ))
    }

    pub fn request_script_enabled(
        &mut self,
        key: &str,
        enabled: bool,
    ) -> Option<(u64, SetUserScriptEnabled)> {
        if !self.mutations_enabled()
            || self.install_confirmation.is_some()
            || self.delete_confirmation.is_some()
        {
            return None;
        }
        let local = self.local.as_ref()?;
        if !local.scripts.iter().any(|script| script.key == key) {
            return None;
        }
        let expected_revision = local.revision.clone();
        let request_id = self.begin_mutation(UserScriptMutationKind::SetScriptEnabled);
        Some((
            request_id,
            SetUserScriptEnabled {
                expected_revision,
                key: key.to_string(),
                enabled,
            },
        ))
    }

    pub fn request_delete(&mut self, key: &str) -> bool {
        if !self.mutations_enabled()
            || self.install_confirmation.is_some()
            || self.delete_confirmation.is_some()
        {
            return false;
        }
        let Some(local) = self.local.as_ref() else {
            return false;
        };
        let Some(script) = local.scripts.iter().find(|script| script.key == key) else {
            return false;
        };
        if script.origin != UserScriptOrigin::User {
            return false;
        }
        self.delete_confirmation = Some(DeleteConfirmation {
            key: script.key.clone(),
            name: script.name.clone(),
            expected_revision: local.revision.clone(),
        });
        self.mutation_error = None;
        true
    }

    pub fn delete_confirmation(&self) -> Option<&DeleteConfirmation> {
        self.delete_confirmation.as_ref()
    }

    pub fn cancel_delete(&mut self) -> bool {
        if self.mutation_phase == OperationPhase::Running {
            return false;
        }
        self.delete_confirmation.take().is_some()
    }

    pub fn confirm_delete(&mut self) -> Option<(u64, DeleteUserScript)> {
        if !self.mutations_enabled() {
            return None;
        }
        let confirmation = self.delete_confirmation.take()?;
        let request_id = self.begin_mutation(UserScriptMutationKind::Delete);
        Some((
            request_id,
            DeleteUserScript {
                expected_revision: confirmation.expected_revision,
                key: confirmation.key.clone(),
                confirmed_key: confirmation.key,
            },
        ))
    }

    pub fn apply_mutation_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<UserScriptMutationOutcome>, UserScriptFailureKind>,
    ) -> bool {
        if request_id != self.current_mutation_request_id {
            return false;
        }
        match result {
            Ok(outcome) => {
                self.current_local_request_id =
                    next_id(self.current_local_request_id, "local scripts invalidation");
                self.local = Some(Arc::new(outcome.workspace.clone()));
                self.local_phase = OperationPhase::Ready;
                self.local_error = None;
                self.mutation_phase = OperationPhase::Ready;
                self.mutation_error = None;
                self.mutation_outcome = Some(outcome);
                self.clamp_local_page();
                self.clamp_market_page();
            }
            Err(error) => {
                self.mutation_phase = OperationPhase::Error;
                self.mutation_error = Some(error);
            }
        }
        true
    }

    pub fn mutations_enabled(&self) -> bool {
        !self.worker_stopped && self.mutation_phase != OperationPhase::Running
    }

    pub fn mark_worker_stopped(&mut self) {
        self.worker_stopped = true;
        self.install_confirmation = None;
        self.delete_confirmation = None;
        if self.local_phase == OperationPhase::Running {
            self.local_phase = OperationPhase::Error;
            self.local_error = Some(UserScriptFailureKind::WorkerStopped);
        }
        if self.market_phase == OperationPhase::Running {
            self.market_phase = OperationPhase::Error;
            self.market_error = Some(UserScriptFailureKind::WorkerStopped);
        }
        if self.mutation_phase == OperationPhase::Running {
            self.mutation_phase = OperationPhase::Error;
            self.mutation_error = Some(UserScriptFailureKind::WorkerStopped);
        }
    }

    fn begin_mutation(&mut self, kind: UserScriptMutationKind) -> u64 {
        self.current_mutation_request_id =
            next_id(self.current_mutation_request_id, "user script mutation");
        self.mutation_phase = OperationPhase::Running;
        self.mutation_kind = Some(kind);
        self.mutation_error = None;
        self.mutation_outcome = None;
        self.current_mutation_request_id
    }

    fn clamp_local_page(&mut self) {
        self.local_page = self
            .local_page
            .min(self.local_page_count().saturating_sub(1));
    }

    fn clamp_market_page(&mut self) {
        self.market_page = self
            .market_page
            .min(self.market_page_count().saturating_sub(1));
    }
}

fn next_id(current: u64, label: &str) -> u64 {
    current
        .checked_add(1)
        .unwrap_or_else(|| panic!("{label} request id overflow"))
}
