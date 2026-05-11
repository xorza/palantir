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
pub(crate) mod frontend;
pub(crate) mod quad;
pub(crate) mod render_buffer;
