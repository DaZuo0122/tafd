use std::sync::Arc;
use tafd_core::{MAX_VOICES, Sample};

/// A single playback voice.
pub struct Voice {
    sample_idx: Option<usize>,
    frame_pos: usize,
    active: bool,
    gain: f32,
}

impl Voice {
    pub const fn new() -> Self {
        Self {
            sample_idx: None,
            frame_pos: 0,
            active: false,
            gain: 1.0,
        }
    }

    pub fn trigger(&mut self, idx: usize, gain: f32) {
        self.sample_idx = Some(idx);
        self.frame_pos = 0;
        self.gain = gain;
        self.active = true;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Mix this voice into the output buffer. Returns true if still active.
    pub fn mix_into(&mut self, output: &mut [f32], samples: &[Arc<Sample>]) -> bool {
        if !self.active {
            return false;
        }
        let Some(idx) = self.sample_idx else {
            self.active = false;
            return false;
        };
        let Some(sample) = samples.get(idx) else {
            self.active = false;
            return false;
        };

        let data = sample.data.as_slice();
        let remaining = data.len().saturating_sub(self.frame_pos);
        if remaining == 0 {
            self.active = false;
            return false;
        }

        let to_mix = remaining.min(output.len());
        let src = &data[self.frame_pos..self.frame_pos + to_mix];
        let gain = self.gain;
        for (out, &s) in output[..to_mix].iter_mut().zip(src.iter()) {
            *out += s * gain;
        }
        self.frame_pos += to_mix;

        if self.frame_pos >= data.len() {
            self.active = false;
            return false;
        }
        true
    }
}

/// Fixed-size voice stealer mixer. No heap allocation during playback.
pub struct Mixer {
    voices: [Voice; MAX_VOICES],
    next_voice: usize,
    master_gain: f32,
    active_voice_count: usize,
}

impl Mixer {
    pub fn new(master_gain: f32, active_voice_count: usize) -> Self {
        let count = active_voice_count.clamp(1, MAX_VOICES);
        Self {
            voices: std::array::from_fn(|_| Voice::new()),
            next_voice: 0,
            master_gain,
            active_voice_count: count,
        }
    }

    /// Trigger a sample by index on the next voice (round-robin steal).
    pub fn trigger(&mut self, idx: usize) {
        let voice_idx = self.next_voice % self.active_voice_count;
        self.next_voice = (self.next_voice + 1) % self.active_voice_count;
        self.voices[voice_idx].trigger(idx, 1.0);
    }

    /// Render mixed audio into the provided buffer.
    pub fn render(&mut self, output: &mut [f32], samples: &[Arc<Sample>]) {
        output.fill(0.0);

        for voice in &mut self.voices[..self.active_voice_count] {
            voice.mix_into(output, samples);
        }

        let gain = self.master_gain;
        for s in output.iter_mut() {
            *s = (*s * gain).clamp(-1.0, 1.0);
        }
    }
}
