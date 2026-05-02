//! Rendering pipeline, three CPU stages plus a wgpu backend:
//!
//! 1. [`encode`] — `&Tree` → `Vec<RenderCmd>` (logical-px). Pure free fn.
//! 2. [`Composer`] — `&[RenderCmd]` → `RenderBuffer` (physical-px quads +
//!    scissor groups). Owns the output + scratch; no GPU handles.
//! 3. [`Painter`] — composition of (1) + (2). Owned by [`Ui`] so
//!    `Ui::end_frame` produces the painted buffer in one call; consumers
//!    pull it via `Ui::frame()`.
//! 4. [`WgpuBackend::submit`] — `&RenderBuffer` → wgpu draws. The only stage
//!    that touches a device/queue.
//!
//! Other backends (software rasterizer, headless capture) consume
//! `&RenderBuffer` directly. A TUI/text backend would skip the compose step
//! and walk `RenderCmd`s itself, since pixel snap and scissor rects don't
//! apply.
//!
//! All per-frame allocations are owned by `Painter` so steady-state
//! rendering is alloc-free.
//!
//! [`Ui`]: crate::ui::Ui
mod backend;
mod buffer;
mod composer;
mod encoder;
mod painter;
mod quad;

pub use backend::WgpuBackend;
pub use buffer::{DrawGroup, RenderBuffer};
pub use composer::Composer;
pub use encoder::{RenderCmd, encode};
pub use painter::{FrameOutput, Painter};
pub use quad::{Quad, QuadPipeline};
