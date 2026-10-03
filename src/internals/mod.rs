//! Everything that does not ship: the test and benchmark subsystems, and
//! the surface that code outside the crate reaches them through: the
//! integration suites under `tests/`, the showcase, and the bench targets.
//! **Not a supported API** — it exists so that code can drive the crate,
//! and it breaks without notice.
//!
//! One of two homes for non-shipping code. The other is the `internals`
//! module at the end of a file, which reaches that file's private items
//! for whoever needs them; this one holds what is a subsystem in its own
//! right. Benchmark drivers live apart, in `bench.rs` files and the
//! `bench` facade.
//!
//! An item here is `pub` when code outside the crate calls it, and
//! `pub(crate)` when only this crate's own tests and benches do. Each
//! module carries the gate of the builds that use it.

#[cfg(feature = "internals")]
pub mod demo_swatches;
#[cfg(feature = "internals")]
pub mod frame_fixture;
pub mod harness;
#[cfg(any(test, feature = "bench"))]
pub(crate) mod paint_capture;
#[cfg(test)]
pub(crate) mod panic_probe;
pub mod record_app;

use crate::text;

/// The GPU test device lives in `crate::gpu` with every other wgpu call
/// — `clippy.toml` keeps wgpu types out of every other module — so this
/// one re-export is how the suites reach it.
pub use crate::gpu::test_gpu::{
    HeadlessTestGpuLease, headless_test_gpu, isolated_headless_test_gpu,
};

/// The raster-scale quantum, so the allocation suite's scale ramp can
/// step exactly one rung a frame. A ramp that spelled the number itself
/// would stop minting fresh raster keys the moment this moved, and a gate
/// that stops missing stops measuring.
pub const TEXT_SCALE_STEP: f32 = text::TEXT_SCALE_STEP;

/// The shaped-buffer cache's short window, so a text audit can warm
/// through the first expiry drain it schedules — a one-off that comes due
/// this many frames after the first shape, whatever the frames between
/// look like.
pub const PROBATION_KEEP_FRAMES: u64 = text::internals::PROBATION_KEEP_FRAMES;

/// Frames one revolution of the shaped-buffer expiry ring takes. A text
/// audit warms and measures in whole revolutions, so a cost the ring
/// incurs once per revolution lands inside its window.
pub const SHAPED_BUFFER_RING_FRAMES: u64 = text::internals::SHAPED_BUFFER_RING_FRAMES;
