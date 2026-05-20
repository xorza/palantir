//! Per-frame counters for `queue.write_buffer` / `queue.write_texture`
//! issued through the [`Queue`](super::queue::Queue) wrapper. Gated
//! behind the `internals` feature.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static BUFFER_CALLS: AtomicU64 = AtomicU64::new(0);
static BUFFER_BYTES: AtomicU64 = AtomicU64::new(0);
static TEXTURE_CALLS: AtomicU64 = AtomicU64::new(0);
static TEXTURE_BYTES: AtomicU64 = AtomicU64::new(0);

pub(super) fn record_buffer(bytes: usize) {
    BUFFER_CALLS.fetch_add(1, Relaxed);
    BUFFER_BYTES.fetch_add(bytes as u64, Relaxed);
}

pub(super) fn record_texture(bytes: u64) {
    TEXTURE_CALLS.fetch_add(1, Relaxed);
    TEXTURE_BYTES.fetch_add(bytes, Relaxed);
}

/// Snapshot the counters and reset to zero. Call between bench iters
/// (or between frames in an instrumented harness) to get per-frame
/// numbers.
pub fn take() -> Stats {
    Stats {
        buffer_calls: BUFFER_CALLS.swap(0, Relaxed),
        buffer_bytes: BUFFER_BYTES.swap(0, Relaxed),
        texture_calls: TEXTURE_CALLS.swap(0, Relaxed),
        texture_bytes: TEXTURE_BYTES.swap(0, Relaxed),
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Stats {
    pub buffer_calls: u64,
    pub buffer_bytes: u64,
    pub texture_calls: u64,
    pub texture_bytes: u64,
}
