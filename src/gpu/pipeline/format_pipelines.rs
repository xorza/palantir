//! [`FormatPipelines`]: every format-dependent `wgpu::RenderPipeline` for one swapchain format, so shaders, buffers and atlases stay format-independent and shared (an sRGB and an HDR window differ only in which set they bind). Built eagerly, including stencil-test twins.

use crate::gpu::pipeline::blit_pipeline::BlitPipeline;
use crate::gpu::pipeline::curve_pipeline::CurvePipeline;
use crate::gpu::pipeline::image_pipeline::ImagePipeline;
use crate::gpu::pipeline::mesh_pipeline::MeshPipeline;
use crate::gpu::pipeline::quad_pipeline::quad_draw::QuadDraw;
use crate::gpu::pipeline::quad_pipeline::{QuadPass, QuadPipeline, QuadVariants};
use crate::gpu::pipeline::raster_pipeline::{RasterPipelines, RasterProgram, RasterVariants};
use crate::gpu::pipeline::stencil_variant::{StencilMode, StencilVariant};

/// All render pipelines for one swapchain format, keyed in the backend by [`TargetFormat`](crate::gpu::surface::render_target::TargetFormat).
#[derive(Debug)]
pub(crate) struct FormatPipelines {
    pub(crate) quad: QuadVariants,
    pub(crate) mesh: StencilVariant,
    pub(crate) image: StencilVariant,
    pub(crate) curve: StencilVariant,
    /// Base + stencil-test pipelines for **both** raster tenants: glyph and icon quads share one shader and group-0 layout, binding whichever atlas the step names. See [`RasterProgram`].
    pub(crate) raster: RasterVariants,
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

    /// These pipelines as a pass binds them: the stencil-test twins where the pass has the
    /// rounded-clip attachment.
    pub(crate) const fn for_pass(&self, stencil: StencilMode) -> PassPipelines<'_> {
        PassPipelines { fmt: self, stencil }
    }
}

/// A format's pipelines for one pass, each picked for its stencil attachment in one place.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PassPipelines<'a> {
    fmt: &'a FormatPipelines,
    stencil: StencilMode,
}

impl<'a> PassPipelines<'a> {
    pub(crate) const fn quads(self) -> QuadPass<'a> {
        QuadPass::new(&self.fmt.quad, self.stencil)
    }

    pub(crate) const fn quad(self, draw: QuadDraw) -> &'a wgpu::RenderPipeline {
        self.quads().pipeline(draw)
    }

    pub(crate) const fn mesh(self) -> &'a wgpu::RenderPipeline {
        self.fmt.mesh.select(self.stencil)
    }

    pub(crate) const fn image(self) -> &'a wgpu::RenderPipeline {
        self.fmt.image.select(self.stencil)
    }

    pub(crate) const fn curve(self) -> &'a wgpu::RenderPipeline {
        self.fmt.curve.select(self.stencil)
    }

    /// Both raster tenants' pipelines, over the program's `layout`.
    pub(crate) const fn raster(self, layout: &'a wgpu::PipelineLayout) -> RasterPipelines<'a> {
        RasterPipelines {
            flat: self.fmt.raster.flat.select(self.stencil),
            spun: self.fmt.raster.spun.select(self.stencil),
            layout,
        }
    }

    /// The stencil-writing mask pipelines, which only a pass with the attachment binds.
    pub(crate) const fn mask_stamp(self) -> &'a wgpu::RenderPipeline {
        &self.fmt.quad.mask_stamp
    }

    pub(crate) const fn mask_clear(self) -> &'a wgpu::RenderPipeline {
        &self.fmt.quad.mask_clear
    }
}
