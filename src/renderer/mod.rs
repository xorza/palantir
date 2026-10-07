//! The CPU half of rendering: encode and compose, orchestrated by `Frontend`.
//!
//! [`frontend`] owns the per-frame allocations and turns a `FrameScene` into `&RenderBuffer`; [`image_registry`] owns registered-image lifetimes. [`RenderBuffer`](render_buffer::RenderBuffer) and [`Quad`](quad::Quad) are the contract with [`crate::gpu`].
pub(crate) mod error;
pub(crate) mod frontend;
pub(crate) mod gpu_paint;
pub(crate) mod gradient_atlas;
pub(crate) mod image_registry;
pub(crate) mod quad;
pub(crate) mod render_buffer;
pub(crate) mod render_owner_id;
pub(crate) mod render_plan;
pub(crate) mod texture_limit;
