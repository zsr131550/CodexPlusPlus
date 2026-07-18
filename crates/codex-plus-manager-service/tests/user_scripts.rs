use std::sync::{Arc, Mutex};

use codex_plus_core::script_market::{MarketScript, ScriptMarketManifest};
use codex_plus_manager_service::{
    DeleteUserScript, InstallMarketScript, ScriptMarketCompatibilityWorkspace,
    ScriptMarketRevision, SetUserScriptEnabled, SetUserScriptsEnabled, SystemProviderEnvironment,
    UserScriptBackupEvidence, UserScriptEnvironment, UserScriptError, UserScriptErrorKind,
    UserScriptMutationOutcome, UserScriptOrigin, UserScriptRevision, UserScriptService,
    UserScriptStatus, UserScriptSummary, UserScriptWorkspace,
};

fn serve_once(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{Read as _, Write as _};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        let _ = stream.write_all(&body);
    });
    (format!("http://{address}/index.json"), worker)
}

#[derive(Clone)]
struct FakeEnvironment {
    state: Arc<Mutex<FakeState>>,
}

struct FakeState {
    local: UserScriptWorkspace,
    market: ScriptMarketCompatibilityWorkspace,
    prepare_count: usize,
    commit_count: usize,
    set_global_count: usize,
    set_script_count: usize,
    delete_count: usize,
}

impl FakeEnvironment {
    fn new(digest: &str) -> Self {
        let local = UserScriptWorkspace {
            revision: UserScriptRevision::from_digest([1; 32]),
            globally_enabled: true,
            scripts: vec![
                UserScriptSummary {
                    key: "builtin:base.js".to_string(),
                    name: "Base".to_string(),
                    origin: UserScriptOrigin::Builtin,
                    enabled: true,
                    status: UserScriptStatus::NotLoaded,
                    market_id: None,
                    version: None,
                },
                UserScriptSummary {
                    key: "user:custom.js".to_string(),
                    name: "Custom".to_string(),
                    origin: UserScriptOrigin::User,
                    enabled: false,
                    status: UserScriptStatus::Disabled,
                    market_id: None,
                    version: None,
                },
            ],
        };
        let market = ScriptMarketCompatibilityWorkspace::from_manifest(ScriptMarketManifest {
            version: 1,
            updated_at: Some("2026-07-18T00:00:00Z".to_string()),
            scripts: vec![MarketScript {
                id: "demo".to_string(),
                name: "Demo".to_string(),
                description: "Useful".to_string(),
                version: "2".to_string(),
                author: "Fixture".to_string(),
                tags: vec!["ui".to_string()],
                homepage: "https://example.invalid/demo".to_string(),
                script_url: "https://downloads.example.invalid/demo.js".to_string(),
                sha256: digest.to_string(),
            }],
        });
        Self {
            state: Arc::new(Mutex::new(FakeState {
                local,
                market,
                prepare_count: 0,
                commit_count: 0,
                set_global_count: 0,
                set_script_count: 0,
                delete_count: 0,
            })),
        }
    }

    fn counts(&self) -> (usize, usize, usize, usize, usize) {
        let state = self.state.lock().unwrap();
        (
            state.prepare_count,
            state.commit_count,
            state.set_global_count,
            state.set_script_count,
            state.delete_count,
        )
    }
}

impl UserScriptEnvironment for FakeEnvironment {
    type Prepared = MarketScript;

    fn inspect_local(&self) -> Result<UserScriptWorkspace, UserScriptError> {
        Ok(self.state.lock().unwrap().local.clone())
    }

    fn refresh_market(&self) -> Result<ScriptMarketCompatibilityWorkspace, UserScriptError> {
        Ok(self.state.lock().unwrap().market.clone())
    }

    fn prepare_market_script(
        &self,
        script: &MarketScript,
    ) -> Result<Self::Prepared, UserScriptError> {
        self.state.lock().unwrap().prepare_count += 1;
        Ok(script.clone())
    }

    fn commit_market_script(
        &self,
        expected_revision: UserScriptRevision,
        prepared: Self::Prepared,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        let mut state = self.state.lock().unwrap();
        state.commit_count += 1;
        if state.local.revision != expected_revision {
            return Err(UserScriptError::new(UserScriptErrorKind::Conflict));
        }
        state.local.revision = UserScriptRevision::from_digest([2; 32]);
        state.local.scripts.push(UserScriptSummary {
            key: format!("user:market-{}.js", prepared.id),
            name: prepared.name,
            origin: UserScriptOrigin::User,
            enabled: true,
            status: UserScriptStatus::NotLoaded,
            market_id: Some(prepared.id),
            version: Some(prepared.version),
        });
        Ok(UserScriptMutationOutcome {
            workspace: state.local.clone(),
            backup: UserScriptBackupEvidence::none(),
        })
    }

    fn set_global_enabled(
        &self,
        expected_revision: UserScriptRevision,
        enabled: bool,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        let mut state = self.state.lock().unwrap();
        state.set_global_count += 1;
        if state.local.revision != expected_revision {
            return Err(UserScriptError::new(UserScriptErrorKind::Conflict));
        }
        state.local.globally_enabled = enabled;
        Ok(UserScriptMutationOutcome {
            workspace: state.local.clone(),
            backup: UserScriptBackupEvidence::none(),
        })
    }

    fn set_script_enabled(
        &self,
        expected_revision: UserScriptRevision,
        key: &str,
        enabled: bool,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        let mut state = self.state.lock().unwrap();
        state.set_script_count += 1;
        if state.local.revision != expected_revision {
            return Err(UserScriptError::new(UserScriptErrorKind::Conflict));
        }
        let script = state
            .local
            .scripts
            .iter_mut()
            .find(|script| script.key == key)
            .ok_or_else(|| UserScriptError::new(UserScriptErrorKind::InvalidTarget))?;
        script.enabled = enabled;
        Ok(UserScriptMutationOutcome {
            workspace: state.local.clone(),
            backup: UserScriptBackupEvidence::none(),
        })
    }

    fn delete_user_script(
        &self,
        expected_revision: UserScriptRevision,
        key: &str,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        let mut state = self.state.lock().unwrap();
        state.delete_count += 1;
        if state.local.revision != expected_revision {
            return Err(UserScriptError::new(UserScriptErrorKind::Conflict));
        }
        state.local.scripts.retain(|script| script.key != key);
        Ok(UserScriptMutationOutcome {
            workspace: state.local.clone(),
            backup: UserScriptBackupEvidence {
                id: "opaque-backup-id".to_string(),
                created: true,
            },
        })
    }
}

#[test]
fn workspace_and_errors_never_expose_script_source_or_private_paths() {
    let environment = FakeEnvironment::new("");
    let service = UserScriptService::new(environment);

    let workspace = service.inspect_local().unwrap();
    let rendered = format!("{workspace:?}");
    assert!(!rendered.contains("secret-source-sentinel"));
    assert!(!rendered.contains("C:/private/scripts"));

    let error = UserScriptError::with_compatibility_detail(
        UserScriptErrorKind::InspectFailed,
        "secret-source-sentinel C:/private/scripts".to_string(),
    );
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains("secret-source-sentinel"));
    assert!(!rendered.contains("C:/private/scripts"));
}

#[test]
fn install_requires_exact_confirmations_current_revisions_and_unverified_acknowledgement() {
    let environment = FakeEnvironment::new("");
    let service = UserScriptService::new(environment.clone());
    let local = service.inspect_local().unwrap();
    let market = service.refresh_market().unwrap();
    let base = InstallMarketScript {
        expected_local_revision: local.revision.clone(),
        expected_market_revision: market.revision.clone(),
        script_id: "demo".to_string(),
        confirmed_script_id: "demo".to_string(),
        confirmed_version: "2".to_string(),
        acknowledge_unverified: false,
    };

    let mut request = base.clone();
    request.confirmed_version = "1".to_string();
    assert_eq!(
        service.install(request).unwrap_err().kind(),
        UserScriptErrorKind::ConfirmationMismatch
    );
    assert_eq!(environment.counts().0, 0);

    let mut request = base.clone();
    request.expected_market_revision = ScriptMarketRevision::from_digest([9; 32]);
    assert_eq!(
        service.install(request).unwrap_err().kind(),
        UserScriptErrorKind::Conflict
    );
    assert_eq!(environment.counts().0, 0);

    let mut request = base.clone();
    request.expected_local_revision = UserScriptRevision::from_digest([9; 32]);
    request.acknowledge_unverified = true;
    assert_eq!(
        service.install(request).unwrap_err().kind(),
        UserScriptErrorKind::Conflict
    );
    assert_eq!(environment.counts().0, 0);

    assert_eq!(
        service.install(base.clone()).unwrap_err().kind(),
        UserScriptErrorKind::UnverifiedNotAcknowledged
    );
    assert_eq!(environment.counts().0, 0);

    let mut acknowledged = base;
    acknowledged.acknowledge_unverified = true;
    let outcome = service.install(acknowledged).unwrap();
    assert_eq!(environment.counts().0, 1);
    assert_eq!(environment.counts().1, 1);
    assert!(
        outcome
            .workspace
            .scripts
            .iter()
            .any(|script| script.market_id.as_deref() == Some("demo"))
    );
}

#[test]
fn invalid_integrity_builtin_delete_and_stale_mutations_fail_before_commit() {
    let invalid_environment = FakeEnvironment::new("bad");
    let invalid_service = UserScriptService::new(invalid_environment.clone());
    let local = invalid_service.inspect_local().unwrap();
    let market = invalid_service.refresh_market().unwrap();
    let error = invalid_service
        .install(InstallMarketScript {
            expected_local_revision: local.revision,
            expected_market_revision: market.revision,
            script_id: "demo".to_string(),
            confirmed_script_id: "demo".to_string(),
            confirmed_version: "2".to_string(),
            acknowledge_unverified: true,
        })
        .unwrap_err();
    assert_eq!(error.kind(), UserScriptErrorKind::InvalidIntegrity);
    assert_eq!(invalid_environment.counts().0, 0);

    let environment = FakeEnvironment::new("");
    let service = UserScriptService::new(environment.clone());
    let local = service.inspect_local().unwrap();
    let builtin_error = service
        .delete(DeleteUserScript {
            expected_revision: local.revision.clone(),
            key: "builtin:base.js".to_string(),
            confirmed_key: "builtin:base.js".to_string(),
        })
        .unwrap_err();
    assert_eq!(builtin_error.kind(), UserScriptErrorKind::InvalidTarget);
    assert_eq!(environment.counts().4, 0);

    let stale_error = service
        .set_global_enabled(SetUserScriptsEnabled {
            expected_revision: UserScriptRevision::from_digest([9; 32]),
            enabled: false,
        })
        .unwrap_err();
    assert_eq!(stale_error.kind(), UserScriptErrorKind::Conflict);
    assert_eq!(environment.counts().2, 0);

    let toggle = service
        .set_script_enabled(SetUserScriptEnabled {
            expected_revision: local.revision.clone(),
            key: "user:custom.js".to_string(),
            enabled: true,
        })
        .unwrap();
    assert!(
        toggle
            .workspace
            .scripts
            .iter()
            .find(|script| script.key == "user:custom.js")
            .unwrap()
            .enabled
    );
    assert_eq!(environment.counts().3, 1);

    let deleted = service
        .delete(DeleteUserScript {
            expected_revision: local.revision,
            key: "user:custom.js".to_string(),
            confirmed_key: "user:custom.js".to_string(),
        })
        .unwrap();
    assert!(deleted.backup.created);
    assert_eq!(deleted.backup.id, "opaque-backup-id");
    assert_eq!(environment.counts().4, 1);
}

#[test]
fn system_environment_isolated_paths_loopback_policy_and_mutations_are_transactional() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let codex_home = temp.path().join("codex");
    let builtin_dir = temp.path().join("builtin");
    let user_dir = temp.path().join("user");
    let config_path = temp.path().join("user_scripts.json");
    let backup_dir = temp.path().join("backups");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::create_dir_all(&builtin_dir).unwrap();
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::write(builtin_dir.join("base.js"), b"builtin").unwrap();
    std::fs::write(user_dir.join("custom.js"), b"recoverable").unwrap();
    std::fs::write(
        &config_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "enabled": true,
            "scripts": {"user:custom.js": false},
            "futureRoot": {"keep": true}
        }))
        .unwrap(),
    )
    .unwrap();
    let base_environment = SystemProviderEnvironment::for_manager_paths(
        state_dir.join("settings.json"),
        codex_home,
        state_dir.join("cc-switch.db"),
        state_dir.join("pending.json"),
        backup_dir.clone(),
        true,
    )
    .with_user_script_paths(&builtin_dir, &user_dir, &config_path);

    let manifest = br#"{"version":1,"scripts":[]}"#.to_vec();
    let (market_url, market_worker) = serve_once(manifest.clone());
    let secure_service = UserScriptService::new(
        base_environment
            .clone()
            .with_script_market_index_url(&market_url),
    );
    assert_eq!(
        secure_service.refresh_market().unwrap_err().kind(),
        UserScriptErrorKind::MarketRefreshFailed
    );

    let loopback_service =
        UserScriptService::new(base_environment.with_loopback_script_market_for_tests(&market_url));
    let market = loopback_service.refresh_market().unwrap();
    market_worker.join().unwrap();
    assert!(market.entries.is_empty());

    let local = loopback_service.inspect_local().unwrap();
    let disabled = loopback_service
        .set_global_enabled(SetUserScriptsEnabled {
            expected_revision: local.revision,
            enabled: false,
        })
        .unwrap();
    assert!(!disabled.workspace.globally_enabled);
    assert!(
        !disabled
            .workspace
            .scripts
            .iter()
            .find(|script| script.key == "user:custom.js")
            .unwrap()
            .enabled
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    assert_eq!(saved["scripts"]["user:custom.js"], false);
    assert_eq!(saved["futureRoot"]["keep"], true);

    let deleted = loopback_service
        .delete(DeleteUserScript {
            expected_revision: disabled.workspace.revision,
            key: "user:custom.js".to_string(),
            confirmed_key: "user:custom.js".to_string(),
        })
        .unwrap();
    assert!(deleted.backup.created);
    assert_eq!(
        std::fs::read(
            backup_dir
                .join("user-scripts")
                .join(&deleted.backup.id)
                .join("script.js")
        )
        .unwrap(),
        b"recoverable"
    );
}
