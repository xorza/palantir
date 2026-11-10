//! [`FormatPipelines`]: every format-dependent `wgpu::RenderPipeline` for one swapchain format, so shaders, buffers and atlases stay format-independent and shared (an sRGB and an HDR window differ only in which set they bind).

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
use crate::gpu::pipeline::curve_pipeline::CurvePipeline;
use crate::gpu::pipeline::effect_pipeline::EffectPipeline;
use crate::gpu::pipeline::image_pipeline::ImagePipeline;
use crate::gpu::pipeline::instanced_pipeline::VariantTarget;
use crate::gpu::pipeline::mesh_pipeline::MeshPipeline;
use crate::gpu::pipeline::pipeline_recipe::Pipeline;
use crate::gpu::pipeline::quad_pipeline::quad_draw::{QuadDraw, QuadDraws};
use crate::gpu::pipeline::quad_pipeline::{QuadPipeline, QuadVariants};
use crate::gpu::pipeline::raster_pipeline::{RasterPipelines, RasterProgram, RasterVariants};
use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
use crate::renderer::render_buffer::shape_form::ShapeForm;

/// All render pipelines for one swapchain format, keyed in the backend by [`TargetFormat`](crate::gpu::surface::render_target::TargetFormat).
///
/// Each is built on the first frame whose census asks for it ([`Self::ensure`]), before the
/// frame's passes borrow the set; [`Self::new`] builds what nearly every frame draws, the solid,
/// gradient and triangle quads and every other family, each its half that takes no clip. A
/// backdrop's forms, a shadow's forms and the halves that take rounded clips wait for a frame that
/// draws one, as the backdrop blur's pipelines wait for a backdrop: an app with none pays for none.
#[derive(Debug)]
pub(crate) struct FormatPipelines {
    format: wgpu::TextureFormat,
    pub(crate) quad: QuadVariants,
    pub(crate) mesh: ClipVariant,
    /// Indexed by [`ShapeForm`]; an image draws no [`ShapeForm::BoxOutside`].
    pub(crate) image: [ClipVariant; ShapeForm::ALL.len()],
    pub(crate) curve: ClipVariant,
    /// The pipelines of **both** raster tenants: glyph and icon quads share one shader and layout,
    /// binding whichever atlas the step names. See [`RasterProgram`].
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
    /// The one shader and layout both raster tenants draw through; see [`RasterProgram`].
    pub(crate) raster: RasterProgram,
    /// The frame's effect instances and words; each effect's pipelines are the store's.
    pub(crate) effect: EffectPipeline,
}

/// What a frame draws through, which its pipelines are built for: whether its draws name rounded
/// clips, the quad draws it takes, and the families it draws.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PipelineCensus {
    pub(crate) clipping: Clipping,
    pub(crate) quads: QuadDraws,
    pub(crate) meshes: bool,
    /// Images of each [`ShapeForm`], indexed by it.
    pub(crate) images: [bool; ShapeForm::ALL.len()],
    pub(crate) curves: bool,
    /// Text or icons, which draw through the raster pipelines.
    pub(crate) rasters: bool,
}

impl PipelineCensus {
    /// What nearly every frame draws, which a format's set is built with.
    fn common() -> Self {
        Self {
            clipping: Clipping::Off,
            quads: QuadDraws::from([
                QuadDraw::Fill(FillForm::BOX_SOLID),
                QuadDraw::Fill(FillForm::new(ShapeForm::Box, FillBrush::Gradient)),
                QuadDraw::Fill(FillForm::new(ShapeForm::Triangle, FillBrush::Solid)),
            ]),
            meshes: true,
            images: [true, false, false, false],
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
            mesh: ClipVariant::NONE,
            image: [ClipVariant::NONE; ShapeForm::ALL.len()],
            curve: ClipVariant::NONE,
            raster: RasterVariants::NONE,
        };
        pipelines.ensure(gpu, programs, PipelineCensus::common());
        pipelines
    }

    /// Build what `census` draws through and the set lacks, before the frame's passes borrow it.
    pub(crate) fn ensure(&mut self, gpu: &Gpu, programs: &Programs, census: PipelineCensus) {
        let target = VariantTarget {
            format: self.format,
            clipping: census.clipping,
        };
        for draw in census.quads.iter() {
            programs.quad.ensure_draw(&mut self.quad, gpu, target, draw);
        }
        if census.meshes {
            programs.mesh.ensure(&mut self.mesh, gpu, target);
        }
        for (form, wanted) in ShapeForm::ALL.into_iter().zip(census.images) {
            if wanted {
                programs
                    .image
                    .ensure(&mut self.image[form as usize], gpu, target, form);
            }
        }
        if census.curves {
            programs.curve.ensure(&mut self.curve, gpu, target);
        }
        if census.rasters {
            programs.raster.ensure(&mut self.raster, gpu, target);
        }
    }

    /// These pipelines as a frame of `clipping` binds them.
    pub(crate) const fn for_frame(&self, clipping: Clipping) -> PassPipelines<'_> {
        PassPipelines {
            fmt: self,
            clipping,
        }
    }
}

/// A format's pipelines for one frame, each picked for whether its draws name rounded clips in
/// one place.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PassPipelines<'a> {
    fmt: &'a FormatPipelines,
    clipping: Clipping,
}

impl<'a> PassPipelines<'a> {
    /// Whether the frame's draws name rounded clips, which picks an effect's pipeline.
    pub(crate) const fn clipping(self) -> Clipping {
        self.clipping
    }

    /// The pipeline a quad of `draw` draws through.
    pub(crate) const fn quad(self, draw: QuadDraw) -> &'a Pipeline {
        self.fmt.quad.get(draw, self.clipping)
    }

    pub(crate) const fn mesh(self) -> &'a Pipeline {
        self.fmt.mesh.get(self.clipping)
    }

    /// The pipeline an image of `form` draws through.
    pub(crate) const fn image(self, form: ShapeForm) -> &'a Pipeline {
        self.fmt.image[form as usize].get(self.clipping)
    }

    pub(crate) const fn curve(self) -> &'a Pipeline {
        self.fmt.curve.get(self.clipping)
    }

    /// Both raster tenants' pipelines.
    pub(crate) const fn raster(self) -> RasterPipelines<'a> {
        RasterPipelines {
            flat: self.fmt.raster.flat.get(self.clipping),
            spun: self.fmt.raster.spun.get(self.clipping),
        }
    }
}
