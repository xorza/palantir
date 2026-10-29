//! The render pipelines, one directory per pipeline beside its `shader.wgsl`,
//! and the recipe, prelude and shader assembly they share.

pub(crate) mod blit_pipeline;
pub(crate) mod curve_pipeline;
pub(crate) mod format_pipelines;
pub(crate) mod image_pipeline;
pub(crate) mod mesh_pipeline;
pub(crate) mod pipeline_recipe;
pub(crate) mod quad_pipeline;
pub(crate) mod raster_pipeline;
pub(crate) mod runs;
pub(crate) mod shader_body;
pub(crate) mod stencil_variant;
pub(crate) mod viewport_push;

use crate::gpu::pipeline::viewport_push::ViewportPush;

/// Size of the immediate (push-constant) region every shader declares in `prelude.wgsl`: the viewport. The backend pushes it after every pipeline bind, and `DeviceRequirements` rejects a device granting fewer bytes.
pub(crate) const IMMEDIATES_BYTES: u32 = ViewportPush::BYTES as u32;

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
