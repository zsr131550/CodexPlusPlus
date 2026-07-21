use std::fmt;
use std::sync::Arc;

use codex_plus_manager_service::{
    ContextBundle, ContextEntryDraft, ContextEntryKey, ContextKind, ContextSyncGuard,
    ContextSyncOutcome, ContextSyncPreview, ContextSyncScope, ContextToolsErrorKind,
    DeleteContextEntry, LoadContextEntryDraft, PreviewContextSync, SaveContextEntry,
    SaveContextEntryMode, SetContextEntryEnabled, SyncContextToLive,
};

use super::provider::OperationPhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextFailureKind {
    Service(ContextToolsErrorKind),
    WorkerStopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextEditorMode {
    Create,
    Edit,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ContextEditorState {
    pub mode: ContextEditorMode,
    pub kind: ContextKind,
    pub id: String,
    pub toml_body: String,
    pub toml_revealed: bool,
    pub expected_provider_revision: codex_plus_manager_service::ProviderRevision,
}

impl fmt::Debug for ContextEditorState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextEditorState")
            .field("mode", &self.mode)
            .field("kind", &self.kind)
            .field("id", &self.id)
            .field("body_present", &!self.toml_body.is_empty())
            .field("body_length", &self.toml_body.len())
            .field("toml_revealed", &self.toml_revealed)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoredMutationKind {
    Save,
    Toggle,
    Delete,
}

pub struct ContextViewState {
    pub workspace_phase: OperationPhase,
    pub current_workspace_request_id: u64,
    pub workspace_error: Option<ContextFailureKind>,
    pub bundle: Option<Arc<ContextBundle>>,
    pub selected_kind: ContextKind,

    pub editor: Option<ContextEditorState>,
    pub draft_phase: OperationPhase,
    pub current_draft_request_id: u64,
    pub draft_error: Option<ContextFailureKind>,
    draft_target: Option<ContextEntryKey>,

    pub mutation_phase: OperationPhase,
    pub current_mutation_request_id: u64,
    pub mutation_error: Option<ContextFailureKind>,
    pending_mutation: Option<StoredMutationKind>,

    pub delete_confirmation: Option<ContextEntryKey>,

    pub preview_phase: OperationPhase,
    pub current_preview_request_id: u64,
    pub preview_error: Option<ContextFailureKind>,
    pub sync_preview: Option<Arc<ContextSyncPreview>>,

    pub sync_phase: OperationPhase,
    pub current_sync_request_id: u64,
    pub sync_error: Option<ContextFailureKind>,
    pub sync_outcome: Option<Arc<ContextSyncOutcome>>,
}

impl Default for ContextViewState {
    fn default() -> Self {
        Self {
            workspace_phase: OperationPhase::Idle,
            current_workspace_request_id: 0,
            workspace_error: None,
            bundle: None,
            selected_kind: ContextKind::Mcp,
            editor: None,
            draft_phase: OperationPhase::Idle,
            current_draft_request_id: 0,
            draft_error: None,
            draft_target: None,
            mutation_phase: OperationPhase::Idle,
            current_mutation_request_id: 0,
            mutation_error: None,
            pending_mutation: None,
            delete_confirmation: None,
            preview_phase: OperationPhase::Idle,
            current_preview_request_id: 0,
            preview_error: None,
            sync_preview: None,
            sync_phase: OperationPhase::Idle,
            current_sync_request_id: 0,
            sync_error: None,
            sync_outcome: None,
        }
    }
}

impl fmt::Debug for ContextViewState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextViewState")
            .field("workspace_phase", &self.workspace_phase)
            .field("has_bundle", &self.bundle.is_some())
            .field("selected_kind", &self.selected_kind)
            .field("editor", &self.editor)
            .field("draft_phase", &self.draft_phase)
            .field("mutation_phase", &self.mutation_phase)
            .field(
                "has_delete_confirmation",
                &self.delete_confirmation.is_some(),
            )
            .field("preview_phase", &self.preview_phase)
            .field("has_preview", &self.sync_preview.is_some())
            .field("sync_phase", &self.sync_phase)
            .field("has_sync_outcome", &self.sync_outcome.is_some())
            .finish_non_exhaustive()
    }
}

impl ContextViewState {
    pub fn begin_workspace_refresh(&mut self) -> u64 {
        self.current_workspace_request_id = next_id(
            self.current_workspace_request_id,
            "context workspace refresh",
        );
        self.workspace_phase = OperationPhase::Running;
        self.workspace_error = None;
        self.current_workspace_request_id
    }

    pub fn apply_workspace_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ContextBundle>, ContextFailureKind>,
    ) -> bool {
        if request_id != self.current_workspace_request_id {
            return false;
        }
        match result {
            Ok(bundle) => {
                self.bundle = Some(bundle);
                self.workspace_phase = OperationPhase::Ready;
                self.workspace_error = None;
            }
            Err(error) => {
                self.workspace_phase = OperationPhase::Error;
                self.workspace_error = Some(error);
            }
        }
        true
    }

    pub fn open_create(&mut self, kind: ContextKind) -> bool {
        let Some(bundle) = self.bundle.as_ref() else {
            return false;
        };
        if self.has_running_mutation() {
            return false;
        }
        self.editor = Some(ContextEditorState {
            mode: ContextEditorMode::Create,
            kind,
            id: String::new(),
            toml_body: String::new(),
            toml_revealed: false,
            expected_provider_revision: bundle.context.provider_revision.clone(),
        });
        self.draft_phase = OperationPhase::Idle;
        self.draft_error = None;
        self.draft_target = None;
        true
    }

    pub fn begin_edit(&mut self, key: ContextEntryKey) -> Option<(u64, LoadContextEntryDraft)> {
        if self.editor.is_some() || self.has_running_mutation() || !self.contains_key(&key) {
            return None;
        }
        let bundle = self.bundle.as_ref()?;
        self.current_draft_request_id = next_id(self.current_draft_request_id, "context draft");
        self.draft_phase = OperationPhase::Running;
        self.draft_error = None;
        self.draft_target = Some(key.clone());
        Some((
            self.current_draft_request_id,
            LoadContextEntryDraft {
                expected_provider_revision: bundle.context.provider_revision.clone(),
                key,
            },
        ))
    }

    pub fn apply_draft_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ContextEntryDraft>, ContextFailureKind>,
    ) -> bool {
        if request_id != self.current_draft_request_id {
            return false;
        }
        match result {
            Ok(draft) => {
                let matches = self.draft_target.as_ref() == Some(&draft.key)
                    && self.bundle.as_ref().is_some_and(|bundle| {
                        bundle.context.provider_revision == draft.provider_revision
                    });
                if !matches {
                    self.draft_phase = OperationPhase::Error;
                    self.draft_error = Some(ContextFailureKind::Service(
                        ContextToolsErrorKind::ProviderConflict,
                    ));
                    self.draft_target = None;
                    return false;
                }
                self.editor = Some(ContextEditorState {
                    mode: ContextEditorMode::Edit,
                    kind: draft.key.kind,
                    id: draft.key.id.clone(),
                    toml_body: draft.toml_body.clone(),
                    toml_revealed: false,
                    expected_provider_revision: draft.provider_revision.clone(),
                });
                self.draft_phase = OperationPhase::Ready;
                self.draft_error = None;
                self.draft_target = None;
            }
            Err(error) => {
                self.draft_phase = OperationPhase::Error;
                self.draft_error = Some(error);
                self.draft_target = None;
            }
        }
        true
    }

    pub fn set_editor_id(&mut self, id: String) -> bool {
        let Some(editor) = self.editor.as_mut() else {
            return false;
        };
        if editor.mode == ContextEditorMode::Edit || self.mutation_phase == OperationPhase::Running
        {
            return false;
        }
        editor.id = id;
        true
    }

    pub fn set_editor_body(&mut self, body: String) -> bool {
        let Some(editor) = self.editor.as_mut() else {
            return false;
        };
        if self.mutation_phase == OperationPhase::Running {
            return false;
        }
        editor.toml_body = body;
        true
    }

    pub fn set_editor_toml_revealed(&mut self, revealed: bool) -> bool {
        let Some(editor) = self.editor.as_mut() else {
            return false;
        };
        editor.toml_revealed = revealed;
        true
    }

    pub fn cancel_editor(&mut self) -> bool {
        if self.mutation_phase == OperationPhase::Running {
            return false;
        }
        self.editor.take().is_some()
    }

    pub fn begin_save(&mut self) -> Option<(u64, SaveContextEntry)> {
        if self.mutation_phase == OperationPhase::Running {
            return None;
        }
        let editor = self.editor.as_ref()?;
        self.current_mutation_request_id =
            next_id(self.current_mutation_request_id, "context stored mutation");
        self.mutation_phase = OperationPhase::Running;
        self.mutation_error = None;
        self.pending_mutation = Some(StoredMutationKind::Save);
        Some((
            self.current_mutation_request_id,
            SaveContextEntry {
                expected_provider_revision: editor.expected_provider_revision.clone(),
                mode: match editor.mode {
                    ContextEditorMode::Create => SaveContextEntryMode::Create,
                    ContextEditorMode::Edit => SaveContextEntryMode::Edit,
                },
                key: ContextEntryKey {
                    kind: editor.kind,
                    id: editor.id.clone(),
                },
                toml_body: editor.toml_body.clone(),
            },
        ))
    }

    pub fn begin_toggle(
        &mut self,
        key: ContextEntryKey,
        enabled: bool,
    ) -> Option<(u64, SetContextEntryEnabled)> {
        if self.has_running_mutation() || !self.contains_key(&key) {
            return None;
        }
        let bundle = self.bundle.as_ref()?;
        self.current_mutation_request_id =
            next_id(self.current_mutation_request_id, "context stored mutation");
        self.mutation_phase = OperationPhase::Running;
        self.mutation_error = None;
        self.pending_mutation = Some(StoredMutationKind::Toggle);
        Some((
            self.current_mutation_request_id,
            SetContextEntryEnabled {
                expected_provider_revision: bundle.context.provider_revision.clone(),
                key,
                enabled,
            },
        ))
    }

    pub fn request_delete(&mut self, key: ContextEntryKey) -> bool {
        if self.has_running_mutation() || !self.contains_key(&key) {
            return false;
        }
        self.delete_confirmation = Some(key);
        true
    }

    pub fn cancel_delete(&mut self) -> bool {
        if self.mutation_phase == OperationPhase::Running {
            return false;
        }
        self.delete_confirmation.take().is_some()
    }

    pub fn begin_delete(&mut self) -> Option<(u64, DeleteContextEntry)> {
        if self.mutation_phase == OperationPhase::Running {
            return None;
        }
        let key = self.delete_confirmation.clone()?;
        let bundle = self.bundle.as_ref()?;
        self.current_mutation_request_id =
            next_id(self.current_mutation_request_id, "context stored mutation");
        self.mutation_phase = OperationPhase::Running;
        self.mutation_error = None;
        self.pending_mutation = Some(StoredMutationKind::Delete);
        Some((
            self.current_mutation_request_id,
            DeleteContextEntry {
                expected_provider_revision: bundle.context.provider_revision.clone(),
                key: key.clone(),
                confirmed_key: key,
            },
        ))
    }

    pub fn apply_stored_mutation_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ContextBundle>, ContextFailureKind>,
    ) -> bool {
        if request_id != self.current_mutation_request_id
            || self.mutation_phase != OperationPhase::Running
        {
            return false;
        }
        match result {
            Ok(bundle) => {
                self.bundle = Some(bundle);
                self.mutation_phase = OperationPhase::Ready;
                self.mutation_error = None;
                if self.pending_mutation == Some(StoredMutationKind::Save) {
                    self.editor = None;
                }
                if self.pending_mutation == Some(StoredMutationKind::Delete) {
                    self.delete_confirmation = None;
                }
                self.sync_preview = None;
                self.sync_outcome = None;
            }
            Err(error) => {
                self.mutation_phase = OperationPhase::Error;
                self.mutation_error = Some(error);
            }
        }
        self.pending_mutation = None;
        true
    }

    pub fn begin_preview(&mut self) -> Option<(u64, PreviewContextSync)> {
        if self.has_running_mutation() || self.editor.is_some() {
            return None;
        }
        let bundle = self.bundle.as_ref()?;
        self.current_preview_request_id =
            next_id(self.current_preview_request_id, "context preview");
        self.preview_phase = OperationPhase::Running;
        self.preview_error = None;
        self.sync_preview = None;
        Some((
            self.current_preview_request_id,
            PreviewContextSync {
                guard: guard_from_bundle(bundle),
                scope: ContextSyncScope::ActiveProvider,
            },
        ))
    }

    pub fn apply_preview_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ContextSyncPreview>, ContextFailureKind>,
    ) -> bool {
        if request_id != self.current_preview_request_id {
            return false;
        }
        match result {
            Ok(preview) => {
                if !self.guard_is_current(&preview.guard) {
                    self.preview_phase = OperationPhase::Error;
                    self.preview_error = Some(ContextFailureKind::Service(
                        ContextToolsErrorKind::ProviderConflict,
                    ));
                    self.sync_preview = None;
                    return false;
                }
                self.sync_preview = Some(preview);
                self.preview_phase = OperationPhase::Ready;
                self.preview_error = None;
            }
            Err(error) => {
                self.preview_phase = OperationPhase::Error;
                self.preview_error = Some(error);
                self.sync_preview = None;
            }
        }
        true
    }

    pub fn cancel_preview(&mut self) -> bool {
        if self.sync_phase == OperationPhase::Running {
            return false;
        }
        self.preview_phase = OperationPhase::Idle;
        self.preview_error = None;
        self.sync_preview.take().is_some()
    }

    pub fn begin_sync(&mut self) -> Option<(u64, SyncContextToLive)> {
        if self.sync_phase == OperationPhase::Running {
            return None;
        }
        let preview = self.sync_preview.as_ref()?;
        if !self.guard_is_current(&preview.guard) {
            return None;
        }
        self.current_sync_request_id = next_id(self.current_sync_request_id, "context sync");
        self.sync_phase = OperationPhase::Running;
        self.sync_error = None;
        self.sync_outcome = None;
        Some((
            self.current_sync_request_id,
            SyncContextToLive {
                guard: preview.guard.clone(),
                scope: ContextSyncScope::ActiveProvider,
            },
        ))
    }

    pub fn apply_sync_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ContextSyncOutcome>, ContextFailureKind>,
    ) -> bool {
        if request_id != self.current_sync_request_id || self.sync_phase != OperationPhase::Running
        {
            return false;
        }
        match result {
            Ok(outcome) => {
                self.bundle = Some(Arc::new(outcome.bundle.clone()));
                self.sync_outcome = Some(outcome);
                self.sync_preview = None;
                self.sync_phase = OperationPhase::Ready;
                self.sync_error = None;
            }
            Err(error) => {
                self.sync_phase = OperationPhase::Error;
                self.sync_error = Some(error);
            }
        }
        true
    }

    fn contains_key(&self, key: &ContextEntryKey) -> bool {
        self.bundle
            .as_ref()
            .is_some_and(|bundle| bundle.context.entries.iter().any(|entry| &entry.key == key))
    }

    fn has_running_mutation(&self) -> bool {
        self.draft_phase == OperationPhase::Running
            || self.mutation_phase == OperationPhase::Running
            || self.preview_phase == OperationPhase::Running
            || self.sync_phase == OperationPhase::Running
    }

    fn guard_is_current(&self, guard: &ContextSyncGuard) -> bool {
        self.bundle
            .as_ref()
            .is_some_and(|bundle| guard_from_bundle(bundle) == *guard)
    }
}

fn guard_from_bundle(bundle: &ContextBundle) -> ContextSyncGuard {
    ContextSyncGuard {
        expected_provider_revision: bundle.context.provider_revision.clone(),
        expected_live_revision: bundle.context.live_revision.clone(),
        expected_ownership_revision: bundle.context.ownership_revision.clone(),
    }
}

fn next_id(current: u64, label: &str) -> u64 {
    current
        .checked_add(1)
        .unwrap_or_else(|| panic!("{label} request id overflow"))
}
