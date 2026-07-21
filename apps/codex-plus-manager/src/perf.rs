use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

const SCRIPT_DURATION: Duration = Duration::from_secs(55);
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
    RequestLocalMarketplaceRepair,
    ConfirmLocalMarketplaceRepair,
    RequestRemoteMarketplaceRepair,
    ConfirmRemoteMarketplaceRepair,
    RefreshMarketplace,
    NavigateSessions,
    RefreshSessions,
    SetSessionQuery,
    SelectAllFilteredSessions,
    OpenDeleteConfirmation,
    CancelDeleteConfirmation,
    RunProviderRepair,
    CancelProviderRepair,
    NavigateScripts,
    RefreshLocalScripts,
    RefreshScriptMarket,
    OpenLocalScripts,
    OpenScriptMarket,
    RequestVerifiedScriptInstall,
    CancelScriptInstall,
    ConfirmVerifiedScriptInstall,
    DisableAllScripts,
    ToggleFirstUserScript,
    RequestScriptConflict,
    RetryScriptConflict,
    RequestDeleteFirstUserScript,
    CancelUserScriptDelete,
    ConfirmUserScriptDelete,
    NavigateZedRemote,
    RefreshZedRemote,
    EditZedPreferences,
    SaveZedPreferences,
    RequestZedOpen,
    CancelZedOpen,
    ConfirmZedOpen,
    RequestZedForget,
    CancelZedForget,
    ConfirmZedForget,
    RequestZedConflictRefresh,
    ConfirmZedConflictRefresh,
    NavigateMaintenance,
    RefreshMaintenance,
    RequestDesktopRepair,
    CancelDesktopRepair,
    ConfirmDesktopRepair,
    MigrateStartAtSignIn,
    DisableStartAtSignIn,
    EnableStartAtSignIn,
    SetMaintenanceLogLimit,
    OpenMaintenanceReport,
    EditMaintenancePath,
    SaveMaintenancePath,
    PickMaintenanceExecutable,
    LaunchMaintenance,
    NavigateSettings,
    EditStepwiseSettings,
    TestStepwiseSettings,
    SaveStepwiseSettings,
    OpenImageOverlaySettings,
    PickOverlayImage,
    SaveImageOverlaySettings,
    RequestImageOverlayReset,
    CancelImageOverlayReset,
    OpenExtraArgsSettings,
    EditExtraArgsSettings,
    SaveExtraArgsSettings,
    NavigateEnhancements,
    EditEnhancements,
    SaveEnhancements,
    NavigateAbout,
    RequestUpdateInstall,
    ConfirmUpdateInstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PerfScriptMode {
    #[default]
    Standard,
    UpdateSmoke,
    DesktopIntegrationSmoke,
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
    pending_script_action: Option<(egui::Key, PerfScriptAction)>,
    next_script_step: usize,
    exit_after: Option<Duration>,
    close_requested: bool,
    final_sent: bool,
    script_mode: PerfScriptMode,
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
        let script_mode = match std::env::var("CODEX_PLUS_NATIVE_PERF_SCENARIO").as_deref() {
            Ok("update-smoke") => PerfScriptMode::UpdateSmoke,
            Ok("desktop-integration-smoke") => PerfScriptMode::DesktopIntegrationSmoke,
            _ => PerfScriptMode::Standard,
        };
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
            pending_script_action: None,
            next_script_step: 0,
            exit_after,
            close_requested: false,
            final_sent: false,
            script_mode,
        })
    }

    pub fn drive(&mut self, ctx: &egui::Context) -> bool {
        let elapsed = self.process_started.elapsed();
        let script_duration = match self.script_mode {
            PerfScriptMode::Standard => SCRIPT_DURATION,
            PerfScriptMode::UpdateSmoke => Duration::from_secs(2),
            PerfScriptMode::DesktopIntegrationSmoke => Duration::from_secs(6),
        };
        if elapsed < script_duration {
            ctx.request_repaint_after(FRAME_INTERVAL);
        }

        if let Some(exit_after) = self.exit_after {
            if elapsed >= exit_after {
                if !self.close_requested {
                    self.close_requested = true;
                    return true;
                }
            } else {
                ctx.request_repaint_after(exit_after - elapsed);
            }
        }
        false
    }

    pub fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let step = match self.script_mode {
            PerfScriptMode::Standard => script_step(self.next_script_step),
            PerfScriptMode::UpdateSmoke => update_smoke_script_step(self.next_script_step),
            PerfScriptMode::DesktopIntegrationSmoke => {
                desktop_integration_smoke_script_step(self.next_script_step)
            }
        };
        if let Some((due, key, action)) = step
            && self.process_started.elapsed() >= due
        {
            let now = Instant::now();
            input.events.push(key_event(key, true));
            input.events.push(key_event(key, false));
            self.pending_input_started = Some(now);
            self.pending_script_action = Some((key, action));
            self.send(PerfEvent::ScriptAction(action.name()));
            self.next_script_step += 1;
            ctx.request_repaint();
        }
    }

    pub fn scripted_action(&mut self, ui: &egui::Ui) -> Option<PerfScriptAction> {
        let (key, action) = self.pending_script_action?;
        if ui.input(|input| input.key_pressed(key)) {
            self.pending_script_action = None;
            Some(action)
        } else {
            None
        }
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

fn script_step(index: usize) -> Option<(Duration, egui::Key, PerfScriptAction)> {
    const INITIAL_KEYS: [egui::Key; 35] = [
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
        egui::Key::F35,
    ];
    const ACTIONS: [PerfScriptAction; 109] = [
        PerfScriptAction::NavigateProviders,
        PerfScriptAction::SelectNextProvider,
        PerfScriptAction::EditProviderName,
        PerfScriptAction::DiscardProvider,
        PerfScriptAction::RefreshLive,
        PerfScriptAction::OpenLiveTab,
        PerfScriptAction::RequestClearLive,
        PerfScriptAction::CancelLiveConfirmation,
        PerfScriptAction::RequestClearLive,
        PerfScriptAction::ConfirmLiveMutation,
        PerfScriptAction::ToggleProviderList,
        PerfScriptAction::NavigateEnvironment,
        PerfScriptAction::RefreshEnvironment,
        PerfScriptAction::SelectFirstEnvironmentConflict,
        PerfScriptAction::RequestEnvironmentCleanup,
        PerfScriptAction::CancelEnvironmentCleanup,
        PerfScriptAction::NavigateProviders,
        PerfScriptAction::OpenCcsImport,
        PerfScriptAction::CloseCcsImport,
        PerfScriptAction::NavigateOverview,
        PerfScriptAction::NavigateContext,
        PerfScriptAction::RefreshContext,
        PerfScriptAction::SelectNextContextKind,
        PerfScriptAction::CreateContextEntry,
        PerfScriptAction::CancelContextEditor,
        PerfScriptAction::OpenFirstContextEntry,
        PerfScriptAction::CancelContextEditor,
        PerfScriptAction::ToggleFirstContextEntry,
        PerfScriptAction::RequestDeleteFirstContextEntry,
        PerfScriptAction::CancelContextDelete,
        PerfScriptAction::PreviewContextSync,
        PerfScriptAction::CancelContextSyncPreview,
        PerfScriptAction::PreviewContextSync,
        PerfScriptAction::ConfirmContextSync,
        PerfScriptAction::RefreshMarketplace,
        PerfScriptAction::RequestLocalMarketplaceRepair,
        PerfScriptAction::ConfirmLocalMarketplaceRepair,
        PerfScriptAction::RequestRemoteMarketplaceRepair,
        PerfScriptAction::ConfirmRemoteMarketplaceRepair,
        PerfScriptAction::NavigateSessions,
        PerfScriptAction::RefreshSessions,
        PerfScriptAction::SetSessionQuery,
        PerfScriptAction::SelectAllFilteredSessions,
        PerfScriptAction::OpenDeleteConfirmation,
        PerfScriptAction::CancelDeleteConfirmation,
        PerfScriptAction::RunProviderRepair,
        PerfScriptAction::CancelProviderRepair,
        PerfScriptAction::NavigateScripts,
        PerfScriptAction::RefreshLocalScripts,
        PerfScriptAction::RefreshScriptMarket,
        PerfScriptAction::OpenLocalScripts,
        PerfScriptAction::OpenScriptMarket,
        PerfScriptAction::RequestVerifiedScriptInstall,
        PerfScriptAction::CancelScriptInstall,
        PerfScriptAction::RequestVerifiedScriptInstall,
        PerfScriptAction::ConfirmVerifiedScriptInstall,
        PerfScriptAction::OpenLocalScripts,
        PerfScriptAction::DisableAllScripts,
        PerfScriptAction::ToggleFirstUserScript,
        PerfScriptAction::RequestScriptConflict,
        PerfScriptAction::RetryScriptConflict,
        PerfScriptAction::RequestDeleteFirstUserScript,
        PerfScriptAction::CancelUserScriptDelete,
        PerfScriptAction::RequestDeleteFirstUserScript,
        PerfScriptAction::ConfirmUserScriptDelete,
        PerfScriptAction::NavigateZedRemote,
        PerfScriptAction::RefreshZedRemote,
        PerfScriptAction::EditZedPreferences,
        PerfScriptAction::SaveZedPreferences,
        PerfScriptAction::RequestZedOpen,
        PerfScriptAction::CancelZedOpen,
        PerfScriptAction::RequestZedOpen,
        PerfScriptAction::ConfirmZedOpen,
        PerfScriptAction::RequestZedForget,
        PerfScriptAction::CancelZedForget,
        PerfScriptAction::RequestZedForget,
        PerfScriptAction::ConfirmZedForget,
        PerfScriptAction::RequestZedConflictRefresh,
        PerfScriptAction::ConfirmZedConflictRefresh,
        PerfScriptAction::NavigateMaintenance,
        PerfScriptAction::RefreshMaintenance,
        PerfScriptAction::RequestDesktopRepair,
        PerfScriptAction::CancelDesktopRepair,
        PerfScriptAction::RequestDesktopRepair,
        PerfScriptAction::ConfirmDesktopRepair,
        PerfScriptAction::MigrateStartAtSignIn,
        PerfScriptAction::DisableStartAtSignIn,
        PerfScriptAction::EnableStartAtSignIn,
        PerfScriptAction::SetMaintenanceLogLimit,
        PerfScriptAction::OpenMaintenanceReport,
        PerfScriptAction::EditMaintenancePath,
        PerfScriptAction::SaveMaintenancePath,
        PerfScriptAction::PickMaintenanceExecutable,
        PerfScriptAction::LaunchMaintenance,
        PerfScriptAction::NavigateSettings,
        PerfScriptAction::EditStepwiseSettings,
        PerfScriptAction::TestStepwiseSettings,
        PerfScriptAction::SaveStepwiseSettings,
        PerfScriptAction::OpenImageOverlaySettings,
        PerfScriptAction::PickOverlayImage,
        PerfScriptAction::SaveImageOverlaySettings,
        PerfScriptAction::RequestImageOverlayReset,
        PerfScriptAction::CancelImageOverlayReset,
        PerfScriptAction::OpenExtraArgsSettings,
        PerfScriptAction::EditExtraArgsSettings,
        PerfScriptAction::SaveExtraArgsSettings,
        PerfScriptAction::NavigateEnhancements,
        PerfScriptAction::EditEnhancements,
        PerfScriptAction::SaveEnhancements,
    ];
    ACTIONS.get(index).map(|action| {
        let key = INITIAL_KEYS.get(index).copied().unwrap_or(egui::Key::F35);
        let milliseconds = u64::try_from(index + 1).expect("script index fits u64") * 500;
        (Duration::from_millis(milliseconds), key, *action)
    })
}

fn update_smoke_script_step(index: usize) -> Option<(Duration, egui::Key, PerfScriptAction)> {
    let (key, action) = match index {
        0 => (egui::Key::F1, PerfScriptAction::NavigateAbout),
        1 => (egui::Key::F2, PerfScriptAction::RequestUpdateInstall),
        2 => (egui::Key::F3, PerfScriptAction::ConfirmUpdateInstall),
        _ => return None,
    };
    let milliseconds = u64::try_from(index + 1).expect("script index fits u64") * 500;
    Some((Duration::from_millis(milliseconds), key, action))
}

fn desktop_integration_smoke_script_step(
    index: usize,
) -> Option<(Duration, egui::Key, PerfScriptAction)> {
    const ACTIONS: [PerfScriptAction; 9] = [
        PerfScriptAction::NavigateMaintenance,
        PerfScriptAction::RefreshMaintenance,
        PerfScriptAction::RequestDesktopRepair,
        PerfScriptAction::CancelDesktopRepair,
        PerfScriptAction::RequestDesktopRepair,
        PerfScriptAction::ConfirmDesktopRepair,
        PerfScriptAction::MigrateStartAtSignIn,
        PerfScriptAction::DisableStartAtSignIn,
        PerfScriptAction::EnableStartAtSignIn,
    ];
    ACTIONS.get(index).map(|action| {
        let milliseconds = u64::try_from(index + 1).expect("script index fits u64") * 500;
        (Duration::from_millis(milliseconds), egui::Key::F35, *action)
    })
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
            Self::RequestLocalMarketplaceRepair => "request_local_marketplace_repair",
            Self::ConfirmLocalMarketplaceRepair => "confirm_local_marketplace_repair",
            Self::RequestRemoteMarketplaceRepair => "request_remote_marketplace_repair",
            Self::ConfirmRemoteMarketplaceRepair => "confirm_remote_marketplace_repair",
            Self::RefreshMarketplace => "refresh_marketplace",
            Self::NavigateSessions => "navigate_sessions",
            Self::RefreshSessions => "refresh_sessions",
            Self::SetSessionQuery => "set_session_query",
            Self::SelectAllFilteredSessions => "select_all_filtered_sessions",
            Self::OpenDeleteConfirmation => "open_delete_confirmation",
            Self::CancelDeleteConfirmation => "cancel_delete_confirmation",
            Self::RunProviderRepair => "run_provider_repair",
            Self::CancelProviderRepair => "cancel_provider_repair",
            Self::NavigateScripts => "navigate_scripts",
            Self::RefreshLocalScripts => "refresh_local_scripts",
            Self::RefreshScriptMarket => "refresh_script_market",
            Self::OpenLocalScripts => "open_local_scripts",
            Self::OpenScriptMarket => "open_script_market",
            Self::RequestVerifiedScriptInstall => "request_verified_script_install",
            Self::CancelScriptInstall => "cancel_script_install",
            Self::ConfirmVerifiedScriptInstall => "confirm_verified_script_install",
            Self::DisableAllScripts => "disable_all_scripts",
            Self::ToggleFirstUserScript => "toggle_first_user_script",
            Self::RequestScriptConflict => "request_script_conflict",
            Self::RetryScriptConflict => "retry_script_conflict",
            Self::RequestDeleteFirstUserScript => "request_delete_first_user_script",
            Self::CancelUserScriptDelete => "cancel_user_script_delete",
            Self::ConfirmUserScriptDelete => "confirm_user_script_delete",
            Self::NavigateZedRemote => "navigate_zed_remote",
            Self::RefreshZedRemote => "refresh_zed_remote",
            Self::EditZedPreferences => "edit_zed_preferences",
            Self::SaveZedPreferences => "save_zed_preferences",
            Self::RequestZedOpen => "request_zed_open",
            Self::CancelZedOpen => "cancel_zed_open",
            Self::ConfirmZedOpen => "confirm_zed_open",
            Self::RequestZedForget => "request_zed_forget",
            Self::CancelZedForget => "cancel_zed_forget",
            Self::ConfirmZedForget => "confirm_zed_forget",
            Self::RequestZedConflictRefresh => "request_zed_conflict_refresh",
            Self::ConfirmZedConflictRefresh => "confirm_zed_conflict_refresh",
            Self::NavigateMaintenance => "navigate_maintenance",
            Self::RefreshMaintenance => "refresh_maintenance",
            Self::RequestDesktopRepair => "request_desktop_repair",
            Self::CancelDesktopRepair => "cancel_desktop_repair",
            Self::ConfirmDesktopRepair => "confirm_desktop_repair",
            Self::MigrateStartAtSignIn => "migrate_start_at_sign_in",
            Self::DisableStartAtSignIn => "disable_start_at_sign_in",
            Self::EnableStartAtSignIn => "enable_start_at_sign_in",
            Self::SetMaintenanceLogLimit => "set_maintenance_log_limit",
            Self::OpenMaintenanceReport => "open_maintenance_report",
            Self::EditMaintenancePath => "edit_maintenance_path",
            Self::SaveMaintenancePath => "save_maintenance_path",
            Self::PickMaintenanceExecutable => "pick_maintenance_executable",
            Self::LaunchMaintenance => "launch_maintenance",
            Self::NavigateSettings => "navigate_settings",
            Self::EditStepwiseSettings => "edit_stepwise_settings",
            Self::TestStepwiseSettings => "test_stepwise_settings",
            Self::SaveStepwiseSettings => "save_stepwise_settings",
            Self::OpenImageOverlaySettings => "open_image_overlay_settings",
            Self::PickOverlayImage => "pick_overlay_image",
            Self::SaveImageOverlaySettings => "save_image_overlay_settings",
            Self::RequestImageOverlayReset => "request_image_overlay_reset",
            Self::CancelImageOverlayReset => "cancel_image_overlay_reset",
            Self::OpenExtraArgsSettings => "open_extra_args_settings",
            Self::EditExtraArgsSettings => "edit_extra_args_settings",
            Self::SaveExtraArgsSettings => "save_extra_args_settings",
            Self::NavigateEnhancements => "navigate_enhancements",
            Self::EditEnhancements => "edit_enhancements",
            Self::SaveEnhancements => "save_enhancements",
            Self::NavigateAbout => "navigate_about",
            Self::RequestUpdateInstall => "request_update_install",
            Self::ConfirmUpdateInstall => "confirm_update_install",
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
        PerfReport, PerfScriptAction, SCRIPT_DURATION, desktop_integration_smoke_script_step,
        maximum_ms, percentile_ms, script_step, update_smoke_script_step, write_report,
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
                Some((Duration::from_millis(milliseconds), key, action))
            );
        }
    }

    #[test]
    fn native_perf_script_appends_the_complete_marketplace_workflow() {
        let expected = [
            (17_500, PerfScriptAction::RefreshMarketplace),
            (18_000, PerfScriptAction::RequestLocalMarketplaceRepair),
            (18_500, PerfScriptAction::ConfirmLocalMarketplaceRepair),
            (19_000, PerfScriptAction::RequestRemoteMarketplaceRepair),
            (19_500, PerfScriptAction::ConfirmRemoteMarketplaceRepair),
        ];

        for (offset, (milliseconds, action)) in expected.into_iter().enumerate() {
            assert_eq!(
                script_step(34 + offset),
                Some((Duration::from_millis(milliseconds), egui::Key::F35, action)),
            );
        }
    }

    #[test]
    fn update_smoke_script_requires_navigation_request_and_confirmation() {
        let expected = [
            (500, egui::Key::F1, PerfScriptAction::NavigateAbout),
            (1_000, egui::Key::F2, PerfScriptAction::RequestUpdateInstall),
            (1_500, egui::Key::F3, PerfScriptAction::ConfirmUpdateInstall),
        ];
        for (index, (milliseconds, key, action)) in expected.into_iter().enumerate() {
            assert_eq!(
                update_smoke_script_step(index),
                Some((Duration::from_millis(milliseconds), key, action))
            );
        }
        assert_eq!(update_smoke_script_step(expected.len()), None);
    }

    #[test]
    fn desktop_integration_smoke_script_covers_the_complete_bounded_workflow() {
        let expected = [
            PerfScriptAction::NavigateMaintenance,
            PerfScriptAction::RefreshMaintenance,
            PerfScriptAction::RequestDesktopRepair,
            PerfScriptAction::CancelDesktopRepair,
            PerfScriptAction::RequestDesktopRepair,
            PerfScriptAction::ConfirmDesktopRepair,
            PerfScriptAction::MigrateStartAtSignIn,
            PerfScriptAction::DisableStartAtSignIn,
            PerfScriptAction::EnableStartAtSignIn,
        ];
        for (index, action) in expected.into_iter().enumerate() {
            assert_eq!(
                desktop_integration_smoke_script_step(index),
                Some((
                    Duration::from_millis((index as u64 + 1) * 500),
                    egui::Key::F35,
                    action,
                ))
            );
        }
        assert_eq!(desktop_integration_smoke_script_step(expected.len()), None);
    }

    #[test]
    fn native_perf_script_appends_the_session_workflow() {
        let expected = [
            (20_000, PerfScriptAction::NavigateSessions),
            (20_500, PerfScriptAction::RefreshSessions),
            (21_000, PerfScriptAction::SetSessionQuery),
            (21_500, PerfScriptAction::SelectAllFilteredSessions),
            (22_000, PerfScriptAction::OpenDeleteConfirmation),
            (22_500, PerfScriptAction::CancelDeleteConfirmation),
            (23_000, PerfScriptAction::RunProviderRepair),
            (23_500, PerfScriptAction::CancelProviderRepair),
        ];

        for (offset, (milliseconds, action)) in expected.into_iter().enumerate() {
            assert_eq!(
                script_step(39 + offset),
                Some((Duration::from_millis(milliseconds), egui::Key::F35, action)),
            );
        }
    }

    #[test]
    fn native_perf_script_appends_the_complete_user_script_workflow() {
        let expected = [
            (24_000, PerfScriptAction::NavigateScripts),
            (24_500, PerfScriptAction::RefreshLocalScripts),
            (25_000, PerfScriptAction::RefreshScriptMarket),
            (25_500, PerfScriptAction::OpenLocalScripts),
            (26_000, PerfScriptAction::OpenScriptMarket),
            (26_500, PerfScriptAction::RequestVerifiedScriptInstall),
            (27_000, PerfScriptAction::CancelScriptInstall),
            (27_500, PerfScriptAction::RequestVerifiedScriptInstall),
            (28_000, PerfScriptAction::ConfirmVerifiedScriptInstall),
            (28_500, PerfScriptAction::OpenLocalScripts),
            (29_000, PerfScriptAction::DisableAllScripts),
            (29_500, PerfScriptAction::ToggleFirstUserScript),
            (30_000, PerfScriptAction::RequestScriptConflict),
            (30_500, PerfScriptAction::RetryScriptConflict),
            (31_000, PerfScriptAction::RequestDeleteFirstUserScript),
            (31_500, PerfScriptAction::CancelUserScriptDelete),
            (32_000, PerfScriptAction::RequestDeleteFirstUserScript),
            (32_500, PerfScriptAction::ConfirmUserScriptDelete),
        ];

        for (offset, (milliseconds, action)) in expected.into_iter().enumerate() {
            assert_eq!(
                script_step(47 + offset),
                Some((Duration::from_millis(milliseconds), egui::Key::F35, action)),
            );
        }
    }

    #[test]
    fn native_perf_script_appends_the_complete_zed_remote_workflow() {
        let expected = [
            (33_000, PerfScriptAction::NavigateZedRemote),
            (33_500, PerfScriptAction::RefreshZedRemote),
            (34_000, PerfScriptAction::EditZedPreferences),
            (34_500, PerfScriptAction::SaveZedPreferences),
            (35_000, PerfScriptAction::RequestZedOpen),
            (35_500, PerfScriptAction::CancelZedOpen),
            (36_000, PerfScriptAction::RequestZedOpen),
            (36_500, PerfScriptAction::ConfirmZedOpen),
            (37_000, PerfScriptAction::RequestZedForget),
            (37_500, PerfScriptAction::CancelZedForget),
            (38_000, PerfScriptAction::RequestZedForget),
            (38_500, PerfScriptAction::ConfirmZedForget),
            (39_000, PerfScriptAction::RequestZedConflictRefresh),
            (39_500, PerfScriptAction::ConfirmZedConflictRefresh),
        ];

        for (offset, (milliseconds, action)) in expected.into_iter().enumerate() {
            assert_eq!(
                script_step(65 + offset),
                Some((Duration::from_millis(milliseconds), egui::Key::F35, action)),
            );
        }
    }

    #[test]
    fn native_perf_script_appends_the_complete_maintenance_and_settings_workflow() {
        let expected = [
            (
                40_000,
                PerfScriptAction::NavigateMaintenance,
                "navigate_maintenance",
            ),
            (
                40_500,
                PerfScriptAction::RefreshMaintenance,
                "refresh_maintenance",
            ),
            (
                41_000,
                PerfScriptAction::RequestDesktopRepair,
                "request_desktop_repair",
            ),
            (
                41_500,
                PerfScriptAction::CancelDesktopRepair,
                "cancel_desktop_repair",
            ),
            (
                42_000,
                PerfScriptAction::RequestDesktopRepair,
                "request_desktop_repair",
            ),
            (
                42_500,
                PerfScriptAction::ConfirmDesktopRepair,
                "confirm_desktop_repair",
            ),
            (
                43_000,
                PerfScriptAction::MigrateStartAtSignIn,
                "migrate_start_at_sign_in",
            ),
            (
                43_500,
                PerfScriptAction::DisableStartAtSignIn,
                "disable_start_at_sign_in",
            ),
            (
                44_000,
                PerfScriptAction::EnableStartAtSignIn,
                "enable_start_at_sign_in",
            ),
            (
                44_500,
                PerfScriptAction::SetMaintenanceLogLimit,
                "set_maintenance_log_limit",
            ),
            (
                45_000,
                PerfScriptAction::OpenMaintenanceReport,
                "open_maintenance_report",
            ),
            (
                45_500,
                PerfScriptAction::EditMaintenancePath,
                "edit_maintenance_path",
            ),
            (
                46_000,
                PerfScriptAction::SaveMaintenancePath,
                "save_maintenance_path",
            ),
            (
                46_500,
                PerfScriptAction::PickMaintenanceExecutable,
                "pick_maintenance_executable",
            ),
            (
                47_000,
                PerfScriptAction::LaunchMaintenance,
                "launch_maintenance",
            ),
            (
                47_500,
                PerfScriptAction::NavigateSettings,
                "navigate_settings",
            ),
            (
                48_000,
                PerfScriptAction::EditStepwiseSettings,
                "edit_stepwise_settings",
            ),
            (
                48_500,
                PerfScriptAction::TestStepwiseSettings,
                "test_stepwise_settings",
            ),
            (
                49_000,
                PerfScriptAction::SaveStepwiseSettings,
                "save_stepwise_settings",
            ),
            (
                49_500,
                PerfScriptAction::OpenImageOverlaySettings,
                "open_image_overlay_settings",
            ),
            (
                50_000,
                PerfScriptAction::PickOverlayImage,
                "pick_overlay_image",
            ),
            (
                50_500,
                PerfScriptAction::SaveImageOverlaySettings,
                "save_image_overlay_settings",
            ),
            (
                51_000,
                PerfScriptAction::RequestImageOverlayReset,
                "request_image_overlay_reset",
            ),
            (
                51_500,
                PerfScriptAction::CancelImageOverlayReset,
                "cancel_image_overlay_reset",
            ),
            (
                52_000,
                PerfScriptAction::OpenExtraArgsSettings,
                "open_extra_args_settings",
            ),
            (
                52_500,
                PerfScriptAction::EditExtraArgsSettings,
                "edit_extra_args_settings",
            ),
            (
                53_000,
                PerfScriptAction::SaveExtraArgsSettings,
                "save_extra_args_settings",
            ),
        ];

        for (offset, (milliseconds, action, name)) in expected.into_iter().enumerate() {
            assert_eq!(
                script_step(79 + offset),
                Some((Duration::from_millis(milliseconds), egui::Key::F35, action)),
            );
            assert_eq!(action.name(), name);
        }
    }

    #[test]
    fn native_perf_script_appends_the_enhancements_workflow() {
        let expected = [
            (
                53_500,
                PerfScriptAction::NavigateEnhancements,
                "navigate_enhancements",
            ),
            (
                54_000,
                PerfScriptAction::EditEnhancements,
                "edit_enhancements",
            ),
            (
                54_500,
                PerfScriptAction::SaveEnhancements,
                "save_enhancements",
            ),
        ];

        for (offset, (milliseconds, action, name)) in expected.into_iter().enumerate() {
            assert_eq!(
                script_step(106 + offset),
                Some((Duration::from_millis(milliseconds), egui::Key::F35, action)),
            );
            assert_eq!(action.name(), name);
        }
        assert_eq!(script_step(109), None);
    }

    #[test]
    fn native_perf_repaint_window_covers_the_complete_script() {
        let (last_due, _, _) = script_step(108).expect("last scripted action");

        assert!(SCRIPT_DURATION >= last_due + Duration::from_millis(500));
    }

    #[test]
    fn native_perf_script_uses_an_optimized_as_invoker_profile() {
        let script = include_str!("../../../scripts/perf/native-manager.ps1");
        let workspace = include_str!("../../../Cargo.toml");
        let build_script = include_str!("../build.rs");

        assert!(script.contains("target\\native-perf\\codex-plus-plus-manager.exe"));
        assert!(script.contains("$FirstFrameLimitMs = 200.0"));
        assert!(
            script.contains("cargo build -p codex-plus-manager --profile native-perf --jobs 1")
        );
        assert!(script.contains("$env:CODEX_PLUS_MANAGER_PERF_AS_INVOKER = '1'"));
        assert!(workspace.contains("[profile.native-perf]"));
        assert!(workspace.contains("inherits = \"release\""));
        assert!(
            build_script.contains("cargo:rerun-if-env-changed=CODEX_PLUS_MANAGER_PERF_AS_INVOKER")
        );
        assert!(build_script.contains("CODEX_PLUS_MANAGER_PERF_AS_INVOKER"));
    }

    #[test]
    fn native_perf_script_streams_sqlite_fixture_without_command_line_quote_loss() {
        let script = include_str!("../../../scripts/perf/native-manager.ps1");

        assert!(script.contains("$CreateFixture | & $Python.Source - $DatabasePath"));
        assert!(!script.contains("& $Python.Source -c $CreateFixture"));
    }

    #[test]
    fn native_perf_script_uses_an_isolated_loopback_user_script_fixture() {
        let script = include_str!("../../../scripts/perf/native-manager.ps1");

        for contract in [
            "CODEX_PLUS_NATIVE_USER_SCRIPT_BUILTIN_DIR",
            "CODEX_PLUS_NATIVE_USER_SCRIPT_USER_DIR",
            "CODEX_PLUS_NATIVE_USER_SCRIPT_CONFIG_PATH",
            "CODEX_PLUS_SCRIPT_MARKET_INDEX_URL",
            "CODEX_PLUS_NATIVE_SCRIPT_MARKET_ALLOW_LOOPBACK",
            "Start-ScriptMarketFixture",
            "Assert-UserScriptWorkflowResult",
        ] {
            assert!(script.contains(contract), "missing {contract}");
        }
    }

    #[test]
    fn native_perf_script_uses_an_isolated_recording_zed_fixture() {
        let script = include_str!("../../../scripts/perf/native-manager.ps1");

        for contract in [
            "CODEX_PLUS_NATIVE_ZED_GLOBAL_STATE_PATH",
            "CODEX_PLUS_NATIVE_ZED_REGISTRY_PATH",
            "CODEX_PLUS_NATIVE_ZED_LAUNCH_RECORD_PATH",
            "New-ZedRemoteFixture",
            "Assert-ZedRemoteWorkflowResult",
        ] {
            assert!(script.contains(contract), "missing {contract}");
        }
    }

    #[test]
    fn native_perf_script_uses_isolated_maintenance_and_settings_fixtures() {
        let script = include_str!("../../../scripts/perf/native-manager.ps1");

        for contract in [
            "CODEX_PLUS_NATIVE_DIAGNOSTIC_LOG_PATH",
            "CODEX_PLUS_NATIVE_LATEST_STATUS_PATH",
            "CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_FIXTURE_STATE",
            "CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_RECORD_PATH",
            "CODEX_PLUS_NATIVE_ENTRYPOINT_SILENT_INSTALLED",
            "CODEX_PLUS_NATIVE_ENTRYPOINT_MANAGEMENT_INSTALLED",
            "CODEX_PLUS_NATIVE_CODEX_LAUNCH_RECORD_PATH",
            "CODEX_PLUS_NATIVE_PATH_PICKER_RESPONSES_PATH",
            "CODEX_PLUS_NATIVE_PATH_PICKER_RECORD_PATH",
            "CODEX_PLUS_NATIVE_STEPWISE_TEST_RECORD_PATH",
            "CODEX_PLUS_NATIVE_STEPWISE_TEST_RESULT",
            "New-MaintenanceSettingsFixture",
            "Assert-MaintenanceSettingsWorkflowResult",
        ] {
            assert!(script.contains(contract), "missing {contract}");
        }
    }
}
