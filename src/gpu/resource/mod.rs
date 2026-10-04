//! Device resources the pipelines draw from: buffers, texture bindings and
//! uploads, registered images, the gradient atlas, and `GpuView` targets.

pub(crate) mod dynamic_buffer;
pub(crate) mod gpu_gradient_atlas;
pub(crate) mod gpu_view_targets;
pub(crate) mod single_quad_buffer;
pub(crate) mod texture_binding;
pub(crate) mod texture_region;
pub(crate) mod wgpu_image_store;
