//! Device resources the pipelines draw from: buffers, texture bindings and
//! uploads, registered images, the gradient atlas, the packed atlases, and `GpuView` targets.

#[cfg(feature = "internals")]
pub(crate) mod bench;
pub(crate) mod dynamic_buffer;
pub(crate) mod gpu_gradient_atlas;
pub(crate) mod gpu_view_targets;
pub(crate) mod packed_atlas;
pub(crate) mod shelf_packer;
pub(crate) mod single_quad_buffer;
pub(crate) mod texture_binding;
pub(crate) mod wgpu_image_store;
