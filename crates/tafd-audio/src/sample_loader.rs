use std::path::Path;
use std::sync::Arc;
use tafd_core::{Result, Sample, TafdError};

/// Load samples from a directory containing WAV files named `click1.wav` … `clickN.wav`.
pub fn load_samples(
    pack_dir: Option<&Path>,
    variation_count: usize,
) -> Result<Vec<Arc<Sample>>> {
    let dir = pack_dir.ok_or_else(|| TafdError::SoundPackLoad {
        path: Path::new("<not configured>").to_path_buf(),
        reason: "No sound pack directory configured".into(),
    })?;
    load_from_directory(dir, variation_count)
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

fn decode_wav(path: &Path) -> Result<Sample> {
    let reader = hound::WavReader::open(path)
        .map_err(|e| TafdError::SoundPackLoad {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    decode_reader(reader, path)
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
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .map(|r| r.map(|s| s as f32 / i32::MAX as f32))
            .collect::<std::result::Result<Vec<f32>, _>>(),
    }
    .map_err(|e| TafdError::SoundPackLoad {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    Ok(Sample::new(data))
}
