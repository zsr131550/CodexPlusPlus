use std::fmt;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use codex_plus_core::manager_instance::{
    ManagerActivation, ManagerInstanceError, ManagerInstanceOwner,
};
use eframe::egui;

use crate::i18n::Locale;

pub const APP_ID: &str = "com.bigpizzav3.codexplusplus.manager";
pub const APP_TITLE: &str = "Codex++ Manager";
pub const LEGACY_NATIVE_APP_ID: &str = "com.codexplusplus.manager.native";

const MAX_PREFERENCE_BYTES: u64 = 4 * 1024 * 1024;
const ACTIVATION_WAIT: Duration = Duration::from_millis(50);
const ACTIVATION_STOP_WAIT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesktopHostPhase {
    #[default]
    Running,
    Quitting,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopHostEvent {
    CloseRequested,
    Minimized(bool),
    Activation(ManagerActivation),
    TrayShow,
    TrayQuit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopHostEffect {
    CancelClose,
    Hide,
    Show,
    Restore,
    Focus,
    ReloadPendingProviderImport,
    ShowUpdate,
    RequestExit,
}

#[derive(Debug, Default)]
pub struct DesktopHostLifecycle {
    phase: DesktopHostPhase,
    minimized_hidden: bool,
}

impl DesktopHostLifecycle {
    pub fn phase(&self) -> DesktopHostPhase {
        self.phase
    }

    pub fn reduce(&mut self, event: DesktopHostEvent) -> Vec<DesktopHostEffect> {
        if self.phase != DesktopHostPhase::Running {
            return Vec::new();
        }

        match event {
            DesktopHostEvent::CloseRequested => {
                vec![DesktopHostEffect::CancelClose, DesktopHostEffect::Hide]
            }
            DesktopHostEvent::Minimized(true) if !self.minimized_hidden => {
                self.minimized_hidden = true;
                vec![DesktopHostEffect::Hide]
            }
            DesktopHostEvent::Minimized(true) => Vec::new(),
            DesktopHostEvent::Minimized(false) => {
                self.minimized_hidden = false;
                Vec::new()
            }
            DesktopHostEvent::TrayShow | DesktopHostEvent::Activation(ManagerActivation::Show) => {
                self.show_effects(None)
            }
            DesktopHostEvent::Activation(ManagerActivation::ReloadPendingProviderImport) => {
                self.show_effects(Some(DesktopHostEffect::ReloadPendingProviderImport))
            }
            DesktopHostEvent::Activation(ManagerActivation::ShowUpdate) => {
                self.show_effects(Some(DesktopHostEffect::ShowUpdate))
            }
            DesktopHostEvent::TrayQuit => {
                self.phase = DesktopHostPhase::Quitting;
                vec![DesktopHostEffect::RequestExit]
            }
        }
    }

    pub fn mark_exited(&mut self) {
        self.phase = DesktopHostPhase::Exited;
    }

    fn show_effects(&mut self, first: Option<DesktopHostEffect>) -> Vec<DesktopHostEffect> {
        self.minimized_hidden = false;
        let mut effects = Vec::with_capacity(4);
        if let Some(first) = first {
            effects.push(first);
        }
        effects.extend([
            DesktopHostEffect::Show,
            DesktopHostEffect::Restore,
            DesktopHostEffect::Focus,
        ]);
        effects
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceMigration {
    Migrated,
    CanonicalAlreadyExists,
    NoLegacyPreferences,
}

#[derive(Debug, thiserror::Error)]
pub enum PreferenceMigrationError {
    #[error("native preference migration source is too large")]
    SourceTooLarge,
    #[error("native preference migration could not read the legacy file")]
    ReadFailed,
    #[error("native preference migration could not prepare the canonical directory")]
    PrepareFailed,
    #[error("native preference migration could not write the temporary file")]
    WriteFailed,
    #[error("native preference migration could not publish the canonical file")]
    CommitFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePersistencePaths {
    state_root: PathBuf,
    canonical: PathBuf,
    legacy: Option<PathBuf>,
}

impl NativePersistencePaths {
    pub fn for_state_override(state_override: Option<PathBuf>) -> Self {
        match state_override {
            Some(state_root) => Self {
                canonical: state_root.join("manager-ui").join("app.ron"),
                legacy: Some(state_root.join("app.ron")),
                state_root,
            },
            None => {
                let state_root = codex_plus_core::paths::default_app_state_dir();
                Self {
                    canonical: state_root.join("manager-ui").join("app.ron"),
                    legacy: eframe::storage_dir(LEGACY_NATIVE_APP_ID)
                        .map(|directory| directory.join("app.ron")),
                    state_root,
                }
            }
        }
    }

    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    pub fn canonical(&self) -> &Path {
        &self.canonical
    }

    pub fn legacy(&self) -> Option<&Path> {
        self.legacy.as_deref()
    }

    pub fn migrate_legacy_if_needed(
        &self,
    ) -> Result<PreferenceMigration, PreferenceMigrationError> {
        if self.canonical.exists() {
            return Ok(PreferenceMigration::CanonicalAlreadyExists);
        }
        let Some(legacy) = self.legacy.as_deref().filter(|path| path.is_file()) else {
            return Ok(PreferenceMigration::NoLegacyPreferences);
        };
        if legacy
            .metadata()
            .map_err(|_| PreferenceMigrationError::ReadFailed)?
            .len()
            > MAX_PREFERENCE_BYTES
        {
            return Err(PreferenceMigrationError::SourceTooLarge);
        }

        let mut source =
            std::fs::File::open(legacy).map_err(|_| PreferenceMigrationError::ReadFailed)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut source)
            .take(MAX_PREFERENCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| PreferenceMigrationError::ReadFailed)?;
        if bytes.len() as u64 > MAX_PREFERENCE_BYTES {
            return Err(PreferenceMigrationError::SourceTooLarge);
        }

        let parent = self
            .canonical
            .parent()
            .ok_or(PreferenceMigrationError::PrepareFailed)?;
        std::fs::create_dir_all(parent).map_err(|_| PreferenceMigrationError::PrepareFailed)?;
        let temporary = migration_temp_path(&self.canonical);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| PreferenceMigrationError::WriteFailed)?;
        if file.write_all(&bytes).is_err() || file.sync_all().is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err(PreferenceMigrationError::WriteFailed);
        }
        drop(file);

        match std::fs::hard_link(&temporary, &self.canonical) {
            Ok(()) => {
                let _ = std::fs::remove_file(&temporary);
                Ok(PreferenceMigration::Migrated)
            }
            Err(_) if self.canonical.exists() => {
                let _ = std::fs::remove_file(&temporary);
                Ok(PreferenceMigration::CanonicalAlreadyExists)
            }
            Err(_) => {
                let _ = std::fs::remove_file(&temporary);
                Err(PreferenceMigrationError::CommitFailed)
            }
        }
    }
}

fn migration_temp_path(canonical: &Path) -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    canonical.with_extension(format!("ron.migrate-{}-{sequence}", std::process::id()))
}

pub struct DesktopHostBootstrap {
    owner: ManagerInstanceOwner,
    initial_actions: Vec<ManagerActivation>,
}

impl DesktopHostBootstrap {
    pub fn new(owner: ManagerInstanceOwner, initial_actions: Vec<ManagerActivation>) -> Self {
        Self {
            owner,
            initial_actions,
        }
    }
}

impl fmt::Debug for DesktopHostBootstrap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopHostBootstrap")
            .field("initial_action_count", &self.initial_actions.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DesktopHostRuntimeError {
    #[error("native desktop host could not create its activation receiver")]
    ActivationReceiver,
    #[error("native desktop host could not start its activation worker")]
    ActivationWorker,
    #[error("native desktop host could not create its tray")]
    Tray,
}

pub struct DesktopHostRuntime {
    owner: Option<ManagerInstanceOwner>,
    tray: Option<TrayController>,
    events: mpsc::Receiver<DesktopHostEvent>,
    stop: Arc<AtomicBool>,
    worker_done: mpsc::Receiver<()>,
    worker: Option<thread::JoinHandle<()>>,
    shutdown: bool,
}

impl DesktopHostRuntime {
    pub fn start(
        bootstrap: DesktopHostBootstrap,
        context: egui::Context,
        locale: Locale,
    ) -> Result<Self, DesktopHostRuntimeError> {
        let DesktopHostBootstrap {
            owner,
            initial_actions,
        } = bootstrap;
        let receiver = owner
            .receiver()
            .map_err(|_| DesktopHostRuntimeError::ActivationReceiver)?;
        let (event_tx, events) = mpsc::channel();
        for action in initial_actions {
            let _ = event_tx.send(DesktopHostEvent::Activation(action));
        }
        let tray = TrayController::new(event_tx.clone(), context.clone(), locale)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_worker = Arc::clone(&stop);
        let (done_tx, worker_done) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("native-desktop-activation".to_owned())
            .spawn(move || {
                while !stop_for_worker.load(Ordering::Acquire) {
                    match receiver.recv_timeout(ACTIVATION_WAIT) {
                        Ok(action) => {
                            if event_tx.send(DesktopHostEvent::Activation(action)).is_err() {
                                break;
                            }
                            context.request_repaint();
                        }
                        Err(ManagerInstanceError::ActivationTimedOut) => {}
                        Err(_) => break,
                    }
                }
                let _ = done_tx.send(());
            })
            .map_err(|_| DesktopHostRuntimeError::ActivationWorker)?;

        Ok(Self {
            owner: Some(owner),
            tray: Some(tray),
            events,
            stop,
            worker_done,
            worker: Some(worker),
            shutdown: false,
        })
    }

    pub fn try_recv(&self) -> Option<DesktopHostEvent> {
        self.events.try_recv().ok()
    }

    pub fn set_locale(&self, locale: Locale) {
        if let Some(tray) = &self.tray {
            tray.set_locale(locale);
        }
    }

    pub fn shutdown(&mut self) {
        if self.shutdown {
            return;
        }
        self.shutdown = true;
        self.stop.store(true, Ordering::Release);
        if self.worker_done.recv_timeout(ACTIVATION_STOP_WAIT).is_ok()
            && let Some(worker) = self.worker.take()
        {
            let _ = worker.join();
        }
        self.worker.take();
        self.tray.take();
        self.owner.take();
    }
}

impl fmt::Debug for DesktopHostRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopHostRuntime")
            .field("owner_present", &self.owner.is_some())
            .field("tray_present", &self.tray.is_some())
            .field("shutdown", &self.shutdown)
            .finish()
    }
}

impl Drop for DesktopHostRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct TrayController {
    _tray: tray_icon::TrayIcon,
    show_item: tray_icon::menu::MenuItem,
    quit_item: tray_icon::menu::MenuItem,
}

impl TrayController {
    fn new(
        events: mpsc::Sender<DesktopHostEvent>,
        context: egui::Context,
        locale: Locale,
    ) -> Result<Self, DesktopHostRuntimeError> {
        let (show_label, quit_label) = tray_labels(locale);
        let menu = tray_icon::menu::Menu::new();
        let show_item =
            tray_icon::menu::MenuItem::with_id("codex-plus-manager-show", show_label, true, None);
        let quit_item =
            tray_icon::menu::MenuItem::with_id("codex-plus-manager-quit", quit_label, true, None);
        menu.append_items(&[&show_item, &quit_item])
            .map_err(|_| DesktopHostRuntimeError::Tray)?;
        let tray = tray_icon::TrayIconBuilder::new()
            .with_tooltip(APP_TITLE)
            .with_icon(generated_tray_icon()?)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|_| DesktopHostRuntimeError::Tray)?;

        let show_id = show_item.id().clone();
        let quit_id = quit_item.id().clone();
        let menu_events = events.clone();
        let menu_context = context.clone();
        tray_icon::menu::MenuEvent::set_event_handler(Some(
            move |event: tray_icon::menu::MenuEvent| {
                let host_event = if event.id == show_id {
                    Some(DesktopHostEvent::TrayShow)
                } else if event.id == quit_id {
                    Some(DesktopHostEvent::TrayQuit)
                } else {
                    None
                };
                if let Some(host_event) = host_event
                    && menu_events.send(host_event).is_ok()
                {
                    menu_context.request_repaint();
                }
            },
        ));

        let tray_id = tray.id().clone();
        tray_icon::TrayIconEvent::set_event_handler(Some(
            move |event: tray_icon::TrayIconEvent| {
                let show = match &event {
                    tray_icon::TrayIconEvent::Click {
                        id,
                        button: tray_icon::MouseButton::Left,
                        button_state: tray_icon::MouseButtonState::Up,
                        ..
                    }
                    | tray_icon::TrayIconEvent::DoubleClick {
                        id,
                        button: tray_icon::MouseButton::Left,
                        ..
                    } => id == &tray_id,
                    _ => false,
                };
                if show && events.send(DesktopHostEvent::TrayShow).is_ok() {
                    context.request_repaint();
                }
            },
        ));

        Ok(Self {
            _tray: tray,
            show_item,
            quit_item,
        })
    }

    fn set_locale(&self, locale: Locale) {
        let (show, quit) = tray_labels(locale);
        self.show_item.set_text(show);
        self.quit_item.set_text(quit);
    }
}

fn generated_tray_icon() -> Result<tray_icon::Icon, DesktopHostRuntimeError> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let offset = ((y * SIZE + x) * 4) as usize;
            let inside = (4..28).contains(&x) && (4..28).contains(&y);
            let mark = (9..14).contains(&x) && (9..23).contains(&y)
                || (9..23).contains(&x) && (9..14).contains(&y)
                || (9..23).contains(&x) && (18..23).contains(&y);
            if inside {
                rgba[offset..offset + 4].copy_from_slice(if mark {
                    &[0xff, 0xff, 0xff, 0xff]
                } else {
                    &[0xd9, 0x3d, 0x47, 0xff]
                });
            }
        }
    }
    tray_icon::Icon::from_rgba(rgba, SIZE, SIZE).map_err(|_| DesktopHostRuntimeError::Tray)
}

fn tray_labels(locale: Locale) -> (&'static str, &'static str) {
    match locale {
        Locale::ZhCn => ("显示窗口", "退出"),
        Locale::En => ("Show", "Quit"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_host_close_and_minimize_hide_without_quitting() {
        let mut lifecycle = DesktopHostLifecycle::default();

        assert_eq!(
            lifecycle.reduce(DesktopHostEvent::CloseRequested),
            [DesktopHostEffect::CancelClose, DesktopHostEffect::Hide]
        );
        assert_eq!(
            lifecycle.reduce(DesktopHostEvent::Minimized(true)),
            [DesktopHostEffect::Hide]
        );
        assert!(
            lifecycle
                .reduce(DesktopHostEvent::Minimized(true))
                .is_empty()
        );
        assert_eq!(lifecycle.phase(), DesktopHostPhase::Running);
    }

    #[test]
    fn desktop_host_typed_activations_restore_focus_and_keep_payload_out_of_events() {
        let mut lifecycle = DesktopHostLifecycle::default();

        assert_eq!(
            lifecycle.reduce(DesktopHostEvent::Activation(
                codex_plus_core::manager_instance::ManagerActivation::Show
            )),
            [
                DesktopHostEffect::Show,
                DesktopHostEffect::Restore,
                DesktopHostEffect::Focus,
            ]
        );
        assert_eq!(
            lifecycle.reduce(DesktopHostEvent::Activation(
                codex_plus_core::manager_instance::ManagerActivation::ReloadPendingProviderImport
            )),
            [
                DesktopHostEffect::ReloadPendingProviderImport,
                DesktopHostEffect::Show,
                DesktopHostEffect::Restore,
                DesktopHostEffect::Focus,
            ]
        );
        assert_eq!(
            lifecycle.reduce(DesktopHostEvent::Activation(
                codex_plus_core::manager_instance::ManagerActivation::ShowUpdate
            )),
            [
                DesktopHostEffect::ShowUpdate,
                DesktopHostEffect::Show,
                DesktopHostEffect::Restore,
                DesktopHostEffect::Focus,
            ]
        );
    }

    #[test]
    fn desktop_host_quit_is_explicit_exactly_once_and_does_not_cancel_exit_close() {
        let mut lifecycle = DesktopHostLifecycle::default();

        assert_eq!(
            lifecycle.reduce(DesktopHostEvent::TrayQuit),
            [DesktopHostEffect::RequestExit]
        );
        assert_eq!(lifecycle.phase(), DesktopHostPhase::Quitting);
        assert!(lifecycle.reduce(DesktopHostEvent::TrayQuit).is_empty());
        assert!(
            lifecycle
                .reduce(DesktopHostEvent::CloseRequested)
                .is_empty()
        );
        assert!(
            lifecycle
                .reduce(DesktopHostEvent::Activation(
                    codex_plus_core::manager_instance::ManagerActivation::Show
                ))
                .is_empty()
        );
        lifecycle.mark_exited();
        assert_eq!(lifecycle.phase(), DesktopHostPhase::Exited);
    }

    #[test]
    fn desktop_host_persistence_uses_canonical_subdirectory_and_legacy_additive_path() {
        let temp = tempfile::tempdir().unwrap();

        let paths = NativePersistencePaths::for_state_override(Some(temp.path().to_path_buf()));

        assert_eq!(paths.canonical(), temp.path().join("manager-ui/app.ron"));
        assert_eq!(paths.legacy().unwrap(), temp.path().join("app.ron"));
    }

    #[test]
    fn desktop_host_migrates_legacy_preferences_once_without_removing_source() {
        let temp = tempfile::tempdir().unwrap();
        let paths = NativePersistencePaths::for_state_override(Some(temp.path().to_path_buf()));
        std::fs::write(paths.legacy().unwrap(), b"legacy-preferences").unwrap();

        assert_eq!(
            paths.migrate_legacy_if_needed().unwrap(),
            PreferenceMigration::Migrated
        );
        assert_eq!(
            std::fs::read(paths.canonical()).unwrap(),
            b"legacy-preferences"
        );
        assert!(paths.legacy().unwrap().exists());

        std::fs::write(paths.legacy().unwrap(), b"new-legacy-value").unwrap();
        assert_eq!(
            paths.migrate_legacy_if_needed().unwrap(),
            PreferenceMigration::CanonicalAlreadyExists
        );
        assert_eq!(
            std::fs::read(paths.canonical()).unwrap(),
            b"legacy-preferences"
        );
    }

    #[test]
    fn desktop_host_migration_ignores_webview_cache_and_missing_legacy_file() {
        let temp = tempfile::tempdir().unwrap();
        let paths = NativePersistencePaths::for_state_override(Some(temp.path().to_path_buf()));
        let cache = temp.path().join("WebView2/Default/Local Storage");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("app.ron"), b"decoy").unwrap();

        assert_eq!(
            paths.migrate_legacy_if_needed().unwrap(),
            PreferenceMigration::NoLegacyPreferences
        );
        assert!(!paths.canonical().exists());
    }

    #[test]
    fn desktop_host_tray_labels_follow_native_locale() {
        assert_eq!(tray_labels(crate::i18n::Locale::ZhCn), ("显示窗口", "退出"));
        assert_eq!(tray_labels(crate::i18n::Locale::En), ("Show", "Quit"));
    }
}
