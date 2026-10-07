//! Non-shipping code: test and bench subsystems, and the surface that
//! integration suites, the showcase and bench targets reach them through.
//! **Not a supported API**; it breaks without notice.
//!
//! Items are `pub` when code outside the crate calls them, `pub(crate)` when
//! only the crate's own tests and benches do; each module carries its own gate.

#[cfg(feature = "internals")]
pub mod demo_swatches;
#[cfg(feature = "internals")]
pub mod frame_fixture;
pub mod harness;
#[cfg(test)]
mod hot_struct_sizes;
#[cfg(any(test, feature = "bench"))]
pub(crate) mod paint_capture;
#[cfg(test)]
pub(crate) mod panic_probe;
pub mod record_app;

use crate::text;

/// The GPU test device lives in `crate::gpu` (`clippy.toml` keeps wgpu types there); re-exported for the suites.
pub use crate::gpu::test_gpu::{
    HeadlessTestGpuLease, headless_test_gpu, isolated_headless_test_gpu,
};

/// The raster-scale quantum, so the allocation suite's scale ramp steps one rung a frame.
pub const TEXT_SCALE_STEP: f32 = text::TEXT_SCALE_STEP;

/// The shaped-buffer cache's short window: the first expiry drain comes due this many frames after the first shape.
pub const PROBATION_KEEP_FRAMES: u64 = text::internals::PROBATION_KEEP_FRAMES;

/// Frames per revolution of the shaped-buffer expiry ring; audits warm and measure in whole revolutions.
pub const SHAPED_BUFFER_RING_FRAMES: u64 = text::internals::SHAPED_BUFFER_RING_FRAMES;
