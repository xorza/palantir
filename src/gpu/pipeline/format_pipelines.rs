//! [`FormatPipelines`]: every format-dependent `wgpu::RenderPipeline` for one swapchain format, so shaders, buffers and atlases stay format-independent and shared (an sRGB and an HDR window differ only in which set they bind).

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::curve_pipeline::CurvePipeline;
use crate::gpu::pipeline::effect_pipeline::EffectPipeline;
use crate::gpu::pipeline::image_pipeline::ImagePipeline;
use crate::gpu::pipeline::instanced_pipeline::VariantTarget;
use crate::gpu::pipeline::mesh_pipeline::MeshPipeline;
use crate::gpu::pipeline::pipeline_recipe::Pipeline;
use crate::gpu::pipeline::quad_pipeline::quad_draw::{QuadDraw, QuadDraws};
use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
use crate::gpu::pipeline::quad_pipeline::{QuadPipeline, QuadVariants};
use crate::gpu::pipeline::raster_pipeline::{RasterPipelines, RasterProgram, RasterVariants};
use crate::gpu::pipeline::stencil_variant::{StencilMode, StencilVariant};

/// All render pipelines for one swapchain format, keyed in the backend by [`TargetFormat`](crate::gpu::surface::render_target::TargetFormat).
///
/// Each is built on the first frame whose census asks for it ([`Self::ensure`]), before the
/// frame's passes borrow the set; [`Self::new`] builds what nearly every frame draws, the plain
/// halves of the solid, gradient and triangle quads and of every other family. A
/// pane's forms, a shadow's forms, the stencil-test twins and the masks wait for a frame that
/// draws one, as the backdrop blur's pipelines wait for a pane: an app with none pays for none.
#[derive(Debug)]
pub(crate) struct FormatPipelines {
    format: wgpu::TextureFormat,
    pub(crate) quad: QuadVariants,
    pub(crate) mesh: StencilVariant,
    pub(crate) image: StencilVariant,
    pub(crate) curve: StencilVariant,
    /// Base + stencil-test pipelines for **both** raster tenants: glyph and icon quads share one shader and group-0 layout, binding whichever atlas the step names. See [`RasterProgram`].
    pub(crate) raster: RasterVariants,
}

/// The format-independent half of each pipeline kind, which holds its shader and layouts and which
/// [`FormatPipelines`] builds from, one field per kind.
#[derive(Debug)]
pub(crate) struct Programs {
    pub(crate) quad: QuadPipeline,
    pub(crate) mesh: MeshPipeline,
    pub(crate) image: ImagePipeline,
    pub(crate) curve: CurvePipeline,
    /// The one shader and group-0 layout both raster tenants draw through; see [`RasterProgram`].
    pub(crate) raster: RasterProgram,
    /// The frame's effect instances and words; each effect's pipelines are the store's.
    pub(crate) effect: EffectPipeline,
}

/// What a frame draws through, which its pipelines are built for: the stencil mode of its passes,
/// the quad draws it takes, and the families it draws.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PipelineCensus {
    pub(crate) stencil: StencilMode,
    pub(crate) quads: QuadDraws,
    pub(crate) meshes: bool,
    pub(crate) images: bool,
    pub(crate) curves: bool,
    /// Text or icons, which draw through the raster pipelines.
    pub(crate) rasters: bool,
}

impl PipelineCensus {
    /// What nearly every frame draws, which a format's set is built with.
    fn common() -> Self {
        Self {
            stencil: StencilMode::Off,
            quads: QuadDraws::from([
                QuadDraw::Form(QuadForm::Solid),
                QuadDraw::Form(QuadForm::Gradient),
                QuadDraw::Form(QuadForm::Triangle),
            ]),
            meshes: true,
            images: true,
            curves: true,
            rasters: true,
        }
    }
}

impl FormatPipelines {
    pub(crate) fn new(gpu: &Gpu, format: wgpu::TextureFormat, programs: &Programs) -> Self {
        let mut pipelines = Self {
            format,
            quad: QuadVariants::NONE,
            mesh: StencilVariant::NONE,
            image: StencilVariant::NONE,
            curve: StencilVariant::NONE,
            raster: RasterVariants::NONE,
        };
        pipelines.ensure(gpu, programs, PipelineCensus::common());
        pipelines
    }

    /// Build what `census` draws through and the set lacks, before the frame's passes borrow it.
    pub(crate) fn ensure(&mut self, gpu: &Gpu, programs: &Programs, census: PipelineCensus) {
        let target = VariantTarget {
            format: self.format,
            mode: census.stencil,
        };
        for draw in census.quads.iter() {
            programs.quad.ensure_draw(&mut self.quad, gpu, target, draw);
        }
        if census.stencil == StencilMode::Test {
            programs.quad.ensure_masks(&mut self.quad, gpu, self.format);
        }
        if census.meshes {
            programs.mesh.ensure(&mut self.mesh, gpu, target);
        }
        if census.images {
            programs.image.ensure(&mut self.image, gpu, target);
        }
        if census.curves {
            programs.curve.ensure(&mut self.curve, gpu, target);
        }
        if census.rasters {
            programs.raster.ensure(&mut self.raster, gpu, target);
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
    /// The pipeline a quad of `draw` draws through.
    pub(crate) const fn quad(self, draw: QuadDraw) -> &'a Pipeline {
        self.fmt.quad.select(draw, self.stencil)
    }

    pub(crate) const fn mesh(self) -> &'a Pipeline {
        self.fmt.mesh.select(self.stencil)
    }

    pub(crate) const fn image(self) -> &'a Pipeline {
        self.fmt.image.select(self.stencil)
    }

    pub(crate) const fn curve(self) -> &'a Pipeline {
        self.fmt.curve.select(self.stencil)
    }

    /// Both raster tenants' pipelines.
    pub(crate) const fn raster(self) -> RasterPipelines<'a> {
        RasterPipelines {
            flat: self.fmt.raster.flat.select(self.stencil),
            spun: self.fmt.raster.spun.select(self.stencil),
        }
    }

    /// Whether the pass has the rounded-clip attachment, which picks an effect's pipeline.
    pub(crate) const fn stencil(self) -> StencilMode {
        self.stencil
    }

    /// The stencil-writing mask pipelines, which only a pass with the attachment binds.
    pub(crate) const fn mask_stamp(self) -> &'a Pipeline {
        self.fmt.quad.mask_stamp()
    }

    pub(crate) const fn mask_clear(self) -> &'a Pipeline {
        self.fmt.quad.mask_clear()
    }
}
