//! Rendering pipeline, split into three pure stages plus a wgpu backend:
//!
//! 1. [`encode`] — `&Tree` → `Vec<RenderCmd>` (logical-px). Pure.
//! 2. [`compose`] — `&[RenderCmd]` → `RenderBuffer` (physical-px quads + scissor
//!    groups). Pure. No GPU handles.
//! 3. [`WgpuBackend::submit`] — `&RenderBuffer` → wgpu draws. The only stage
//!    that touches a device/queue.
//!
//! Other backends (software rasterizer, headless capture) consume
//! `&RenderBuffer` directly. A TUI/text backend would skip `compose` and walk
//! `RenderCmd`s itself, since pixel snap and scissor rects don't apply.
//!
//! All allocations are owned by the caller (the `Vec<RenderCmd>` and
//! `RenderBuffer`) so steady-state rendering is alloc-free.
mod backend;
mod buffer;
mod compose;
mod composer;
mod encoder;
mod quad;

pub use backend::WgpuBackend;
pub use buffer::{DrawGroup, RenderBuffer, ScissorRect};
pub use compose::{ComposeParams, compose};
pub use composer::Composer;
pub use encoder::{RenderCmd, encode};
pub use quad::{Quad, QuadPipeline};
