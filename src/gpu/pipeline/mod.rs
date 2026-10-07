//! The render pipelines, one directory per pipeline beside its `shader.wgsl`,
//! and the recipe, prelude and shader assembly they share.

pub(crate) mod blit_pipeline;
pub(crate) mod curve_pipeline;
pub(crate) mod format_pipelines;
pub(crate) mod image_pipeline;
pub(crate) mod mesh_pipeline;
pub(crate) mod pipeline_recipe;
pub(crate) mod quad_pipeline;
pub(crate) mod shader_body;
pub(crate) mod stencil_variant;

use crate::gpu::surface::viewport::ViewportPush;

/// Size of the immediate (push-constant) region every shader declares in `prelude.wgsl`: the viewport. The backend pushes it after every pipeline bind, and `DeviceRequirements` rejects a device granting fewer bytes.
pub(crate) const IMMEDIATES_BYTES: u32 = ViewportPush::BYTES as u32;
