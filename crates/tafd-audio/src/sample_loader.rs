use std::path::Path;
use std::sync::Arc;
use tafd_core::{Result, Sample, TafdError};

/// Load samples from a directory (WAV files) or fall back to embedded defaults.
pub fn load_samples(
    pack_dir: Option<&Path>,
    variation_count: usize,
) -> Result<Vec<Arc<Sample>>> {
    match pack_dir {
        Some(dir) => load_from_directory(dir, variation_count),
        None => load_embedded(variation_count),
    }
}

fn load_from_directory(dir: &Path, variation_count: usize) -> Result<Vec<Arc<Sample>>> {
    let mut samples = Vec::with_capacity(variation_count);
    for i in 0..variation_count {
        let path = dir.join(format!("click{}.wav", i + 1));
        if !path.exists() {
            return Err(TafdError::SoundPackLoad {
                path,
                reason: "file not found".into(),
            });
        }
        let sample = decode_wav(&path)?;
        samples.push(Arc::new(sample));
    }
    Ok(samples)
}

fn load_embedded(variation_count: usize) -> Result<Vec<Arc<Sample>>> {
    let embedded: Vec<&[u8]> = vec![
        include_bytes!("../../../assets/sounds/click1.wav"),
        include_bytes!("../../../assets/sounds/click2.wav"),
        include_bytes!("../../../assets/sounds/click3.wav"),
        include_bytes!("../../../assets/sounds/click4.wav"),
        include_bytes!("../../../assets/sounds/click5.wav"),
        include_bytes!("../../../assets/sounds/click6.wav"),
        include_bytes!("../../../assets/sounds/click7.wav"),
        include_bytes!("../../../assets/sounds/click8.wav"),
    ];

    let mut samples = Vec::with_capacity(variation_count.min(embedded.len()));
    for i in 0..variation_count.min(embedded.len()) {
        let sample = decode_wav_bytes(embedded[i])?;
        samples.push(Arc::new(sample));
    }
    Ok(samples)
}

fn decode_wav(path: &Path) -> Result<Sample> {
    let reader = hound::WavReader::open(path)
        .map_err(|e| TafdError::SoundPackLoad {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    decode_reader(reader, path)
}

fn decode_wav_bytes(bytes: &[u8]) -> Result<Sample> {
    let reader = hound::WavReader::new(bytes)
        .map_err(|e| TafdError::SoundPackLoad {
            path: Path::new("<embedded>").to_path_buf(),
            reason: e.to_string(),
        })?;
    decode_reader(reader, Path::new("<embedded>"))
}

fn decode_reader<R: std::io::Read>(
    mut reader: hound::WavReader<R>,
    path: &Path,
) -> Result<Sample> {
    let spec = reader.spec();
    let data: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            let samples: std::result::Result<Vec<f32>, hound::Error> = reader.samples::<f32>().collect();
            samples
        }
        hound::SampleFormat::Int => {
            let samples: std::result::Result<Vec<i32>, hound::Error> = reader.samples::<i32>().collect();
            samples.map(|v| v.into_iter().map(|s| s as f32 / i32::MAX as f32).collect())
        }
    }
    .map_err(|e| TafdError::SoundPackLoad {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    Ok(Sample::new(data))
}
