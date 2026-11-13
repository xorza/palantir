//! The GPU half of the shadow cutout tables: the bake into the pool's mask pages and the
//! per-instance stream `vs_shadow` reads; the shadows read the pages through the atlas group. The CPU half is
//! [`CutoutPlan`], kept on each window's render stream, which places its tables through the
//! shared [`CutoutPlacements`].

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_textures::{self, AtlasTextures};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::atlas_binding::ATLAS_GROUP;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::quad_pipeline::cutout_placements::CutoutPlacements;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{BakeTable, CornerTables, CutoutPlan};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::gpu::resource::pipeline_recipe::{self, PipelineRecipe, vertex_attr};
use bytemuck::{Pod, Zeroable};
use std::mem::offset_of;

/// The bake pipeline and this frame's tables. Where each key's table sits is the backend's
/// [`CutoutPlacements`], a map with no device.
#[derive(Debug)]
pub(crate) struct CutoutTables {
    bake_pipeline: wgpu::RenderPipeline,
    tables: DynamicBuffer<BakeInstance>,
    corners: DynamicBuffer<CornerTables>,
    /// This frame's bakes, retained.
    bakes: Vec<BakeTable>,
    /// [`Self::bakes`] as drawn, by page.
    instances: Vec<BakeInstance>,
}

/// One table a bake draws, with the edge of the page it lands on, which maps its texels to clip
/// space.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BakeInstance {
    table: BakeTable,
    page_edge: f32,
}

impl BakeInstance {
    const fn page(&self) -> usize {
        CutoutPlan::table_page(self.table.table)
    }
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

    /// Bake [`Self::bakes`] into their pages: a pass a page, which loads what the page holds
    /// around them, as the draws cover only their spans.
    fn bake(&mut self, ctx: &mut GpuCtx<'_>, textures: &AtlasTextures) {
        if self.bakes.is_empty() {
            return;
        }
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
                page_edge: page_texture(page).width() as f32,
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

    /// Bind `atlas`, the atlas group, and the corner stream for a shadow draw.
    pub(crate) fn bind<'a>(&'a self, rec: &mut PassRecorder<'a>, atlas: &'a wgpu::BindGroup) {
        rec.group(ATLAS_GROUP, atlas);
        rec.vertex(1, &self.corners.buffer);
    }
}

const BAKE_ATTRS: [wgpu::VertexAttribute; 6] = [
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
        wgpu::VertexFormat::Float32,
        offset_of!(BakeInstance, page_edge),
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

#[cfg(test)]
mod tests;
