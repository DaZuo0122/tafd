//! Audio-engine-only stress test.
//! Floods INPUT_QUEUE at a configurable rate and runs for a set duration.
//! Usage: cargo run --example stress --release

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const CPS: f64 = 10.0; // 600 CPM
const DURATION_SECS: u64 = 60;

fn main() {
    println!("TAFD Audio Engine Stress Test");
    println!("Rate: {} CPS ({} CPM) | Duration: {}s", CPS, CPS * 60.0, DURATION_SECS);

    let config = tafd_core::Config::default();

    // Resolve asset dir relative to this example binary
    // Examples live in target/release/examples/, assets are in target/release/
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
    let samples = tafd_audio::load_samples(Some(&pack_dir), config.sound_pack.default_variation_count)
        .expect("Failed to load samples");
    println!("Loaded {} samples", samples.len());

    let audio = tafd_audio::AudioEngine::new(&config.audio, samples)
        .expect("Failed to start audio engine");
    println!("Audio engine started");

    let shutdown = Arc::new(AtomicBool::new(false));
    let start = Instant::now();

    // Stress thread: enqueue keycodes at steady rate
    let stress = {
        let shutdown = shutdown.clone();
        thread::spawn(move || {
            let interval = Duration::from_secs_f64(1.0 / CPS);
            while !shutdown.load(Ordering::Relaxed) {
                // Push a rotating set of keycodes to exercise different sample slots
                let keycode = (start.elapsed().as_millis() % 8) as u32;
                if tafd_core::INPUT_QUEUE.push(keycode).is_err() {
                    eprintln!("Queue overflow!");
                }
                thread::sleep(interval);
            }
        })
    };

    // Monitor thread: print stats periodically
    let monitor = {
        let shutdown = shutdown.clone();
        thread::spawn(move || {
            let mut last_count = 0usize;
            while !shutdown.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(5));
                let elapsed = start.elapsed().as_secs_f64();
                let current = tafd_core::INPUT_QUEUE.len();
                let delta = current.saturating_sub(last_count);
                last_count = current;
                println!(
                    "  t={:5.1}s | queue_len={:2} | queue_delta={:2}",
                    elapsed,
                    current,
                    delta,
                );
            }
        })
    };

    // Run for requested duration
    thread::sleep(Duration::from_secs(DURATION_SECS));
    shutdown.store(true, Ordering::Relaxed);

    stress.join().unwrap();
    monitor.join().unwrap();

    println!("\nStress test complete.");
    println!("Audio stream alive: {}", !audio.is_shutdown());
    drop(audio);
}
