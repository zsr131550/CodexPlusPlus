use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

#[test]
#[ignore = "opens a real native window and tray icon"]
fn desktop_host_real_window_primary_secondary_protocol_and_tray_smoke() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = SmokeFixture::new(temp.path());
    let mut primary = ChildGuard::new(fixture.command().spawn().unwrap());

    wait_for_file(
        &fixture.endpoint_path,
        &mut primary,
        Duration::from_secs(20),
    );
    assert!(primary.try_wait().unwrap().is_none());

    let protocol_status = wait_for_exit(
        fixture
            .command()
            .arg(provider_url("Desktop Host Smoke"))
            .spawn()
            .unwrap(),
        Duration::from_secs(5),
    );
    assert!(protocol_status.success());
    let pending = codex_plus_core::provider_import::load_pending_provider_import_at(
        &fixture.pending_import_path,
    )
    .unwrap()
    .unwrap();
    assert_eq!(pending.name, "Desktop Host Smoke");
    assert!(!pending.api_key.is_empty());
    assert!(primary.try_wait().unwrap().is_none());

    let update_status = wait_for_exit(
        fixture.command().arg("--show-update").spawn().unwrap(),
        Duration::from_secs(5),
    );
    assert!(update_status.success());
    let show_status = wait_for_exit(fixture.command().spawn().unwrap(), Duration::from_secs(5));
    assert!(show_status.success());
    assert!(primary.try_wait().unwrap().is_none());

    let primary_status = primary.wait(Duration::from_secs(20));
    assert!(primary_status.success());
    assert!(fixture.perf_report_path.is_file());
    assert!(fixture.persistence_path.is_file());
    assert!(!fixture.endpoint_path.exists());
}

struct SmokeFixture {
    root: PathBuf,
    state_dir: PathBuf,
    endpoint_path: PathBuf,
    pending_import_path: PathBuf,
    persistence_path: PathBuf,
    perf_report_path: PathBuf,
    port: u16,
}

impl SmokeFixture {
    fn new(root: &Path) -> Self {
        let state_dir = root.join("state");
        Self {
            root: root.to_path_buf(),
            endpoint_path: state_dir.join("manager-instance-endpoint.json"),
            pending_import_path: root.join("pending-provider-import.json"),
            persistence_path: state_dir.join("manager-ui/app.ron"),
            perf_report_path: root.join("perf.json"),
            state_dir,
            port: available_port(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codex-plus-plus-manager-native"));
        command
            .env("CODEX_PLUS_GUARD_PORT", self.port.to_string())
            .env("CODEX_PLUS_NATIVE_STATE_DIR", &self.state_dir)
            .env(
                "CODEX_PLUS_NATIVE_SETTINGS_PATH",
                self.root.join("settings.json"),
            )
            .env("CODEX_PLUS_NATIVE_CODEX_HOME", self.root.join("codex-home"))
            .env(
                "CODEX_PLUS_NATIVE_CCS_DB_PATH",
                self.root.join("cc-switch.db"),
            )
            .env(
                "CODEX_PLUS_NATIVE_PENDING_IMPORT_PATH",
                &self.pending_import_path,
            )
            .env("CODEX_PLUS_NATIVE_BACKUP_DIR", self.root.join("backups"))
            .env(
                "CODEX_PLUS_NATIVE_CONTEXT_OWNERSHIP_PATH",
                self.root.join("context-live-ownership.json"),
            )
            .env(
                "CODEX_PLUS_NATIVE_DIAGNOSTIC_LOG_PATH",
                self.root.join("diagnostic.jsonl"),
            )
            .env(
                "CODEX_PLUS_NATIVE_LATEST_STATUS_PATH",
                self.root.join("latest-status.json"),
            )
            .env(
                "CODEX_PLUS_NATIVE_WATCHER_DISABLED_FLAG_PATH",
                self.root.join("watcher.disabled"),
            )
            .env("CODEX_PLUS_NATIVE_PERF_REPORT", &self.perf_report_path)
            .env("CODEX_PLUS_NATIVE_PERF_EXIT_AFTER_MS", "8000")
            .env("CODEX_PLUS_NATIVE_ENV_PROCESS_ONLY", "1");
        command
    }
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.0.as_mut().unwrap().try_wait()
    }

    fn wait(&mut self, timeout: Duration) -> ExitStatus {
        let child = self.0.take().unwrap();
        wait_for_exit(child, timeout)
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn available_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_for_file(path: &Path, child: &mut ChildGuard, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.is_file() {
            return;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("primary native manager exited before publishing endpoint: {status}");
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for primary native manager endpoint");
}

fn wait_for_exit(mut child: Child, timeout: Duration) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let status = child.wait().unwrap();
    panic!("native manager process did not exit before timeout: {status}");
}

fn provider_url(name: &str) -> String {
    let mut url = url::Url::parse("codexplusplus://v1/import/provider").unwrap();
    url.query_pairs_mut()
        .append_pair("resource", "provider")
        .append_pair("name", name)
        .append_pair("baseUrl", "https://desktop-host-smoke.invalid/v1")
        .append_pair("apiKey", "desktop-host-smoke-secret")
        .append_pair("wireApi", "responses")
        .append_pair("relayMode", "pureApi");
    url.to_string()
}
