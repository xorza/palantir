//! [`FormatPipelines`]: every format-dependent `wgpu::RenderPipeline` for one swapchain format, so shaders, buffers and atlases stay format-independent and shared (an sRGB and an HDR window differ only in which set they bind).

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::atlas_binding::AtlasSlots;
use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
use crate::gpu::pipeline::curve_pipeline::CurvePipeline;
use crate::gpu::pipeline::effect_pipeline::EffectPipeline;
use crate::gpu::pipeline::image_pipeline::ImagePipeline;
use crate::gpu::pipeline::instanced_pipeline::FrameVariant;
use crate::gpu::pipeline::mesh_pipeline::MeshPipeline;
use crate::gpu::pipeline::quad_pipeline::quad_draw::{QuadDraw, QuadDraws};
use crate::gpu::pipeline::quad_pipeline::{QuadPipeline, QuadVariants};
use crate::gpu::pipeline::raster_pipeline::{
    RasterDraws, RasterPipeline, RasterPipelinePair, RasterVariants,
};
use crate::gpu::resource::pipeline_recipe::Pipeline;
use crate::renderer::render_buffer::effect_draw_row::EffectKey;
use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
use crate::renderer::render_buffer::shape_form::ShapeForm;

/// All render pipelines for one swapchain format, keyed in the backend by [`TargetFormat`](crate::gpu::texel::TargetFormat).
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
    /// Indexed by [`ShapeForm`].
    pub(crate) image: [ClipVariant; ShapeForm::ALL.len()],
    pub(crate) curve: ClipVariant,
    /// The pipelines of **both** raster tenants: glyph and icon quads share one shader and layout,
    /// binding whichever atlas the step names. See [`RasterPipeline`].
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
    /// The one shader and layout both raster tenants draw through; see [`RasterPipeline`].
    pub(crate) raster: RasterPipeline,
    /// The frame's effect instances and words; each effect's pipelines are the store's.
    pub(crate) effect: EffectPipeline,
}

/// What a frame draws through, which its pipelines are built for: its variant, the quad draws it
/// takes, and the families it draws.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PipelineCensus {
    pub(crate) variant: FrameVariant,
    pub(crate) quads: QuadDraws,
    pub(crate) meshes: bool,
    /// Images of each [`ShapeForm`], indexed by it.
    pub(crate) images: [bool; ShapeForm::ALL.len()],
    pub(crate) curves: bool,
    /// The raster pipelines its text and icons draw through.
    pub(crate) rasters: RasterDraws,
}

impl PipelineCensus {
    /// What nearly every frame draws, which a format's set is built with.
    fn common() -> Self {
        Self {
            variant: FrameVariant {
                clipping: Clipping::Off,
                slots: AtlasSlots::FIRST,
            },
            quads: QuadDraws::from([
                QuadDraw::Fill(FillForm::BOX_SOLID),
                QuadDraw::Fill(FillForm::new(ShapeForm::Box, FillBrush::Gradient)),
                QuadDraw::Fill(FillForm::new(ShapeForm::Triangle, FillBrush::Solid)),
            ]),
            meshes: true,
            images: [true, false, false],
            curves: true,
            rasters: RasterDraws::Flat,
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
        let target = census.variant.target(self.format);
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
        programs
            .raster
            .ensure(&mut self.raster, gpu, target, census.rasters);
    }

    /// These pipelines as a frame of `variant` binds them, with `effects`, the format's effect
    /// pipelines the effect store built at each key's [`EffectKey::slot`], and `atlas`, the atlas
    /// group its raster and shadow draws read the pages through.
    pub(crate) const fn for_frame<'a>(
        &'a self,
        variant: FrameVariant,
        effects: &'a [ClipVariant],
        atlas: &'a wgpu::BindGroup,
    ) -> PassPipelines<'a> {
        PassPipelines {
            fmt: self,
            effects,
            variant,
            atlas,
        }
    }
}

/// A format's pipelines for one frame, each picked for whether its draws name rounded clips in
/// one place: every family's, the effects' among them, which the effect store builds and the
/// frame borrows for as long as its passes record. Refused: a copy of the effects' pipelines in
/// the effect family, a second place a frame's pipelines live.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PassPipelines<'a> {
    fmt: &'a FormatPipelines,
    /// The format's effect pipelines, at each key's [`EffectKey::slot`].
    effects: &'a [ClipVariant],
    variant: FrameVariant,
    /// The [`AtlasGroup`](crate::gpu::pipeline::atlas_binding::AtlasGroup)'s group.
    atlas: &'a wgpu::BindGroup,
}

impl<'a> PassPipelines<'a> {
    /// The pipeline a quad of `draw` draws through.
    pub(crate) const fn quad(self, draw: QuadDraw) -> &'a Pipeline {
        self.fmt
            .quad
            .get(draw, self.variant.clipping, self.variant.slots.mask)
    }

    pub(crate) const fn mesh(self) -> &'a Pipeline {
        self.fmt.mesh.get(self.variant.clipping)
    }

    /// The pipeline an image of `form` draws through.
    pub(crate) const fn image(self, form: ShapeForm) -> &'a Pipeline {
        self.fmt.image[form as usize].get(self.variant.clipping)
    }

    pub(crate) const fn curve(self) -> &'a Pipeline {
        self.fmt.curve.get(self.variant.clipping)
    }

    /// The pipeline an effect draw of `key` draws through, which the store built before the
    /// frame's passes.
    pub(crate) const fn effect(self, key: EffectKey) -> &'a Pipeline {
        self.effects[key.slot()].get(self.variant.clipping)
    }

    /// Both raster tenants' pipelines, drawing through the atlas group.
    pub(crate) const fn raster(self) -> RasterPipelinePair<'a> {
        self.fmt
            .raster
            .pair(self.variant.clipping, self.variant.slots, self.atlas)
    }

    /// The atlas group a draw that reads the pages binds at
    /// [`ATLAS_GROUP`](crate::gpu::pipeline::atlas_binding::ATLAS_GROUP).
    pub(crate) const fn atlas(self) -> &'a wgpu::BindGroup {
        self.atlas
    }
}
