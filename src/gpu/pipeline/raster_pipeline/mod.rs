//! The GPU program every raster quad draws through, whichever tenant rasterized it: one shader module and one
//! group-0 layout, so one pipeline pair per swapchain format. Each tenant still owns its atlas; a shared
//! layout shares the shape of a binding, not the space behind it. `gpu::raster`'s `RasterPass` is
//! the CPU half.

pub(crate) mod raster_quad;

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::clip_variant::ClipVariant;
use crate::gpu::pipeline::instanced_pipeline::VariantTarget;
use crate::gpu::pipeline::pipeline_recipe::{self, Pipeline, PipelineRecipe};
use crate::gpu::pipeline::raster_pipeline::raster_quad::{
    FLAG_COLOR, FLAG_DESATURATE, FLAG_MASK, U_BITS, V_SHIFT,
};
use crate::gpu::pipeline::raster_pipeline::raster_quad::{RasterQuad, SpunRasterQuad};
use crate::gpu::pipeline::shader_body::{ShaderBody, ShaderConstant};
use crate::gpu::resource::texture_binding::{self, Filtering, TextureBinding};
use crate::primitives::wgsl;

/// The two pipeline pairs one swapchain format draws raster quads with.
#[derive(Debug)]
pub(crate) struct RasterVariants {
    /// Unturned quads, drawn texel for texel.
    pub(crate) flat: ClipVariant,
    /// [`SpunRasterQuad`]s, turned and filtered.
    pub(crate) spun: ClipVariant,
}

impl RasterVariants {
    /// No pipeline built.
    pub(crate) const NONE: Self = Self {
        flat: ClipVariant::NONE,
        spun: ClipVariant::NONE,
    };
}

/// The pipelines one raster draw binds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RasterPipelinePair<'a> {
    pub(crate) flat: &'a Pipeline,
    pub(crate) spun: &'a Pipeline,
}

/// Shader and group-0 layout, built once and lent to every raster tenant. Atlases keep clones of the layout
/// (a refcount bump), so one pipeline binds against either atlas without relying on structural compatibility.
#[derive(Debug)]
pub(crate) struct RasterPipeline {
    shader: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
}

impl RasterPipeline {
    /// This family's shader body.
    pub(crate) const SHADER: ShaderBody = ShaderBody {
        label: "palantir.raster.shader",
        wgsl: include_str!("shader.wgsl"),
        includes: &[wgsl::FRAME],
        constants: SHADER_CONSTANTS,
    };

    pub(crate) fn new(gpu: &Gpu, textures: &TextureBinding) -> Self {
        let layout = Self::create_layout(gpu);
        // The frame group, then the atlas.
        let pipeline_layout = pipeline_recipe::pipeline_layout(
            gpu,
            "palantir.raster.pl",
            &[Some(textures.frame()), Some(&layout)],
        );
        Self {
            shader: Self::SHADER.module(gpu),
            layout,
            pipeline_layout,
        }
    }

    pub(crate) const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Build the halves of `variants` `target` binds, where they are not built: the only
    /// format-dependent part here, for unturned quads, and for the [`SpunRasterQuad`]s a paint
    /// animation turns.
    pub(crate) fn ensure(&self, variants: &mut RasterVariants, gpu: &Gpu, target: VariantTarget) {
        variants.flat.ensure(
            gpu,
            target.clipping,
            &PipelineRecipe {
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
        variants.spun.ensure(
            gpu,
            target.clipping,
            &PipelineRecipe {
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

    /// The atlas group, [`ATLAS_GROUP`]: mask at 0, color at 1, no sampler (the shader reads each
    /// texel by index).
    fn create_layout(gpu: &Gpu) -> wgpu::BindGroupLayout {
        gpu.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.raster.atlas layout"),
                entries: &[
                    texture_binding::texture_entry(0, Filtering::Filtered),
                    texture_binding::texture_entry(1, Filtering::Filtered),
                ],
            })
    }
}

/// The group an atlas binds at, past the frame group.
pub(crate) const ATLAS_GROUP: u32 = 1;

/// The numbers [`RasterPipeline::SHADER`] takes from the Rust types that own them.
const SHADER_CONSTANTS: &[ShaderConstant] = &[
    ShaderConstant::uint("ATLAS_GROUP", ATLAS_GROUP),
    ShaderConstant::uint("U_BITS", U_BITS),
    ShaderConstant::uint("V_SHIFT", V_SHIFT),
    ShaderConstant::uint("FLAG_MASK", FLAG_MASK),
    ShaderConstant::uint("FLAG_DESATURATE", FLAG_DESATURATE),
    ShaderConstant::uint("FLAG_COLOR", FLAG_COLOR),
];
