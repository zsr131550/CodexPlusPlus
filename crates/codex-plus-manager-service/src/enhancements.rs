use std::fmt;
use std::sync::Arc;

use codex_plus_core::settings::{BackendSettings, LaunchMode};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::revision_ledger::{RevisionLedger, RevisionScope, RevisionTicket, scoped_fingerprint};

const ENHANCEMENTS_FINGERPRINT_DOMAIN: &[u8] = b"codex-plus-manager/enhancements/v1";

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnhancementRevision(RevisionTicket);

impl fmt::Debug for EnhancementRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EnhancementRevision([opaque])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EnhancementSettings {
    pub enabled: bool,
    pub computer_use_guard: bool,
    pub launch_mode: LaunchMode,
    pub plugin_marketplace_unlock: bool,
    pub plugin_auto_expand: bool,
    pub model_whitelist_unlock: bool,
    pub service_tier_controls: bool,
    pub session_delete: bool,
    pub markdown_export: bool,
    pub paste_fix: bool,
    pub project_move: bool,
    pub thread_id_badge: bool,
    pub conversation_view: bool,
    pub thread_scroll_restore: bool,
    pub pet_real_mouse_look: bool,
    pub force_chinese_locale: bool,
    pub fast_startup: bool,
    pub native_menu_placement: bool,
    pub native_menu_localization: bool,
    pub zed_remote_open: bool,
    pub upstream_worktree_create: bool,
}

impl EnhancementSettings {
    fn from_backend(settings: &BackendSettings) -> Self {
        Self {
            enabled: settings.enhancements_enabled,
            computer_use_guard: settings.computer_use_guard_enabled,
            launch_mode: settings.launch_mode,
            plugin_marketplace_unlock: settings.codex_app_plugin_marketplace_unlock,
            plugin_auto_expand: settings.codex_app_plugin_auto_expand,
            model_whitelist_unlock: settings.codex_app_model_whitelist_unlock,
            service_tier_controls: settings.codex_app_service_tier_controls,
            session_delete: settings.codex_app_session_delete,
            markdown_export: settings.codex_app_markdown_export,
            paste_fix: settings.codex_app_paste_fix,
            project_move: settings.codex_app_project_move,
            thread_id_badge: settings.codex_app_thread_id_badge,
            conversation_view: settings.codex_app_conversation_view,
            thread_scroll_restore: settings.codex_app_thread_scroll_restore,
            pet_real_mouse_look: settings.codex_app_pet_real_mouse_look,
            force_chinese_locale: settings.codex_app_force_chinese_locale,
            fast_startup: settings.codex_app_fast_startup,
            native_menu_placement: settings.codex_app_native_menu_placement,
            native_menu_localization: settings.codex_app_native_menu_localization,
            zed_remote_open: settings.codex_app_zed_remote_open,
            upstream_worktree_create: settings.codex_app_upstream_worktree_create,
        }
    }
}

impl Default for EnhancementSettings {
    fn default() -> Self {
        Self::from_backend(&BackendSettings::default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnhancementWorkspace {
    pub revision: EnhancementRevision,
    pub settings: EnhancementSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveEnhancements {
    pub expected_revision: EnhancementRevision,
    pub settings: EnhancementSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetEnhancements {
    pub expected_revision: EnhancementRevision,
    pub confirmed: bool,
}

pub trait EnhancementSettingsEnvironment: Send + Sync + 'static {
    fn load_enhancement_settings(&self) -> anyhow::Result<BackendSettings>;
    fn update_enhancement_settings_if<F>(
        &self,
        payload: Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnhancementErrorKind {
    SettingsReadFailed,
    SettingsWriteFailed,
    SettingsConflict,
    InvalidRevision,
    ConfirmationRequired,
    WorkerStopped,
}

#[derive(Clone, PartialEq, Eq)]
pub struct EnhancementError {
    kind: EnhancementErrorKind,
    refreshed_workspace: Option<Box<EnhancementWorkspace>>,
}

impl EnhancementError {
    pub fn new(kind: EnhancementErrorKind) -> Self {
        Self {
            kind,
            refreshed_workspace: None,
        }
    }

    pub fn kind(&self) -> EnhancementErrorKind {
        self.kind
    }

    pub fn refreshed_workspace(&self) -> Option<&EnhancementWorkspace> {
        self.refreshed_workspace.as_deref()
    }

    fn with_refreshed_workspace(mut self, workspace: Option<EnhancementWorkspace>) -> Self {
        self.refreshed_workspace = workspace.map(Box::new);
        self
    }

    fn detail(&self) -> &'static str {
        match self.kind {
            EnhancementErrorKind::SettingsReadFailed => "enhancement settings read failed",
            EnhancementErrorKind::SettingsWriteFailed => "enhancement settings write failed",
            EnhancementErrorKind::SettingsConflict => "enhancement settings changed on disk",
            EnhancementErrorKind::InvalidRevision => "enhancement settings revision is invalid",
            EnhancementErrorKind::ConfirmationRequired => {
                "enhancement settings reset confirmation is required"
            }
            EnhancementErrorKind::WorkerStopped => "enhancement settings worker stopped",
        }
    }
}

impl fmt::Debug for EnhancementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnhancementError")
            .field("kind", &self.kind)
            .field(
                "has_refreshed_workspace",
                &self.refreshed_workspace.is_some(),
            )
            .finish()
    }
}

impl fmt::Display for EnhancementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for EnhancementError {}

#[derive(Clone)]
pub struct EnhancementSettingsService<E> {
    environment: E,
    revisions: Arc<RevisionLedger>,
}

impl<E> EnhancementSettingsService<E> {
    pub fn new(environment: E) -> Self {
        Self {
            environment,
            revisions: Arc::new(RevisionLedger::default()),
        }
    }
}

impl<E: EnhancementSettingsEnvironment> EnhancementSettingsService<E> {
    pub fn load(&self) -> Result<EnhancementWorkspace, EnhancementError> {
        let settings = self
            .environment
            .load_enhancement_settings()
            .map_err(|_| EnhancementError::new(EnhancementErrorKind::SettingsReadFailed))?;
        Ok(self.workspace_from_settings(&settings))
    }

    pub fn save(
        &self,
        request: SaveEnhancements,
    ) -> Result<EnhancementWorkspace, EnhancementError> {
        let expected = self
            .revisions
            .take(request.expected_revision.0, RevisionScope::Enhancements)
            .ok_or_else(|| EnhancementError::new(EnhancementErrorKind::InvalidRevision))?;
        let payload = enhancement_payload(&request.settings);
        let updated = self
            .environment
            .update_enhancement_settings_if(payload, move |current| {
                enhancement_fingerprint(current) == expected
            })
            .map_err(|_| EnhancementError::new(EnhancementErrorKind::SettingsWriteFailed))?;
        if updated.is_none() {
            return Err(
                EnhancementError::new(EnhancementErrorKind::SettingsConflict)
                    .with_refreshed_workspace(self.load().ok()),
            );
        }
        self.load()
    }

    pub fn reset(
        &self,
        request: ResetEnhancements,
    ) -> Result<EnhancementWorkspace, EnhancementError> {
        if !request.confirmed {
            return Err(EnhancementError::new(
                EnhancementErrorKind::ConfirmationRequired,
            ));
        }
        self.save(SaveEnhancements {
            expected_revision: request.expected_revision,
            settings: EnhancementSettings::default(),
        })
    }

    fn workspace_from_settings(&self, settings: &BackendSettings) -> EnhancementWorkspace {
        EnhancementWorkspace {
            revision: EnhancementRevision(self.revisions.issue(
                RevisionScope::Enhancements,
                enhancement_fingerprint(settings),
            )),
            settings: EnhancementSettings::from_backend(settings),
        }
    }
}

pub trait EnhancementSettingsSource: Send + Sync + 'static {
    fn load(&self) -> Result<EnhancementWorkspace, EnhancementError>;
    fn save(&self, request: SaveEnhancements) -> Result<EnhancementWorkspace, EnhancementError>;
    fn reset(&self, request: ResetEnhancements) -> Result<EnhancementWorkspace, EnhancementError>;
}

impl<E: EnhancementSettingsEnvironment> EnhancementSettingsSource
    for EnhancementSettingsService<E>
{
    fn load(&self) -> Result<EnhancementWorkspace, EnhancementError> {
        EnhancementSettingsService::load(self)
    }

    fn save(&self, request: SaveEnhancements) -> Result<EnhancementWorkspace, EnhancementError> {
        EnhancementSettingsService::save(self, request)
    }

    fn reset(&self, request: ResetEnhancements) -> Result<EnhancementWorkspace, EnhancementError> {
        EnhancementSettingsService::reset(self, request)
    }
}

fn enhancement_fingerprint(settings: &BackendSettings) -> [u8; 32] {
    scoped_fingerprint(
        ENHANCEMENTS_FINGERPRINT_DOMAIN,
        &EnhancementSettings::from_backend(settings),
    )
}

fn enhancement_payload(settings: &EnhancementSettings) -> Value {
    let mut payload = Map::new();
    payload.insert(
        "enhancementsEnabled".to_owned(),
        Value::Bool(settings.enabled),
    );
    payload.insert(
        "computerUseGuardEnabled".to_owned(),
        Value::Bool(settings.computer_use_guard),
    );
    payload.insert(
        "launchMode".to_owned(),
        serde_json::to_value(settings.launch_mode).expect("LaunchMode serializes"),
    );
    for (key, enabled) in [
        (
            "codexAppPluginMarketplaceUnlock",
            settings.plugin_marketplace_unlock,
        ),
        ("codexAppPluginAutoExpand", settings.plugin_auto_expand),
        (
            "codexAppModelWhitelistUnlock",
            settings.model_whitelist_unlock,
        ),
        (
            "codexAppServiceTierControls",
            settings.service_tier_controls,
        ),
        ("codexAppSessionDelete", settings.session_delete),
        ("codexAppMarkdownExport", settings.markdown_export),
        ("codexAppPasteFix", settings.paste_fix),
        ("codexAppProjectMove", settings.project_move),
        ("codexAppThreadIdBadge", settings.thread_id_badge),
        ("codexAppConversationView", settings.conversation_view),
        (
            "codexAppThreadScrollRestore",
            settings.thread_scroll_restore,
        ),
        ("codexAppPetRealMouseLook", settings.pet_real_mouse_look),
        ("codexAppForceChineseLocale", settings.force_chinese_locale),
        ("codexAppFastStartup", settings.fast_startup),
        (
            "codexAppNativeMenuPlacement",
            settings.native_menu_placement,
        ),
        (
            "codexAppNativeMenuLocalization",
            settings.native_menu_localization,
        ),
        ("codexAppZedRemoteOpen", settings.zed_remote_open),
        (
            "codexAppUpstreamWorktreeCreate",
            settings.upstream_worktree_create,
        ),
    ] {
        payload.insert(key.to_owned(), Value::Bool(enabled));
    }
    Value::Object(payload)
}
