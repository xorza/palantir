//! What GPU cases share: the window they draw into, and how they read device time back.

use crate::gpu::bench_gpu::BenchGpu;
use crate::host::harness::Harness;
use glam::UVec2;
use std::time::Duration;

/// A window of `size` at scale 1 on `gpu` whose panels paint no theme chrome, so a GPU case's own
/// draws are all a frame holds.
pub(crate) fn plain_window(gpu: &BenchGpu, size: UVec2, collect_gpu_stats: bool) -> Harness {
    let mut window = Harness::builder(size)
        .swapchain_present()
        .collect_gpu_stats(collect_gpu_stats)
        .gpu(gpu.handles())
        .build();
    window.ui().theme_mut().panel_chrome = None;
    window
}

/// Run `frame` until it returns a device time: timestamps read back a frame late.
///
/// # Panics
///
/// Panics if eight frames return none, though the adapter writes timestamps.
pub(crate) fn settle_timestamps(mut frame: impl FnMut() -> Option<Duration>) {
    let timed = (0..8).any(|_| frame().is_some());
    assert!(timed, "no timestamps, though the adapter writes them");
}

/// The sum of `iters` of `frame`'s device times, for `iter_custom`.
///
/// # Panics
///
/// Panics if a frame returns none once [`settle_timestamps`] saw one.
pub(crate) fn device_time(iters: u64, mut frame: impl FnMut() -> Option<Duration>) -> Duration {
    (0..iters)
        .map(|_| frame().expect("a timestamp every frame once one arrived"))
        .sum()
}
