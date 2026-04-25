use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, StreamConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tafd_core::{AudioConfig, Sample, INPUT_QUEUE, MAX_VOICES, Result, TafdError};

use crate::mixer::Mixer;

pub struct AudioEngine {
    _stream: cpal::Stream,
    shutdown: Arc<AtomicBool>,
}

impl AudioEngine {
    pub fn new(config: &AudioConfig, samples: Vec<Arc<Sample>>) -> Result<Self> {
        let host = cpal::default_host();
        let device = Self::select_device(&host, config.preferred_device.as_deref())?;

        log::info!(
            "Using audio device: {}",
            device.name().unwrap_or_else(|_| "unknown".into())
        );

        let default_config = device
            .default_output_config()
            .map_err(|e| TafdError::AudioEngine(e.to_string()))?;

        let requested_sample_rate = config.sample_rate;
        let sample_rate = if requested_sample_rate > 0 {
            match device.supported_output_configs() {
                Ok(mut configs) => {
                    let is_supported = configs.any(|range| {
                        range.min_sample_rate().0 <= requested_sample_rate
                            && requested_sample_rate <= range.max_sample_rate().0
                    });

                    if is_supported {
                        requested_sample_rate
                    } else {
                        let fallback = default_config.sample_rate().0;
                        log::warn!(
                            "Requested sample rate {} Hz is not supported by the device. Falling back to {} Hz.",
                            requested_sample_rate,
                            fallback
                        );
                        fallback
                    }
                }
                Err(e) => {
                    let fallback = default_config.sample_rate().0;
                    log::warn!(
                        "Failed to query supported output configs ({}). Falling back to device default {} Hz.",
                        e,
                        fallback
                    );
                    fallback
                }
            }
        } else {
            default_config.sample_rate().0
        };

        let channels = if config.channels > 0 {
            config.channels
        } else {
            default_config.channels()
        };

        let stream_config = StreamConfig {
            channels,
            sample_rate: SampleRate(sample_rate),
            buffer_size: if config.buffer_size > 0 {
                BufferSize::Fixed(config.buffer_size)
            } else {
                BufferSize::Default
            },
        };

        let sample_count = samples.len();
        let samples = Arc::new(samples);
        let samples2 = Arc::clone(&samples);

        let mut mixer = Mixer::new(
            config.master_gain,
            config.voice_count.clamp(1, MAX_VOICES),
        );
        let mut mixer2 = Mixer::new(
            config.master_gain,
            config.voice_count.clamp(1, MAX_VOICES),
        );

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();
        let shutdown_clone2 = shutdown.clone();

        let fallback_config = if stream_config.buffer_size != BufferSize::Default {
            Some(StreamConfig {
                channels: stream_config.channels,
                sample_rate: stream_config.sample_rate,
                buffer_size: BufferSize::Default,
            })
        } else {
            None
        };

        let stream = device
            .build_output_stream(
                &stream_config,
                move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                    // Elevate thread priority on first callback (best effort)
                    #[cfg(windows)]
                    unsafe {
                        use windows::Win32::System::Threading::{
                            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
                        };
                        static PRIORITY_SET: AtomicBool = AtomicBool::new(false);
                        if !PRIORITY_SET.load(Ordering::Relaxed) {
                            let _ = SetThreadPriority(
                                GetCurrentThread(),
                                THREAD_PRIORITY_TIME_CRITICAL,
                            );
                            PRIORITY_SET.store(true, Ordering::Relaxed);
                        }
                    }

                    while let Some(keycode) = INPUT_QUEUE.pop() {
                        let sample_idx = (keycode as usize) % sample_count;
                        mixer.trigger(sample_idx);
                    }
                    mixer.render(data, &samples);
                },
                move |err| {
                    log::error!("Audio stream error: {}", err);
                    shutdown_clone.store(true, Ordering::Relaxed);
                },
                None,
            )
            .or_else(|e| {
                if let Some(ref fb_config) = fallback_config {
                    log::warn!(
                        "Requested buffer size rejected ({}), retrying with device default",
                        e
                    );
                    device.build_output_stream(
                        fb_config,
                        move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                            #[cfg(windows)]
                            unsafe {
                                use windows::Win32::System::Threading::{
                                    GetCurrentThread, SetThreadPriority,
                                    THREAD_PRIORITY_TIME_CRITICAL,
                                };
                                static PRIORITY_SET: AtomicBool = AtomicBool::new(false);
                                if !PRIORITY_SET.load(Ordering::Relaxed) {
                                    let _ = SetThreadPriority(
                                        GetCurrentThread(),
                                        THREAD_PRIORITY_TIME_CRITICAL,
                                    );
                                    PRIORITY_SET.store(true, Ordering::Relaxed);
                                }
                            }

                            while let Some(keycode) = INPUT_QUEUE.pop() {
                                let sample_idx = (keycode as usize) % sample_count;
                                mixer2.trigger(sample_idx);
                            }
                            mixer2.render(data, &samples2);
                        },
                        move |err| {
                            log::error!("Audio stream error: {}", err);
                            shutdown_clone2.store(true, Ordering::Relaxed);
                        },
                        None,
                    )
                } else {
                    Err(e)
                }
            })
            .map_err(|e| TafdError::AudioEngine(e.to_string()))?;

        stream
            .play()
            .map_err(|e| TafdError::AudioEngine(e.to_string()))?;

        Ok(Self {
            _stream: stream,
            shutdown,
        })
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }

    fn select_device(host: &cpal::Host, preferred: Option<&str>) -> Result<cpal::Device> {
        if let Some(name) = preferred {
            let devices = host
                .output_devices()
                .map_err(|e| TafdError::AudioEngine(e.to_string()))?;
            let name_lower = name.to_lowercase();
            for dev in devices {
                if let Ok(dev_name) = dev.name() {
                    if dev_name.to_lowercase().contains(&name_lower) {
                        return Ok(dev);
                    }
                }
            }
            log::warn!(
                "Preferred device '{}' not found, falling back to default",
                name
            );
        }
        host.default_output_device()
            .ok_or_else(|| TafdError::DeviceNotFound("default output device".into()))
    }
}
