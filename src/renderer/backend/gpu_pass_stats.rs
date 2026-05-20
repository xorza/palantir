//! Latest main-pass GPU duration, refreshed by [`GpuTimings`] each
//! frame from `wgpu` timestamp-query readbacks.
//!
//! Single global atomic, read by the `frame_stats` debug overlay and
//! the `frame` bench's GPU-stats arm. Independent of
//! [`super::write_stats`] (which counts uploads) — pass duration
//! describes how long the GPU spent executing the main pass, upload
//! stats describe how much we asked the driver to transfer.
//!
//! [`GpuTimings`]: super::gpu_timings::GpuTimings

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// Sentinel for "no timing recorded yet". `u64::MAX` rather than `0`
/// so the (extremely unlikely) case of a 0-ns pass doesn't get hidden
/// behind the same value as "no data."
const UNINIT: u64 = u64::MAX;

static LAST_PASS_NS: AtomicU64 = AtomicU64::new(UNINIT);

pub(super) fn record_pass_ns(ns: u64) {
    LAST_PASS_NS.store(ns, Relaxed);
}

/// `Some(milliseconds)` once at least one frame's main-pass
/// timestamps have resolved through the readback path; `None` if the
/// adapter lacks `TIMESTAMP_QUERY` or no resolve has landed yet
/// (first 1-2 frames after start, since the staging-buffer map_async
/// needs one round-trip). Storage is nanoseconds internally; we
/// convert to ms at the read site because every consumer
/// (debug overlay, bench reporters) wants ms for display.
pub fn last_pass_ms() -> Option<f32> {
    let ns = LAST_PASS_NS.load(Relaxed);
    if ns == UNINIT {
        None
    } else {
        Some(ns as f32 / 1_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests share the global atomic, so they must run sequentially.
    // Wrap each in a mutex held for the body. `parking_lot` isn't
    // available here; we use a stdlib Mutex.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Reset to the uninitialised sentinel — the global is process-
    /// wide, so each test has to put it back to a known state.
    fn reset() {
        LAST_PASS_NS.store(UNINIT, Relaxed);
    }

    #[test]
    fn last_pass_ms_starts_uninit() {
        let _g = LOCK.lock().unwrap();
        reset();
        assert_eq!(last_pass_ms(), None);
    }

    #[test]
    fn record_then_read_round_trips_ms() {
        let _g = LOCK.lock().unwrap();
        reset();
        record_pass_ns(2_345_000); // 2.345 ms
        let ms = last_pass_ms().expect("recorded value visible");
        assert!((ms - 2.345).abs() < 1e-4, "got {ms}");
    }

    #[test]
    fn record_overrides_previous_value() {
        // Pin: the atomic stores the *latest* sample, not an EMA or
        // accumulated total. `frame_stats` wants "how long was the
        // GPU busy last frame," not "average of all frames."
        let _g = LOCK.lock().unwrap();
        reset();
        record_pass_ns(1_000_000);
        record_pass_ns(5_000_000);
        assert!((last_pass_ms().unwrap() - 5.0).abs() < 1e-4);
    }
}
