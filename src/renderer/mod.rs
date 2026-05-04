//! Rendering pipeline, split into a CPU **frontend** (encode + compose,
//! orchestrated by `Frontend`) and a GPU **backend** (`WgpuBackend`):
//!
//! - [`frontend`] owns the per-frame allocations (cmd vec, render buffer)
//!   and turns `&Tree` into `&RenderBuffer`. Pure CPU; no device handles.
//! - [`backend`] consumes `&RenderBuffer` and submits draws. The only
//!   stage that touches a device/queue.
//!
//! `RenderBuffer` and `Quad` live at this level — they're the contract
//! between frontend and backend. Other backends (software rasterizer,
//! headless capture) consume `&RenderBuffer` directly. A TUI/text
//! backend would skip the compose step and walk the encoder's
//! `RenderCmdBuffer` itself, since pixel snap and scissor rects don't
//! apply.
//!
//! [`Ui`]: crate::ui::Ui
pub(crate) mod backend;
pub(crate) mod buffer;
pub(crate) mod frontend;
pub(crate) mod quad;
