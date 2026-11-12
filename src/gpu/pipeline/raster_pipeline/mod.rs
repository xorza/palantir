//! The GPU program every raster quad draws through, whichever tenant rasterized it: one shader
//! module and one atlas layout, so one pipeline pair per swapchain format, and [`RasterGroup`], the
//! one group over the pool's mask and color pages both tenants draw through. `gpu::raster`'s
//! `RasterPass` is the CPU half.

pub(crate) mod raster_quad;

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::page_list::MAX_PAGES;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::clip_variant::ClipVariant;
use crate::gpu::pipeline::instanced_pipeline::VariantTarget;
use crate::gpu::pipeline::pipeline_recipe::{self, Pipeline, PipelineRecipe};
use crate::gpu::pipeline::raster_pipeline::raster_quad::{
    FLAG_COLOR, FLAG_DESATURATE, FLAG_MASK, FLAG_PAGE, U_BITS, V_SHIFT,
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

/// The pipelines and the atlas group one raster draw binds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RasterPipelinePair<'a> {
    pub(crate) flat: &'a Pipeline,
    pub(crate) spun: &'a Pipeline,
    /// [`RasterGroup::group`].
    pub(crate) atlas: &'a wgpu::BindGroup,
}

/// Shader and atlas layout, built once for every raster tenant.
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

    /// The atlas group, [`ATLAS_GROUP`]: the mask pages at `0..MAX_PAGES`, the color pages after,
    /// no sampler (the shader reads each texel by index).
    fn create_layout(gpu: &Gpu) -> wgpu::BindGroupLayout {
        let entries: [wgpu::BindGroupLayoutEntry; 2 * MAX_PAGES] = std::array::from_fn(|binding| {
            texture_binding::texture_entry(binding as u32, Filtering::Filtered)
        });
        gpu.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.raster.atlas layout"),
                entries: &entries,
            })
    }
}

/// The atlas group both raster tenants draw through: the pool's mask pages, then its color pages,
/// made again where a page's view changes.
#[derive(Debug)]
pub(crate) struct RasterGroup {
    group: wgpu::BindGroup,
    /// The generations of the mask and the color pages `group` binds.
    bound: [u64; 2],
}

/// The page lists a [`RasterGroup`] binds, mask then color.
const RASTER_FORMATS: [AtlasFormat; 2] = [AtlasFormat::R8, AtlasFormat::Rgba8];

impl RasterGroup {
    pub(crate) fn new(gpu: &Gpu, program: &RasterPipeline, textures: &AtlasTextures) -> Self {
        Self {
            group: Self::bind(gpu, program, textures),
            bound: RASTER_FORMATS.map(|format| textures.generation(format)),
        }
    }

    /// Make the group again where `textures` changed a page view since it was made, after the
    /// pool's commit and before any draw of the frame.
    pub(crate) fn follow(&mut self, gpu: &Gpu, program: &RasterPipeline, textures: &AtlasTextures) {
        let bound = RASTER_FORMATS.map(|format| textures.generation(format));
        if bound != self.bound {
            self.group = Self::bind(gpu, program, textures);
            self.bound = bound;
        }
    }

    pub(crate) const fn group(&self) -> &wgpu::BindGroup {
        &self.group
    }

    fn bind(gpu: &Gpu, program: &RasterPipeline, textures: &AtlasTextures) -> wgpu::BindGroup {
        let views = RASTER_FORMATS.map(|format| textures.views(format));
        let entries: [wgpu::BindGroupEntry<'_>; 2 * MAX_PAGES] =
            std::array::from_fn(|binding| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: wgpu::BindingResource::TextureView(
                    views[binding / MAX_PAGES][binding % MAX_PAGES],
                ),
            });
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.raster.atlas"),
            layout: &program.layout,
            entries: &entries,
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
    ShaderConstant::uint("FLAG_PAGE", FLAG_PAGE),
    ShaderConstant::uint("FLAG_DESATURATE", FLAG_DESATURATE),
    ShaderConstant::uint("FLAG_COLOR", FLAG_COLOR),
];
