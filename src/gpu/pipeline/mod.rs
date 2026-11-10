//! The render pipelines, one directory per pipeline beside its WGSL (`shader.wgsl`, or one file a
//! concern where a pipeline holds several), and the recipe, prelude and shader assembly they share.

pub(crate) mod blit_pipeline;
pub(crate) mod clip_variant;
pub(crate) mod curve_pipeline;
pub(crate) mod effect_pipeline;
pub(crate) mod format_pipelines;
pub(crate) mod image_pipeline;
pub(crate) mod instanced_pipeline;
pub(crate) mod mesh_pipeline;
pub(crate) mod pass_recorder;
pub(crate) mod pipeline_recipe;
pub(crate) mod quad_pipeline;
pub(crate) mod raster_pipeline;
pub(crate) mod runs;
pub(crate) mod shader_body;
pub(crate) mod shape_template;
pub(crate) mod viewport_push;

use crate::gpu::device::device_requirements::IMMEDIATES_BYTES;
use crate::gpu::pipeline::viewport_push::ViewportPush;

const _: () = assert!(
    ViewportPush::BYTES as u32 == IMMEDIATES_BYTES,
    "the viewport fills the immediate region the device requirements ask for"
);

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::backdrop::backdrop_blur::BackdropBlur;
    use crate::gpu::pipeline::blit_pipeline::BlitPipeline;
    use crate::gpu::pipeline::curve_pipeline::CurvePipeline;
    use crate::gpu::pipeline::image_pipeline::ImagePipeline;
    use crate::gpu::pipeline::mesh_pipeline::MeshPipeline;
    use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
    use crate::gpu::pipeline::raster_pipeline::RasterProgram;
    use crate::gpu::pipeline::shader_body::ShaderBody;

    /// Every shader body the backend compiles, which the shader tests walk.
    pub(crate) const SHADERS: [ShaderBody; 7] = [
        QuadPipeline::SHADER,
        MeshPipeline::SHADER,
        CurvePipeline::SHADER,
        ImagePipeline::SHADER,
        RasterProgram::SHADER,
        BlitPipeline::SHADER,
        BackdropBlur::SHADER,
    ];
}
