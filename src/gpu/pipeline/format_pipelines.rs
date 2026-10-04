//! [`FormatPipelines`] — every format-dependent `wgpu::RenderPipeline`
//! for one swapchain color format, bundled so the rest of the backend's
//! GPU state (shaders, vertex/instance buffers, the glyph + gradient
//! atlases, the image texture cache) stays format-independent and shared.
//!
//! The pipeline objects are the *only* thing that carries the color
//! target's format; pulling them out here lets a single set of resources
//! drive any number of formats — a window on an sRGB output and one on an
//! HDR output share every atlas and buffer, differing only in which
//! `FormatPipelines` their draws bind. Built eagerly — every kind, and the
//! stencil-test twin of the kinds that have one — so the set is complete the
//! moment it exists.

use crate::gpu::pipeline::blit_pipeline::BlitPipeline;
use crate::gpu::pipeline::curve_pipeline::CurvePipeline;
use crate::gpu::pipeline::image_pipeline::ImagePipeline;
use crate::gpu::pipeline::mesh_pipeline::MeshPipeline;
use crate::gpu::pipeline::quad_pipeline::{QuadPipeline, QuadVariants};
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::raster::raster_program::RasterProgram;

/// All render pipelines built against one swapchain color format. Keyed
/// by [`TargetFormat`](crate::gpu::surface::render_target::TargetFormat)
/// in the backend so windows on different-format outputs each bind the
/// right set while sharing every other resource.
#[derive(Debug)]
pub(crate) struct FormatPipelines {
    pub(crate) quad: QuadVariants,
    pub(crate) mesh: StencilVariant,
    pub(crate) image: StencilVariant,
    pub(crate) curve: StencilVariant,
    /// Base + stencil-test pipelines for **both** raster tenants: a glyph
    /// quad and an icon quad are one draw against one shader and one
    /// group-0 layout, so they are one pipeline pair binding whichever
    /// atlas the step names. See [`RasterProgram`].
    pub(crate) raster: StencilVariant,
    /// Presents the retained backbuffer onto a target that cannot be copied
    /// into. One pipeline, no stencil twin: it runs in a pass of its own,
    /// after every clipped draw.
    pub(crate) blit: wgpu::RenderPipeline,
}

/// The format-independent resource structs [`FormatPipelines::new`] reads shaders
/// and pipeline layouts off. They live side by side on the backend and are handed over as a
/// set, so a new pipeline kind is one field here rather than one more parameter at
/// every call.
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
