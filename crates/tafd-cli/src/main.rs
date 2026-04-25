use clap::Parser;
use directories::ProjectDirs;
use log::info;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tafd_audio::{load_samples, AudioEngine};
use tafd_core::{
    build_key_lut, merge_config, AudioConfig, Config, InputConfig, Result, SoundPackConfig,
    TafdError,
};
use tafd_input::backend::create as create_input_backend;

#[derive(Parser, Debug)]
#[command(name = "tafd")]
#[command(about = "Typewriter Acoustic Feedback Daemon")]
struct Cli {
    /// Path to config file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Sound pack directory
    #[arg(long, value_name = "DIR")]
    sound_pack: Option<PathBuf>,

    /// Preferred audio device name
    #[arg(long, value_name = "NAME")]
    device: Option<String>,

    /// Master gain (0.0 - 1.0)
    #[arg(long, value_name = "GAIN")]
    gain: Option<f32>,

    /// Disable repeat suppression
    #[arg(long)]
    no_repeat_suppress: bool,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

fn main() {
    if let Err(e) = run() {
        log::error!("Fatal error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let mut builder = env_logger::Builder::from_default_env();
    if cli.verbose {
        builder.filter_level(log::LevelFilter::Debug);
    } else {
        builder.filter_level(log::LevelFilter::Info);
    }
    builder.init();

    eprintln!("TAFD starting up...");
    info!("TAFD starting up");

    // Load and merge configuration
    let config = load_config(&cli)?;
    info!("Configuration loaded");

    // Ensure config directory exists and write default if missing
    if let Some(dirs) = ProjectDirs::from("", "tafd", "tafd") {
        let config_dir = dirs.config_dir();
        std::fs::create_dir_all(config_dir)?;
        let config_path = config_dir.join("config.toml");
        if !config_path.exists() {
            let default_toml = default_config_toml();
            std::fs::write(&config_path, default_toml)?;
            info!("Wrote default config to {}", config_path.display());
        }
    }

    // Load samples
    let samples = load_samples(
        config.sound_pack.pack_dir.as_deref(),
        config.sound_pack.default_variation_count,
    )?;
    info!("Loaded {} samples", samples.len());

    // Build key LUT (currently not passed to audio engine, but available for future use)
    let _lut = build_key_lut(&config.sound_pack, config.input.unknown_key_mapping);

    // Initialize audio engine
    let audio = AudioEngine::new(&config.audio, samples)?;
    info!("Audio engine started");

    // Initialize input backend
    let input_backend = create_input_backend(&config.input)?;
    info!("Input backend created");

    // Setup shutdown signal
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_input = shutdown.clone();
    let shutdown_main = shutdown.clone();

    ctrlc::set_handler(move || {
        info!("Shutdown signal received");
        shutdown_main.store(true, Ordering::Relaxed);
    })
    .map_err(|e| TafdError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    // Spawn input thread
    let input_handle = std::thread::spawn(move || {
        if let Err(e) = input_backend.run(&shutdown_input) {
            log::error!("Input thread error: {e}");
        }
    });

    info!("TAFD running. Press Ctrl+C to exit.");

    // Main loop: watchdog
    while !shutdown.load(Ordering::Relaxed) {
        if audio.is_shutdown() {
            log::error!("Audio stream shut down unexpectedly");
            shutdown.store(true, Ordering::Relaxed);
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }

    // Wait for input thread to finish
    shutdown.store(true, Ordering::Relaxed);
    #[cfg(windows)]
    tafd_input::win32::wake_input_thread();
    let join_result = input_handle.join();
    if join_result.is_err() {
        log::warn!("Input thread panicked or failed to join");
    }

    info!("TAFD shut down gracefully");
    Ok(())
}

fn load_config(cli: &Cli) -> Result<Config> {
    let mut config = Config::default();

    // 1. Load from file if present
    let file_config = if let Some(ref path) = cli.config {
        load_config_file(path).ok()
    } else if let Some(dirs) = ProjectDirs::from("", "tafd", "tafd") {
        let path = dirs.config_dir().join("config.toml");
        if path.exists() {
            match load_config_file(&path) {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    log::warn!("Failed to load config file {}: {e}. Using defaults.", path.display());
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    if let Some(file_cfg) = file_config {
        config = merge_config(config, file_cfg);
    }

    // 2. Apply CLI overrides
    let cli_config = config_from_cli(cli);
    config = merge_config(config, cli_config);

    // Clamp voice count
    config.audio.voice_count = config.audio.voice_count.clamp(1, tafd_core::MAX_VOICES);

    Ok(config)
}

fn load_config_file(path: &PathBuf) -> Result<Config> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| TafdError::Config(format!("Failed to read {}: {e}", path.display())))?;
    let config: Config = toml::from_str(&contents)
        .map_err(|e| TafdError::Config(format!("Failed to parse {}: {e}", path.display())))?;
    Ok(config)
}

fn config_from_cli(cli: &Cli) -> Config {
    Config {
        audio: AudioConfig {
            preferred_device: cli.device.clone(),
            master_gain: cli.gain.unwrap_or(tafd_core::DEFAULT_MASTER_GAIN),
            ..AudioConfig::default()
        },
        input: InputConfig {
            suppress_repeat: !cli.no_repeat_suppress,
            ..InputConfig::default()
        },
        sound_pack: SoundPackConfig {
            pack_dir: cli.sound_pack.clone(),
            ..SoundPackConfig::default()
        },
    }
}

fn default_config_toml() -> &'static str {
    r#"[audio]
sample_rate = 48000
channels = 1
buffer_size = 128
master_gain = 0.3
voice_count = 8
# preferred_device = "Speakers"

[input]
suppress_repeat = true
unknown_key_mapping = 0

[sound_pack]
default_variation_count = 8
# pack_dir = "C:/Users/Me/sounds"

# Optional: per-key sound mapping
# [sound_pack.per_key_map]
# 0x0D = 0   # Enter -> click 0
# 0x20 = 1   # Space -> click 1
"#
}
