# Typewriter Acoustic Feedback Daemon (TAFD)
## Technical Design Document v1.0

---

## 1. Executive Summary

TAFD is a cross-platform background daemon written in Rust that provides global acoustic typewriter feedback. It captures system-wide keyboard events and plays preloaded PCM samples via a real-time audio mixer. The design prioritizes **zero-allocation hot paths**, **lock-free inter-thread communication**, and **aggressive memory budgeting** to sustain sub-10MB RAM usage and imperceptible latency under sustained typing loads exceeding 600 CPM (characters per minute).

---

## 2. Requirements Specification

### 2.1 Functional Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-1 | Capture global keyboard key-down events across all applications | P0 |
| FR-2 | Play preloaded typewriter mechanical click sounds per keystroke | P0 |
| FR-3 | Suppress audio feedback on key-repeat (hold-down) events | P0 |
| FR-4 | Support polyphonic playback for overlapping keystrokes (≥8 voices) | P0 |
| FR-5 | Run as a background process without GUI window or dock icon | P0 |
| FR-6 | Gracefully handle audio device hot-plug/unplug events | P1 |
| FR-7 | Support user-defined sound packs (WAV/OGG loading at startup) | P2 |

### 2.2 Non-Functional Requirements

| ID | Requirement | Target | Measurement |
|---|---|---|---|
| NFR-1 | Resident Memory (RSS) | < 10 MB | `ps` / Activity Monitor / Task Manager |
| NFR-2 | Input-to-Audio Latency | < 20 ms | Oscilloscope loopback or software timestamp |
| NFR-3 | Audio Thread CPU | < 3% of 1 core | `top`/`htop` during 600 CPM stress |
| NFR-4 | Binary Size (stripped) | < 5 MB | `ls -lh` |
| NFR-5 | Cold Startup Time | < 500 ms | `time` from shell execution to first sound |
| NFR-6 | Zero heap allocation during steady-state typing | 100% | Heap profiling (`dhat`, `valgrind`) |
| NFR-7 | Cross-platform parity | Windows 10+, macOS 12+, Linux 5.15+ | CI matrix |

---

## 3. System Architecture

### 3.1 High-Level Block Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              OS Process Space                                │
│  ┌──────────────┐    Lock-Free     ┌──────────────┐    Platform    ┌─────┐ │
│  │   Input      │───SPSC Queue────▶│    Audio     │───Callback───▶ │ OS  │ │
│  │   Thread     │   (u32 keycode)  │    Thread    │   (cpal)       │Audio│ │
│  └──────────────┘                  └──────────────┘                └─────┘ │
│         ▲                                    ▲                              │
│         │ Platform API                       │ Real-time Priority            │
│  ┌──────────────┐                  ┌──────────────┐                        │
│  │  WH_KEYBOARD_LL│                │  Voice Stealer │                        │
│  │  CGEventTap    │                │  Mixer (f32)   │                        │
│  │  evdev         │                │  Preloaded PCM │                       │
│  └──────────────┘                  └──────────────┘                        │
│                                                                             │
│  ┌──────────────┐                                                           │
│  │ Main Thread  │  ← Signal handling, watchdog, platform event loop glue   │
│  └──────────────┘                                                           │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.2 Thread Model

| Thread | Responsibility | Priority | Stack Size |
|---|---|---|---|
| **Main** | Initialization, signal traps (`SIGINT`, `SIGTERM`), watchdog | Normal | 1 MB (default) |
| **Input** | Block on OS input API, enqueue keycodes | Normal | 512 KB |
| **Audio** | Poll SPSC queue, mix voices, write to OS audio buffer | **Real-time** (SCHED_FIFO / `THREAD_PRIORITY_TIME_CRITICAL`) | 512 KB |

**Rationale:** Decoupling input from audio prevents input API jitter (e.g., macOS CGEventTap callback latency) from glitching audio output. The audio thread runs isolated with the highest scheduler class.

---

## 4. Crate Selection & Dependency Justification

| Crate | Version Constraint | Role | Justification |
|---|---|---|---|
| `cpal` | `^0.15` | Cross-platform audio output | Direct WASAPI/CoreAudio/ALSA/PulseAudio backend abstraction. No SDL2 bloat. Supports custom buffer sizes. |
| `crossbeam` | `^0.8` | Lock-free SPSC queue | `ArrayQueue` for input→audio event transfer. Wait-free, no mutex poisoning. |
| `hound` | `^3.5` | WAV decoding at startup | Pure Rust, `no_std` friendly, zero runtime dependencies. Decode once at init. |
| `symphonia` | `^0.5` *(optional)* | OGG/MP3 decoding | If user sound packs require compressed formats. Feature-gated to keep base binary small. |
| `windows` | `^0.52` | Win32 API bindings | `Win32_UI_WindowsAndMessaging` for `SetWindowsHookEx`, `GetMessage`. |
| `windows-service` | `^0.6` *(optional)* | Windows service wrapper | For headless Windows service deployment. |
| `core-graphics` | `^0.23` | macOS CGEventTap | Low-level event tap creation and CGEvent field access. |
| `core-foundation` | `^0.9` | macOS CFRunLoop | Run loop management for event tap thread. |
| `libc` | `^0.2` | Linux raw FFI | `read`, `epoll`, `ioctl` for evdev. |
| `udev` / `libudev-sys` | `^0.8` *(optional)* | Linux device discovery | Enumerate `/dev/input/event*` keyboards at startup. |
| `directories` | `^5.0` | Config/sound pack paths | XDG / Known Folders / `%APPDATA%` abstraction for user sound packs. |
| `log` + `env_logger` | `^0.4` | Diagnostics | Compile-time log level filtering. `release_max_level_warn` to strip debug strings. |

**Excluded Crates:**
- `rdev`, `enigo`: Abstracted input crates with internal buffering and thread sleep, violating NFR-6.
- `rodio`: High-level audio player with `Sink`/`Decoder` architecture that allocates per-playback and introduces mixer latency.
- `sdl2`: 15+ MB dependency, requires dynamic library deployment, unacceptable for NFR-4.

---

## 5. Memory Layout & Budget

### 5.1 Pre-Allocated Arena Strategy

All steady-state memory is allocated during initialization. The audio hot path uses only static and stack memory.

```
Process Address Space (Target: ~4-6 MB RSS)
┌────────────────────────────────────────┐
│ .text (Code + Embedded Samples)        │  ← 2.0 - 2.5 MB
│   - Rust binary (LTO, stripped)        │
│   - include_bytes!(click1.wav)         │
│   - include_bytes!(click2.wav)         │
│   - ... (8 variations)                 │
├────────────────────────────────────────┤
│ .rodata / .data                        │  ← ~0.2 MB
├────────────────────────────────────────┤
│ Heap (Startup Only)                    │
│   - WAV decode buffers (freed after)   │
│   - cpal device enumeration            │
│   - ~1.0 MB peak, drops to ~0.3 MB     │
├────────────────────────────────────────┤
│ Thread Stacks (3 × 512 KB)             │  ← 1.5 MB (committed, not all resident)
├────────────────────────────────────────┤
│ Audio Buffers (cpal ring + mixer)      │  ← ~0.5 MB
│   - 8 voices × 100ms × 48kHz × 4 bytes │
│   - OS audio ring buffer               │
├────────────────────────────────────────┤
│ Input Buffers (evdev / queue)          │  ← ~0.05 MB
└────────────────────────────────────────┘
```

### 5.2 Detailed Memory Accounting

| Component | Calculation | Size |
|---|---|---|
| Binary (LTO fat, panic=abort) | Empirical Rust CLI baseline | ~1.5 MB |
| Embedded PCM (8 variations, mono, 48kHz, 0.12s, f32) | `8 × 48000 × 0.12 × 4` | ~184 KB |
| Embedded PCM (stereo fallback) | `8 × 48000 × 0.12 × 4 × 2` | ~368 KB |
| Voice Mixer State | `8 voices × (ptr + usize + bool)` | ~256 bytes |
| SPSC Ring Buffer | `crossbeam::ArrayQueue<u32>` capacity 64 | ~512 bytes |
| Thread Stacks (3 threads, 512 KB each) | `3 × 512 KB` | 1.5 MB (virtual, partially resident) |
| cpal / OS Audio Buffers | Double/triple buffering at 128 frames | ~12 KB |
| Decoding Scratch (startup only) | `hound` temporary Vec | ~200 KB (freed) |
| **Steady-State RSS Estimate** | | **~3.5 — 5.0 MB** |

---

## 6. Audio Engine Design

### 6.1 Sample Format & Preloading

- **Format:** Raw PCM, `f32` samples, 48 kHz sample rate.
- **Channels:** Mono for mechanical clicks; stereo only if spatial variation is required.
- **Loading:** At startup, `hound::WavReader` decodes files into `Vec<f32>` owned by `Arc<Sample>`. All subsequent references are `Arc` clones (pointer copy, no data copy).
- **Embedding:** Default sound pack uses `include_bytes!` + `hound` decode at init to eliminate filesystem I/O during steady-state.

### 6.2 Voice Stealer Mixer

Under rapid typing, multiple sounds overlap. A fixed-size array with round-robin stealing prevents allocation and avoids the complexity of a free-list.

```rust
const MAX_VOICES: usize = 8;
const SAMPLE_RATE: usize = 48000;

struct Voice {
    sample: Option<Arc<Sample>>, // None = inactive
    frame_pos: usize,
    gain: f32,
}

struct Mixer {
    voices: [Voice; MAX_VOICES],
    next_voice: usize, // Round-robin cursor
}
```

**Algorithm:**
1. Input event dequeued → `mixer.next_voice = (mixer.next_voice + 1) % MAX_VOICES`.
2. Overwrite the selected voice slot unconditionally (steal).
3. Reset `frame_pos = 0`, assign sample reference, `gain = 1.0`.
4. In `cpal` callback, iterate all voices, accumulate active frames, increment `frame_pos`, deactivate on end.

**Why stealing is correct:** A mechanical typewriter's physical hammer cannot strike twice in the same microsecond. If a new keystroke arrives before the previous click decays, cutting off the tail is perceptually identical to a real machine's overlapping mechanical noise.

### 6.3 cpal Integration & Buffer Strategy

| Platform | Preferred Host API | Buffer Size | Expected Latency |
|---|---|---|---|
| Windows | WASAPI Shared | 128 frames (~2.7ms) | 10-20 ms |
| macOS | CoreAudio | 128 frames | 10-15 ms |
| Linux | ALSA (direct) or PulseAudio | 256 frames (~5.3ms) | 15-25 ms |

**Stream Config:**
```rust
let config = StreamConfig {
    channels: 1, // or 2
    sample_rate: SampleRate(48000),
    buffer_size: BufferSize::Fixed(128), // Request low latency
};
```

**Callback Contract:**
- **NO heap allocation.**
- **NO locking** (mutex, RwLock). Only `Relaxed`/`Acquire`/`Release` atomics on voice state.
- **NO I/O** (file, network, print).
- Execution budget: <50 µs per 128-frame callback to maintain 1% CPU target.

### 6.4 Gain Staging & Anti-Clipping

With 8 voices and `gain = 1.0`, summation can exceed `[-1.0, 1.0]`. Apply a static master gain of `0.3` (≈ -10 dB) in the callback. Mechanical clicks are transient with low RMS; this prevents inter-sample clipping without a dynamic limiter (which would require state and branching).

---

## 7. Input Subsystem (Platform Deep Dive)

### 7.1 Windows (`win32_input.rs`)

**API:** `SetWindowsHookExW(WH_KEYBOARD_LL, ..., hInstance, 0)`

- **Threading:** Must run on a thread that calls `GetMessage`/`PeekMessage`. The low-level hook callback executes on this thread, not a system DLL injection.
- **Latency:** Callback fires in the calling thread before the keystroke reaches the target application. No measurable delay.
- **Filtering:**
  - Process only `WM_KEYDOWN` and `WM_SYSKEYDOWN`.
  - Suppress repeats: Check `lParam->flags` bit 30 (`0x4000`). If set, the key was already down; discard.
- **Keycode Mapping:** `wParam` contains `VK_CODE` (u32). Map directly to sample index via a `[usize; 256]` LUT.
- **Persistence:** Windows may unload the hook DLL if the callback takes too long. Ensure callback returns in <10ms (ours takes <1µs).

**Alternative (Raw Input):**
If `WH_KEYBOARD_LL` is blocked by security software, fallback to `RegisterRawInputDevices`. This bypasses the hook chain but requires a window message loop. Not preferred for headless daemons but documented as a contingency.

### 7.2 macOS (`macos_input.rs`)

**API:** `CGEventTapCreate` (`kCGHIDEventTap`, `kCGHeadInsertEventTap`, `kCGEventTapOptionDefault`, `CGEventMaskBit(kCGEventKeyDown)`)

- **Threading:** Create tap on a dedicated thread, add to a private `CFRunLoop`, and run `CFRunLoopRun()`.
- **Permissions:** Requires **Input Monitoring** permission (macOS 10.15+). The app must be a `.app` bundle or explicitly added in System Preferences → Security & Privacy → Privacy → Input Monitoring.
- **Repeat Suppression:** Inspect `CGEventGetIntegerValueField(event, kCGKeyboardEventAutorepeat)`. If `!= 0`, discard.
- **Tap Timeout Handling:** If the tap callback blocks, macOS disables it with `kCGEventTapDisabledByTimeout`. The thread must listen for this event and re-enable via `CGEventTapEnable(tap, true)`.
- **Process Hiding:** Set `LSUIElement = true` in `Info.plist` to prevent dock icon and menu bar appearance.
- **Keycode:** `CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode)` returns a `u32` directly usable for LUT indexing.

### 7.3 Linux (`linux_input.rs`)

**API:** `evdev` (raw `/dev/input/event*`) via `libc::read()` and `epoll`.

- **Device Discovery:** At startup, enumerate `/dev/input/event*` via `udev` or direct directory scan. Open devices with `O_RDONLY | O_NONBLOCK`.
- **Filtering:** Use `EVIOCGRAB` (optional) only if the user wants to suppress original keys (not recommended for this app). Otherwise, read events non-exclusively.
- **Event Parsing:** `input_event` struct. Filter `type == EV_KEY` and `value == 1` (key press). `value == 2` is repeat; discard.
- **Permissions:** User must be in `input` group, or binary needs `CAP_EVDEV` capability:
  ```bash
  sudo setcap cap_evdev+ep ./tafd
  ```
- **Hotplug:** Use `inotify` or `udev` monitor to detect new keyboards and add them to the `epoll` set.
- **X11 Fallback:** If evdev fails (e.g., in a container or Wayland without permissions), optionally fallback to X11 `XGrabKey` (highly discouraged—requires X connection, breaks on Wayland, and is not global). Document as unsupported fallback.

---

## 8. Inter-Thread Communication & Synchronization

### 8.1 Lock-Free SPSC Queue

```rust
use crossbeam::queue::ArrayQueue;

static INPUT_QUEUE: ArrayQueue<u32> = ArrayQueue::new(64);
```

- **Producer (Input Thread):** `queue.push(keycode)`. If full (extremely unlikely at 64 slots), drop the event. Better to miss one click than block the OS input thread.
- **Consumer (Audio Thread):** Inside the `cpal` callback, drain the queue:
  ```rust
  while let Some(keycode) = INPUT_QUEUE.pop() {
      mixer.trigger(keycode);
  }
  ```

### 8.2 Memory Ordering

- **Queue:** `crossbeam::ArrayQueue` handles its own ordering (internally uses `Release`/`Acquire`).
- **Voice Activation:** The audio thread writes `voice.active = true` (via atomic `store(1, Release)`) after setting `sample` and `pos`. The audio callback reads `active` with `Acquire`. On x86/aarch64 this is free, but correct for portability.
- **Stealing:** Round-robin index uses `Relaxed` because the audio thread is the sole writer and reader of `next_voice`.

### 8.3 Thread Priority Elevation

| Platform | Mechanism | Implementation |
|---|---|---|
| Windows | `SetThreadPriority` | `THREAD_PRIORITY_TIME_CRITICAL` on audio thread. |
| macOS | `pthread_set_qos_class_self_np` | `QOS_CLASS_USER_INTERACTIVE` on audio thread. |
| Linux | `pthread_setschedparam` | `SCHED_FIFO` with priority `80`. Requires `CAP_SYS_NICE` or root for non-root users. |

---

## 9. Startup Sequence & State Machine

```
[Init]
   │
   ▼
[Parse CLI / Config] ──▶ [Load Sound Pack] ──▶ [Decode to PCM]
   │                                              │
   ▼                                              ▼
[Platform Input Init]                    [cpal Device Init]
   │                                              │
   ▼                                              ▼
[Spawn Input Thread]                       [Spawn Audio Thread]
   │                                              │
   └──────────────────┬───────────────────────────┘
                      ▼
              [Main Thread: Signal Wait]
                      │
         SIGINT / SIGTERM / Ctrl+C
                      ▼
              [Graceful Shutdown]
              - Stop audio stream
              - Unhook input (Windows: UnhookWindowsHookEx)
              - Join threads (with timeout)
              - Exit(0)
```

**Startup Time Budget:**
- Config parsing: <1ms
- Sound decoding (8 WAVs): ~20ms
- cpal device enumeration: ~50ms
- Thread spawning + priority setup: ~10ms
- **Total:** <100ms, well within NFR-5.

---

## 10. Error Handling & Resilience

| Scenario | Behavior |
|---|---|
| **Audio device unplugged** | cpal stream errors. Catch in error callback, pause 1s, re-enumerate devices, rebuild stream. |
| **No audio device at startup** | Log warning, enter idle loop retrying every 5s. Do not crash. |
| **macOS tap timeout** | Detect `kCGEventTapDisabledByTimeout`, re-enable tap immediately. |
| **Linux permission denied** | Log explicit error message: "Add user to 'input' group or run `setcap`." Exit with code `77` (EX_PERM). |
| **Queue overflow** | Drop event. Log at `trace` level. |
| **Unknown keycode** | Map to default click (index 0) via LUT defaulting. |

---

## 11. Build & Release Configuration

### 11.1 Cargo Profile

```toml
[package]
name = "tafd"
version = "0.1.0"
edition = "2021"

[profile.release]
opt-level = 3          # Aggressive optimization
lto = "fat"            # Link-time optimization across all crates
codegen-units = 1      # Single codegen unit for maximum LTO
panic = "abort"        # No unwinding tables
strip = true           # Strip debug symbols
debug = false          # No debuginfo in release

[dependencies]
cpal = "0.15"
crossbeam = "0.8"
hound = "3.5"
directories = "5.0"
log = { version = "0.4", features = ["release_max_level_warn"] }
env_logger = "0.11"

[target.'cfg(windows)'.dependencies]
windows = { version = "0.52", features = ["Win32_UI_WindowsAndMessaging"] }

[target.'cfg(target_os = "macos")'.dependencies]
core-graphics = "0.23"
core-foundation = "0.9"

[target.'cfg(target_os = "linux")'.dependencies]
libc = "0.2"
```

### 11.2 Feature Flags

| Feature | Description | Default |
|---|---|---|
| `symphonia` | Enable OGG/MP3 sound pack loading | Off |
| `windows-service` | Build as Windows service binary | Off |
| `udev` | Use libudev for Linux device discovery | On |

---

## 12. Security & Permission Matrix

| Platform | Permission | User Action | Daemon Behavior |
|---|---|---|---|
| **Windows** | None (standard user) | None | Works out-of-the-box. LLKH requires no elevation. |
| **macOS** | Input Monitoring | Add app in System Preferences → Privacy | Check `AXIsProcessTrustedWithOptions` at startup; show OS alert if denied. |
| **macOS** | Accessibility (if using AX APIs) | Same as above | Usually bundled with Input Monitoring prompt. |
| **Linux** | `input` group membership | `sudo usermod -aG input $USER` | Check `EACCES` on `/dev/input/event0`; print helpful instructions. |
| **Linux** | `CAP_SYS_NICE` (optional) | `sudo setcap cap_sys_nice+ep ./tafd` | Required only for `SCHED_FIFO` audio thread. Without it, audio thread runs normal priority (still acceptable). |
| **Linux** | `CAP_EVDEV` (optional) | `sudo setcap cap_evdev+ep ./tafd` | Alternative to `input` group membership. |

---

## 13. Testing & Performance Validation

### 13.1 Test Harnesses

| Test | Method | Pass Criteria |
|---|---|---|
| **Memory Leak** | `valgrind --tool=dhat` or `heaptrack` during 10-min typing simulation | No heap growth after first 30s. |
| **RAM Ceiling** | `ps -o rss= -p <pid>` after 5 min steady-state | RSS ≤ 10,240 KB. |
| **Latency** | Input thread timestamps event; audio callback logs first sample write. Difference measured. | p99 < 20ms. |
| **Stress Test** | Automated 600 CPM (10 CPS) burst for 60s | No audio underruns (cpal callback completes before deadline). |
| **Zero-Allocation Proof** | Build with `dhat` or custom global allocator counting post-init | Allocation count does not increase after startup phase. |

### 13.2 CI/CD Matrix

| Target | Toolchain | Test Focus |
|---|---|---|
| `x86_64-pc-windows-gnu` | stable | Hook lifecycle, service feature |
| `x86_64-apple-darwin` | stable | CGEventTap, .app bundling |
| `aarch64-apple-darwin` | stable | Apple Silicon audio path |
| `x86_64-unknown-linux-gnu` | stable | evdev, ALSA, systemd unit |
| `x86_64-unknown-linux-musl` | stable | Static binary, musl size check |

---

## 14. Deployment & Packaging

### 14.1 Windows
- **Portable:** Single `.exe` with embedded sounds.
- **Installer:** WiX/MSI with option to register as background service (`windows-service` feature).
- **Autostart:** Registry key `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

### 14.2 macOS
- **Bundle:** `TAFD.app` with `LSUIElement = true` in `Info.plist`.
- **Autostart:** `launchd` plist in `~/Library/LaunchAgents`.
- **Notarization:** Required for Gatekeeper on macOS 10.15+.

### 14.3 Linux
- **Binary:** Static or dynamic binary in `/usr/local/bin`.
- **Autostart:** systemd user unit: `~/.config/systemd/user/tafd.service`.
- **Packaging:** `.deb`, `.rpm`, or AUR PKGBUILD depending on distribution target.

---

## 15. Future Extensibility

| Feature | Impact | Approach |
|---|---|---|
| **Per-key sound profiles** | LUT expansion | 256-entry array mapping keycode → sample index + gain. |
| **Volume ducking** | Mixer state increase | Add master gain atomic f32 adjusted by config watcher thread. |
| **Sound pack hot-reload** | Heap allocation during reload | Feature-gated: decode new pack in separate thread, atomically swap `Arc<Vec<Sample>>`. |
| **Typing analytics** | New thread + storage | Separate non-real-time thread consuming a secondary MPSC queue. Does not affect audio path. |
| **Network sync** | High latency tolerance | UDP broadcast of key events for co-located typewriter sound effects. Independent of local audio path. |

---

## 16. Summary of Design Decisions

| Decision | Alternative | Rationale |
|---|---|---|
| **Native platform hooks** over `rdev` | `rdev` crate | Eliminates abstraction overhead and hidden allocations. |
| **cpal** over `rodio` | `rodio` | `rodio`'s `Sink`/`Decoder` allocates per-playback and adds 50-100ms latency. |
| **Voice stealer** over dynamic allocation | `Vec<Voice>` + push/pop | Fixed array is cache-friendly and requires no allocator in callback. |
| **Lock-free queue** over `Mutex` | `std::sync::Mutex` | Prevents priority inversion between real-time audio thread and input thread. |
| **Mono f32 PCM** over compressed audio | OGG streaming | Decoding compressed streams in real-time violates zero-allocation and adds CPU load. |
| **Panic=abort** over unwinding | Default unwinding | Saves ~200KB binary size and eliminates exception handling tables. |

---

**Document Version:** 1.0  
**Date:** 2026-04-25  
**Status:** Draft for Implementation Review