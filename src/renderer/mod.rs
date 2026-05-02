//! Rendering pipeline, three CPU stages plus a wgpu backend:
//!
//! 1. [`encode`] — `&Tree` → `Vec<RenderCmd>` (logical-px). Pure free fn.
//! 2. [`Composer`] — `&[RenderCmd]` → `RenderBuffer` (physical-px quads +
//!    scissor groups). Owns the output + scratch; no GPU handles.
//! 3. [`Pipeline`] — composition of (1) + (2). Typical app entry point: holds
//!    a `Composer` and the encoded `RenderCmd` vec, calls `build(tree, …)`
//!    each frame.
//! 4. [`WgpuBackend::submit`] — `&RenderBuffer` → wgpu draws. The only stage
//!    that touches a device/queue.
//!
//! Other backends (software rasterizer, headless capture) consume
//! `&RenderBuffer` directly. A TUI/text backend would skip the compose step
//! and walk `RenderCmd`s itself, since pixel snap and scissor rects don't
//! apply.
//!
//! All allocations are owned by `Pipeline` so steady-state rendering is
//! alloc-free.
mod backend;
mod buffer;
mod composer;
mod encoder;
mod pipeline;
mod quad;

pub use backend::WgpuBackend;
pub use buffer::{DrawGroup, RenderBuffer};
pub use composer::{ComposeParams, Composer};
pub use encoder::{RenderCmd, encode};
pub use pipeline::Pipeline;
pub use quad::{Quad, QuadPipeline};
