//! The GPU half of the shadow cutout tables: the atlas they are baked into,
//! the bake pass, and the per-instance stream `vs_shadow` reads beside the
//! quads. The CPU half, which tables and where, is [`CutoutPlan`], which
//! each window keeps, since its census describes that window's frames.

use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{
    BakeTable, CornerTables, CutoutPlan, ShadowEntry,
};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use std::slice;

/// The atlas, the bake pipeline and this frame's tables.
#[derive(Debug)]
pub(crate) struct CutoutTables {
    /// Whether tables can be baked at all. Not on GL, where `R32Float` is a
    /// render target only with `EXT_color_buffer_float`: every corner keeps
    /// the shaded cutout, and the atlas is a 1×1 stand-in the bind group
    /// still needs.
    bake: bool,
    atlas: wgpu::TextureView,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    bake_pipeline: wgpu::RenderPipeline,
    tables: DynamicBuffer<BakeTable>,
    corners: DynamicBuffer<CornerTables>,
    /// The uploaded plan's [`ShadowEntry`] per quad, beside `corners`.
    entries: Vec<ShadowEntry>,
}

impl CutoutTables {
    /// The atlas's texel format: one `f32` cutout per texel, read by
    /// `textureLoad` and filtered in the shader.
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

    /// The tables for the quad `shader`, which holds `vs_cutout_bake` and
    /// `fs_cutout_bake`.
    pub(crate) fn new(device: &wgpu::Device, shader: &wgpu::ShaderModule) -> Self {
        let bake = device.adapter_info().backend != wgpu::Backend::Gl;
        let side = if bake { CutoutPlan::ATLAS_SIZE } else { 1 };
        let usage = if bake {
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT
        } else {
            wgpu::TextureUsages::TEXTURE_BINDING
        };
        let atlas = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("palantir.quad.cutouts"),
                size: wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: Self::FORMAT,
                usage,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.quad.cutouts.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.quad.cutouts.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&atlas),
            }],
        });
        let bake_layout = PipelineRecipe::pipeline_layout(device, "palantir.quad.cutouts.pl", &[]);
        let bake_buffer = Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<BakeTable>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &BAKE_ATTRS,
        });
        let bake_pipeline = PipelineRecipe {
            label: "palantir.quad.cutouts.bake",
            shader,
            layout: &bake_layout,
            vertex_entry: "vs_cutout_bake",
            constants: &[],
            vertex_buffers: slice::from_ref(&bake_buffer),
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            color_format: Self::FORMAT,
            fragment_entry: "fs_cutout_bake",
            color_writes: wgpu::ColorWrites::RED,
            blend: None,
            depth_stencil: None,
        }
        .build(device);
        Self {
            bake,
            atlas,
            layout,
            bind_group,
            bake_pipeline,
            tables: DynamicBuffer::vertex(device, "palantir.quad.cutouts.tables", 16),
            corners: DynamicBuffer::vertex(device, "palantir.quad.cutouts.corners", 256),
            entries: Vec::new(),
        }
    }

    /// Whether this device can bake tables: what every window's
    /// [`CutoutPlan`] is built with.
    pub(crate) const fn bakes(&self) -> bool {
        self.bake
    }

    /// Group 1 of the shadow pipeline: the atlas.
    pub(crate) const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The per-instance stream `vs_shadow` reads at location 9, parallel
    /// to the quads.
    pub(crate) const fn corner_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<CornerTables>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &CORNER_ATTRS,
        }
    }

    /// Upload `plan`'s corner tables and bake its tables on `ctx`'s
    /// encoder, ahead of the pass that draws the shadows.
    pub(crate) fn prepare(&mut self, ctx: &mut GpuCtx<'_>, plan: &CutoutPlan) {
        self.corners.upload_instances(ctx, plan.corners());
        self.entries.clear();
        self.entries.extend_from_slice(plan.entries());
        let tables = plan.tables();
        if tables.is_empty() {
            return;
        }
        self.tables.upload_instances(ctx, tables);
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("palantir.quad.cutouts.bake"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.atlas,
                depth_slice: None,
                resolve_target: None,
                // Every texel a repainted pixel reads is baked this frame;
                // the rest is never read. So the pass keeps nothing: a load
                // would read the whole atlas into tile memory, which on a
                // tiler costs as much as a frame's shading.
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
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
        pass.draw(0..4, 0..tables.len() as u32);
    }

    /// Each quad's [`ShadowEntry`] this frame, parallel to the quads.
    pub(crate) fn entries(&self) -> &[ShadowEntry] {
        &self.entries
    }

    /// Bind the atlas and the corner stream for a shadow draw.
    pub(crate) fn bind<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_bind_group(1, &self.bind_group, &[]);
        pass.set_vertex_buffer(1, self.corners.buffer.slice(..));
    }
}

const BAKE_ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Uint32,
    1 => Float32,
    2 => Float32,
    3 => Uint16x2,
    4 => Uint16x2,
];

const CORNER_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![9 => Uint32x4];

// The bake attributes must sit where `BakeTable` keeps its fields, as
// `QUAD_INSTANCE_ATTRS` checks for `Quad`.
const _: () = {
    use std::mem::offset_of;
    assert!(BAKE_ATTRS[0].offset == offset_of!(BakeTable, table) as u64);
    assert!(BAKE_ATTRS[1].offset == offset_of!(BakeTable, r) as u64);
    assert!(BAKE_ATTRS[2].offset == offset_of!(BakeTable, sigma) as u64);
    assert!(BAKE_ATTRS[3].offset == offset_of!(BakeTable, lo) as u64);
    assert!(BAKE_ATTRS[4].offset == offset_of!(BakeTable, hi) as u64);
};
