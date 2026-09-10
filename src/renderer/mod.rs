//! The CPU half of the rendering pipeline: encode and compose, orchestrated
//! by `Frontend`.
//!
//! - [`frontend`] owns the per-frame allocations (the composer's scratch and
//!   the render buffer) and turns a `FrameScene` into `&RenderBuffer`. Pure
//!   CPU; no device handles.
//! - [`image_registry`] owns registered-image lifetimes. Its texels go to
//!   an `ImageStore` that [`crate::gpu`] attaches, so it names no device.
//!
//! [`RenderBuffer`](render_buffer::RenderBuffer) and [`Quad`](quad::Quad)
//! live at this level as the contract with [`crate::gpu`], which consumes a
//! `&RenderBuffer` and submits the draws. Geometry and schedule rows are CPU
//! data; `GpuView` targets are a device-only side channel carried by the same
//! frame result so they composite through the image path.
//!
//! Both halves are owned once by each host and driven with the active private
//! `WindowDriver` behind the public host facades.
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
