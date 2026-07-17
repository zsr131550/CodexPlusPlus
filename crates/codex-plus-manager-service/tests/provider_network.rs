use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use codex_plus_core::settings::{BackendSettings, RelayMode, RelayProfile};
use codex_plus_manager_service::{
    DiagnoseProviderProfile, DoctorDetailKind, DoctorOutcome, FetchProviderModels,
    NetworkModelsResponse, NetworkTestResponse, ProviderDoctorCheckId, ProviderEnvironment,
    ProviderEnvironmentNetworkError, ProviderModelsResult, ProviderNetworkEnvironment,
    ProviderNetworkFailureKind, ProviderProfile, ProviderService, ProviderSource,
    ProviderTestOutcome, TestProviderProfile, masked_auth_preview, masked_config_preview,
};
use serde_json::Value;

const SECRET: &str = "sentinel-secret-must-never-escape";

#[derive(Clone)]
struct FakeEnvironment {
    settings: BackendSettings,
    test_result: Arc<Mutex<Result<NetworkTestResponse, ProviderEnvironmentNetworkError>>>,
    models_result: Arc<Mutex<Result<NetworkModelsResponse, ProviderEnvironmentNetworkError>>>,
    test_calls: Arc<AtomicUsize>,
    model_calls: Arc<AtomicUsize>,
}

impl FakeEnvironment {
    fn new(
        test_result: Result<NetworkTestResponse, ProviderEnvironmentNetworkError>,
        models_result: Result<NetworkModelsResponse, ProviderEnvironmentNetworkError>,
    ) -> Self {
        Self {
            settings: BackendSettings::default(),
            test_result: Arc::new(Mutex::new(test_result)),
            models_result: Arc::new(Mutex::new(models_result)),
            test_calls: Arc::new(AtomicUsize::new(0)),
            model_calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ProviderEnvironment for FakeEnvironment {
    fn load_settings(&self) -> anyhow::Result<BackendSettings> {
        Ok(self.settings.clone())
    }

    fn update_settings_if<F>(
        &self,
        _payload: Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool,
    {
        Ok(predicate(&self.settings).then(|| self.settings.clone()))
    }
}

impl ProviderNetworkEnvironment for FakeEnvironment {
    fn test_provider(
        &self,
        _profile: &RelayProfile,
        _model: &str,
    ) -> Result<NetworkTestResponse, ProviderEnvironmentNetworkError> {
        self.test_calls.fetch_add(1, Ordering::Relaxed);
        self.test_result.lock().unwrap().clone()
    }

    fn fetch_provider_models(
        &self,
        _profile: &RelayProfile,
    ) -> Result<NetworkModelsResponse, ProviderEnvironmentNetworkError> {
        self.model_calls.fetch_add(1, Ordering::Relaxed);
        self.models_result.lock().unwrap().clone()
    }
}

fn raw_endpoint() -> String {
    format!("https://user:{SECRET}@api.example.test/v1?token={SECRET}#{SECRET}")
}

fn ready_environment() -> FakeEnvironment {
    FakeEnvironment::new(
        Ok(NetworkTestResponse {
            http_status: 200,
            endpoint: raw_endpoint(),
        }),
        Ok(NetworkModelsResponse {
            models: vec!["model-a".to_string(), "model-b".to_string()],
            endpoint: raw_endpoint(),
        }),
    )
}

fn api_profile() -> RelayProfile {
    RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "model-a".to_string(),
        base_url: raw_endpoint(),
        upstream_base_url: raw_endpoint(),
        api_key: SECRET.to_string(),
        relay_mode: RelayMode::PureApi,
        test_model: "model-a".to_string(),
        config_contents: format!("secret_token = \"{SECRET}\"\npublic_value = \"visible\"\n"),
        auth_contents: format!(r#"{{"OPENAI_API_KEY":"{SECRET}","nested":[1,true]}}"#),
        ..RelayProfile::default()
    }
}

fn ordinary_request(profile: RelayProfile) -> TestProviderProfile {
    TestProviderProfile {
        profile: ProviderProfile::Ordinary(profile),
        default_test_model: "fallback-model".to_string(),
    }
}

#[test]
fn connectivity_short_circuits_official_aggregate_and_missing_configuration() {
    let environment = ready_environment();
    let calls = environment.test_calls.clone();
    let source = ProviderService::new(environment);
    let official = RelayProfile {
        relay_mode: RelayMode::Official,
        official_mix_api_key: false,
        ..api_profile()
    };
    let aggregate = RelayProfile {
        relay_mode: RelayMode::Aggregate,
        ..api_profile()
    };
    let missing = RelayProfile {
        base_url: String::new(),
        upstream_base_url: String::new(),
        api_key: String::new(),
        auth_contents: String::new(),
        config_contents: String::new(),
        ..api_profile()
    };

    let official = source.test_profile(ordinary_request(official)).unwrap();
    let aggregate = source
        .test_profile(TestProviderProfile {
            profile: ProviderProfile::Aggregate {
                shell: aggregate,
                routing: codex_plus_core::settings::AggregateRelayProfile {
                    id: "relay-a".to_string(),
                    name: "Aggregate".to_string(),
                    strategy: Default::default(),
                    members: Vec::new(),
                },
            },
            default_test_model: String::new(),
        })
        .unwrap();
    let missing = source.test_profile(ordinary_request(missing)).unwrap();

    assert_eq!(official.outcome, ProviderTestOutcome::OfficialNoApiRequired);
    assert_eq!(
        aggregate.outcome,
        ProviderTestOutcome::Failure(ProviderNetworkFailureKind::AggregateUnsupported)
    );
    assert_eq!(
        missing.outcome,
        ProviderTestOutcome::Failure(ProviderNetworkFailureKind::MissingConfiguration)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn connectivity_maps_http_statuses_to_stable_outcomes() {
    let cases = [
        (200, ProviderTestOutcome::Success),
        (
            401,
            ProviderTestOutcome::Failure(ProviderNetworkFailureKind::Unauthorized),
        ),
        (
            404,
            ProviderTestOutcome::Failure(ProviderNetworkFailureKind::NotFound),
        ),
        (
            429,
            ProviderTestOutcome::Failure(ProviderNetworkFailureKind::RateLimited),
        ),
        (
            503,
            ProviderTestOutcome::Failure(ProviderNetworkFailureKind::UpstreamFailure),
        ),
    ];

    for (status, expected) in cases {
        let environment = FakeEnvironment::new(
            Ok(NetworkTestResponse {
                http_status: status,
                endpoint: raw_endpoint(),
            }),
            ready_environment().models_result.lock().unwrap().clone(),
        );
        let result = ProviderService::new(environment)
            .test_profile(ordinary_request(api_profile()))
            .unwrap();

        assert_eq!(result.outcome, expected);
        assert_eq!(result.http_status, Some(status));
        assert_eq!(
            result.endpoint.as_ref().unwrap().as_str(),
            "https://api.example.test/v1"
        );
        assert!(!format!("{result:?}").contains(SECRET));
    }
}

#[test]
fn model_discovery_returns_safe_results_and_stable_failures() {
    let source = ProviderService::new(ready_environment());
    let result: ProviderModelsResult = source
        .fetch_models(FetchProviderModels {
            profile: ProviderProfile::Ordinary(api_profile()),
        })
        .unwrap();

    assert_eq!(result.models, ["model-a", "model-b"]);
    assert_eq!(result.endpoint.as_str(), "https://api.example.test/v1");
    assert!(!format!("{result:?}").contains(SECRET));

    let empty = FakeEnvironment::new(
        ready_environment().test_result.lock().unwrap().clone(),
        Ok(NetworkModelsResponse {
            models: Vec::new(),
            endpoint: raw_endpoint(),
        }),
    );
    let error = ProviderService::new(empty)
        .fetch_models(FetchProviderModels {
            profile: ProviderProfile::Ordinary(api_profile()),
        })
        .unwrap_err();
    assert_eq!(error.kind(), ProviderNetworkFailureKind::InvalidResponse);

    let timeout = FakeEnvironment::new(
        ready_environment().test_result.lock().unwrap().clone(),
        Err(ProviderEnvironmentNetworkError::new(
            ProviderNetworkFailureKind::Timeout,
            None,
            Some(raw_endpoint()),
            format!("transport failed with {SECRET}"),
        )),
    );
    let error = ProviderService::new(timeout)
        .fetch_models(FetchProviderModels {
            profile: ProviderProfile::Ordinary(api_profile()),
        })
        .unwrap_err();
    assert_eq!(error.kind(), ProviderNetworkFailureKind::Timeout);
    assert!(!format!("{error:?} {error}").contains(SECRET));
}

#[test]
fn doctor_warns_when_test_model_is_missing_from_discovery() {
    let environment = FakeEnvironment::new(
        Ok(NetworkTestResponse {
            http_status: 200,
            endpoint: raw_endpoint(),
        }),
        Ok(NetworkModelsResponse {
            models: vec!["different-model".to_string()],
            endpoint: raw_endpoint(),
        }),
    );
    let report = ProviderService::new(environment)
        .diagnose_profile(DiagnoseProviderProfile {
            profile: ProviderProfile::Ordinary(api_profile()),
            default_test_model: "fallback-model".to_string(),
        })
        .unwrap();

    assert_eq!(report.outcome, DoctorOutcome::Warning);
    assert!(report.checks.iter().any(|check| {
        check.id == ProviderDoctorCheckId::Models
            && check.detail == DoctorDetailKind::TestModelMissing
    }));
    assert!(report.checks.iter().any(|check| {
        check.id == ProviderDoctorCheckId::Request
            && check.detail == DoctorDetailKind::RequestSucceeded
    }));
    assert!(!format!("{report:?}").contains(SECRET));
}

#[test]
fn masked_previews_never_echo_secrets_or_invalid_raw_contents() {
    let auth = format!(r#"{{"token":"{SECRET}","nested":[1,true,null]}}"#);
    let config = format!("api_key = \"{SECRET}\"\npublic_value = \"visible\"\n");

    let masked_auth = masked_auth_preview(&auth);
    let masked_config = masked_config_preview(&config);

    assert!(!masked_auth.contains(SECRET));
    assert!(masked_auth.contains("token"));
    assert!(!masked_config.contains(SECRET));
    assert!(masked_config.contains("visible"));
    assert_eq!(
        masked_auth_preview(&format!("invalid {SECRET}")),
        "<invalid auth JSON>"
    );
    assert_eq!(
        masked_config_preview(&format!("[invalid {SECRET}")),
        "<invalid config TOML>"
    );
}
