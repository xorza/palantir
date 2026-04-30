//! Renderer is split in two:
//!
//! - [`encoder`] — pure tree → [`RenderCmd`] translation. No GPU. Unit-testable.
//! - [`backend`] — wgpu pipelines, scissor processing, `RenderPass` submission.
//!
//! `Renderer` (re-exported below) drives the wgpu backend. Other backends
//! (software, Vello, headless test harness) can be added by consuming the
//! `RenderCmd` stream from `encoder::encode`.

mod backend;
mod encoder;
mod quad;

pub use backend::{RenderFrame, Renderer};
pub use encoder::{RenderCmd, encode};
pub use quad::{Quad, QuadPipeline};
