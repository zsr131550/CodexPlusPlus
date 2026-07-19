use codex_plus_core::zed_remote::{
    self, SshTarget, ZedOpenStrategy, ZedRemoteError, ZedRemoteProject, ZedRemoteProjectSource,
    ZedRemoteRegistryStore,
};
use serde_json::json;
use std::sync::{Arc, Barrier};

#[test]
fn build_zed_remote_url_with_user_host_port_and_encoded_path() {
    let url = zed_remote::build_zed_remote_url(
        &SshTarget {
            user: "alice".to_string(),
            host: "example.com".to_string(),
            port: Some(2222),
        },
        "/home/alice/My Project/你好.py",
    )
    .unwrap();

    assert_eq!(
        url,
        "ssh://alice@example.com:2222/home/alice/My%20Project/%E4%BD%A0%E5%A5%BD.py"
    );
}

#[test]
fn build_zed_remote_url_allows_host_without_user() {
    let url = zed_remote::build_zed_remote_url(
        &SshTarget {
            user: String::new(),
            host: "box.internal".to_string(),
            port: None,
        },
        "/srv/app/main.py",
    )
    .unwrap();

    assert_eq!(url, "ssh://box.internal/srv/app/main.py");
}

#[test]
fn build_zed_remote_url_rejects_invalid_inputs() {
    let error = zed_remote::build_zed_remote_url(
        &SshTarget {
            user: "alice".to_string(),
            host: "bad host".to_string(),
            port: None,
        },
        "/a.py",
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ZedRemoteError::Validation("Invalid SSH host")
    ));
}

#[test]
fn build_zed_remote_url_allows_bracketed_ipv6_host() {
    let url = zed_remote::build_zed_remote_url(
        &SshTarget {
            user: "alice".to_string(),
            host: "[::1]".to_string(),
            port: Some(2222),
        },
        "/home/alice/a.py",
    )
    .unwrap();

    assert_eq!(url, "ssh://alice@[::1]:2222/home/alice/a.py");
}

#[test]
fn open_strategy_defaults_to_add_to_focused_workspace() {
    assert_eq!(
        zed_remote::zed_open_strategy_from_payload(&json!({})),
        ZedOpenStrategy::AddToFocusedWorkspace
    );
    assert_eq!(
        zed_remote::zed_open_strategy_from_payload(&json!({"strategy": "reuseWindow"})),
        ZedOpenStrategy::ReuseWindow
    );
    assert_eq!(
        zed_remote::zed_open_strategy_from_payload(&json!({"strategy": "unknown"})),
        ZedOpenStrategy::AddToFocusedWorkspace
    );
}

#[test]
fn launch_args_for_add_strategy_are_zed_dash_a_url() {
    assert_eq!(
        zed_remote::zed_cli_args_for_strategy(
            ZedOpenStrategy::AddToFocusedWorkspace,
            "ssh://example.com/home/app"
        ),
        vec!["-a".to_string(), "ssh://example.com/home/app".to_string()]
    );
}

#[test]
fn launch_args_for_reuse_strategy_are_zed_dash_r_url() {
    assert_eq!(
        zed_remote::zed_cli_args_for_strategy(
            ZedOpenStrategy::ReuseWindow,
            "ssh://example.com/home/app"
        ),
        vec!["-r".to_string(), "ssh://example.com/home/app".to_string()]
    );
}

#[test]
fn launch_args_for_new_window_strategy_are_zed_dash_n_url() {
    assert_eq!(
        zed_remote::zed_cli_args_for_strategy(
            ZedOpenStrategy::NewWindow,
            "ssh://example.com/home/app"
        ),
        vec!["-n".to_string(), "ssh://example.com/home/app".to_string()]
    );
}

#[test]
fn launch_args_for_default_strategy_are_plain_url() {
    assert_eq!(
        zed_remote::zed_cli_args_for_strategy(
            ZedOpenStrategy::Default,
            "ssh://example.com/home/app"
        ),
        vec!["ssh://example.com/home/app".to_string()]
    );
}

#[test]
fn launch_plan_contains_exact_url_and_strategy_but_debug_is_redacted() {
    let plan = zed_remote::prepare_zed_remote_launch(
        &SshTarget {
            user: "zed-user-sentinel".to_string(),
            host: "host-sentinel.example.test".to_string(),
            port: Some(2222),
        },
        "/workspace-sentinel/a b",
        ZedOpenStrategy::NewWindow,
    )
    .unwrap();

    assert_eq!(
        plan.url(),
        "ssh://zed-user-sentinel@host-sentinel.example.test:2222/workspace-sentinel/a%20b"
    );
    assert_eq!(plan.strategy(), ZedOpenStrategy::NewWindow);
    assert_eq!(plan.args(), vec!["-n".to_string(), plan.url().to_string()]);
    let debug = format!("{plan:?}");
    assert!(debug.contains("NewWindow"));
    assert!(!debug.contains("sentinel"));
    assert!(!debug.contains("workspace"));
    assert!(!debug.contains("ssh://"));
}

#[test]
fn zed_domain_debug_output_omits_operational_metadata() {
    let project = ZedRemoteProject {
        id: "id-sentinel".to_string(),
        label: "label-sentinel".to_string(),
        host_id: "host-id-sentinel".to_string(),
        ssh: SshTarget {
            user: "user-sentinel".to_string(),
            host: "host-sentinel.example.test".to_string(),
            port: Some(2222),
        },
        path: "/path-sentinel/project".to_string(),
        url: "ssh://url-sentinel/path".to_string(),
        source: ZedRemoteProjectSource::Recent,
        last_opened_at_ms: Some(42),
        is_current: false,
    };

    let target_debug = format!("{:?}", project.ssh);
    let project_debug = format!("{project:?}");
    assert!(!target_debug.contains("sentinel"));
    assert!(!target_debug.contains("2222"));
    assert!(project_debug.contains("Recent"));
    assert!(project_debug.contains("port_present"));
    assert!(project_debug.contains("last_opened_present"));
    assert!(!project_debug.contains("sentinel"));
    assert!(!project_debug.contains("ssh://"));
}

#[test]
fn typed_zed_availability_contains_no_executable_paths() {
    let availability = zed_remote::zed_availability();

    assert_eq!(
        availability.platform_supported,
        cfg!(target_os = "macos") || cfg!(target_os = "windows") || cfg!(target_os = "linux")
    );
    let debug = format!("{availability:?}");
    assert!(debug.contains("platform_supported"));
    assert!(!debug.contains("zed.exe"));
    assert!(!debug.contains("Zed.app"));
}

#[test]
fn target_from_payload_splits_codex_managed_authority() {
    let target =
        zed_remote::target_from_payload(&json!({"ssh": {"host": "longnv@192.168.100.31"}}))
            .unwrap();

    assert_eq!(
        target,
        SshTarget {
            user: "longnv".to_string(),
            host: "192.168.100.31".to_string(),
            port: None,
        }
    );
}

#[test]
fn registry_revision_distinguishes_missing_empty_and_changed_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recent.json");
    let store = ZedRemoteRegistryStore::new(path.clone());

    let missing = store.inspect().unwrap();
    std::fs::write(&path, br#"{"projects":[]}"#).unwrap();
    let empty = store.inspect().unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "projects": [{
                "id": "zed-remote-project:fixture",
                "label": "fixture",
                "hostId": "fixture-host",
                "ssh": {"user": "alice", "host": "example.test", "port": 2222},
                "path": "/srv/fixture",
                "url": "ssh://alice@example.test:2222/srv/fixture",
                "source": "recent",
                "lastOpenedAtMs": 42,
                "isCurrent": false
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let populated = store.inspect().unwrap();

    assert_ne!(missing.revision, empty.revision);
    assert_ne!(empty.revision, populated.revision);
    assert!(missing.projects.is_empty());
    assert!(empty.projects.is_empty());
    assert_eq!(populated.projects.len(), 1);
}

#[test]
fn malformed_registry_is_reported_and_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recent.json");
    std::fs::write(&path, b"not json").unwrap();
    let before = std::fs::read(&path).unwrap();

    let error = ZedRemoteRegistryStore::new(path.clone())
        .inspect()
        .unwrap_err();

    assert!(matches!(error, ZedRemoteError::RegistryParse(_)));
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn discovery_uses_the_supplied_registry_snapshot_without_reopening_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recent.json");
    let original = recent_project("original-sentinel", 20);
    let replacement = recent_project("replacement-sentinel", 30);
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({ "projects": [original.clone()] })).unwrap(),
    )
    .unwrap();
    let snapshot = ZedRemoteRegistryStore::new(path.clone()).inspect().unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({ "projects": [replacement.clone()] })).unwrap(),
    )
    .unwrap();

    let projects = zed_remote::list_zed_remote_projects_from_sources(
        None,
        &json!({}),
        &snapshot.projects,
        None,
    )
    .unwrap();

    assert!(projects.iter().any(|project| project.id == original.id));
    assert!(projects.iter().all(|project| project.id != replacement.id));
}

#[test]
fn remember_and_forget_reject_stale_revision_and_preserve_unknown_fields() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recent.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "custom": {"keep": true},
            "projects": []
        }))
        .unwrap(),
    )
    .unwrap();
    let store = ZedRemoteRegistryStore::new(path.clone());
    let initial = store.inspect().unwrap();
    let project = recent_project("a", 20);

    let remembered = store
        .remember_if_revision(&initial.revision, project.clone())
        .unwrap();
    let bytes_after_remember = std::fs::read(&path).unwrap();
    let stale = store.forget_if_revision(&initial.revision, &project.id);

    assert_eq!(remembered.affected, 1);
    assert!(matches!(stale, Err(ZedRemoteError::RegistryConflict)));
    assert_eq!(std::fs::read(&path).unwrap(), bytes_after_remember);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap()["custom"],
        json!({"keep": true})
    );

    let current = store.inspect().unwrap();
    let forgotten = store
        .forget_if_revision(&current.revision, &project.id)
        .unwrap();
    assert_eq!(forgotten.affected, 1);
    assert!(forgotten.snapshot.projects.is_empty());
}

#[test]
fn registry_store_keeps_newest_unique_hundred_and_forgets_exact_id() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recent.json");
    let projects = (0..101)
        .map(|index| recent_project(&format!("p{index}"), index))
        .collect::<Vec<_>>();
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({ "projects": projects })).unwrap(),
    )
    .unwrap();
    let store = ZedRemoteRegistryStore::new(path);

    let initial = store.inspect().unwrap();
    let newest = recent_project("newest", 500);
    let remembered = store
        .remember_if_revision(&initial.revision, newest.clone())
        .unwrap();
    assert_eq!(remembered.snapshot.projects.len(), 100);
    assert_eq!(remembered.snapshot.projects[0].id, newest.id);
    assert!(
        remembered
            .snapshot
            .projects
            .windows(2)
            .all(|items| items[0].last_opened_at_ms >= items[1].last_opened_at_ms)
    );

    let duplicate = recent_project("newest", 600);
    let remembered_again = store
        .remember_if_revision(&remembered.snapshot.revision, duplicate.clone())
        .unwrap();
    assert_eq!(
        remembered_again
            .snapshot
            .projects
            .iter()
            .filter(|project| project.id == duplicate.id)
            .count(),
        1
    );
    assert_eq!(
        remembered_again.snapshot.projects[0].last_opened_at_ms,
        Some(600)
    );

    let exact = remembered_again.snapshot.projects[1].id.clone();
    let mut similar_project = recent_project("similar", 550);
    similar_project.id = format!("{exact}-suffix");
    let with_similar = store
        .remember_if_revision(&remembered_again.snapshot.revision, similar_project.clone())
        .unwrap();
    let forgotten = store
        .forget_if_revision(&with_similar.snapshot.revision, &exact)
        .unwrap();
    assert_eq!(forgotten.affected, 1);
    assert!(
        forgotten
            .snapshot
            .projects
            .iter()
            .all(|project| project.id != exact)
    );
    assert!(
        forgotten
            .snapshot
            .projects
            .iter()
            .any(|project| project.id == similar_project.id)
    );
}

#[test]
fn registry_store_serializes_two_instances_without_lost_updates() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recent.json");
    let first_store = ZedRemoteRegistryStore::new(path.clone());
    let second_store = ZedRemoteRegistryStore::new(path.clone());
    let revision = first_store.inspect().unwrap().revision;
    let barrier = Arc::new(Barrier::new(3));

    let first = {
        let barrier = Arc::clone(&barrier);
        let revision = revision.clone();
        std::thread::spawn(move || {
            barrier.wait();
            first_store.remember_if_revision(&revision, recent_project("a", 20))
        })
    };
    let second = {
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            second_store.remember_if_revision(&revision, recent_project("b", 30))
        })
    };
    barrier.wait();

    let outcomes = [first.join().unwrap(), second.join().unwrap()];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(ZedRemoteError::RegistryConflict)))
            .count(),
        1
    );
    assert_eq!(
        ZedRemoteRegistryStore::new(path)
            .inspect()
            .unwrap()
            .projects
            .len(),
        1
    );
}

fn recent_project(suffix: &str, last_opened_at_ms: i64) -> ZedRemoteProject {
    ZedRemoteProject {
        id: format!("zed-remote-project:{suffix}"),
        label: format!("project-{suffix}"),
        host_id: format!("host-{suffix}"),
        ssh: SshTarget {
            user: "alice".to_string(),
            host: format!("{suffix}.example.test"),
            port: Some(2222),
        },
        path: format!("/srv/{suffix}"),
        url: format!("ssh://alice@{suffix}.example.test:2222/srv/{suffix}"),
        source: ZedRemoteProjectSource::Recent,
        last_opened_at_ms: Some(last_opened_at_ms),
        is_current: false,
    }
}

#[test]
fn registry_lists_remote_projects_from_global_state() {
    let state = json!({
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
            "label": "sealos-skills",
        }],
        "project-order": ["main"],
    });
    let temp = tempfile::tempdir().unwrap();
    let projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({}),
        Some(&temp.path().join("recent.json")),
        None,
    )
    .unwrap();

    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].label, "sealos-skills");
    assert_eq!(
        projects[0].source,
        ZedRemoteProjectSource::CodexRemoteProject
    );
    assert_eq!(
        projects[0].url,
        "ssh://longnv@192.168.100.31/Users/longnv/bin/repo/sealos-skills"
    );
}

#[test]
fn registry_prefers_current_thread_workspace_hint() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "thread-workspace-root-hints": {
            "019e39c1-worktree": "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        },
    });
    let temp = tempfile::tempdir().unwrap();
    let projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({"threadId": "019e39c1-worktree"}),
        Some(&temp.path().join("recent.json")),
        None,
    )
    .unwrap();
    let current = projects.iter().find(|project| project.is_current).unwrap();

    assert_eq!(
        current.path,
        "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    );
    assert_eq!(current.source, ZedRemoteProjectSource::CurrentThread);
}

#[test]
fn registry_dedupes_same_user_host_port_path() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "thread-workspace-root-hints": {
            "019e39c1-worktree": "/Users/longnv/bin/repo/sealos-skills",
        },
    });
    let temp = tempfile::tempdir().unwrap();
    let projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({"threadId": "019e39c1-worktree"}),
        Some(&temp.path().join("recent.json")),
        None,
    )
    .unwrap();

    assert_eq!(
        projects
            .iter()
            .filter(|project| project.path == "/Users/longnv/bin/repo/sealos-skills")
            .count(),
        1
    );
}

#[test]
fn registry_marks_recent_opened_project() {
    let state = json!({
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
            "label": "sealos-skills",
        }],
    });
    let temp = tempfile::tempdir().unwrap();
    let registry_path = temp.path().join("recent.json");
    let mut projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({}),
        Some(&registry_path),
        None,
    )
    .unwrap();
    projects[0].source = ZedRemoteProjectSource::Recent;
    projects[0].last_opened_at_ms = Some(42);
    std::fs::write(
        &registry_path,
        serde_json::to_vec(&json!({ "projects": projects })).unwrap(),
    )
    .unwrap();

    let projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({}),
        Some(&registry_path),
        None,
    )
    .unwrap();

    assert_eq!(projects[0].last_opened_at_ms, Some(42));
}

#[test]
fn registry_lists_sqlite_thread_cwd_candidates() {
    let state = json!({
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
    });
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("state_5.sqlite");
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT NOT NULL)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads (id, cwd) VALUES (?1, ?2)",
        (
            "019e39c1-worktree",
            "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        ),
    )
    .unwrap();
    drop(db);

    let projects = zed_remote::list_zed_remote_projects_from_state(
        &state,
        &json!({}),
        Some(&temp.path().join("recent.json")),
        Some(&db_path),
    )
    .unwrap();

    assert!(projects.iter().any(|project| {
        project.source == ZedRemoteProjectSource::SqliteThreadCwd
            && project.path == "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    }));
}

#[test]
fn resolve_ssh_target_from_global_state_for_codex_managed_connection() {
    let state = json!({
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "displayName": "remote",
            "source": "codex-managed",
            "hostname": "longnv@192.168.100.31",
            "sshPort": null,
        }]
    });

    let target =
        zed_remote::resolve_ssh_target_from_global_state(&state, "remote-ssh-codex-managed:remote")
            .unwrap();

    assert_eq!(
        target,
        SshTarget {
            user: "longnv".to_string(),
            host: "192.168.100.31".to_string(),
            port: None,
        }
    );
}

#[test]
fn fallback_open_request_uses_selected_remote_project() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
            "sshPort": null,
        }],
        "remote-projects": [{
            "id": "032e652b-7956-4e6e-83bd-b29f456c6c3d",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
            "label": "sealos-skills",
        }],
        "project-order": ["032e652b-7956-4e6e-83bd-b29f456c6c3d"],
    });

    let request =
        zed_remote::fallback_open_request_from_global_state_with_context(&state, "", "", "", "")
            .unwrap();

    assert_eq!(
        request,
        json!({
            "hostId": "remote-ssh-codex-managed:remote",
            "ssh": {"user": "longnv", "host": "192.168.100.31", "port": null},
            "path": "/Users/longnv/bin/repo/sealos-skills",
        })
    );
}

#[test]
fn fallback_open_request_prefers_project_order_for_selected_host() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [
            {"id": "old", "hostId": "remote-ssh-codex-managed:remote", "remotePath": "/Users/longnv/bin/repo/old"},
            {"id": "current", "hostId": "remote-ssh-codex-managed:remote", "remotePath": "/Users/longnv/bin/repo/current"},
            {"id": "other-host", "hostId": "remote-ssh-codex-managed:other", "remotePath": "/srv/other"}
        ],
        "project-order": ["other-host", "current", "old"],
    });

    let request =
        zed_remote::fallback_open_request_from_global_state_with_context(&state, "", "", "", "")
            .unwrap();

    assert_eq!(request["hostId"], "remote-ssh-codex-managed:remote");
    assert_eq!(request["path"], "/Users/longnv/bin/repo/current");
}

#[test]
fn fallback_open_request_prefers_remote_project_id_context() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [
            {
                "id": "032e652b-7956-4e6e-83bd-b29f456c6c3d",
                "hostId": "remote-ssh-codex-managed:remote",
                "remotePath": "/Users/longnv/bin/repo/sealos-skills",
            },
            {
                "id": "a21be7c9-a917-433a-bfc7-f422a34c2185",
                "hostId": "remote-ssh-codex-managed:remote",
                "remotePath": "/Users/longnv/bin/repo/Vocabloom",
            },
        ],
        "project-order": ["032e652b-7956-4e6e-83bd-b29f456c6c3d", "a21be7c9-a917-433a-bfc7-f422a34c2185"],
    });

    let request = zed_remote::fallback_open_request_from_global_state_with_context(
        &state,
        "remote-ssh-codex-managed:remote",
        "",
        "",
        "a21be7c9-a917-433a-bfc7-f422a34c2185",
    )
    .unwrap();

    assert_eq!(request["hostId"], "remote-ssh-codex-managed:remote");
    assert_eq!(request["path"], "/Users/longnv/bin/repo/Vocabloom");
}

#[test]
fn fallback_open_request_treats_remote_project_id_as_path() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "032e652b-7956-4e6e-83bd-b29f456c6c3d",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "project-order": ["032e652b-7956-4e6e-83bd-b29f456c6c3d"],
    });

    let request = zed_remote::fallback_open_request_from_global_state_with_context(
        &state,
        "remote-ssh-codex-managed:remote",
        "",
        "",
        "/Users/longnv/bin/repo/Vocabloom",
    )
    .unwrap();

    assert_eq!(request["hostId"], "remote-ssh-codex-managed:remote");
    assert_eq!(request["path"], "/Users/longnv/bin/repo/Vocabloom");
}

#[test]
fn fallback_open_request_prefers_thread_workspace_hint() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "project-order": ["main"],
        "thread-workspace-root-hints": {
            "019e39c1-worktree": "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        },
    });

    let request = zed_remote::fallback_open_request_from_global_state_with_context(
        &state,
        "",
        "019e39c1-worktree",
        "",
        "",
    )
    .unwrap();

    assert_eq!(request["hostId"], "remote-ssh-codex-managed:remote");
    assert_eq!(
        request["path"],
        "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    );
}

#[test]
fn fallback_open_request_accepts_local_prefixed_thread_workspace_hint() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "project-order": ["main"],
        "thread-workspace-root-hints": {
            "019e39c1-worktree": "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        },
    });

    let request = zed_remote::fallback_open_request_from_global_state_with_context(
        &state,
        "",
        "local:019e39c1-worktree",
        "",
        "",
    )
    .unwrap();

    assert_eq!(request["hostId"], "remote-ssh-codex-managed:remote");
    assert_eq!(
        request["path"],
        "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    );
}

#[test]
fn fallback_open_request_response_passes_thread_workspace_hint() {
    let state = json!({
        "selected-remote-host-id": "remote-ssh-codex-managed:remote",
        "codex-managed-remote-connections": [{
            "hostId": "remote-ssh-codex-managed:remote",
            "hostname": "longnv@192.168.100.31",
        }],
        "remote-projects": [{
            "id": "main",
            "hostId": "remote-ssh-codex-managed:remote",
            "remotePath": "/Users/longnv/bin/repo/sealos-skills",
        }],
        "thread-workspace-root-hints": {
            "019e39c1-worktree": "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        },
    });

    let request = zed_remote::fallback_open_request_from_global_state_with_context(
        &state,
        "",
        "019e39c1-worktree",
        "",
        "",
    )
    .unwrap();

    assert_eq!(
        request["path"],
        "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    );
}

#[test]
fn workspace_root_from_sqlite_reads_thread_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT NOT NULL)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads (id, cwd) VALUES (?1, ?2)",
        (
            "019e39c1-worktree",
            "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix",
        ),
    )
    .unwrap();
    drop(db);

    let cwd = zed_remote::workspace_root_from_sqlite("local:019e39c1-worktree", Some(&db_path));

    assert_eq!(
        cwd,
        "/Users/longnv/bin/repo/sealos-skills/.worktree/zed-fix"
    );
}

#[test]
fn fallback_open_request_reports_missing_remote_project() {
    let state = json!({"selected-remote-host-id": "remote-ssh-codex-managed:remote"});

    let error =
        zed_remote::fallback_open_request_from_global_state_with_context(&state, "", "", "", "")
            .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Cannot determine remote workspace or file for Zed"
    );
}

#[test]
fn resolve_ssh_target_response_reports_missing_host_id() {
    let result = zed_remote::resolve_ssh_target_response(&json!({"hostId": ""}));

    assert_eq!(
        result,
        json!({"status": "failed", "message": "Remote host id is required"})
    );
}

#[test]
fn open_zed_remote_returns_failed_response_for_validation_error() {
    let result = zed_remote::open_zed_remote(&json!({"ssh": {"host": ""}, "path": "/a.py"}));

    assert_eq!(
        result,
        json!({"status": "failed", "message": "Cannot determine remote SSH host for this file"})
    );
}
