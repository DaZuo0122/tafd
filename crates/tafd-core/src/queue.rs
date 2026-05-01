use crossbeam::queue::ArrayQueue;
use std::sync::LazyLock;

pub const QUEUE_CAP: usize = 64;

/// Lock-free SPSC queue from input thread to audio callback.
pub static INPUT_QUEUE: LazyLock<ArrayQueue<u32>> = LazyLock::new(|| ArrayQueue::new(QUEUE_CAP));
