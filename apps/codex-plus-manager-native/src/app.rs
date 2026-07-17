use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use codex_plus_manager_service::OverviewSource;
use eframe::egui;

use crate::fonts;
use crate::persistence::{self, PersistedUiState};
use crate::runtime::{DispatchError, OverviewDispatcher};
use crate::state::{AppState, OverviewFailureKind, OverviewPhase};
use crate::theme;
use crate::views::shell::{ShellAction, ShellViewModel, render_shell};

pub struct NativeManagerApp {
    state: AppState,
    persisted: PersistedUiState,
    dispatcher: OverviewDispatcher,
    last_updated: Option<String>,
    worker_stopped: bool,
}

impl NativeManagerApp {
    pub fn new(
        creation: &eframe::CreationContext<'_>,
        cjk_font: Option<Vec<u8>>,
        source: Arc<dyn OverviewSource>,
    ) -> Self {
        egui_extras::install_image_loaders(&creation.egui_ctx);
        if let Some(bytes) = cjk_font {
            fonts::install_cjk_font(&creation.egui_ctx, bytes);
        }

        let persisted = persistence::load(creation.storage);
        theme::apply(&creation.egui_ctx, persisted.theme);
        let repaint_context = creation.egui_ctx.clone();
        let dispatcher =
            OverviewDispatcher::spawn(source, Arc::new(move || repaint_context.request_repaint()));
        let mut app = Self {
            state: AppState::default(),
            persisted,
            dispatcher,
            last_updated: None,
            worker_stopped: false,
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        let request_id = self.state.overview.begin_refresh();
        if self.dispatcher.request(request_id).is_err() {
            self.worker_stopped = true;
            self.state
                .overview
                .apply_response(request_id, Err(OverviewFailureKind::WorkerStopped));
        }
    }

    fn apply_action(&mut self, ctx: &egui::Context, action: ShellAction) {
        match action {
            ShellAction::Navigate(route) => self.state.route = route,
            ShellAction::Refresh | ShellAction::Retry => self.refresh(),
            ShellAction::SetLocale(locale) => self.persisted.locale = locale,
            ShellAction::SetTheme(mode) => {
                self.persisted.theme = mode;
                theme::apply(ctx, mode);
            }
        }
        ctx.request_repaint();
    }

    fn reduce_ready_responses(&mut self) {
        loop {
            match self.dispatcher.try_recv() {
                Ok(Some(response)) => {
                    let request_id = response.request_id;
                    let result = response.result.map_err(|_| OverviewFailureKind::LoadFailed);
                    if self.state.overview.apply_response(request_id, result)
                        && self.state.overview.phase == OverviewPhase::Ready
                    {
                        self.last_updated = Some(current_utc_time());
                    }
                }
                Ok(None) => break,
                Err(DispatchError::WorkerStopped) => {
                    if !self.worker_stopped {
                        self.worker_stopped = true;
                        self.state.overview.apply_response(
                            self.state.overview.current_request_id,
                            Err(OverviewFailureKind::WorkerStopped),
                        );
                    }
                    break;
                }
            }
        }
    }

    fn view_model(&self) -> ShellViewModel {
        ShellViewModel {
            route: self.state.route,
            locale: self.persisted.locale,
            theme: self.persisted.theme,
            overview_phase: self.state.overview.phase,
            overview_snapshot: self.state.overview.snapshot.clone(),
            overview_error: self.state.overview.error,
            last_updated: self.last_updated.clone(),
            renderer: "WGPU".to_owned(),
        }
    }
}

impl eframe::App for NativeManagerApp {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.reduce_ready_responses();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let model = self.view_model();
        for action in render_shell(ui, &model) {
            self.apply_action(ui.ctx(), action);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        persistence::save(storage, &self.persisted);
    }
}

fn current_utc_time() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_utc_time(seconds)
}

fn format_utc_time(seconds_since_epoch: u64) -> String {
    let seconds = seconds_since_epoch % 86_400;
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::format_utc_time;

    #[test]
    fn refresh_time_is_stable_and_explicitly_utc() {
        assert_eq!(format_utc_time(3_661), "01:01:01 UTC");
        assert_eq!(format_utc_time(86_400), "00:00:00 UTC");
    }
}
