use std::sync::Arc;
use tafd_core::{MAX_VOICES, Sample};

/// A single playback voice.
pub struct Voice {
    sample: Option<Arc<Sample>>,
    frame_pos: usize,
    active: bool,
    gain: f32,
}

impl Voice {
    pub const fn new() -> Self {
        Self {
            sample: None,
            frame_pos: 0,
            active: false,
            gain: 1.0,
        }
    }

    pub fn trigger(&mut self, sample: Arc<Sample>, gain: f32) {
        self.sample = Some(sample);
        self.frame_pos = 0;
        self.gain = gain;
        self.active = true;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Mix this voice into the output buffer. Returns true if still active.
    pub fn mix_into(&mut self, output: &mut [f32]) -> bool {
        if !self.active {
            return false;
        }
        let Some(ref sample) = self.sample else {
            self.active = false;
            return false;
        };

        let data = sample.data.as_slice();
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

    /// Trigger a sample on the next voice (round-robin steal).
    pub fn trigger(&mut self, sample: Arc<Sample>) {
        let idx = self.next_voice % self.active_voice_count;
        self.next_voice = (self.next_voice + 1) % self.active_voice_count;
        self.voices[idx].trigger(sample, 1.0);
    }

    /// Render mixed audio into the provided buffer.
    pub fn render(&mut self, output: &mut [f32]) {
        // First zero the buffer
        for s in output.iter_mut() {
            *s = 0.0;
        }

        for voice in &mut self.voices[..self.active_voice_count] {
            voice.mix_into(output);
        }

        // Apply master gain and hard clamp
        let gain = self.master_gain;
        for s in output.iter_mut() {
            *s *= gain;
            *s = s.clamp(-1.0, 1.0);
        }
    }
}
