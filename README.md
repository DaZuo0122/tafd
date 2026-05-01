# TAFD — Typewriter Acoustic Feedback Daemon

TAFD is a cross-platform background daemon that gives your keyboard mechanical typewriter acoustics. It captures system-wide key presses and plays preloaded PCM click sounds through a real-time, lock-free audio mixer.

## Features

- **Global keystroke feedback** — Works across all applications without focusing any window.
- **Sub-10 MB memory footprint** — Aggressive pre-allocation at startup; zero heap allocation during steady-state typing.
- **Imperceptible latency** — Input and audio run on separate threads connected by a lock-free SPSC queue. The audio thread operates at real-time priority with buffer sizes as low as 128 frames.
- **Polyphonic playback** — Up to 8 simultaneous voices with round-robin voice stealing for overlapping keystrokes.
- **Repeat suppression** — Ignores key-hold repeats so only physical presses trigger sounds.
- **Custom sound packs** — Load your own WAV samples from a directory (optional TOML-based per-key mapping).
- **Cross-platform** — Windows 10+, macOS 12+, and Linux 5.15+.

## Architecture

```
┌──────────────┐    Lock-Free      ┌──────────────┐    Platform    ┌─────┐
│   Input      │───SPSC Queue─────▶│    Audio     │───Callback───▶ │ OS  │
│   Thread     │   (key events)    │   Thread     │   (cpal)       │Audio│
└──────────────┘                   └──────────────┘                └─────┘
```

- **Input Thread** — Blocks on the native OS input API (`WH_KEYBOARD_LL` on Windows, `CGEventTap` on macOS, `evdev` on Linux) and enqueues keycodes.
- **Audio Thread** — Drains the queue inside the `cpal` output callback, mixes active voices, and writes to the OS audio buffer. No locks, no allocation.
- **Main Thread** — Handles initialization, signal traps (`Ctrl+C`), and a lightweight watchdog.

## Crate Layout

| Crate | Responsibility |
|---|---|
| `tafd-core` | Shared types, config, error handling, and the lock-free event queue. |
| `tafd-audio` | `cpal` integration, voice-stealer mixer, and sample loading/decoding. |
| `tafd-input` | Platform-native global input hooks (Windows, macOS, Linux). |
| `tafd-cli` | Binary entry point, argument parsing with `clap`, and config merging. |

## Configuration

TAFD can be configured via a TOML file and/or CLI arguments. The config file is searched at the platform-specific config directory (e.g. `~/.config/tafd/config.toml` on Linux, `%APPDATA%\tafd\config.toml` on Windows).

Example `config.toml`:

```toml
[audio]
sample_rate = 48000
channels = 1
buffer_size = 128
master_gain = 1.0
voice_count = 8

[input]
suppress_repeat = true

[sound_pack]
default_variation_count = 8
pack_dir = "/path/to/your/sound-pack"

# Optional: map specific keycodes to sample variations
# [sound_pack.per_key_map]
# 0x0D = 0   # Enter
# 0x20 = 1   # Space
```

CLI overrides:

```
tafd --sound-pack ./my-sounds --gain 1.5 --device "Speakers"
```

## Sound Packs

A sound pack is a folder containing WAV files named in a simple convention. TAFD loads them at startup, decodes them to `f32` PCM, and references them from a pre-allocated voice pool at runtime.

## Platform Notes

### Windows
- Runs out-of-the-box as a standard user. No elevation required.
- Uses a low-level keyboard hook (`WH_KEYBOARD_LL`). Some security software may warn about the global hook.

### macOS
- Requires **Input Monitoring** permission. Add TAFD to *System Preferences → Security & Privacy → Privacy → Input Monitoring*.
- The binary should be bundled as an `.app` with `LSUIElement = true` to avoid a dock icon.

### Linux
- Requires membership in the `input` group (or `CAP_EVDEV`) to read from `/dev/input/event*`.
  ```bash
  sudo usermod -aG input $USER
  ```
- Optional real-time audio scheduling requires `CAP_SYS_NICE`:
  ```bash
  sudo setcap cap_sys_nice+ep ./tafd
  ```

## License

MIT License
