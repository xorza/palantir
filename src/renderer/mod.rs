//! Rendering pipeline, split into a CPU **frontend** (encode + compose,
//! orchestrated by `Frontend`) and a GPU **backend** (`WgpuBackend`):
//!
//! - [`frontend`] owns the per-frame allocations (cmd vec, render buffer)
//!   and turns `&Tree` into `&RenderBuffer`. Pure CPU; no device handles.
//! - [`backend`] consumes `&RenderBuffer` and submits draws. The only
//!   stage that touches a device/queue.
//!
//! [`RenderBuffer`](render_buffer::RenderBuffer) and [`Quad`](quad::Quad)
//! live at this level as the frontend↔backend contract. Geometry and schedule
//! rows are CPU data; `GpuView` targets are a wgpu-only side channel carried by
//! the same frame result so they composite through the image path.
//!
//! Both halves are owned once by each host and driven with the active private
//! [`WindowDriver`](crate::host::window_driver::WindowDriver) behind the public
//! host facades.
pub(crate) mod backend;
pub(crate) mod frontend;
pub(crate) mod gpu_view;
pub(crate) mod gradient_atlas;
pub(crate) mod image_registry;
pub(crate) mod plan;
pub(crate) mod quad;
pub(crate) mod render_buffer;
pub(crate) mod render_owner;
pub(crate) mod texture_id;
