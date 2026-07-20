use codex_plus_core::desktop_integration::DesktopRepairOperation;
use codex_plus_core::startup_registration::StartupRegistrationOperation;

use crate::{
    DesktopIntegrationEnvironment, DesktopIntegrationEnvironmentError,
    DesktopIntegrationEnvironmentSnapshot,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemDesktopIntegrationEnvironment;

impl SystemDesktopIntegrationEnvironment {
    pub const fn new() -> Self {
        Self
    }
}

impl DesktopIntegrationEnvironment for SystemDesktopIntegrationEnvironment {
    fn inspect_desktop_integration(
        &self,
    ) -> Result<DesktopIntegrationEnvironmentSnapshot, DesktopIntegrationEnvironmentError> {
        #[cfg(windows)]
        {
            let current_exe = std::env::current_exe()
                .map_err(|_| DesktopIntegrationEnvironmentError::InspectFailed)?;
            let launcher_path = current_exe
                .parent()
                .map(|dir| dir.join(format!("{}.exe", codex_plus_core::install::SILENT_BINARY)))
                .ok_or(DesktopIntegrationEnvironmentError::InspectFailed)?;
            let repair =
                codex_plus_core::desktop_integration::inspect_system_windows_desktop(current_exe)
                    .map_err(|_| DesktopIntegrationEnvironmentError::InspectFailed)?;
            let sign_in =
                codex_plus_core::startup_registration::inspect_system_startup_registration(
                    launcher_path,
                )
                .map_err(|_| DesktopIntegrationEnvironmentError::InspectFailed)?;
            Ok(DesktopIntegrationEnvironmentSnapshot::Windows {
                repair: Box::new(repair),
                sign_in,
            })
        }

        #[cfg(target_os = "macos")]
        {
            let current_exe = std::env::current_exe()
                .map_err(|_| DesktopIntegrationEnvironmentError::InspectFailed)?;
            let repair =
                codex_plus_core::desktop_integration::inspect_system_macos_desktop(current_exe)
                    .map_err(|_| DesktopIntegrationEnvironmentError::InspectFailed)?;
            Ok(DesktopIntegrationEnvironmentSnapshot::Macos { repair })
        }

        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Ok(DesktopIntegrationEnvironmentSnapshot::Unsupported)
        }
    }

    fn apply_desktop_repair_operation(
        &self,
        operation: &DesktopRepairOperation,
    ) -> Result<(), DesktopIntegrationEnvironmentError> {
        #[cfg(windows)]
        {
            codex_plus_core::desktop_integration::apply_system_windows_repair_operation(operation)
                .map_err(|_| DesktopIntegrationEnvironmentError::EffectFailed)
        }

        #[cfg(target_os = "macos")]
        {
            codex_plus_core::desktop_integration::apply_system_macos_repair_operation(operation)
                .map_err(|_| DesktopIntegrationEnvironmentError::EffectFailed)
        }

        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = operation;
            Err(DesktopIntegrationEnvironmentError::EffectFailed)
        }
    }

    fn apply_startup_registration_operation(
        &self,
        operation: &StartupRegistrationOperation,
    ) -> Result<(), DesktopIntegrationEnvironmentError> {
        #[cfg(windows)]
        {
            codex_plus_core::startup_registration::apply_system_startup_registration_operation(
                operation,
            )
            .map_err(|_| DesktopIntegrationEnvironmentError::EffectFailed)
        }

        #[cfg(not(windows))]
        {
            let _ = operation;
            Err(DesktopIntegrationEnvironmentError::EffectFailed)
        }
    }
}
