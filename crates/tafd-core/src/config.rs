use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Hard structural invariant: maximum simultaneous voices.
pub const MAX_VOICES: usize = 8;

/// Default config values.
pub const DEFAULT_SAMPLE_RATE: u32 = 48000;
pub const DEFAULT_CHANNELS: u16 = 1;
pub const DEFAULT_BUFFER_SIZE: u32 = 0;
pub const DEFAULT_MASTER_GAIN: f32 = 0.3;
pub const DEFAULT_VOICE_COUNT: usize = 8;
pub const DEFAULT_SUPPRESS_REPEAT: bool = true;
pub const DEFAULT_UNKNOWN_KEY_MAPPING: u32 = 0;
pub const DEFAULT_VARIATION_COUNT: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub input: InputConfig,
    #[serde(default)]
    pub sound_pack: SoundPackConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            audio: AudioConfig::default(),
            input: InputConfig::default(),
            sound_pack: SoundPackConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioConfig {
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default = "default_channels")]
    pub channels: u16,
    #[serde(default = "default_buffer_size")]
    pub buffer_size: u32,
    #[serde(default = "default_master_gain")]
    pub master_gain: f32,
    #[serde(default)]
    pub preferred_device: Option<String>,
    #[serde(default = "default_voice_count")]
    pub voice_count: usize,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            channels: DEFAULT_CHANNELS,
            buffer_size: DEFAULT_BUFFER_SIZE,
            master_gain: DEFAULT_MASTER_GAIN,
            preferred_device: None,
            voice_count: DEFAULT_VOICE_COUNT,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputConfig {
    #[serde(default = "default_suppress_repeat")]
    pub suppress_repeat: bool,
    #[serde(default = "default_unknown_key_mapping")]
    pub unknown_key_mapping: u32,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            suppress_repeat: DEFAULT_SUPPRESS_REPEAT,
            unknown_key_mapping: DEFAULT_UNKNOWN_KEY_MAPPING,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SoundPackConfig {
    #[serde(default)]
    pub pack_dir: Option<PathBuf>,
    #[serde(default)]
    pub per_key_map: Option<HashMap<u32, usize>>,
    #[serde(default = "default_variation_count")]
    pub default_variation_count: usize,
}

impl Default for SoundPackConfig {
    fn default() -> Self {
        Self {
            pack_dir: None,
            per_key_map: None,
            default_variation_count: DEFAULT_VARIATION_COUNT,
        }
    }
}

fn default_sample_rate() -> u32 { DEFAULT_SAMPLE_RATE }
fn default_channels() -> u16 { DEFAULT_CHANNELS }
fn default_buffer_size() -> u32 { DEFAULT_BUFFER_SIZE }
fn default_master_gain() -> f32 { DEFAULT_MASTER_GAIN }
fn default_voice_count() -> usize { DEFAULT_VOICE_COUNT }
fn default_suppress_repeat() -> bool { DEFAULT_SUPPRESS_REPEAT }
fn default_unknown_key_mapping() -> u32 { DEFAULT_UNKNOWN_KEY_MAPPING }
fn default_variation_count() -> usize { DEFAULT_VARIATION_COUNT }

/// Merge `other` into `base`. Any non-None / non-default field in `other` overwrites `base`.
/// For CLI merging we use a simpler overlay: CLI args produce a partial Config and overwrite.
pub fn merge_config(mut base: Config, other: Config) -> Config {
    // audio
    if other.audio.sample_rate != DEFAULT_SAMPLE_RATE {
        base.audio.sample_rate = other.audio.sample_rate;
    }
    if other.audio.channels != DEFAULT_CHANNELS {
        base.audio.channels = other.audio.channels;
    }
    if other.audio.buffer_size != DEFAULT_BUFFER_SIZE {
        base.audio.buffer_size = other.audio.buffer_size;
    }
    if (other.audio.master_gain - DEFAULT_MASTER_GAIN).abs() > f32::EPSILON {
        base.audio.master_gain = other.audio.master_gain;
    }
    if other.audio.preferred_device.is_some() {
        base.audio.preferred_device = other.audio.preferred_device;
    }
    if other.audio.voice_count != DEFAULT_VOICE_COUNT {
        base.audio.voice_count = other.audio.voice_count.clamp(1, MAX_VOICES);
    }

    // input
    if other.input.suppress_repeat != DEFAULT_SUPPRESS_REPEAT {
        base.input.suppress_repeat = other.input.suppress_repeat;
    }
    if other.input.unknown_key_mapping != DEFAULT_UNKNOWN_KEY_MAPPING {
        base.input.unknown_key_mapping = other.input.unknown_key_mapping;
    }

    // sound_pack
    if other.sound_pack.pack_dir.is_some() {
        base.sound_pack.pack_dir = other.sound_pack.pack_dir;
    }
    if other.sound_pack.per_key_map.is_some() {
        base.sound_pack.per_key_map = other.sound_pack.per_key_map;
    }
    if other.sound_pack.default_variation_count != DEFAULT_VARIATION_COUNT {
        base.sound_pack.default_variation_count = other.sound_pack.default_variation_count;
    }

    base
}

/// Build a lookup table mapping keycode (u8) to sample index (u8).
/// Returns a 256-entry array; unknown keys map to `default_sample`.
pub fn build_key_lut(config: &SoundPackConfig, default_sample: u32) -> [u8; 256] {
    let default = default_sample.clamp(0, u8::MAX as u32) as u8;
    let mut lut = [default; 256];
    if let Some(ref map) = config.per_key_map {
        for (&key, &sample_idx) in map.iter() {
            if key <= 255 {
                lut[key as usize] = (sample_idx as u8).min(u8::MAX);
            }
        }
    }
    lut
}
