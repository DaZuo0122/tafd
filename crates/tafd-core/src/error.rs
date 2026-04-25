use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TafdError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Audio device not found: {0}")]
    DeviceNotFound(String),

    #[error("Audio engine error: {0}")]
    AudioEngine(String),

    #[error("Failed to load sound pack at {path}: {reason}")]
    SoundPackLoad { path: PathBuf, reason: String },

    #[error("Input initialization failed: {0}")]
    InputInit(String),

    #[error("Permission denied accessing input devices. {0}")]
    InputPermission(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Shutdown signal received")]
    Shutdown,
}

pub type Result<T> = std::result::Result<T, TafdError>;
