//! Device resources the pipelines draw from: buffers, texture bindings, the upload context and the
//! staging buffers uploads and readbacks pass through, the pipeline recipe and shader assembly
//! every pipeline builds by, registered images and the groups effects bind them through, the
//! frame group, the shelf packer, the staging target a whole-target device draws page writes into,
//! and `GpuView` targets.

#[cfg(feature = "internals")]
pub(crate) mod bench;
pub(crate) mod dynamic_buffer;
pub(crate) mod effect_image_groups;
pub(crate) mod frame_group;
pub(crate) mod gpu_ctx;
pub(crate) mod gpu_view_targets;
pub(crate) mod map_state;
pub(crate) mod pipeline_recipe;
pub(crate) mod readback_staging;
pub(crate) mod shader_body;
pub(crate) mod shelf_packer;
pub(crate) mod staging_target;
pub(crate) mod texture_binding;
pub(crate) mod upload_staging;
pub(crate) mod wgpu_image_store;
