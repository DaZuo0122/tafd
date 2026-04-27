//! Fair microbenchmark comparing the old mixer (dev branch) vs the new mixer
//! (perf/audio-hot-path-optimizations branch).
//!
//! The contributor's real wins are:
//!   1. Avoiding Arc::<Sample>::clone in the audio callback trigger path.
//!   2. Removing the inner Arc<Vec<f32>> from Sample.
//!
//! This benchmark measures both the trigger path and the render path fairly
//! by copying the *actual* old implementation into the benchmark file.
//!
//! Usage: cargo run --example bench_mixer --release -p tafd-audio

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;
use tafd_audio::mixer::Mixer as NewMixer;
use tafd_core::{MAX_VOICES, Sample as NewSample};

const BUFFER_SIZE: usize = 128;
const SAMPLE_RATE: usize = 48_000;
const SAMPLE_FRAMES: usize = SAMPLE_RATE * 2; // 2-second sample
const RENDER_ITERS: u64 = 500_000;
const TRIGGER_ITERS: u64 = 10_000_000;

fn make_new_samples(count: usize) -> Vec<Arc<NewSample>> {
    (0..count)
        .map(|i| {
            let data: Vec<f32> = (0..SAMPLE_FRAMES)
                .map(|n| ((n as f32 + i as f32) * 440.0 / SAMPLE_RATE as f32).sin() * 0.5)
                .collect();
            Arc::new(NewSample::new(data))
        })
        .collect()
}

// ============================================================================
//  OLD CODE — exactly as it existed on the dev branch before this PR
// ============================================================================

/// Old Sample: inner Arc<Vec<f32>> (double indirection in hot loop).
#[derive(Debug, Clone)]
struct OldSample {
    data: Arc<Vec<f32>>,
}

impl OldSample {
    fn new(data: Vec<f32>) -> Self {
        Self { data: Arc::new(data) }
    }
}

struct OldVoice {
    sample: Option<Arc<OldSample>>,
    frame_pos: usize,
    active: bool,
    gain: f32,
}

impl OldVoice {
    const fn new() -> Self {
        Self {
            sample: None,
            frame_pos: 0,
            active: false,
            gain: 1.0,
        }
    }

    fn trigger(&mut self, sample: Arc<OldSample>, gain: f32) {
        self.sample = Some(sample);
        self.frame_pos = 0;
        self.gain = gain;
        self.active = true;
    }

    fn mix_into(&mut self, output: &mut [f32]) -> bool {
        if !self.active {
            return false;
        }
        let Some(ref sample) = self.sample else {
            self.active = false;
            return false;
        };

        let data = &sample.data;
        let len = data.len();
        let gain = self.gain;

        for frame in output.iter_mut() {
            if self.frame_pos >= len {
                self.active = false;
                return false;
            }
            *frame += data[self.frame_pos] * gain;
            self.frame_pos += 1;
        }
        true
    }
}

struct OldMixer {
    voices: [OldVoice; MAX_VOICES],
    next_voice: usize,
    master_gain: f32,
    active_voice_count: usize,
}

impl OldMixer {
    fn new(master_gain: f32, active_voice_count: usize) -> Self {
        let count = active_voice_count.clamp(1, MAX_VOICES);
        Self {
            voices: std::array::from_fn(|_| OldVoice::new()),
            next_voice: 0,
            master_gain,
            active_voice_count: count,
        }
    }

    fn trigger(&mut self, sample: Arc<OldSample>) {
        let idx = self.next_voice % self.active_voice_count;
        self.next_voice = (self.next_voice + 1) % self.active_voice_count;
        self.voices[idx].trigger(sample, 1.0);
    }

    fn render(&mut self, output: &mut [f32]) {
        for s in output.iter_mut() {
            *s = 0.0;
        }
        for voice in &mut self.voices[..self.active_voice_count] {
            voice.mix_into(output);
        }
        let gain = self.master_gain;
        for s in output.iter_mut() {
            *s *= gain;
            *s = s.clamp(-1.0, 1.0);
        }
    }
}

// ============================================================================
//  BENCHMARKS
// ============================================================================

fn bench_trigger_old(label: &str, iterations: u64, sample: &Arc<OldSample>) {
    let mut mixer = OldMixer::new(0.8, 1);
    mixer.trigger(sample.clone());

    let start = Instant::now();
    for _ in 0..iterations {
        black_box(&mut mixer).trigger(black_box(sample.clone()));
    }
    let elapsed = start.elapsed();
    let ns = elapsed.as_nanos() as f64 / iterations as f64;
    println!("{}: {:.2} ns/trigger", label, ns);
}

fn bench_trigger_new(label: &str, iterations: u64) {
    let mut mixer = NewMixer::new(0.8, 1);
    mixer.trigger(0);

    let start = Instant::now();
    for _ in 0..iterations {
        black_box(&mut mixer).trigger(black_box(0usize));
    }
    let elapsed = start.elapsed();
    let ns = elapsed.as_nanos() as f64 / iterations as f64;
    println!("{}: {:.2} ns/trigger", label, ns);
}

fn bench_render_old(label: &str, iterations: u64, sample: &Arc<OldSample>) {
    let mut mixer = OldMixer::new(0.8, 1);
    let mut output = vec![0f32; BUFFER_SIZE];
    mixer.trigger(sample.clone());

    let start = Instant::now();
    for i in 0..iterations {
        if i % 100 == 0 {
            mixer.trigger(sample.clone());
        }
        mixer.render(black_box(&mut output));
    }
    let elapsed = start.elapsed();
    let ns = elapsed.as_nanos() as f64 / iterations as f64;
    println!("{}: {:.1} ns/render", label, ns);
}

fn bench_render_new(label: &str, iterations: u64, samples: &[Arc<NewSample>]) {
    let mut mixer = NewMixer::new(0.8, 1);
    let mut output = vec![0f32; BUFFER_SIZE];
    mixer.trigger(0);

    let start = Instant::now();
    for i in 0..iterations {
        if i % 100 == 0 {
            mixer.trigger(0);
        }
        mixer.render(black_box(&mut output), black_box(samples));
    }
    let elapsed = start.elapsed();
    let ns = elapsed.as_nanos() as f64 / iterations as f64;
    println!("{}: {:.1} ns/render", label, ns);
}

fn bench_callback_old(label: &str, iterations: u64, sample: &Arc<OldSample>) {
    let mut mixer = OldMixer::new(0.8, 1);
    let mut output = vec![0f32; BUFFER_SIZE];

    let start = Instant::now();
    for i in 0..iterations {
        if i % 100 == 0 {
            mixer.trigger(sample.clone());
        }
        mixer.render(black_box(&mut output));
    }
    let elapsed = start.elapsed();
    let ns = elapsed.as_nanos() as f64 / iterations as f64;
    println!("{}: {:.1} ns/callback", label, ns);
}

fn bench_callback_new(label: &str, iterations: u64, samples: &[Arc<NewSample>]) {
    let mut mixer = NewMixer::new(0.8, 1);
    let mut output = vec![0f32; BUFFER_SIZE];

    let start = Instant::now();
    for i in 0..iterations {
        if i % 100 == 0 {
            mixer.trigger(0);
        }
        mixer.render(black_box(&mut output), black_box(samples));
    }
    let elapsed = start.elapsed();
    let ns = elapsed.as_nanos() as f64 / iterations as f64;
    println!("{}: {:.1} ns/callback", label, ns);
}

// ============================================================================
//  MAIN
// ============================================================================

fn main() {
    println!("=== tafd mixer fair benchmark ===");
    println!(
        "buffer={} frames | sample_rate={} Hz | render_iters={} | trigger_iters={}",
        BUFFER_SIZE, SAMPLE_RATE, RENDER_ITERS, TRIGGER_ITERS
    );
    println!(
        "CPU: {}\n",
        std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| {
            std::fs::read_to_string("/proc/cpuinfo")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("model name"))
                        .map(|l| l.to_string())
                })
                .unwrap_or_else(|| "unknown".into())
        })
    );

    let old_data: Vec<f32> = (0..SAMPLE_FRAMES)
        .map(|n| (n as f32 * 440.0 / SAMPLE_RATE as f32).sin() * 0.5)
        .collect();
    let old_sample = Arc::new(OldSample::new(old_data));

    let new_samples = make_new_samples(8);

    // ------------------------------------------------------------------------
    // 1. Trigger path — the *actual* hot-path win in the audio callback
    // ------------------------------------------------------------------------
    println!("--- Trigger path (cost per keystroke in audio callback) ---");
    bench_trigger_old(
        "OLD (Arc::<Sample>::clone + struct store)",
        TRIGGER_ITERS,
        &old_sample,
    );
    bench_trigger_new(
        "NEW (usize index store)                   ",
        TRIGGER_ITERS,
    );

    // ------------------------------------------------------------------------
    // 2. Render path — 1 voice, full pipeline
    // ------------------------------------------------------------------------
    println!("\n--- Render path (1 voice, buffer={}) ---", BUFFER_SIZE);
    bench_render_old(
        "OLD (double Arc, per-frame branch, separate gain+clamp)",
        RENDER_ITERS,
        &old_sample,
    );
    bench_render_new(
        "NEW (single Arc, zip slice, fused gain/clamp)         ",
        RENDER_ITERS,
        &new_samples,
    );

    // ------------------------------------------------------------------------
    // 3. Full callback simulation (trigger + render)
    // ------------------------------------------------------------------------
    println!("\n--- Full callback simulation (trigger every 100 renders) ---");
    bench_callback_old(
        "OLD (trigger + render)",
        RENDER_ITERS,
        &old_sample,
    );
    bench_callback_new(
        "NEW (trigger + render)",
        RENDER_ITERS,
        &new_samples,
    );

    // ------------------------------------------------------------------------
    // Correctness checks
    // ------------------------------------------------------------------------
    println!("\n--- correctness ---");
    let mut old_mixer = OldMixer::new(1.0, 4);
    let mut old_output = vec![0f32; BUFFER_SIZE];
    for _ in 0..4 {
        old_mixer.trigger(old_sample.clone());
    }
    old_mixer.render(&mut old_output);
    let old_nonzero = old_output.iter().filter(|&&s| s != 0.0).count();
    let old_clamp = old_output.iter().all(|&s| s >= -1.0 && s <= 1.0);
    println!(
        "OLD nonzero={}/{} clamp={}",
        old_nonzero,
        BUFFER_SIZE,
        if old_clamp { "PASS" } else { "FAIL" }
    );

    let mut new_mixer = NewMixer::new(1.0, 4);
    let mut new_output = vec![0f32; BUFFER_SIZE];
    for i in 0..4 {
        new_mixer.trigger(i);
    }
    new_mixer.render(&mut new_output, &new_samples);
    let new_nonzero = new_output.iter().filter(|&&s| s != 0.0).count();
    let new_clamp = new_output.iter().all(|&s| s >= -1.0 && s <= 1.0);
    println!(
        "NEW nonzero={}/{} clamp={}",
        new_nonzero,
        BUFFER_SIZE,
        if new_clamp { "PASS" } else { "FAIL" }
    );
}
