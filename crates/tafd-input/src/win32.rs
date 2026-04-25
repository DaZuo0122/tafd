use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tafd_core::{InputConfig, INPUT_QUEUE, Result, TafdError};
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT,
    WM_SYSKEYDOWN,
};

static INPUT_THREAD_ID: AtomicU32 = AtomicU32::new(0);

/// Wake the input thread so it can observe shutdown.
pub fn wake_input_thread() {
    let tid = INPUT_THREAD_ID.load(Ordering::Relaxed);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

pub struct Win32Input {
    suppress_repeat: bool,
}

impl Win32Input {
    pub fn new(config: &InputConfig) -> Result<Self> {
        Ok(Self {
            suppress_repeat: config.suppress_repeat,
        })
    }
}

impl super::InputBackend for Win32Input {
    fn run(&self, shutdown: &AtomicBool) -> Result<()> {
        let suppress_repeat = self.suppress_repeat;

        // Install the low-level keyboard hook
        let hook = unsafe {
            let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .map_err(|e| TafdError::InputInit(format!("GetModuleHandleW failed: {e}")))?;
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(hook_proc),
                HINSTANCE(hinst.0),
                0,
            )
            .map_err(|e| TafdError::InputInit(format!("SetWindowsHookExW failed: {e}")))?
        };

        // Set global state for the hook proc
        HOOK_STATE.with(|s| {
            let mut state = s.borrow_mut();
            state.suppress_repeat = suppress_repeat;
        });

        log::info!("Windows low-level keyboard hook installed");

        // Register thread ID so shutdown can post WM_QUIT
        INPUT_THREAD_ID.store(
            unsafe { windows::Win32::System::Threading::GetCurrentThreadId() },
            Ordering::Relaxed,
        );

        // Run message loop
        let mut msg = Default::default();
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if ret.0 <= 0 {
                break;
            }
        }

        // Unhook
        unsafe {
            let _ = UnhookWindowsHookEx(hook);
        }
        log::info!("Windows keyboard hook removed");
        Ok(())
    }
}

// Thread-local state accessible from the hook proc
thread_local! {
    static HOOK_STATE: std::cell::RefCell<HookState> = std::cell::RefCell::new(HookState::new());
}

struct HookState {
    suppress_repeat: bool,
}

impl HookState {
    fn new() -> Self {
        Self {
            suppress_repeat: true,
        }
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let msg = wparam.0 as u32;

        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            // Check repeat flag (bit 30)
            let is_repeat = (info.flags.0 & 0x4000) != 0;
            let suppress = HOOK_STATE.with(|s| s.borrow().suppress_repeat);

            if !is_repeat || !suppress {
                let keycode = info.vkCode;
                if INPUT_QUEUE.push(keycode).is_err() {
                    log::trace!("Input queue full, dropping key event");
                }
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}
