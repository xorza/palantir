//! The GPU program every raster quad draws through, whichever tenant rasterized it: one shader
//! module, so one pipeline pair per swapchain format, over the pool's mask and color pages both
//! tenants draw through ([`AtlasGroup`](crate::gpu::pipeline::atlas_binding::AtlasGroup)).
//! `gpu::raster`'s `RasterPass` is the CPU half.

pub(crate) mod raster_quad;

use crate::gpu::atlas::page_slots::PageSlots;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::atlas_binding::{ATLAS_GROUP, AtlasSlots};
use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
use crate::gpu::pipeline::instanced_pipeline::VariantTarget;
use crate::gpu::pipeline::raster_pipeline::raster_quad::{
    FLAG_COLOR, FLAG_DESATURATE, FLAG_MASK, FLAG_PAGE, U_BITS, V_SHIFT,
};
use crate::gpu::pipeline::raster_pipeline::raster_quad::{RasterQuad, SpunRasterQuad};
use crate::gpu::resource::pipeline_recipe::{self, Pipeline, PipelineRecipe};
use crate::gpu::resource::shader_body::{ShaderBody, ShaderConstant};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::primitives::wgsl;

/// The two pipeline pairs one swapchain format draws raster quads with, for each pair of mask and
/// color [`PageSlots`], at [`slots_idx`].
#[derive(Debug)]
pub(crate) struct RasterVariants {
    /// Unturned quads, drawn texel for texel.
    flat: [ClipVariant; SLOT_PAIRS],
    /// [`SpunRasterQuad`]s, turned and filtered.
    spun: [ClipVariant; SLOT_PAIRS],
}

/// Each pair of a mask list's open slots and a color list's.
const SLOT_PAIRS: usize = PageSlots::COUNT * PageSlots::COUNT;

/// The place of `slots`' pair in a [`RasterVariants`] array.
const fn slots_idx(slots: AtlasSlots) -> usize {
    slots.mask.idx() * PageSlots::COUNT + slots.color.idx()
}

impl RasterVariants {
    /// No pipeline built.
    pub(crate) const NONE: Self = Self {
        flat: [const { ClipVariant::NONE }; SLOT_PAIRS],
        spun: [const { ClipVariant::NONE }; SLOT_PAIRS],
    };

    /// The pair a frame of `clipping` over `slots` binds, which its census built, the spun half
    /// where it turns a raster, drawing through `atlas`, the pool's atlas group.
    pub(crate) const fn pair<'a>(
        &'a self,
        clipping: Clipping,
        slots: AtlasSlots,
        atlas: &'a wgpu::BindGroup,
    ) -> RasterPipelinePair<'a> {
        let idx = slots_idx(slots);
        RasterPipelinePair {
            flat: self.flat[idx].get(clipping),
            spun: self.spun[idx].built(clipping),
            atlas,
        }
    }
}

/// Which raster pipelines a frame draws through. A turned raster is a raster, so a frame that
/// draws the spun pipeline draws the flat one too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RasterDraws {
    /// No text or icon.
    None,
    /// Text or icons, none turned.
    Flat,
    /// Text or icons, a paint animation turning one.
    FlatAndSpun,
}

impl RasterDraws {
    /// What a frame of `rasters`, its text and icons, draws, `turned` where an animation turns
    /// one.
    pub(crate) const fn of(rasters: bool, turned: bool) -> Self {
        match (rasters, turned) {
            (false, _) => Self::None,
            (true, false) => Self::Flat,
            (true, true) => Self::FlatAndSpun,
        }
    }
}

/// The pipelines and the atlas group one raster draw binds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RasterPipelinePair<'a> {
    pub(crate) flat: &'a Pipeline,
    /// `None` in a frame that turns no raster, which builds no spun pipeline.
    pub(crate) spun: Option<&'a Pipeline>,
    /// The pool's [`AtlasGroup`](crate::gpu::pipeline::atlas_binding::AtlasGroup).
    pub(crate) atlas: &'a wgpu::BindGroup,
}

/// Shader and pipeline layout, built once for every raster tenant.
#[derive(Debug)]
pub(crate) struct RasterPipeline {
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
}

impl RasterPipeline {
    /// This family's shader body.
    pub(crate) const SHADER: ShaderBody = ShaderBody {
        label: "palantir.raster.shader",
        wgsl: include_str!("shader.wgsl"),
        includes: &[wgsl::FRAME, wgsl::ATLAS_SLOTS],
        constants: SHADER_CONSTANTS,
    };

    /// The program over the frame group and `atlas`, the atlas group's layout.
    pub(crate) fn new(gpu: &Gpu, textures: &TextureBinding, atlas: &wgpu::BindGroupLayout) -> Self {
        let pipeline_layout = pipeline_recipe::pipeline_layout(
            gpu,
            "palantir.raster.pl",
            &[Some(textures.frame_layout()), Some(atlas)],
        );
        Self {
            shader: Self::SHADER.module(gpu),
            pipeline_layout,
        }
    }

    /// Build the halves of `variants` `target` binds that `draws` names, where they are not built:
    /// the only format-dependent part here, for unturned quads, and for the [`SpunRasterQuad`]s a
    /// paint animation turns, each pinned to the target's open mask and color slots. Refused: the
    /// spun half built with the flat one, for every pair of slot sets a session meets, whether or
    /// not a frame turns a raster.
    pub(crate) fn ensure(
        &self,
        variants: &mut RasterVariants,
        gpu: &Gpu,
        target: VariantTarget,
        draws: RasterDraws,
    ) {
        let idx = slots_idx(target.slots);
        let constants = target.slots.constants();
        if draws != RasterDraws::None {
            variants.flat[idx].ensure(
                gpu,
                target.clipping,
                &PipelineRecipe {
                    constants: &constants,
                    vertex_buffers: &[Some(RasterQuad::LAYOUT)],
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..PipelineRecipe::color(
                        "palantir.raster.pipeline",
                        &self.shader,
                        &self.pipeline_layout,
                        target.format,
                    )
                },
            );
        }
        if draws == RasterDraws::FlatAndSpun {
            variants.spun[idx].ensure(
                gpu,
                target.clipping,
                &PipelineRecipe {
                    constants: &constants,
                    vertex_buffers: &[Some(SpunRasterQuad::LAYOUT)],
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    vertex_entry: "vs_spun",
                    fragment_entry: "fs_spun",
                    ..PipelineRecipe::color(
                        "palantir.raster.pipeline.spun",
                        &self.shader,
                        &self.pipeline_layout,
                        target.format,
                    )
                },
            );
        }
    }
}

/// The numbers [`RasterPipeline::SHADER`] takes from the Rust types that own them.
const SHADER_CONSTANTS: &[ShaderConstant] = &[
    ShaderConstant::uint("ATLAS_GROUP", ATLAS_GROUP),
    ShaderConstant::uint("U_BITS", U_BITS),
    ShaderConstant::uint("V_SHIFT", V_SHIFT),
    ShaderConstant::uint("FLAG_MASK", FLAG_MASK),
    ShaderConstant::uint("FLAG_PAGE", FLAG_PAGE),
    ShaderConstant::uint("FLAG_DESATURATE", FLAG_DESATURATE),
    ShaderConstant::uint("FLAG_COLOR", FLAG_COLOR),
];

#[cfg(test)]
mod tests;
