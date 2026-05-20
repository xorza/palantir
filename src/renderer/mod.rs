//! Rendering pipeline, split into a CPU **frontend** (encode + compose,
//! orchestrated by `Frontend`) and a GPU **backend** (`WgpuBackend`):
//!
//! - [`frontend`] owns the per-frame allocations (cmd vec, render buffer)
//!   and turns `&Tree` into `&RenderBuffer`. Pure CPU; no device handles.
//! - [`backend`] consumes `&RenderBuffer` and submits draws. The only
//!   stage that touches a device/queue.
//!
//! [`RenderBuffer`](render_buffer::RenderBuffer) and [`Quad`](quad::Quad)
//! live at this level — they're the frontend↔backend contract. Pure
//! CPU data; no device handles. Other backends (software rasterizer,
//! headless capture) consume `&RenderBuffer` directly. A TUI/text
//! backend would skip the compose step and walk the encoder's
//! `RenderCmdBuffer` itself, since pixel snap and scissor rects don't
//! apply.
//!
//! Both halves are owned and driven from [`Host`](crate::host::Host),
//! the public top-level handle.
pub(crate) mod backend;
pub use backend::DEFAULT_IMAGE_BUDGET_BYTES;
/// Counting wrapper around `wgpu::Queue` — every `write_buffer` /
/// `write_texture` call routed through this type bumps the per-frame
/// counters under [`write_stats`] (gated on `internals`). Production
/// builds compile to a zero-cost passthrough.
pub use backend::Queue;
/// Per-frame counters for `queue.write_buffer` / `write_texture` calls
/// issued through [`Queue`]. Gated behind `internals` for the frame
/// bench's write-attribution arm.
#[cfg(feature = "internals")]
pub mod write_stats {
    pub use crate::renderer::backend::write_stats::{Stats, take};
}
pub(crate) mod caches;
pub mod frontend;
pub(crate) mod gradient_atlas;
pub(crate) mod quad;
pub(crate) mod render_buffer;
/// Polyline → fringe-AA mesh tessellator consumed by `Composer`.
/// Renderer-side rather than primitive: it lowers user authoring
/// (`Shape::Polyline`, stroked rounded rects) into the GPU mesh
/// vertex layout. Exposed `pub` for the `stroke_tessellate` bench's
/// `test_support` reach-in path.
pub mod stroke_tessellate;
