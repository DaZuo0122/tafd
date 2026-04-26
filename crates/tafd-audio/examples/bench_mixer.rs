//! Microbenchmark for the mixer hot path.
//! No audio hardware required.
//! Usage: cargo run --example bench_mixer --release -p tafd-audio

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;
use tafd_audio::mixer::Mixer;
use tafd_core::Sample;

const BUFFER_SIZE: usize = 128;
const SAMPLE_RATE: usize = 48_000;
const SAMPLE_FRAMES: usize = SAMPLE_RATE * 2; // 2-second sample
const ITERATIONS: u64 = 500_000;

fn make_samples(count: usize) -> Vec<Arc<Sample>> {
    (0..count)
        .map(|i| {
            let data: Vec<f32> = (0..SAMPLE_FRAMES)
                .map(|n| ((n as f32 + i as f32) * 440.0 / SAMPLE_RATE as f32).sin() * 0.5)
                .collect();
            Arc::new(Sample::new(data))
        })
        .collect()
}

// ---- Old implementation (before this PR) ----

fn mix_into_old(
    frame_pos: &mut usize,
    active: &mut bool,
    sample: &Arc<Sample>,
    output: &mut [f32],
    gain: f32,
) -> bool {
    let data = sample.data.as_slice();
    let len = data.len();
    for frame in output.iter_mut() {
        if *frame_pos >= len {
            *active = false;
            return false;
        }
        *frame += data[*frame_pos] * gain;
        *frame_pos += 1;
    }
    true
}

fn bench_old(label: &str, iterations: u64, samples: &[Arc<Sample>]) {
    let mut output = vec![0f32; BUFFER_SIZE];
    let mut frame_pos = 0usize;
    let mut active = true;

    let start = Instant::now();
    for i in 0..iterations {
        if i % 100 == 0 {
            frame_pos = 0;
            active = true;
        }
        output.fill(0.0);
        mix_into_old(
            &mut frame_pos,
            &mut active,
            black_box(&samples[0]),
            black_box(&mut output),
            1.0,
        );
        // old: separate gain+clamp pass
        for s in output.iter_mut() {
            *s *= 0.8;
            *s = s.clamp(-1.0, 1.0);
        }
    }
    let elapsed = start.elapsed();
    let ns = elapsed.as_nanos() as f64 / iterations as f64;
    println!("{}: {:.1} ns/render", label, ns);
}

// ---- New implementation (this PR) ----

fn bench_new(label: &str, iterations: u64, samples: &[Arc<Sample>]) {
    let mut mixer = Mixer::new(0.8, 1);
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

fn bench_new_8v(label: &str, iterations: u64, samples: &[Arc<Sample>]) {
    let mut mixer = Mixer::new(0.8, 8);
    let mut output = vec![0f32; BUFFER_SIZE];
    for i in 0..8 {
        mixer.trigger(i % samples.len());
    }

    let start = Instant::now();
    for i in 0..iterations {
        if i % 100 == 0 {
            mixer.trigger((i as usize) % samples.len());
        }
        mixer.render(black_box(&mut output), black_box(samples));
    }
    let elapsed = start.elapsed();
    let ns = elapsed.as_nanos() as f64 / iterations as f64;
    let rps = 1_000_000_000.0 / ns;
    let margin = rps / (SAMPLE_RATE as f64 / BUFFER_SIZE as f64);
    println!("{}: {:.1} ns/render | {:.0}x realtime margin", label, ns, margin);
}

fn main() {
    println!("=== tafd mixer hot-path benchmark ===");
    println!(
        "buffer={} frames | sample_rate={} Hz | iterations={}",
        BUFFER_SIZE, SAMPLE_RATE, ITERATIONS
    );
    println!("CPU: {}\n", std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|s| s.lines().find(|l| l.starts_with("model name")).map(|l| l.to_string()))
            .unwrap_or_else(|| "unknown".into())
    }));

    let samples = make_samples(8);

    println!("--- 1-voice mix_into  (per-render, buffer=128) ---");
    bench_old("BEFORE (per-frame branch, separate gain pass)", ITERATIONS, &samples);
    bench_new("AFTER  (zip slice loop, fused gain/clamp)    ", ITERATIONS, &samples);

    println!("\n--- 8-voice render ---");
    bench_new_8v("AFTER  (8 voices, full render pipeline)", ITERATIONS, &samples);

    // Correctness checks
    println!("\n--- correctness ---");
    let mut mixer = Mixer::new(1.0, 4);
    let mut output = vec![0f32; BUFFER_SIZE];
    for i in 0..4 { mixer.trigger(i); }
    mixer.render(&mut output, &samples);
    let nonzero = output.iter().filter(|&&s| s != 0.0).count();
    let clamp_ok = output.iter().all(|&s| s >= -1.0 && s <= 1.0);
    println!("non-zero output frames : {}/{} {}", nonzero, BUFFER_SIZE, if nonzero > 0 { "PASS" } else { "FAIL" });
    println!("all frames in [-1, 1]  : {}", if clamp_ok { "PASS" } else { "FAIL" });
}
