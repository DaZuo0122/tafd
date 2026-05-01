pub mod backend;

#[cfg(windows)]
pub mod win32;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "linux")]
pub mod linux;

pub use backend::InputBackend;
