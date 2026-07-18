use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

const SCRIPT_DURATION: Duration = Duration::from_secs(20);
const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);
const FINAL_FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PerfReport {
    pub first_ui_frame_ms: Option<f64>,
    pub overview_ready_ms: Option<f64>,
    pub cpu_frame_samples_ms: Vec<f64>,
    pub input_latency_samples_ms: Vec<f64>,
    #[serde(default)]
    pub script_actions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfScriptAction {
    NavigateProviders,
    SelectNextProvider,
    EditProviderName,
    DiscardProvider,
    RefreshLive,
    OpenLiveTab,
    RequestClearLive,
    CancelLiveConfirmation,
    ConfirmLiveMutation,
    ToggleProviderList,
    NavigateEnvironment,
    RefreshEnvironment,
    SelectFirstEnvironmentConflict,
    RequestEnvironmentCleanup,
    CancelEnvironmentCleanup,
    OpenCcsImport,
    CloseCcsImport,
    NavigateOverview,
    RefreshPendingImport,
    DismissPendingImport,
    NavigateContext,
    RefreshContext,
    SelectNextContextKind,
    CreateContextEntry,
    CancelContextEditor,
    OpenFirstContextEntry,
    ToggleFirstContextEntry,
    RequestDeleteFirstContextEntry,
    CancelContextDelete,
    PreviewContextSync,
    CancelContextSyncPreview,
    ConfirmContextSync,
}

enum PerfEvent {
    FirstUiFrame(f64),
    OverviewReady(f64),
    CpuFrame(f64),
    InputLatency(f64),
    ScriptAction(&'static str),
    Final(mpsc::Sender<()>),
}

pub struct PerfRecorder {
    process_started: Instant,
    events: mpsc::Sender<PerfEvent>,
    first_frame_recorded: bool,
    overview_ready_recorded: bool,
    pending_input_started: Option<Instant>,
    next_script_step: usize,
    exit_after: Option<Duration>,
    close_requested: bool,
    final_sent: bool,
}

impl PerfRecorder {
    pub fn from_env(process_started: Instant) -> Option<Self> {
        let report_path = std::env::var_os("CODEX_PLUS_NATIVE_PERF_REPORT")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)?;
        let exit_after = std::env::var("CODEX_PLUS_NATIVE_PERF_EXIT_AFTER_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_millis);
        let (event_tx, event_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-perf-reporter".to_owned())
            .spawn(move || report_worker(report_path, event_rx))
            .expect("spawn native performance reporter");

        Some(Self {
            process_started,
            events: event_tx,
            first_frame_recorded: false,
            overview_ready_recorded: false,
            pending_input_started: None,
            next_script_step: 0,
            exit_after,
            close_requested: false,
            final_sent: false,
        })
    }

    pub fn drive(&mut self, ctx: &egui::Context) {
        let elapsed = self.process_started.elapsed();
        if elapsed < SCRIPT_DURATION {
            ctx.request_repaint_after(FRAME_INTERVAL);
        }

        if let Some(exit_after) = self.exit_after {
            if elapsed >= exit_after {
                if !self.close_requested {
                    self.close_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            } else {
                ctx.request_repaint_after(exit_after - elapsed);
            }
        }
    }

    pub fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if let Some((due, key)) = script_step(self.next_script_step)
            && self.process_started.elapsed() >= due
        {
            let now = Instant::now();
            input.events.push(key_event(key, true));
            input.events.push(key_event(key, false));
            self.pending_input_started = Some(now);
            if let Some(action) = script_action_for_key(key) {
                self.send(PerfEvent::ScriptAction(action.name()));
            }
            self.next_script_step += 1;
            ctx.request_repaint();
        }
    }

    pub fn scripted_action(&self, ui: &egui::Ui) -> Option<PerfScriptAction> {
        ui.input(|input| {
            [
                egui::Key::F1,
                egui::Key::F2,
                egui::Key::F3,
                egui::Key::F4,
                egui::Key::F5,
                egui::Key::F6,
                egui::Key::F7,
                egui::Key::F8,
                egui::Key::F9,
                egui::Key::F10,
                egui::Key::F11,
                egui::Key::F12,
                egui::Key::F13,
                egui::Key::F14,
                egui::Key::F15,
                egui::Key::F16,
                egui::Key::F17,
                egui::Key::F18,
                egui::Key::F19,
                egui::Key::F20,
                egui::Key::F21,
                egui::Key::F22,
                egui::Key::F23,
                egui::Key::F24,
                egui::Key::F25,
                egui::Key::F26,
                egui::Key::F27,
                egui::Key::F28,
                egui::Key::F29,
                egui::Key::F30,
                egui::Key::F31,
                egui::Key::F32,
                egui::Key::F33,
                egui::Key::F34,
            ]
            .into_iter()
            .find(|key| input.key_pressed(*key))
            .and_then(script_action_for_key)
        })
    }

    pub fn record_ui_frame(&mut self, cpu_usage_seconds: Option<f32>) {
        if !self.first_frame_recorded {
            self.first_frame_recorded = true;
            self.send(PerfEvent::FirstUiFrame(elapsed_ms(self.process_started)));
        }
        if let Some(seconds) = cpu_usage_seconds {
            let milliseconds = f64::from(seconds) * 1_000.0;
            if milliseconds.is_finite() && milliseconds >= 0.0 {
                self.send(PerfEvent::CpuFrame(milliseconds));
            }
        }
        if let Some(started) = self.pending_input_started.take() {
            self.send(PerfEvent::InputLatency(elapsed_ms(started)));
        }
    }

    pub fn record_overview_ready(&mut self) {
        if !self.overview_ready_recorded {
            self.overview_ready_recorded = true;
            self.send(PerfEvent::OverviewReady(elapsed_ms(self.process_started)));
        }
    }

    pub fn finish(&mut self) {
        if self.final_sent {
            return;
        }
        self.final_sent = true;
        let (ack_tx, ack_rx) = mpsc::channel();
        if self.events.send(PerfEvent::Final(ack_tx)).is_ok() {
            let _ = ack_rx.recv_timeout(FINAL_FLUSH_TIMEOUT);
        }
    }

    fn send(&self, event: PerfEvent) {
        let _ = self.events.send(event);
    }
}

pub fn percentile_ms(samples: &[f64], percentile: f64) -> Option<f64> {
    if !percentile.is_finite() {
        return None;
    }
    let mut samples = valid_samples(samples);
    if samples.is_empty() {
        return None;
    }
    samples.sort_by(f64::total_cmp);
    let percentile = percentile.clamp(0.0, 1.0);
    let rank = ((samples.len() as f64 * percentile).ceil() as usize)
        .saturating_sub(1)
        .min(samples.len() - 1);
    Some(samples[rank])
}

pub fn maximum_ms(samples: &[f64]) -> Option<f64> {
    valid_samples(samples).into_iter().reduce(f64::max)
}

fn valid_samples(samples: &[f64]) -> Vec<f64> {
    samples
        .iter()
        .copied()
        .filter(|sample| sample.is_finite() && *sample >= 0.0)
        .collect()
}

fn script_step(index: usize) -> Option<(Duration, egui::Key)> {
    const KEYS: [egui::Key; 34] = [
        egui::Key::F1,
        egui::Key::F2,
        egui::Key::F3,
        egui::Key::F4,
        egui::Key::F5,
        egui::Key::F6,
        egui::Key::F7,
        egui::Key::F8,
        egui::Key::F9,
        egui::Key::F10,
        egui::Key::F11,
        egui::Key::F12,
        egui::Key::F13,
        egui::Key::F14,
        egui::Key::F15,
        egui::Key::F16,
        egui::Key::F17,
        egui::Key::F18,
        egui::Key::F19,
        egui::Key::F20,
        egui::Key::F21,
        egui::Key::F22,
        egui::Key::F23,
        egui::Key::F24,
        egui::Key::F25,
        egui::Key::F26,
        egui::Key::F27,
        egui::Key::F28,
        egui::Key::F29,
        egui::Key::F30,
        egui::Key::F31,
        egui::Key::F32,
        egui::Key::F33,
        egui::Key::F34,
    ];
    KEYS.get(index).map(|key| {
        let milliseconds = u64::try_from(index + 1).expect("script index fits u64") * 500;
        (Duration::from_millis(milliseconds), *key)
    })
}

fn script_action_for_key(key: egui::Key) -> Option<PerfScriptAction> {
    match key {
        egui::Key::F1 => Some(PerfScriptAction::NavigateProviders),
        egui::Key::F2 => Some(PerfScriptAction::SelectNextProvider),
        egui::Key::F3 => Some(PerfScriptAction::EditProviderName),
        egui::Key::F4 => Some(PerfScriptAction::DiscardProvider),
        egui::Key::F5 => Some(PerfScriptAction::RefreshLive),
        egui::Key::F6 => Some(PerfScriptAction::OpenLiveTab),
        egui::Key::F7 | egui::Key::F9 => Some(PerfScriptAction::RequestClearLive),
        egui::Key::F8 => Some(PerfScriptAction::CancelLiveConfirmation),
        egui::Key::F10 => Some(PerfScriptAction::ConfirmLiveMutation),
        egui::Key::F11 => Some(PerfScriptAction::ToggleProviderList),
        egui::Key::F12 => Some(PerfScriptAction::NavigateEnvironment),
        egui::Key::F13 => Some(PerfScriptAction::RefreshEnvironment),
        egui::Key::F14 => Some(PerfScriptAction::SelectFirstEnvironmentConflict),
        egui::Key::F15 => Some(PerfScriptAction::RequestEnvironmentCleanup),
        egui::Key::F16 => Some(PerfScriptAction::CancelEnvironmentCleanup),
        egui::Key::F17 => Some(PerfScriptAction::NavigateProviders),
        egui::Key::F18 => Some(PerfScriptAction::OpenCcsImport),
        egui::Key::F19 => Some(PerfScriptAction::CloseCcsImport),
        egui::Key::F20 => Some(PerfScriptAction::NavigateOverview),
        egui::Key::F21 => Some(PerfScriptAction::NavigateContext),
        egui::Key::F22 => Some(PerfScriptAction::RefreshContext),
        egui::Key::F23 => Some(PerfScriptAction::SelectNextContextKind),
        egui::Key::F24 => Some(PerfScriptAction::CreateContextEntry),
        egui::Key::F25 | egui::Key::F27 => Some(PerfScriptAction::CancelContextEditor),
        egui::Key::F26 => Some(PerfScriptAction::OpenFirstContextEntry),
        egui::Key::F28 => Some(PerfScriptAction::ToggleFirstContextEntry),
        egui::Key::F29 => Some(PerfScriptAction::RequestDeleteFirstContextEntry),
        egui::Key::F30 => Some(PerfScriptAction::CancelContextDelete),
        egui::Key::F31 | egui::Key::F33 => Some(PerfScriptAction::PreviewContextSync),
        egui::Key::F32 => Some(PerfScriptAction::CancelContextSyncPreview),
        egui::Key::F34 => Some(PerfScriptAction::ConfirmContextSync),
        _ => None,
    }
}

impl PerfScriptAction {
    fn name(self) -> &'static str {
        match self {
            Self::NavigateProviders => "navigate_providers",
            Self::SelectNextProvider => "select_next_provider",
            Self::EditProviderName => "edit_provider_name",
            Self::DiscardProvider => "discard_provider",
            Self::RefreshLive => "refresh_live",
            Self::OpenLiveTab => "open_live_tab",
            Self::RequestClearLive => "request_clear_live",
            Self::CancelLiveConfirmation => "cancel_live_confirmation",
            Self::ConfirmLiveMutation => "confirm_live_mutation",
            Self::ToggleProviderList => "toggle_provider_list",
            Self::NavigateEnvironment => "navigate_environment",
            Self::RefreshEnvironment => "refresh_environment",
            Self::SelectFirstEnvironmentConflict => "select_environment_conflict",
            Self::RequestEnvironmentCleanup => "request_environment_cleanup",
            Self::CancelEnvironmentCleanup => "cancel_environment_cleanup",
            Self::OpenCcsImport => "open_ccs_import",
            Self::CloseCcsImport => "close_ccs_import",
            Self::NavigateOverview => "navigate_overview",
            Self::RefreshPendingImport => "refresh_pending_import",
            Self::DismissPendingImport => "dismiss_pending_import",
            Self::NavigateContext => "navigate_context",
            Self::RefreshContext => "refresh_context",
            Self::SelectNextContextKind => "select_next_context_kind",
            Self::CreateContextEntry => "create_context_entry",
            Self::CancelContextEditor => "cancel_context_editor",
            Self::OpenFirstContextEntry => "open_first_context_entry",
            Self::ToggleFirstContextEntry => "toggle_first_context_entry",
            Self::RequestDeleteFirstContextEntry => "request_delete_first_context_entry",
            Self::CancelContextDelete => "cancel_context_delete",
            Self::PreviewContextSync => "preview_context_sync",
            Self::CancelContextSyncPreview => "cancel_context_sync_preview",
            Self::ConfirmContextSync => "confirm_context_sync",
        }
    }
}

fn key_event(key: egui::Key, pressed: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000.0
}

fn report_worker(path: PathBuf, events: mpsc::Receiver<PerfEvent>) {
    let mut report = PerfReport::default();
    while let Ok(event) = events.recv() {
        let (write_now, final_ack) = match event {
            PerfEvent::FirstUiFrame(value) => {
                report.first_ui_frame_ms = Some(value);
                (true, None)
            }
            PerfEvent::OverviewReady(value) => {
                report.overview_ready_ms = Some(value);
                (false, None)
            }
            PerfEvent::CpuFrame(value) => {
                report.cpu_frame_samples_ms.push(value);
                (false, None)
            }
            PerfEvent::InputLatency(value) => {
                report.input_latency_samples_ms.push(value);
                (false, None)
            }
            PerfEvent::ScriptAction(action) => {
                report.script_actions.push(action.to_owned());
                (false, None)
            }
            PerfEvent::Final(ack) => (true, Some(ack)),
        };

        if write_now && let Err(error) = write_report(&path, &report) {
            let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
                "native_manager.perf_report_failed",
                serde_json::json!({
                    "path": path,
                    "error": error.to_string(),
                }),
            );
        }
        if let Some(ack) = final_ack {
            let _ = ack.send(());
        }
    }
}

fn write_report(path: &Path, report: &PerfReport) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(report).map_err(std::io::Error::other)?;
    let mut temporary = path.to_path_buf();
    let extension = path.extension().and_then(|value| value.to_str());
    temporary.set_extension(match extension {
        Some(extension) => format!("{extension}.tmp"),
        None => "tmp".to_owned(),
    });
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use eframe::egui;

    use super::{
        PerfReport, PerfScriptAction, maximum_ms, percentile_ms, script_action_for_key,
        script_step, write_report,
    };

    #[test]
    fn p95_and_max_use_sorted_cpu_frame_samples() {
        let samples = vec![4.0, 8.0, 12.0, 16.0, 40.0];
        assert_eq!(percentile_ms(&samples, 0.95), Some(40.0));
        assert_eq!(maximum_ms(&samples), Some(40.0));
    }

    #[test]
    fn invalid_samples_are_ignored() {
        let samples = vec![f64::NAN, -1.0, 4.0, f64::INFINITY, 8.0];
        assert_eq!(percentile_ms(&samples, 0.5), Some(4.0));
        assert_eq!(maximum_ms(&samples), Some(8.0));
        assert_eq!(percentile_ms(&[], 0.95), None);
    }

    #[test]
    fn report_serializes_threshold_fields() {
        let report = PerfReport {
            first_ui_frame_ms: Some(320.0),
            overview_ready_ms: Some(410.0),
            cpu_frame_samples_ms: vec![4.0, 5.0],
            input_latency_samples_ms: vec![6.0],
            script_actions: vec!["navigate_providers".to_owned()],
        };
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["first_ui_frame_ms"], 320.0);
        assert_eq!(value["overview_ready_ms"], 410.0);
        assert_eq!(value["cpu_frame_samples_ms"][1], 5.0);
        assert_eq!(value["input_latency_samples_ms"][0], 6.0);
        assert_eq!(value["script_actions"][0], "navigate_providers");
    }

    #[test]
    fn report_writer_replaces_an_existing_report() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("report.json");
        let mut report = PerfReport {
            first_ui_frame_ms: Some(100.0),
            ..PerfReport::default()
        };
        write_report(&path, &report).unwrap();
        report.overview_ready_ms = Some(150.0);
        write_report(&path, &report).unwrap();

        let stored: PerfReport = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(stored.first_ui_frame_ms, Some(100.0));
        assert_eq!(stored.overview_ready_ms, Some(150.0));
    }

    #[test]
    fn native_perf_script_covers_provider_import_environment_and_context_paths() {
        let expected = [
            (500, egui::Key::F1, PerfScriptAction::NavigateProviders),
            (1_000, egui::Key::F2, PerfScriptAction::SelectNextProvider),
            (1_500, egui::Key::F3, PerfScriptAction::EditProviderName),
            (2_000, egui::Key::F4, PerfScriptAction::DiscardProvider),
            (2_500, egui::Key::F5, PerfScriptAction::RefreshLive),
            (3_000, egui::Key::F6, PerfScriptAction::OpenLiveTab),
            (3_500, egui::Key::F7, PerfScriptAction::RequestClearLive),
            (
                4_000,
                egui::Key::F8,
                PerfScriptAction::CancelLiveConfirmation,
            ),
            (4_500, egui::Key::F9, PerfScriptAction::RequestClearLive),
            (5_000, egui::Key::F10, PerfScriptAction::ConfirmLiveMutation),
            (5_500, egui::Key::F11, PerfScriptAction::ToggleProviderList),
            (6_000, egui::Key::F12, PerfScriptAction::NavigateEnvironment),
            (6_500, egui::Key::F13, PerfScriptAction::RefreshEnvironment),
            (
                7_000,
                egui::Key::F14,
                PerfScriptAction::SelectFirstEnvironmentConflict,
            ),
            (
                7_500,
                egui::Key::F15,
                PerfScriptAction::RequestEnvironmentCleanup,
            ),
            (
                8_000,
                egui::Key::F16,
                PerfScriptAction::CancelEnvironmentCleanup,
            ),
            (8_500, egui::Key::F17, PerfScriptAction::NavigateProviders),
            (9_000, egui::Key::F18, PerfScriptAction::OpenCcsImport),
            (9_500, egui::Key::F19, PerfScriptAction::CloseCcsImport),
            (10_000, egui::Key::F20, PerfScriptAction::NavigateOverview),
            (10_500, egui::Key::F21, PerfScriptAction::NavigateContext),
            (11_000, egui::Key::F22, PerfScriptAction::RefreshContext),
            (
                11_500,
                egui::Key::F23,
                PerfScriptAction::SelectNextContextKind,
            ),
            (12_000, egui::Key::F24, PerfScriptAction::CreateContextEntry),
            (
                12_500,
                egui::Key::F25,
                PerfScriptAction::CancelContextEditor,
            ),
            (
                13_000,
                egui::Key::F26,
                PerfScriptAction::OpenFirstContextEntry,
            ),
            (
                13_500,
                egui::Key::F27,
                PerfScriptAction::CancelContextEditor,
            ),
            (
                14_000,
                egui::Key::F28,
                PerfScriptAction::ToggleFirstContextEntry,
            ),
            (
                14_500,
                egui::Key::F29,
                PerfScriptAction::RequestDeleteFirstContextEntry,
            ),
            (
                15_000,
                egui::Key::F30,
                PerfScriptAction::CancelContextDelete,
            ),
            (15_500, egui::Key::F31, PerfScriptAction::PreviewContextSync),
            (
                16_000,
                egui::Key::F32,
                PerfScriptAction::CancelContextSyncPreview,
            ),
            (16_500, egui::Key::F33, PerfScriptAction::PreviewContextSync),
            (17_000, egui::Key::F34, PerfScriptAction::ConfirmContextSync),
        ];
        for (index, (milliseconds, key, action)) in expected.into_iter().enumerate() {
            assert_eq!(
                script_step(index),
                Some((Duration::from_millis(milliseconds), key))
            );
            assert_eq!(script_action_for_key(key), Some(action));
        }
        assert_eq!(script_step(expected.len()), None);
    }
}
