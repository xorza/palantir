//! Device resources the pipelines draw from: buffers, texture bindings, the staging buffers
//! uploads and readbacks pass through, registered images and the groups effects bind them
//! through, the gradient rows, the packed atlases, and `GpuView` targets.

#[cfg(feature = "internals")]
pub(crate) mod bench;
pub(crate) mod dynamic_buffer;
pub(crate) mod effect_image_groups;
pub(crate) mod frame_group;
pub(crate) mod gpu_view_targets;
pub(crate) mod readback_staging;
pub(crate) mod shelf_packer;
pub(crate) mod texture_binding;
pub(crate) mod upload_staging;
pub(crate) mod wgpu_effect_store;
pub(crate) mod wgpu_image_store;
