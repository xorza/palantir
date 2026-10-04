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

/// Size of the immediate (push-constant) region every palantir shader
/// declares, through the one `var<immediate> imm: Immediates` in
/// `prelude.wgsl`: the viewport. Every pipeline layout declares this
/// size, since every shader carries the prelude's declaration.
///
/// The backend pushes the viewport after every pipeline bind, so no draw
/// relies on immediate bytes surviving a pipeline switch: a missed push
/// is silent NDC corruption, quads at the wrong scale painting outside
/// their damage scissor.
///
/// `DeviceRequirements` asks every device for exactly this many bytes and
/// rejects one that grants fewer.
pub(crate) const IMMEDIATES_BYTES: u32 = ViewportPush::BYTES as u32;
