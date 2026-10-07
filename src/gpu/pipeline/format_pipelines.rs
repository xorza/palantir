//! [`FormatPipelines`]: every format-dependent `wgpu::RenderPipeline` for one swapchain format, so shaders, buffers and atlases stay format-independent and shared (an sRGB and an HDR window differ only in which set they bind). Built eagerly, including stencil-test twins.

use crate::gpu::pipeline::blit_pipeline::BlitPipeline;
use crate::gpu::pipeline::curve_pipeline::CurvePipeline;
use crate::gpu::pipeline::image_pipeline::ImagePipeline;
use crate::gpu::pipeline::mesh_pipeline::MeshPipeline;
use crate::gpu::pipeline::quad_pipeline::{QuadPipeline, QuadVariants};
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::raster::raster_program::RasterProgram;

/// All render pipelines for one swapchain format, keyed in the backend by [`TargetFormat`](crate::gpu::surface::render_target::TargetFormat).
#[derive(Debug)]
pub(crate) struct FormatPipelines {
    pub(crate) quad: QuadVariants,
    pub(crate) mesh: StencilVariant,
    pub(crate) image: StencilVariant,
    pub(crate) curve: StencilVariant,
    /// Base + stencil-test pipelines for **both** raster tenants: glyph and icon quads share one shader and group-0 layout, binding whichever atlas the step names. See [`RasterProgram`].
    pub(crate) raster: StencilVariant,
    /// Presents the retained backbuffer onto a target that can't be copied into. No stencil twin: it runs in its own pass after every clipped draw.
    pub(crate) blit: wgpu::RenderPipeline,
}

/// The format-independent resources [`FormatPipelines::new`] reads shaders and layouts from, handed over as a set so a new pipeline kind is one field.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PipelineSources<'a> {
    pub(crate) quad: &'a QuadPipeline,
    pub(crate) mesh: &'a MeshPipeline,
    pub(crate) image: &'a ImagePipeline,
    pub(crate) curve: &'a CurvePipeline,
    pub(crate) raster: &'a RasterProgram,
    pub(crate) blit: &'a BlitPipeline,
}

impl FormatPipelines {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        sources: PipelineSources<'_>,
    ) -> Self {
        let PipelineSources {
            quad,
            mesh,
            image,
            curve,
            raster,
            blit,
        } = sources;
        Self {
            quad: quad.build_variants(device, format),
            mesh: mesh.build_variants(device, format),
            image: image.build_variants(device, format),
            curve: curve.build_variants(device, format),
            raster: raster.build_variants(device, format),
            blit: blit.build(device, format),
        }
    }
}
