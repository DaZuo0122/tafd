use std::ffi::CString;
use std::fs;
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use tafd_core::{InputConfig, INPUT_QUEUE, Result, TafdError};

const EV_KEY: u16 = 0x01;
const O_RDONLY: i32 = 0;
const O_NONBLOCK: i32 = 0x800;
const EPOLLIN: u32 = 0x001;
const EPOLL_CTL_ADD: i32 = 1;
const EVTYPE_BUF_LEN: u32 = 4;
const KEY_BITS_BUF_LEN: u32 = 96;
const KEY_A: usize = 30;
const KEY_Z: usize = 44;

const fn eviocgbit(ev: u32, len: u32) -> libc::c_ulong {
    // _IOC(READ=2, type='E'=0x45, nr=0x20+ev, size=len)
    ((2_u32 << 30) | (len << 16) | (0x45 << 8) | (0x20 + ev)) as libc::c_ulong
}

#[repr(C)]
struct input_event {
    time_sec: libc::c_long,
    time_usec: libc::c_long,
    type_: u16,
    code: u16,
    value: i32,
}

pub struct LinuxInput {
    suppress_repeat: bool,
}

impl LinuxInput {
    pub fn new(config: &InputConfig) -> Result<Self> {
        Ok(Self {
            suppress_repeat: config.suppress_repeat,
        })
    }
}

impl super::InputBackend for LinuxInput {
    fn run(&self, shutdown: &AtomicBool) -> Result<()> {
        log::info!("Linux input backend starting (evdev)");

        let devices = discover_evdev_devices()?;
        if devices.is_empty() {
            return Err(TafdError::InputInit(
                "No input devices found in /dev/input".into(),
            ));
        }

        let epoll_fd = unsafe { libc::epoll_create1(libc::O_CLOEXEC) };
        if epoll_fd < 0 {
            return Err(TafdError::InputInit(format!(
                "epoll_create1 failed: {}",
                std::io::Error::last_os_error()
            )));
        }

        // Add all device fds to epoll
        for fd in &devices {
            let mut ev = libc::epoll_event {
                events: EPOLLIN,
                u64: *fd as u64,
            };
            let ret = unsafe { libc::epoll_ctl(epoll_fd, EPOLL_CTL_ADD, *fd, &mut ev) };
            if ret < 0 {
                log::warn!(
                    "Failed to add fd {} to epoll: {}",
                    fd,
                    std::io::Error::last_os_error()
                );
            }
        }

        let mut events: [libc::epoll_event; 16] = unsafe { std::mem::zeroed() };

        while !shutdown.load(Ordering::Relaxed) {
            let nfds = unsafe {
                libc::epoll_wait(
                    epoll_fd,
                    events.as_mut_ptr(),
                    events.len() as i32,
                    100, // 100ms timeout so we can check shutdown
                )
            };

            if nfds < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() != Some(libc::EINTR) {
                    log::error!("epoll_wait error: {err}");
                }
                continue;
            }

            for i in 0..nfds as usize {
                let fd = events[i].u64 as RawFd;
                if let Err(e) = read_device(fd, self.suppress_repeat) {
                    log::warn!("Error reading device fd {fd}: {e}");
                }
            }
        }

        // Cleanup
        unsafe {
            libc::close(epoll_fd);
            for fd in devices {
                libc::close(fd);
            }
        }

        log::info!("Linux input backend stopped");
        Ok(())
    }
}

fn discover_evdev_devices() -> Result<Vec<RawFd>> {
    let mut fds = Vec::new();
    let mut had_eacces = false;

    let entries = match fs::read_dir("/dev/input") {
        Ok(e) => e,
        Err(e) => {
            return Err(TafdError::InputInit(format!(
                "Failed to read /dev/input: {e}"
            )))
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name_str = match name.to_str() {
            Some(s) => s,
            None => continue,
        };
        if !name_str.starts_with("event") {
            continue;
        }

        let path = entry.path();
        let c_path = match CString::new(path.as_os_str().as_encoded_bytes()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let fd = unsafe { libc::open(c_path.as_ptr(), O_RDONLY | O_NONBLOCK) };
        if fd < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EACCES) {
                had_eacces = true;
                log::warn!("Permission denied opening {}", path.display());
            } else {
                log::debug!("Failed to open {}: {err}", path.display());
            }
            continue;
        }

        if !has_keyboard_capability(fd) {
            log::debug!(
                "Skipping non-keyboard device {} (no EV_KEY alpha keys)",
                path.display()
            );
            unsafe { libc::close(fd) };
            continue;
        }

        fds.push(fd);
        log::info!("Opened keyboard device: {}", path.display());
    }

    if fds.is_empty() && had_eacces {
        return Err(TafdError::InputPermission(
            "Add user to 'input' group or run `sudo setcap cap_evdev+ep ./tafd`.".into(),
        ));
    }

    Ok(fds)
}

fn has_keyboard_capability(fd: RawFd) -> bool {
    let mut evtype_bits: u32 = 0;
    let ret = unsafe {
        libc::ioctl(fd, eviocgbit(0, EVTYPE_BUF_LEN), &mut evtype_bits as *mut u32)
    };
    if ret < 0 {
        return false;
    }
    if evtype_bits & (1 << (EV_KEY as u32)) == 0 {
        return false;
    }

    let mut key_bits = [0u8; KEY_BITS_BUF_LEN as usize];
    let ret = unsafe {
        libc::ioctl(fd, eviocgbit(EV_KEY as u32, KEY_BITS_BUF_LEN), key_bits.as_mut_ptr())
    };
    if ret < 0 {
        return false;
    }

    for code in KEY_A..=KEY_Z {
        if key_bits[code / 8] & (1 << (code % 8)) != 0 {
            return true;
        }
    }
    false
}

fn read_device(fd: RawFd, suppress_repeat: bool) -> std::io::Result<()> {
    let mut event: input_event = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<input_event>();

    loop {
        let n = unsafe {
            libc::read(
                fd,
                &mut event as *mut _ as *mut libc::c_void,
                size,
            )
        };

        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EAGAIN)
                || err.raw_os_error() == Some(libc::EWOULDBLOCK)
            {
                return Ok(());
            }
            return Err(err);
        }

        if n as usize != size {
            // Partial read; for simplicity just return and let next epoll handle it
            return Ok(());
        }

        if event.type_ == EV_KEY && event.value == 1 {
            // Key press (value 2 is repeat, 0 is release)
            let keycode = event.code as u32;
            if INPUT_QUEUE.push(keycode).is_err() {
                log::trace!("Input queue full, dropping key event");
            }
        } else if event.type_ == EV_KEY && event.value == 2 && suppress_repeat {
            // Repeat event - drop
            continue;
        }
    }
}
