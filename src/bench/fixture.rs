//! The window a GPU case draws into.

use crate::gpu::bench_gpu::BenchGpu;
use crate::host::headless_window::HeadlessWindow;
use glam::UVec2;

/// A window of `size` at scale 1 on `gpu` whose panels paint no theme chrome, so a GPU case's own
/// draws are all a frame holds.
pub(crate) fn plain_window(gpu: &BenchGpu, size: UVec2, collect_gpu_stats: bool) -> HeadlessWindow {
    let mut window = HeadlessWindow::builder(gpu.handles())
        .collect_gpu_stats(collect_gpu_stats)
        .open(size, 1.0);
    window.ui().theme_mut().panel_chrome = None;
    window
}
