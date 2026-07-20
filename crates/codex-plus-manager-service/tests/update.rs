use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use codex_plus_core::update::UpdateTarget;
use codex_plus_manager_service::{
    InstallUpdate, SystemUpdateEnvironment, UpdateAvailability, UpdateDownload, UpdateEnvironment,
    UpdateEnvironmentError, UpdateEnvironmentErrorKind, UpdateErrorKind, UpdateLimits,
    UpdateProgress, UpdateService,
};
use serde_json::json;

#[derive(Clone)]
struct FakeEnvironment {
    state: Arc<Mutex<FakeState>>,
}

struct FakeState {
    current_version: String,
    metadata: Vec<u8>,
    final_url: String,
    content_length: Option<u64>,
    chunks: Vec<Result<Vec<u8>, UpdateEnvironmentError>>,
    written: Arc<Mutex<Vec<u8>>>,
    created: usize,
    published: usize,
    cleaned: usize,
    launched: usize,
}

impl FakeEnvironment {
    fn available() -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                current_version: "1.0.0".to_owned(),
                metadata: metadata("2.0.0", "https://secret.example.test/setup.exe"),
                final_url: "https://objects.example.test/setup.exe".to_owned(),
                content_length: Some(5),
                chunks: vec![Ok(b"ab".to_vec()), Ok(b"cde".to_vec())],
                written: Arc::new(Mutex::new(Vec::new())),
                created: 0,
                published: 0,
                cleaned: 0,
                launched: 0,
            })),
        }
    }

    fn update(&self, mutate: impl FnOnce(&mut FakeState)) {
        mutate(&mut self.state.lock().unwrap());
    }
}

struct FakeArtifact {
    written: Arc<Mutex<Vec<u8>>>,
}

impl Write for FakeArtifact {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.written.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl UpdateEnvironment for FakeEnvironment {
    type Artifact = FakeArtifact;

    fn current_version(&self) -> String {
        self.state.lock().unwrap().current_version.clone()
    }

    fn target(&self) -> UpdateTarget {
        UpdateTarget::WindowsX64
    }

    fn fetch_release_metadata(
        &self,
        _maximum_bytes: usize,
    ) -> Result<Vec<u8>, UpdateEnvironmentError> {
        Ok(self.state.lock().unwrap().metadata.clone())
    }

    fn open_asset_download(&self, _url: &str) -> Result<UpdateDownload, UpdateEnvironmentError> {
        let state = self.state.lock().unwrap();
        Ok(UpdateDownload::new(
            state.final_url.clone(),
            state.content_length,
            state.chunks.clone().into_iter(),
        ))
    }

    fn create_update_artifact(
        &self,
        _safe_name: &str,
    ) -> Result<Self::Artifact, UpdateEnvironmentError> {
        let mut state = self.state.lock().unwrap();
        state.created += 1;
        Ok(FakeArtifact {
            written: Arc::clone(&state.written),
        })
    }

    fn publish_update_artifact(
        &self,
        _artifact: &mut Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        self.state.lock().unwrap().published += 1;
        Ok(())
    }

    fn cleanup_update_artifact(&self, _artifact: &mut Self::Artifact) {
        self.state.lock().unwrap().cleaned += 1;
    }

    fn launch_update_artifact(
        &self,
        _artifact: &Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        self.state.lock().unwrap().launched += 1;
        Ok(())
    }
}

fn metadata(version: &str, asset_url: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "version": version,
        "body": "safe summary",
        "assets": [{
            "name": format!("CodexPlusPlus-{version}-windows-x64-setup.exe"),
            "url": asset_url
        }]
    }))
    .unwrap()
}

fn available_candidate(
    result: codex_plus_manager_service::UpdateCheckResult,
) -> codex_plus_manager_service::UpdateCandidate {
    match result.availability {
        UpdateAvailability::Available(candidate) => candidate,
        other => panic!("expected available candidate, got {other:?}"),
    }
}

#[test]
fn check_returns_safe_candidate_and_replaces_the_previous_revision() {
    let environment = FakeEnvironment::available();
    let service = UpdateService::new(environment.clone());

    let first = available_candidate(service.check().unwrap());
    assert_eq!(first.version, "2.0.0");
    assert!(first.asset_name.ends_with("setup.exe"));
    let debug = format!("{first:?}");
    assert!(!debug.contains("secret.example.test"));
    assert!(!debug.contains("https://"));

    environment.update(|state| {
        state.metadata = metadata("2.1.0", "https://second.example.test/setup.exe");
    });
    let second = available_candidate(service.check().unwrap());
    assert_ne!(first.revision, second.revision);

    let error = service
        .install(
            InstallUpdate {
                revision: first.revision,
                confirmed_version: first.version,
            },
            Arc::new(|_| {}),
        )
        .unwrap_err();
    assert_eq!(error.kind(), UpdateErrorKind::InvalidRevision);
}

#[test]
fn check_result_debug_omits_release_summary_and_private_transport_data() {
    let environment = FakeEnvironment::available();
    environment.update(|state| {
        state.metadata = serde_json::to_vec(&json!({
            "version": "2.0.0",
            "body": "notes https://private.example.test/C:/secret/token",
            "assets": [{
                "name": "CodexPlusPlus-2.0.0-windows-x64-setup.exe",
                "url": "https://asset-secret.example.test/setup.exe"
            }]
        }))
        .unwrap();
    });
    let service = UpdateService::new(environment);

    let result = service.check().unwrap();
    let debug = format!("{result:?}");

    assert!(debug.contains("summary_bytes"));
    assert!(!debug.contains("private.example.test"));
    assert!(!debug.contains("asset-secret.example.test"));
    assert!(!debug.contains("C:/secret/token"));
}

#[test]
fn failed_new_check_invalidates_the_previous_candidate() {
    let environment = FakeEnvironment::available();
    let service = UpdateService::new(environment.clone());
    let candidate = available_candidate(service.check().unwrap());
    environment.update(|state| state.metadata = b"not-json".to_vec());

    assert_eq!(
        service.check().unwrap_err().kind(),
        UpdateErrorKind::MetadataInvalid
    );
    let error = service
        .install(
            InstallUpdate {
                revision: candidate.revision,
                confirmed_version: candidate.version,
            },
            Arc::new(|_| {}),
        )
        .unwrap_err();
    assert_eq!(error.kind(), UpdateErrorKind::InvalidRevision);
}

#[test]
fn confirmed_install_streams_progress_publishes_and_launches_once() {
    let environment = FakeEnvironment::available();
    let service = UpdateService::new(environment.clone());
    let candidate = available_candidate(service.check().unwrap());
    let progress = Arc::new(Mutex::new(Vec::<UpdateProgress>::new()));
    let progress_sink = Arc::clone(&progress);

    let started = service
        .install(
            InstallUpdate {
                revision: candidate.revision,
                confirmed_version: candidate.version.clone(),
            },
            Arc::new(move |update| progress_sink.lock().unwrap().push(update)),
        )
        .unwrap();

    assert_eq!(started.version, "2.0.0");
    let state = environment.state.lock().unwrap();
    assert_eq!(*state.written.lock().unwrap(), b"abcde");
    assert_eq!(
        (
            state.created,
            state.published,
            state.cleaned,
            state.launched
        ),
        (1, 1, 0, 1)
    );
    assert_eq!(
        *progress.lock().unwrap(),
        vec![
            UpdateProgress {
                downloaded_bytes: 2,
                total_bytes: Some(5),
            },
            UpdateProgress {
                downloaded_bytes: 5,
                total_bytes: Some(5),
            },
        ]
    );
    drop(state);

    let error = service
        .install(
            InstallUpdate {
                revision: candidate.revision,
                confirmed_version: candidate.version,
            },
            Arc::new(|_| {}),
        )
        .unwrap_err();
    assert_eq!(error.kind(), UpdateErrorKind::InvalidRevision);
}

#[test]
fn invalid_confirmation_and_stream_failures_never_launch() {
    let environment = FakeEnvironment::available();
    let service = UpdateService::with_limits(environment.clone(), UpdateLimits::new(64 * 1024, 5));
    let candidate = available_candidate(service.check().unwrap());

    let error = service
        .install(
            InstallUpdate {
                revision: candidate.revision,
                confirmed_version: "wrong".to_owned(),
            },
            Arc::new(|_| {}),
        )
        .unwrap_err();
    assert_eq!(error.kind(), UpdateErrorKind::ConfirmationRequired);
    assert_eq!(environment.state.lock().unwrap().created, 0);

    let error = service
        .install(
            InstallUpdate {
                revision: candidate.revision,
                confirmed_version: candidate.version.clone(),
            },
            Arc::new(|_| {}),
        )
        .unwrap_err();
    assert_eq!(error.kind(), UpdateErrorKind::InvalidRevision);

    environment.update(|state| {
        state.content_length = None;
        state.chunks = vec![Ok(b"abc".to_vec()), Ok(b"def".to_vec())];
    });
    let candidate = available_candidate(service.check().unwrap());
    let error = service
        .install(
            InstallUpdate {
                revision: candidate.revision,
                confirmed_version: candidate.version,
            },
            Arc::new(|_| {}),
        )
        .unwrap_err();
    assert_eq!(error.kind(), UpdateErrorKind::DownloadTooLarge);

    let state = environment.state.lock().unwrap();
    assert_eq!(
        (
            state.created,
            state.published,
            state.cleaned,
            state.launched
        ),
        (1, 0, 1, 0)
    );
}

#[test]
fn insecure_final_redirect_is_rejected_before_file_creation() {
    let environment = FakeEnvironment::available();
    environment.update(|state| {
        state.final_url = "http://objects.example.test/setup.exe".to_owned();
    });
    let service = UpdateService::new(environment.clone());
    let candidate = available_candidate(service.check().unwrap());

    let error = service
        .install(
            InstallUpdate {
                revision: candidate.revision,
                confirmed_version: candidate.version,
            },
            Arc::new(|_| {}),
        )
        .unwrap_err();

    assert_eq!(error.kind(), UpdateErrorKind::InsecureAsset);
    let state = environment.state.lock().unwrap();
    assert_eq!(
        (
            state.created,
            state.published,
            state.cleaned,
            state.launched
        ),
        (0, 0, 0, 0)
    );
}

#[test]
fn environment_error_debug_does_not_expose_detail() {
    let error = UpdateEnvironmentError::new(
        UpdateEnvironmentErrorKind::Transport,
        "https://secret.example.test/token/private/path",
    );
    let debug = format!("{error:?}");

    assert!(debug.contains("Transport"));
    assert!(!debug.contains("secret.example.test"));
    assert!(!debug.contains("private/path"));
}

#[test]
fn system_artifacts_publish_without_overwrite_and_cleanup_only_owned_files() {
    let root = tempfile::tempdir().unwrap();
    let environment = SystemUpdateEnvironment::new(root.path().join("private-updates")).unwrap();
    let mut first = environment
        .create_update_artifact("CodexPlusPlus-2.0.0-windows-x64-setup.exe")
        .unwrap();
    let mut second = environment
        .create_update_artifact("CodexPlusPlus-2.0.0-windows-x64-setup.exe")
        .unwrap();
    first.write_all(b"first").unwrap();
    second.write_all(b"second").unwrap();
    environment.publish_update_artifact(&mut first).unwrap();
    environment.publish_update_artifact(&mut second).unwrap();

    let update_root = root.path().join("private-updates");
    let files = std::fs::read_dir(&update_root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 2);
    assert!(
        files
            .iter()
            .all(|path| path.extension().and_then(|value| value.to_str()) != Some("part"))
    );
    let mut contents = files
        .iter()
        .map(|path| std::fs::read(path).unwrap())
        .collect::<Vec<_>>();
    contents.sort();
    assert_eq!(contents, vec![b"first".to_vec(), b"second".to_vec()]);

    let debug = format!("{environment:?} {first:?}");
    assert!(!debug.contains("private-updates"));
    environment.cleanup_update_artifact(&mut first);
    assert_eq!(std::fs::read_dir(&update_root).unwrap().count(), 1);
    environment.cleanup_update_artifact(&mut second);
    assert_eq!(std::fs::read_dir(&update_root).unwrap().count(), 0);
}
