use std::sync::{Arc, Barrier};

use codex_plus_core::script_market::{
    MAX_MANIFEST_BYTES, MAX_SCRIPT_BYTES, MarketFetchPolicy, MarketScript, MarketScriptIntegrity,
    ScriptMarketErrorKind, classify_digest, download_script_with_policy,
    fetch_market_manifest_with_policy, prepare_market_script_content, validate_market_url,
};
use codex_plus_core::user_scripts::{
    UserScriptManager, UserScriptMutationErrorKind, UserScriptOrigin, UserScriptStatus,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

struct UserScriptFixture {
    _temp: tempfile::TempDir,
    builtin_dir: std::path::PathBuf,
    user_dir: std::path::PathBuf,
    config_path: std::path::PathBuf,
    manager: UserScriptManager,
}

impl UserScriptFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let builtin_dir = temp.path().join("builtin");
        let user_dir = temp.path().join("user");
        let config_path = temp.path().join("user_scripts.json");
        std::fs::create_dir_all(&builtin_dir).unwrap();
        std::fs::create_dir_all(&user_dir).unwrap();
        let manager = UserScriptManager::new(&builtin_dir, &user_dir, &config_path);
        Self {
            _temp: temp,
            builtin_dir,
            user_dir,
            config_path,
            manager,
        }
    }

    fn write_builtin(&self, name: &str, content: &[u8]) {
        std::fs::write(self.builtin_dir.join(name), content).unwrap();
    }

    fn write_user(&self, name: &str, content: &[u8]) {
        std::fs::write(self.user_dir.join(name), content).unwrap();
    }

    fn write_config(&self, value: Value) {
        std::fs::write(
            &self.config_path,
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }

    fn read_config(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.config_path).unwrap()).unwrap()
    }

    fn market_script(&self, version: &str, digest: String) -> MarketScript {
        MarketScript {
            id: "demo".to_string(),
            name: "Demo".to_string(),
            description: "Fixture".to_string(),
            version: version.to_string(),
            author: "Fixture".to_string(),
            tags: vec!["test".to_string()],
            homepage: "https://example.invalid/demo".to_string(),
            script_url: "https://example.invalid/demo.js".to_string(),
            sha256: digest,
        }
    }
}

fn sha256_hex(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

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
    (format!("http://{address}/script.js"), worker)
}

#[test]
fn typed_inventory_is_deterministic_and_keeps_the_legacy_json_shape() {
    let fixture = UserScriptFixture::new();
    fixture.write_builtin("b.js", b"builtin");
    fixture.write_user("a.js", b"user");

    let typed = fixture.manager.typed_inventory().unwrap();

    assert_eq!(
        typed
            .scripts
            .iter()
            .map(|item| item.key.as_str())
            .collect::<Vec<_>>(),
        ["builtin:b.js", "user:a.js"]
    );
    assert_eq!(typed.scripts[0].source, UserScriptOrigin::Builtin);
    assert_eq!(typed.scripts[1].source, UserScriptOrigin::User);
    assert_eq!(typed.scripts[0].status, UserScriptStatus::NotLoaded);

    let legacy = fixture.manager.inventory().unwrap();
    for key in ["enabled", "builtin_dir", "user_dir", "scripts"] {
        assert!(legacy.get(key).is_some(), "missing compatibility key {key}");
    }
    assert_eq!(legacy["scripts"][0]["source"], "builtin");
    assert_eq!(legacy["scripts"][0]["status"], "not_loaded");
    assert_eq!(legacy["scripts"][0]["error"], "");

    let debug = format!("{typed:?}");
    assert!(!debug.contains(&fixture.builtin_dir.to_string_lossy().to_string()));
    assert!(!debug.contains(&fixture.user_dir.to_string_lossy().to_string()));
}

#[test]
fn mutations_preserve_unknown_config_and_unrelated_choices() {
    let fixture = UserScriptFixture::new();
    fixture.write_config(json!({
        "enabled": true,
        "scripts": {
            "user:a.js": true,
            "user:b.js": false,
            "future-script-value": {"keep": true}
        },
        "market": {
            "user:b.js": {
                "id": "b",
                "name": "B",
                "version": "1",
                "script_url": "https://example.invalid/b.js",
                "future": 7
            }
        },
        "futureRoot": {"keep": true}
    }));

    fixture
        .manager
        .set_script_enabled("user:a.js", false)
        .unwrap();
    fixture.manager.set_global_enabled(false).unwrap();

    let saved = fixture.read_config();
    assert_eq!(saved["futureRoot"]["keep"], true);
    assert_eq!(saved["scripts"]["user:a.js"], false);
    assert_eq!(saved["scripts"]["user:b.js"], false);
    assert_eq!(saved["scripts"]["future-script-value"]["keep"], true);
    assert_eq!(saved["market"]["user:b.js"]["future"], 7);
}

#[test]
fn revision_changes_when_script_bytes_change_without_size_change() {
    let fixture = UserScriptFixture::new();
    fixture.write_user("a.js", b"aaaa");
    let before = fixture.manager.inspect().unwrap().revision;

    fixture.write_user("a.js", b"bbbb");
    let after = fixture.manager.inspect().unwrap().revision;

    assert_ne!(before, after);
    let debug = format!("{before:?} {after:?}");
    assert!(!debug.contains("aaaa"));
    assert!(!debug.contains("bbbb"));
}

#[test]
fn independent_managers_serialize_mutations_without_losing_keys() {
    const WORKERS: usize = 12;
    let fixture = UserScriptFixture::new();
    fixture.write_config(json!({
        "enabled": true,
        "scripts": {},
        "futureRoot": "preserved"
    }));
    let barrier = Arc::new(Barrier::new(WORKERS));
    let mut workers = Vec::new();

    for index in 0..WORKERS {
        let manager = UserScriptManager::new(
            &fixture.builtin_dir,
            &fixture.user_dir,
            &fixture.config_path,
        );
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            manager
                .set_script_enabled(&format!("user:{index}.js"), index % 2 == 0)
                .unwrap();
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }

    let saved = fixture.read_config();
    assert_eq!(saved["futureRoot"], "preserved");
    for index in 0..WORKERS {
        assert_eq!(saved["scripts"][format!("user:{index}.js")], index % 2 == 0);
    }
}

#[test]
fn supplied_digest_must_be_valid_and_match_downloaded_bytes() {
    let fixture = UserScriptFixture::new();
    assert_eq!(classify_digest(""), MarketScriptIntegrity::Unverified);
    assert_eq!(classify_digest("bad"), MarketScriptIntegrity::Invalid);
    assert_eq!(
        classify_digest(&sha256_hex(b"expected")),
        MarketScriptIntegrity::Verified
    );

    let invalid = fixture.market_script("1", "bad".to_string());
    assert_eq!(
        prepare_market_script_content(&invalid, b"expected")
            .unwrap_err()
            .kind(),
        ScriptMarketErrorKind::InvalidIntegrity
    );

    let mismatched = fixture.market_script("1", sha256_hex(b"expected"));
    assert_eq!(
        prepare_market_script_content(&mismatched, b"changed")
            .unwrap_err()
            .kind(),
        ScriptMarketErrorKind::IntegrityMismatch
    );

    let prepared = prepare_market_script_content(&mismatched, b"expected").unwrap();
    assert_eq!(prepared.integrity(), MarketScriptIntegrity::Verified);
    assert_eq!(prepared.byte_count(), b"expected".len());
    let debug = format!("{prepared:?}");
    assert!(!debug.contains("expected"));
    assert!(!debug.contains("example.invalid"));
}

#[tokio::test]
async fn market_transport_rejects_insecure_urls_and_bounds_script_bodies() {
    let secure = MarketFetchPolicy::https_only();
    assert!(validate_market_url("https://example.invalid/script.js", secure).is_ok());
    assert_eq!(
        validate_market_url("http://example.invalid/script.js", secure)
            .unwrap_err()
            .kind(),
        ScriptMarketErrorKind::InsecureTransport
    );

    let loopback = MarketFetchPolicy::loopback_http_for_tests();
    let (at_limit_url, at_limit_worker) = serve_once(vec![b'a'; MAX_SCRIPT_BYTES]);
    let at_limit = download_script_with_policy(&at_limit_url, loopback)
        .await
        .unwrap();
    at_limit_worker.join().unwrap();
    assert_eq!(at_limit.len(), MAX_SCRIPT_BYTES);

    let (overflow_url, overflow_worker) = serve_once(vec![b'b'; MAX_SCRIPT_BYTES + 1]);
    let overflow = download_script_with_policy(&overflow_url, loopback)
        .await
        .unwrap_err();
    overflow_worker.join().unwrap();
    assert_eq!(overflow.kind(), ScriptMarketErrorKind::ResponseTooLarge);

    let mut manifest_at_limit = br#"{"version":1,"scripts":[]}"#.to_vec();
    manifest_at_limit.resize(MAX_MANIFEST_BYTES, b' ');
    let (manifest_url, manifest_worker) = serve_once(manifest_at_limit);
    let manifest = fetch_market_manifest_with_policy(&manifest_url, loopback)
        .await
        .unwrap();
    manifest_worker.join().unwrap();
    assert!(manifest.scripts.is_empty());

    let mut manifest_overflow = br#"{"version":1,"scripts":[]}"#.to_vec();
    manifest_overflow.resize(MAX_MANIFEST_BYTES + 1, b' ');
    let (manifest_overflow_url, manifest_overflow_worker) = serve_once(manifest_overflow);
    let manifest_error = fetch_market_manifest_with_policy(&manifest_overflow_url, loopback)
        .await
        .unwrap_err();
    manifest_overflow_worker.join().unwrap();
    assert_eq!(
        manifest_error.kind(),
        ScriptMarketErrorKind::ResponseTooLarge
    );
}

#[test]
fn market_update_and_user_delete_create_recoverable_backups() {
    let fixture = UserScriptFixture::new();
    let backup_root = fixture._temp.path().join("backups");
    let manager = fixture.manager.clone().with_backup_root(&backup_root);
    let v1 = fixture.market_script("1", String::new());
    let v1_prepared = prepare_market_script_content(&v1, b"old-version").unwrap();
    let initial = manager.inspect().unwrap();
    manager
        .commit_market_script(&initial.revision, &v1_prepared)
        .unwrap();

    let v2 = fixture.market_script("2", sha256_hex(b"new-version"));
    let v2_prepared = prepare_market_script_content(&v2, b"new-version").unwrap();
    let before_update = manager.inspect().unwrap();
    let updated = manager
        .commit_market_script(&before_update.revision, &v2_prepared)
        .unwrap();

    assert!(updated.backup.created);
    assert_eq!(
        std::fs::read(
            backup_root
                .join("user-scripts")
                .join(&updated.backup.id)
                .join("script.js")
        )
        .unwrap(),
        b"old-version"
    );
    assert_eq!(
        std::fs::read(fixture.user_dir.join("market-demo.js")).unwrap(),
        b"new-version"
    );
    assert_eq!(
        fixture.read_config()["market"]["user:market-demo.js"]["version"],
        "2"
    );

    fixture.write_user("custom.js", b"secret-source-sentinel");
    manager.set_script_enabled("user:custom.js", false).unwrap();
    let before_delete = manager.inspect().unwrap();
    let deleted = manager
        .delete_user_script_with_backup(&before_delete.revision, "user:custom.js")
        .unwrap();

    assert!(deleted.backup.created);
    assert_eq!(
        std::fs::read(
            backup_root
                .join("user-scripts")
                .join(&deleted.backup.id)
                .join("script.js")
        )
        .unwrap(),
        b"secret-source-sentinel"
    );
    assert!(!fixture.user_dir.join("custom.js").exists());
    let debug = format!("{deleted:?}");
    assert!(!debug.contains("secret-source-sentinel"));
    assert!(!debug.contains(&backup_root.to_string_lossy().to_string()));
}

#[test]
fn stale_revision_blocks_commit_and_config_failure_restores_old_script() {
    let fixture = UserScriptFixture::new();
    let manager = fixture
        .manager
        .clone()
        .with_backup_root(fixture._temp.path().join("backups"));
    fixture.write_user("market-demo.js", b"old");
    let stale = manager.inspect().unwrap();
    fixture.write_user("market-demo.js", b"external");
    let prepared =
        prepare_market_script_content(&fixture.market_script("2", sha256_hex(b"new")), b"new")
            .unwrap();

    let conflict = manager
        .commit_market_script(&stale.revision, &prepared)
        .unwrap_err();
    assert_eq!(conflict.kind(), UserScriptMutationErrorKind::Conflict);
    assert_eq!(
        std::fs::read(fixture.user_dir.join("market-demo.js")).unwrap(),
        b"external"
    );

    let broken_temp = tempfile::tempdir().unwrap();
    let user_dir = broken_temp.path().join("user");
    let config_path = broken_temp.path().join("user_scripts.json");
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::create_dir_all(&config_path).unwrap();
    std::fs::write(user_dir.join("market-demo.js"), b"old").unwrap();
    let broken =
        UserScriptManager::new(broken_temp.path().join("builtin"), &user_dir, &config_path)
            .with_backup_root(broken_temp.path().join("backups"));
    let before = broken.inspect().unwrap();
    let error = broken
        .commit_market_script(&before.revision, &prepared)
        .unwrap_err();

    assert_eq!(error.kind(), UserScriptMutationErrorKind::WriteFailed);
    assert!(error.rollback_verified());
    assert_eq!(
        std::fs::read(user_dir.join("market-demo.js")).unwrap(),
        b"old"
    );
}

#[test]
fn revisioned_toggles_reject_stale_requests_and_preserve_individual_choices() {
    let fixture = UserScriptFixture::new();
    fixture.write_user("a.js", b"a");
    fixture.write_config(json!({
        "enabled": true,
        "scripts": {"user:a.js": false},
        "futureRoot": true
    }));
    let initial = fixture.manager.inspect().unwrap();
    let disabled = fixture
        .manager
        .set_global_enabled_if_revision(&initial.revision, false)
        .unwrap();

    assert!(!disabled.inspection.inventory.enabled);
    assert!(!disabled.inspection.inventory.scripts[0].enabled);
    let stale = fixture
        .manager
        .set_script_enabled_if_revision(&initial.revision, "user:a.js", true)
        .unwrap_err();
    assert_eq!(stale.kind(), UserScriptMutationErrorKind::Conflict);
    let saved = fixture.read_config();
    assert_eq!(saved["enabled"], false);
    assert_eq!(saved["scripts"]["user:a.js"], false);
    assert_eq!(saved["futureRoot"], true);
}
