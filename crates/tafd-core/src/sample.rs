use std::sync::Arc;

/// A preloaded PCM sample: mono f32 at the configured sample rate.
#[derive(Debug, Clone)]
pub struct Sample {
    pub data: Arc<Vec<f32>>,
}

impl Sample {
    pub fn new(data: Vec<f32>) -> Self {
        Self {
            data: Arc::new(data),
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
