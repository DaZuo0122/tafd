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

        let supported = device
            .default_output_config()
            .map_err(|e| TafdError::AudioEngine(e.to_string()))?;

        let sample_rate = if config.sample_rate > 0 {
            config.sample_rate
        } else {
            supported.sample_rate().0
        };

        let channels = if config.channels > 0 {
            config.channels
        } else {
            supported.channels()
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
        let samples2 = samples.clone();

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
                        if let Some(sample) = samples.get(sample_idx) {
                            mixer.trigger(sample.clone());
                        }
                    }
                    mixer.render(data);
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
                                if let Some(sample) = samples2.get(sample_idx) {
                                    mixer2.trigger(sample.clone());
                                }
                            }
                            mixer2.render(data);
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
            for dev in devices {
                if let Ok(dev_name) = dev.name() {
                    if dev_name.to_lowercase().contains(&name.to_lowercase()) {
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
