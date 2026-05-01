//! Audio-engine stress test that probes the upper performance limit.
//! Usage: cargo run --example stress --release -- [OPTIONS]

use clap::Parser;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(name = "stress")]
struct Args {
    /// Base characters per second per thread
    #[arg(short, long, default_value_t = 30.0)]
    cps: f64,

    /// Number of concurrent stress threads
    #[arg(short, long, default_value_t = 4)]
    threads: usize,

    /// Test duration in seconds
    #[arg(short, long, default_value_t = 60)]
    duration: u64,

    /// Burst mode: flood queue without pacing (ignores --cps)
    #[arg(long)]
    burst: bool,

    /// Ramp mode: double CPS every 10s until drops occur
    #[arg(long)]
    ramp: bool,
}

struct Stats {
    enqueued: AtomicU64,
    dropped: AtomicU64,
    peak_queue_len: AtomicU64,
}

fn main() {
    let args = Args::parse();

    println!("TAFD Audio Engine Stress Test (Upper Limit)");
    println!(
        "Threads: {} | CPS: {} | Duration: {}s | Burst: {} | Ramp: {}",
        args.threads,
        if args.burst {
            "MAX".into()
        } else {
            args.cps.to_string()
        },
        args.duration,
        args.burst,
        args.ramp
    );

    let config = tafd_core::Config::default();

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .expect("Failed to get exe directory");

    let candidates = [
        exe_dir.join("assets/sounds"),
        exe_dir.join("../assets/sounds"),
    ];
    let pack_dir = candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .expect("Failed to locate asset directory");

    println!("Loading samples from: {}", pack_dir.display());
    let samples =
        tafd_audio::load_samples(Some(&pack_dir), config.sound_pack.default_variation_count)
            .expect("Failed to load samples");
    println!("Loaded {} samples", samples.len());

    let audio = tafd_audio::AudioEngine::new(&config.audio, samples)
        .expect("Failed to start audio engine");
    println!("Audio engine started");

    let shutdown = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(Stats {
        enqueued: AtomicU64::new(0),
        dropped: AtomicU64::new(0),
        peak_queue_len: AtomicU64::new(0),
    });
    let start = Instant::now();

    // Spawn stress threads
    let mut handles = Vec::new();
    for thread_id in 0..args.threads {
        let shutdown = shutdown.clone();
        let stats = stats.clone();
        let burst = args.burst;
        let base_cps = args.cps;
        let ramp = args.ramp;

        let handle = thread::spawn(move || {
            let mut interval = if base_cps > 0.0 {
                Duration::from_secs_f64(1.0 / base_cps)
            } else {
                Duration::from_micros(1)
            };
            let mut last_ramp = Instant::now();
            let mut current_cps = base_cps;
            let mut ramp_count = 0u32;

            while !shutdown.load(Ordering::Relaxed) {
                if ramp && last_ramp.elapsed().as_secs() >= 10 {
                    current_cps *= 2.0;
                    ramp_count += 1;
                    interval = if current_cps > 0.0 {
                        Duration::from_secs_f64(1.0 / current_cps)
                    } else {
                        Duration::from_micros(1)
                    };
                    println!(
                        "  [Thread {}] Ramp #{}: CPS -> {:.1}",
                        thread_id, ramp_count, current_cps
                    );
                    last_ramp = Instant::now();
                }

                let keycode =
                    ((start.elapsed().as_millis() as usize + thread_id) % 8) as u32;
                match tafd_core::INPUT_QUEUE.push(keycode) {
                    Ok(_) => {
                        stats.enqueued.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        stats.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }

                let len = tafd_core::INPUT_QUEUE.len() as u64;
                stats.peak_queue_len.fetch_max(len, Ordering::Relaxed);

                if !burst {
                    thread::sleep(interval);
                }
            }
        });
        handles.push(handle);
    }

    // Monitor thread
    let monitor = {
        let shutdown = shutdown.clone();
        let stats = stats.clone();
        thread::spawn(move || {
            let mut last_enqueued = 0u64;
            let mut last_dropped = 0u64;

            while !shutdown.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(5));

                let elapsed = start.elapsed().as_secs_f64();
                let enqueued = stats.enqueued.load(Ordering::Relaxed);
                let dropped = stats.dropped.load(Ordering::Relaxed);
                let peak = stats.peak_queue_len.load(Ordering::Relaxed);
                let current_queue = tafd_core::INPUT_QUEUE.len() as u64;

                let delta_enq = enqueued.saturating_sub(last_enqueued);
                let delta_drop = dropped.saturating_sub(last_dropped);
                last_enqueued = enqueued;
                last_dropped = dropped;

                println!(
                    "  t={:5.1}s | enq={:10} (+{:6}) | drop={:8} (+{:4}) | peak_q={:2} | cur_q={:2}",
                    elapsed, enqueued, delta_enq, dropped, delta_drop, peak, current_queue
                );

                if dropped > 0 && !args.burst {
                    println!("  WARNING: Events are being dropped. Queue limit reached.");
                }
            }
        })
    };

    // Run for requested duration
    thread::sleep(Duration::from_secs(args.duration));
    shutdown.store(true, Ordering::Relaxed);

    for h in handles {
        let _ = h.join();
    }
    let _ = monitor.join();

    let total_enqueued = stats.enqueued.load(Ordering::Relaxed);
    let total_dropped = stats.dropped.load(Ordering::Relaxed);
    let peak = stats.peak_queue_len.load(Ordering::Relaxed);
    let total_events = total_enqueued + total_dropped;

    println!("\n=== Stress Test Complete ===");
    println!("Total enqueued:     {}", total_enqueued);
    println!("Total dropped:      {}", total_dropped);
    println!("Peak queue len:     {}", peak);
    println!(
        "Drop rate:          {:.4}%",
        (total_dropped as f64 / total_events.max(1) as f64) * 100.0
    );
    println!("Audio stream alive: {}", !audio.is_shutdown());
    drop(audio);
}
