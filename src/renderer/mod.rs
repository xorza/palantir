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
//! backend would skip the compose step and walk `RenderCmd`s itself,
//! since pixel snap and scissor rects don't apply.
//!
//! [`Ui`]: crate::ui::Ui
mod backend;
mod buffer;
mod frontend;
mod quad;

pub use backend::WgpuBackend;
pub use buffer::{DrawGroup, RenderBuffer};
#[cfg(test)]
pub(crate) use frontend::Encoder;
pub use frontend::{Composer, FrameOutput, Frontend, RenderCmd, RenderCmdBuffer};
pub use quad::Quad;
