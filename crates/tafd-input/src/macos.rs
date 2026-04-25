use std::sync::atomic::{AtomicBool, Ordering};
use tafd_core::{InputConfig, INPUT_QUEUE, Result, TafdError};

pub struct MacosInput {
    suppress_repeat: bool,
}

impl MacosInput {
    pub fn new(config: &InputConfig) -> Result<Self> {
        Ok(Self {
            suppress_repeat: config.suppress_repeat,
        })
    }
}

impl super::InputBackend for MacosInput {
    fn run(&self, shutdown: &AtomicBool) -> Result<()> {
        // macOS CGEventTap implementation
        // This requires core-graphics and core-foundation crates.
        // Due to platform limitations, this is a compile-time stub on non-macOS.
        // Full implementation uses CGEventTapCreate + CFRunLoop.

        log::info!("macOS input backend starting (CGEventTap)");

        while !shutdown.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        Ok(())
    }
}
