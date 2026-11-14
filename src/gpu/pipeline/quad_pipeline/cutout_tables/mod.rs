//! The GPU half of the shadow cutout tables: the bake into the pool's mask pages and the
//! per-instance stream `vs_shadow` reads; the shadows read the pages through the atlas group. The
//! CPU half is [`CutoutPlan`], kept on each window's render stream, which places its tables
//! through the shared [`CutoutPlacements`].

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_textures::{self, AtlasTextures};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::pass_traffic::PassTraffic;
use crate::gpu::pipeline::atlas_binding::ATLAS_GROUP;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::quad_pipeline::cutout_placements::CutoutPlacements;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{BakeTable, CornerTables, CutoutPlan};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::gpu::resource::pipeline_recipe::{self, PipelineRecipe, vertex_attr};
use crate::gpu::resource::shelf_packer::ShelfPacker;
use crate::gpu::resource::staging_target::StagingTarget;
use crate::primitives::geometry::urect::URect;
use bytemuck::{Pod, Zeroable};
use glam::{IVec2, UVec2, Vec2};
use std::mem::offset_of;

#[cfg(feature = "internals")]
pub(crate) mod bench;

/// The bake pipeline and this frame's tables. Where each key's table sits is the backend's
/// [`CutoutPlacements`], a map with no device.
#[derive(Debug)]
pub(crate) struct CutoutTables {
    bake_pipeline: wgpu::RenderPipeline,
    tables: DynamicBuffer<BakeInstance>,
    corners: DynamicBuffer<CornerTables>,
    /// This frame's bakes, retained.
    bakes: Vec<BakeTable>,
    /// [`Self::bakes`] as drawn: by page, or on a staging device by round.
    instances: Vec<BakeInstance>,
    /// What the device's passes move, which picks where a bake draws: onto its page, or, where a
    /// pass moves its whole target, into [`Self::staging`] and copied onto its page.
    traffic: PassTraffic,
    staging: StagingTarget,
    /// Places a round's bakes in [`Self::staging`].
    staging_packer: ShelfPacker,
    /// This frame's copies from [`Self::staging`], retained, by round.
    copies: Vec<StagedBake>,
}

/// One table a bake draws, with what turns a texel of its page into one of the target the pass
/// renders into, and that target's size, which map its texels to clip space.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BakeInstance {
    table: BakeTable,
    to_target: IVec2,
    target_size: Vec2,
}

impl BakeInstance {
    const fn page(&self) -> usize {
        CutoutPlan::table_page(self.table.table)
    }
}

/// One bake a staging device drew into its staging target: the round it was drawn in, the texels
/// it holds there, and where they land on their page.
#[derive(Clone, Copy, Debug)]
struct StagedBake {
    round: u32,
    from: URect,
    page: usize,
    at: UVec2,
}

impl CutoutTables {
    /// The tables for the quad `shader` (`vs_cutout_bake`, `fs_cutout_bake`).
    pub(crate) fn new(gpu: &Gpu, shader: &wgpu::ShaderModule) -> Self {
        let bake_layout = pipeline_recipe::pipeline_layout(gpu, "palantir.quad.cutouts.pl", &[]);
        let bake_pipeline = PipelineRecipe {
            vertex_entry: "vs_cutout_bake",
            vertex_buffers: &[Some(BAKE_LAYOUT)],
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            fragment_entry: "fs_cutout_bake",
            color_writes: wgpu::ColorWrites::RED,
            blend: None,
            ..PipelineRecipe::color(
                "palantir.quad.cutouts.bake",
                shader,
                &bake_layout,
                atlas_textures::texture_format(AtlasFormat::R8),
            )
        }
        .build(gpu)
        .pipeline;
        Self {
            bake_pipeline,
            tables: DynamicBuffer::vertex(gpu, "palantir.quad.cutouts.tables", 16),
            corners: DynamicBuffer::vertex(gpu, "palantir.quad.cutouts.corners", 256),
            bakes: Vec::new(),
            instances: Vec::new(),
            traffic: gpu.pass_traffic(),
            staging: StagingTarget::new(
                "palantir.quad.cutouts.staging",
                atlas_textures::texture_format(AtlasFormat::R8),
            ),
            staging_packer: ShelfPacker::new(0),
            copies: Vec::new(),
        }
    }

    /// The per-instance stream `vs_shadow` reads at location 9, parallel to the quads.
    pub(crate) const CORNER_LAYOUT: wgpu::VertexBufferLayout<'static> =
        pipeline_recipe::vertex_layout(
            size_of::<CornerTables>(),
            wgpu::VertexStepMode::Instance,
            &CORNER_ATTRS,
        );

    /// Upload `plan`'s corner tables and bake what of its tables the mask pages lack, as
    /// `placements` notes them baked, on `ctx`'s encoder after the pool's commit and before the
    /// shadow pass.
    pub(crate) fn prepare(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        plan: &CutoutPlan,
        placements: &mut CutoutPlacements,
        textures: &AtlasTextures,
    ) {
        self.corners.upload_instances(ctx, plan.corners());
        placements.bakes(plan.tables(), &mut self.bakes);
        self.bake(ctx, textures);
    }

    /// Bake [`Self::bakes`] into their pages, as the device's passes move their targets: a pass a
    /// page, which loads what the page holds around them, as the draws cover only their spans; or,
    /// where a pass moves its whole target, a pass over the staging target a round of them fits,
    /// and a copy each onto its page (`StagingTarget`). The staging steps down with the frames that
    /// bake nothing too.
    fn bake(&mut self, ctx: &mut GpuCtx<'_>, textures: &AtlasTextures) {
        let staged = match self.traffic {
            PassTraffic::Drawn => UVec2::ZERO,
            PassTraffic::WholeTarget => self.plan_staging(ctx.gpu),
        };
        self.staging.decay(ctx.gpu, ctx.frame, staged);
        if self.bakes.is_empty() {
            return;
        }
        match self.traffic {
            PassTraffic::Drawn => self.bake_onto_pages(ctx, textures),
            PassTraffic::WholeTarget => self.bake_staged(ctx, textures),
        }
    }

    /// Bake onto the pages, a pass each.
    fn bake_onto_pages(&mut self, ctx: &mut GpuCtx<'_>, textures: &AtlasTextures) {
        let page_texture = |page: usize| {
            textures
                .page_texture(AtlasFormat::R8, page)
                .expect("a placed table's page was committed")
        };
        self.instances.clear();
        self.instances.extend(self.bakes.iter().map(|&table| {
            let page = CutoutPlan::table_page(table.table);
            BakeInstance {
                table,
                to_target: IVec2::ZERO,
                target_size: Vec2::splat(page_texture(page).width() as f32),
            }
        }));
        self.instances.sort_unstable_by_key(BakeInstance::page);
        self.tables.upload_instances(ctx, &self.instances);
        let views = textures.views(AtlasFormat::R8);
        let mut start = 0;
        for run in self.instances.chunk_by(|a, b| a.page() == b.page()) {
            let end = start + run.len() as u32;
            let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palantir.quad.cutouts.bake"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: views[run[0].page()],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.bake_pipeline);
            pass.set_vertex_buffer(0, self.tables.buffer.slice(..));
            pass.draw(0..4, start..end);
            start = end;
        }
    }

    /// Place each bake in the staging target, in rounds that each fit the device's edge, into
    /// [`Self::copies`], and reserve the staging for the largest; the texels the frame draws there.
    fn plan_staging(&mut self, gpu: &Gpu) -> UVec2 {
        self.copies.clear();
        if self.bakes.is_empty() {
            return UVec2::ZERO;
        }
        let edge = gpu.max_texture_dimension().get();
        self.staging_packer
            .reset(u16::try_from(edge).unwrap_or(u16::MAX));
        let mut round = 0;
        let mut extent = UVec2::ZERO;
        for bake in &self.bakes {
            let size = bake.hi - bake.lo;
            let place = self.staging_packer.allocate(size).unwrap_or_else(|| {
                round += 1;
                self.staging_packer.clear();
                self.staging_packer
                    .allocate(size)
                    .expect("a table fits a mask page, which the device's edge holds")
            });
            let from = URect {
                min: place.as_uvec2(),
                size: size.as_uvec2(),
            };
            extent = extent.max(from.max());
            self.copies.push(StagedBake {
                round,
                from,
                page: CutoutPlan::table_page(bake.table),
                at: CutoutPlan::table_origin(bake.table) + bake.lo.as_uvec2(),
            });
        }
        self.staging.reserve(gpu, extent);
        extent
    }

    /// Bake into the staging target, a pass a round, each round's bakes copied onto their pages
    /// after its pass.
    fn bake_staged(&mut self, ctx: &mut GpuCtx<'_>, textures: &AtlasTextures) {
        let target_size = self.staging.size().as_vec2();
        self.instances.clear();
        self.instances
            .extend(
                self.bakes
                    .iter()
                    .zip(&self.copies)
                    .map(|(&table, copy)| BakeInstance {
                        table,
                        to_target: copy.from.min.as_ivec2() - copy.at.as_ivec2(),
                        target_size,
                    }),
            );
        self.tables.upload_instances(ctx, &self.instances);
        let mut start = 0;
        for round in self.copies.chunk_by(|a, b| a.round == b.round) {
            let end = start + round.len() as u32;
            {
                let mut pass = self.staging.begin_pass(ctx.encoder);
                pass.set_pipeline(&self.bake_pipeline);
                pass.set_vertex_buffer(0, self.tables.buffer.slice(..));
                pass.draw(0..4, start..end);
            }
            for copy in round {
                let page = textures
                    .page_texture(AtlasFormat::R8, copy.page)
                    .expect("a placed table's page was committed");
                self.staging.copy_to(ctx.encoder, copy.from, page, copy.at);
            }
            start = end;
        }
    }

    /// Bind `atlas`, the atlas group, and the corner stream for a shadow draw.
    pub(crate) fn bind<'a>(&'a self, rec: &mut PassRecorder<'a>, atlas: &'a wgpu::BindGroup) {
        rec.group(ATLAS_GROUP, atlas);
        rec.vertex(1, &self.corners.buffer);
    }
}

const BAKE_ATTRS: [wgpu::VertexAttribute; 7] = [
    vertex_attr(
        0,
        wgpu::VertexFormat::Uint32,
        offset_of!(BakeInstance, table.table),
    ),
    vertex_attr(
        1,
        wgpu::VertexFormat::Float32,
        offset_of!(BakeInstance, table.r),
    ),
    vertex_attr(
        2,
        wgpu::VertexFormat::Float32,
        offset_of!(BakeInstance, table.sigma),
    ),
    vertex_attr(
        3,
        wgpu::VertexFormat::Uint16x2,
        offset_of!(BakeInstance, table.lo),
    ),
    vertex_attr(
        4,
        wgpu::VertexFormat::Uint16x2,
        offset_of!(BakeInstance, table.hi),
    ),
    vertex_attr(
        5,
        wgpu::VertexFormat::Sint32x2,
        offset_of!(BakeInstance, to_target),
    ),
    vertex_attr(
        6,
        wgpu::VertexFormat::Float32x2,
        offset_of!(BakeInstance, target_size),
    ),
];

const BAKE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<BakeInstance>(),
    wgpu::VertexStepMode::Instance,
    &BAKE_ATTRS,
);

/// Past the quad's own lanes, `QUAD_INSTANCE_ATTRS`, as `vs_shadow` reads it.
/// The shadow pipeline's location past a shadow's own (`SHADOW_INSTANCE_ATTRS`).
const CORNER_ATTRS: [wgpu::VertexAttribute; 1] = [vertex_attr(
    9,
    wgpu::VertexFormat::Uint32x4,
    offset_of!(CornerTables, 0),
)];

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::pipeline::quad_pipeline::cutout_tables::CutoutTables;
    use crate::gpu::resource::staging_target::StagingTarget;

    impl CutoutTables {
        /// The target a device whose passes move their whole target bakes into.
        pub(crate) const fn staging(&self) -> &StagingTarget {
            &self.staging
        }
    }
}

#[cfg(test)]
mod tests;
