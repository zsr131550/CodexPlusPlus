use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

use codex_plus_manager_service::{
    PluginMarketplaceCompatibilityWorkspace, PluginMarketplaceEnvironment, PluginMarketplaceError,
    PluginMarketplaceErrorKind, PluginMarketplaceKind, PluginMarketplaceRepair,
    PluginMarketplaceRepairOutcome, PluginMarketplaceRevision, PluginMarketplaceService,
    PluginMarketplaceStatus, PluginMarketplaceWorkspace, RepairPluginMarketplace,
    SystemProviderEnvironment,
};

#[derive(Clone)]
struct FakeMarketplaceEnvironment {
    state: Arc<Mutex<PluginMarketplaceCompatibilityWorkspace>>,
    live_lock: Arc<Mutex<()>>,
    prepare_count: Arc<AtomicUsize>,
    prepare_gate: Option<Arc<PrepareGate>>,
}

struct PrepareGate {
    entered: Barrier,
    release: Barrier,
}

#[derive(Debug)]
struct FakePreparation {
    kind: PluginMarketplaceKind,
}

impl FakeMarketplaceEnvironment {
    fn unhealthy() -> Self {
        Self {
            state: Arc::new(Mutex::new(compatibility_workspace(1, false, false))),
            live_lock: Arc::new(Mutex::new(())),
            prepare_count: Arc::new(AtomicUsize::new(0)),
            prepare_gate: None,
        }
    }

    fn with_prepare_gate(mut self) -> Self {
        self.prepare_gate = Some(Arc::new(PrepareGate {
            entered: Barrier::new(2),
            release: Barrier::new(2),
        }));
        self
    }

    fn prepare_count(&self) -> usize {
        self.prepare_count.load(Ordering::SeqCst)
    }

    fn wait_until_preparing(&self) {
        self.prepare_gate.as_ref().unwrap().entered.wait();
    }

    fn release_preparation(&self) {
        self.prepare_gate.as_ref().unwrap().release.wait();
    }

    fn replace_workspace(&self, revision: u8, local_healthy: bool, remote_healthy: bool) {
        *self.state.lock().unwrap() =
            compatibility_workspace(revision, local_healthy, remote_healthy);
    }
}

impl PluginMarketplaceEnvironment for FakeMarketplaceEnvironment {
    type Preparation = FakePreparation;

    fn inspect_plugin_marketplaces(
        &self,
    ) -> Result<PluginMarketplaceCompatibilityWorkspace, PluginMarketplaceError> {
        Ok(self.state.lock().unwrap().clone())
    }

    fn prepare_plugin_marketplace(
        &self,
        kind: PluginMarketplaceKind,
    ) -> Result<Self::Preparation, PluginMarketplaceError> {
        self.prepare_count.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.prepare_gate {
            gate.entered.wait();
            gate.release.wait();
        }
        Ok(FakePreparation { kind })
    }

    fn commit_plugin_marketplace(
        &self,
        expected_revision: PluginMarketplaceRevision,
        kind: PluginMarketplaceKind,
        prepared: Self::Preparation,
    ) -> Result<PluginMarketplaceRepair, PluginMarketplaceError> {
        assert_eq!(prepared.kind, kind);
        let _lock = self.live_lock.lock().unwrap();
        let mut state = self.state.lock().unwrap();
        if state.workspace.revision != expected_revision {
            if !state.workspace.status(kind).needs_repair {
                return Ok(PluginMarketplaceRepair {
                    outcome: PluginMarketplaceRepairOutcome::AlreadyHealthy,
                    initialized: false,
                    configured: false,
                    workspace: state.workspace.clone(),
                });
            }
            return Err(PluginMarketplaceError::new(
                PluginMarketplaceErrorKind::Conflict,
            ));
        }

        let next = if kind == PluginMarketplaceKind::Local {
            &mut state.workspace.local
        } else {
            &mut state.workspace.remote
        };
        next.available = true;
        next.config_registered = true;
        next.needs_repair = false;
        next.plugin_count = 1;
        next.skill_count = 1;
        state.workspace.revision = revision(9);
        Ok(PluginMarketplaceRepair {
            outcome: PluginMarketplaceRepairOutcome::Initialized,
            initialized: true,
            configured: true,
            workspace: state.workspace.clone(),
        })
    }
}

#[test]
fn stale_revision_is_rejected_before_preparation() {
    let environment = FakeMarketplaceEnvironment::unhealthy();
    let service = PluginMarketplaceService::new(environment.clone());

    let error = service
        .repair(RepairPluginMarketplace {
            expected_revision: revision(0),
            kind: PluginMarketplaceKind::Local,
            confirmed_kind: PluginMarketplaceKind::Local,
        })
        .unwrap_err();

    assert_eq!(error.kind(), PluginMarketplaceErrorKind::Conflict);
    assert_eq!(environment.prepare_count(), 0);
}

#[test]
fn mismatched_confirmation_is_rejected_before_preparation() {
    let environment = FakeMarketplaceEnvironment::unhealthy();
    let service = PluginMarketplaceService::new(environment.clone());

    let error = service
        .repair(RepairPluginMarketplace {
            expected_revision: revision(1),
            kind: PluginMarketplaceKind::Local,
            confirmed_kind: PluginMarketplaceKind::Remote,
        })
        .unwrap_err();

    assert_eq!(error.kind(), PluginMarketplaceErrorKind::Conflict);
    assert_eq!(environment.prepare_count(), 0);
}

#[test]
fn preparation_runs_before_the_live_mutation_lock_is_acquired() {
    let environment = FakeMarketplaceEnvironment::unhealthy().with_prepare_gate();
    let service = PluginMarketplaceService::new(environment.clone());
    let repair = std::thread::spawn(move || {
        service.repair(RepairPluginMarketplace {
            expected_revision: revision(1),
            kind: PluginMarketplaceKind::Local,
            confirmed_kind: PluginMarketplaceKind::Local,
        })
    });

    environment.wait_until_preparing();
    assert!(environment.live_lock.try_lock().is_ok());
    environment.release_preparation();

    let result = repair.join().unwrap().unwrap();
    assert_eq!(result.outcome, PluginMarketplaceRepairOutcome::Initialized);
}

#[test]
fn target_becoming_healthy_during_preparation_is_idempotent() {
    let environment = FakeMarketplaceEnvironment::unhealthy().with_prepare_gate();
    let service = PluginMarketplaceService::new(environment.clone());
    let repair = std::thread::spawn(move || {
        service.repair(RepairPluginMarketplace {
            expected_revision: revision(1),
            kind: PluginMarketplaceKind::Local,
            confirmed_kind: PluginMarketplaceKind::Local,
        })
    });

    environment.wait_until_preparing();
    environment.replace_workspace(2, true, false);
    environment.release_preparation();

    let result = repair.join().unwrap().unwrap();
    assert_eq!(
        result.outcome,
        PluginMarketplaceRepairOutcome::AlreadyHealthy
    );
    assert!(!result.workspace.local.needs_repair);
}

#[test]
fn changed_unhealthy_state_during_preparation_conflicts() {
    let environment = FakeMarketplaceEnvironment::unhealthy().with_prepare_gate();
    let service = PluginMarketplaceService::new(environment.clone());
    let repair = std::thread::spawn(move || {
        service.repair(RepairPluginMarketplace {
            expected_revision: revision(1),
            kind: PluginMarketplaceKind::Local,
            confirmed_kind: PluginMarketplaceKind::Local,
        })
    });

    environment.wait_until_preparing();
    environment.replace_workspace(2, false, true);
    environment.release_preparation();

    let error = repair.join().unwrap().unwrap_err();
    assert_eq!(error.kind(), PluginMarketplaceErrorKind::Conflict);
}

#[test]
fn compatibility_and_error_debug_output_are_path_safe() {
    let workspace = compatibility_workspace(1, false, false);
    let workspace_debug = format!("{workspace:?}");
    let error_debug = format!(
        "{:?}",
        PluginMarketplaceError::new(PluginMarketplaceErrorKind::WriteFailed)
    );

    assert!(!workspace_debug.contains("secret-codex-home"));
    assert!(!workspace_debug.contains("secret-local-marketplace"));
    assert!(!workspace_debug.contains("secret-remote-marketplace"));
    assert_eq!(
        error_debug,
        "PluginMarketplaceError { kind: WriteFailed, http_status: None }"
    );
}

#[test]
fn system_environment_repairs_embedded_remote_and_preserves_config() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("config.toml"), "unknown_key = true\n").unwrap();
    let environment =
        SystemProviderEnvironment::for_paths(temp.path().join("settings.json"), home.clone());
    let service = PluginMarketplaceService::new(environment);
    let initial = service.inspect().unwrap();

    let repair = service
        .repair(RepairPluginMarketplace {
            expected_revision: initial.revision,
            kind: PluginMarketplaceKind::Remote,
            confirmed_kind: PluginMarketplaceKind::Remote,
        })
        .unwrap();

    assert_eq!(repair.outcome, PluginMarketplaceRepairOutcome::Initialized);
    assert!(!repair.workspace.remote.needs_repair);
    assert!(repair.workspace.remote.plugin_count > 0);
    assert!(repair.workspace.remote.skill_count > 0);
    let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(config.contains("unknown_key = true"));
    assert!(config.contains("openai-curated-remote"));
}

#[test]
fn system_environment_registers_existing_local_marketplace_without_network() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    write_local_marketplace(&home);
    let environment =
        SystemProviderEnvironment::for_paths(temp.path().join("settings.json"), home.clone());
    let service = PluginMarketplaceService::new(environment);
    let initial = service.inspect().unwrap();

    let repair = service
        .repair(RepairPluginMarketplace {
            expected_revision: initial.revision,
            kind: PluginMarketplaceKind::Local,
            confirmed_kind: PluginMarketplaceKind::Local,
        })
        .unwrap();

    assert_eq!(repair.outcome, PluginMarketplaceRepairOutcome::Configured);
    assert!(!repair.workspace.local.needs_repair);
    assert!(home.join(".tmp/plugins/plugins/gmail").is_dir());
}

#[test]
fn system_revision_changes_when_marketplace_content_changes_without_count_change() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    write_local_marketplace(&home);
    let skill = home.join(".tmp/plugins/plugins/gmail/SKILL.md");
    std::fs::write(&skill, "first revision").unwrap();
    let environment = SystemProviderEnvironment::for_paths(temp.path().join("settings.json"), home);

    let initial = environment.inspect_plugin_marketplaces().unwrap();
    std::fs::write(skill, "second revision").unwrap();
    let changed = environment.inspect_plugin_marketplaces().unwrap();

    assert_eq!(
        initial.workspace.local.plugin_count,
        changed.workspace.local.plugin_count
    );
    assert_eq!(
        initial.workspace.local.skill_count,
        changed.workspace.local.skill_count
    );
    assert_ne!(initial.workspace.revision, changed.workspace.revision);
}

#[test]
fn system_commit_rechecks_revision_and_rejects_changed_unhealthy_state() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    std::fs::create_dir_all(&home).unwrap();
    let environment =
        SystemProviderEnvironment::for_paths(temp.path().join("settings.json"), home.clone());
    let initial = environment.inspect_plugin_marketplaces().unwrap();
    let prepared = environment
        .prepare_plugin_marketplace(PluginMarketplaceKind::Remote)
        .unwrap();
    std::fs::write(home.join("config.toml"), "changed = true\n").unwrap();

    let error = environment
        .commit_plugin_marketplace(
            initial.workspace.revision,
            PluginMarketplaceKind::Remote,
            prepared,
        )
        .unwrap_err();

    assert_eq!(error.kind(), PluginMarketplaceErrorKind::Conflict);
    assert!(!home.join(".tmp/plugins-remote").exists());
}

#[test]
fn system_commit_rejects_a_mismatched_preparation_without_side_effects() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    std::fs::create_dir_all(&home).unwrap();
    let environment =
        SystemProviderEnvironment::for_paths(temp.path().join("settings.json"), home.clone());
    let initial = environment.inspect_plugin_marketplaces().unwrap();
    let prepared = environment
        .prepare_plugin_marketplace(PluginMarketplaceKind::Remote)
        .unwrap();

    let error = environment
        .commit_plugin_marketplace(
            initial.workspace.revision,
            PluginMarketplaceKind::Local,
            prepared,
        )
        .unwrap_err();

    assert_eq!(error.kind(), PluginMarketplaceErrorKind::Conflict);
    assert!(!home.join(".tmp/plugins-remote").exists());
    assert!(!home.join("config.toml").exists());
}

#[test]
fn system_commit_waits_for_the_shared_live_mutation_lock() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    std::fs::create_dir_all(&home).unwrap();
    let environment =
        SystemProviderEnvironment::for_paths(temp.path().join("settings.json"), home.clone());
    let initial = environment.inspect_plugin_marketplaces().unwrap();
    let prepared = environment
        .prepare_plugin_marketplace(PluginMarketplaceKind::Remote)
        .unwrap();
    let live_lock = codex_plus_core::relay_config::acquire_relay_live_mutation_lock(&home).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = environment.commit_plugin_marketplace(
            initial.workspace.revision,
            PluginMarketplaceKind::Remote,
            prepared,
        );
        result_tx.send(result).unwrap();
    });

    started_rx.recv().unwrap();
    assert!(
        result_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err()
    );
    drop(live_lock);
    let result = result_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();

    assert!(!result.workspace.remote.needs_repair);
}

#[test]
fn two_system_handles_serialize_commit_and_the_loser_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    std::fs::create_dir_all(&home).unwrap();
    let settings = temp.path().join("settings.json");
    let first_environment = SystemProviderEnvironment::for_paths(settings.clone(), home.clone());
    let second_environment = SystemProviderEnvironment::for_paths(settings, home.clone());
    let initial = first_environment.inspect_plugin_marketplaces().unwrap();
    let first_prepared = first_environment
        .prepare_plugin_marketplace(PluginMarketplaceKind::Remote)
        .unwrap();
    let second_prepared = second_environment
        .prepare_plugin_marketplace(PluginMarketplaceKind::Remote)
        .unwrap();
    let gate = Arc::new(Barrier::new(3));
    let first_gate = gate.clone();
    let first_revision = initial.workspace.revision.clone();
    let first = std::thread::spawn(move || {
        first_gate.wait();
        first_environment.commit_plugin_marketplace(
            first_revision,
            PluginMarketplaceKind::Remote,
            first_prepared,
        )
    });
    let second_gate = gate.clone();
    let second_revision = initial.workspace.revision;
    let second = std::thread::spawn(move || {
        second_gate.wait();
        second_environment.commit_plugin_marketplace(
            second_revision,
            PluginMarketplaceKind::Remote,
            second_prepared,
        )
    });

    gate.wait();
    let outcomes = [
        first.join().unwrap().unwrap().outcome,
        second.join().unwrap().unwrap().outcome,
    ];

    assert!(outcomes.contains(&PluginMarketplaceRepairOutcome::Initialized));
    assert!(outcomes.contains(&PluginMarketplaceRepairOutcome::AlreadyHealthy));
    let staging_count = std::fs::read_dir(home.join(".tmp"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("plugins-remote-embedded-")
        })
        .count();
    assert_eq!(staging_count, 0);
}

#[test]
fn system_commit_is_idempotent_when_target_became_healthy() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    std::fs::create_dir_all(&home).unwrap();
    let environment =
        SystemProviderEnvironment::for_paths(temp.path().join("settings.json"), home.clone());
    let initial = environment.inspect_plugin_marketplaces().unwrap();
    let prepared = environment
        .prepare_plugin_marketplace(PluginMarketplaceKind::Remote)
        .unwrap();
    codex_plus_core::plugin_marketplace::ensure_openai_curated_remote_marketplace_available(&home)
        .unwrap();

    let repair = environment
        .commit_plugin_marketplace(
            initial.workspace.revision,
            PluginMarketplaceKind::Remote,
            prepared,
        )
        .unwrap();

    assert_eq!(
        repair.outcome,
        PluginMarketplaceRepairOutcome::AlreadyHealthy
    );
    assert!(!repair.workspace.remote.needs_repair);
}

fn compatibility_workspace(
    revision_value: u8,
    local_healthy: bool,
    remote_healthy: bool,
) -> PluginMarketplaceCompatibilityWorkspace {
    PluginMarketplaceCompatibilityWorkspace::new(
        PluginMarketplaceWorkspace {
            revision: revision(revision_value),
            local: status(local_healthy),
            remote: status(remote_healthy),
        },
        PathBuf::from(r"C:\secret-codex-home"),
        Some(PathBuf::from(r"C:\secret-local-marketplace")),
        Some(PathBuf::from(r"C:\secret-remote-marketplace")),
    )
}

fn status(healthy: bool) -> PluginMarketplaceStatus {
    PluginMarketplaceStatus {
        available: healthy,
        config_registered: healthy,
        needs_repair: !healthy,
        plugin_count: usize::from(healthy),
        skill_count: usize::from(healthy),
    }
}

fn revision(value: u8) -> PluginMarketplaceRevision {
    PluginMarketplaceRevision::from_digest([value; 32])
}

fn write_local_marketplace(home: &std::path::Path) {
    let root = home.join(".tmp").join("plugins");
    std::fs::create_dir_all(root.join(".agents/plugins")).unwrap();
    std::fs::create_dir_all(root.join("plugins/gmail")).unwrap();
    std::fs::write(
        root.join(".agents/plugins/marketplace.json"),
        r#"{"name":"openai-curated","plugins":[{"name":"gmail","path":"./plugins/gmail"}]}"#,
    )
    .unwrap();
}
