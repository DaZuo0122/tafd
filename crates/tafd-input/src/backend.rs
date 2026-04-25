use std::sync::atomic::AtomicBool;
use tafd_core::{InputConfig, Result};

/// Platform-agnostic input backend.
pub trait InputBackend: Send {
    /// Block and run the input loop until `shutdown` is true.
    fn run(&self, shutdown: &AtomicBool) -> Result<()>;
}

/// Create the appropriate platform input backend.
pub fn create(config: &InputConfig) -> Result<Box<dyn InputBackend>> {
    #[cfg(windows)]
    {
        Ok(Box::new(win32::Win32Input::new(config)?))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacosInput::new(config)?))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::LinuxInput::new(config)?))
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Err(TafdError::InputInit("Unsupported platform".into()))
    }
}

#[cfg(windows)]
use crate::win32;
#[cfg(target_os = "macos")]
use crate::macos;
#[cfg(target_os = "linux")]
use crate::linux;
