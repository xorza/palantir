//! Non-shipping code: test and bench subsystems, and the surface that
//! integration suites, the showcase and bench targets reach them through.
//! **Not a supported API**; it breaks without notice.
//!
//! Items are `pub` when code outside the crate calls them, `pub(crate)` when
//! only the crate's own tests and benches do. A module only the crate's own
//! tests use is gated `#[cfg(test)]`.

pub mod demo_swatches;
pub mod frame_fixture;
#[cfg(test)]
mod hot_struct_sizes;
pub mod inspected_widget;
pub(crate) mod paint_capture;
#[cfg(test)]
pub(crate) mod panic_probe;
pub mod record_app;
#[cfg(test)]
pub(crate) mod scratch_report;
pub mod ui_harness;

use crate::input_state::capture;
use crate::text;
use std::time::Duration;

/// The bench device lives in `crate::gpu` (`clippy.toml` keeps wgpu types there); re-exported for
/// benches outside the crate, which share its adapter check and its devices.
pub use crate::gpu::bench_gpu::{BenchGpu, Timing};
/// A texture a harness renders into and reads back, and what it is made as.
pub use crate::gpu::surface::owned_target::{OwnedTarget, TargetSpec};
/// The GPU test device lives in `crate::gpu` (`clippy.toml` keeps wgpu types there); re-exported for the suites.
pub use crate::gpu::test_gpu::{
    HeadlessTestGpuLease, headless_test_gpu, isolated_headless_test_gpu,
};
/// One window an app runs in with no desktop.
pub use crate::host::headless_window::{HeadlessWindow, HeadlessWindowBuilder};

/// The raster-scale quantum, so the allocation suite's scale ramp steps one rung a frame.
pub const TEXT_SCALE_STEP: f32 = text::TEXT_SCALE_STEP;

/// The shaped-buffer cache's short window: the first expiry drain comes due this many frames after the first shape.
pub const PROBATION_KEEP_FRAMES: u64 = text::internals::PROBATION_KEEP_FRAMES;

/// Frames per revolution of the shaped-buffer expiry ring; audits warm and measure in whole revolutions.
pub const SHAPED_BUFFER_RING_FRAMES: u64 = text::internals::SHAPED_BUFFER_RING_FRAMES;

/// How soon a second press on the same target makes a double click; a driver that steps its own
/// clock spaces separate clicks past it.
pub const DOUBLE_CLICK_WINDOW: Duration = capture::DOUBLE_CLICK_WINDOW;
