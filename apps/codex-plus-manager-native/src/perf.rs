use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

const SCRIPT_DURATION: Duration = Duration::from_secs(5);
const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);
const FINAL_FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PerfReport {
    pub first_ui_frame_ms: Option<f64>,
    pub overview_ready_ms: Option<f64>,
    pub cpu_frame_samples_ms: Vec<f64>,
    pub input_latency_samples_ms: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfScriptAction {
    NavigateAbout,
    NavigateOverview,
    ToggleLocale,
    ToggleTheme,
    Refresh,
}

enum PerfEvent {
    FirstUiFrame(f64),
    OverviewReady(f64),
    CpuFrame(f64),
    InputLatency(f64),
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
            self.next_script_step += 1;
            ctx.request_repaint();
        }
    }

    pub fn scripted_action(&self, ui: &egui::Ui) -> Option<PerfScriptAction> {
        ui.input(|input| {
            if input.key_pressed(egui::Key::F6) {
                Some(PerfScriptAction::NavigateAbout)
            } else if input.key_pressed(egui::Key::F7) {
                Some(PerfScriptAction::NavigateOverview)
            } else if input.key_pressed(egui::Key::F8) {
                Some(PerfScriptAction::ToggleLocale)
            } else if input.key_pressed(egui::Key::F9) {
                Some(PerfScriptAction::ToggleTheme)
            } else if input.key_pressed(egui::Key::F10) {
                Some(PerfScriptAction::Refresh)
            } else {
                None
            }
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
    const STEPS: [(u64, egui::Key); 5] = [
        (500, egui::Key::F6),
        (1_000, egui::Key::F7),
        (1_500, egui::Key::F8),
        (2_000, egui::Key::F9),
        (2_500, egui::Key::F10),
    ];
    STEPS
        .get(index)
        .map(|(milliseconds, key)| (Duration::from_millis(*milliseconds), *key))
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
    use super::{PerfReport, maximum_ms, percentile_ms, write_report};

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
        };
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["first_ui_frame_ms"], 320.0);
        assert_eq!(value["overview_ready_ms"], 410.0);
        assert_eq!(value["cpu_frame_samples_ms"][1], 5.0);
        assert_eq!(value["input_latency_samples_ms"][0], 6.0);
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
}
