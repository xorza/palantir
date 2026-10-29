//! The GPU half of the shadow cutout tables: the baked atlas, the bake pass, and the per-instance stream `vs_shadow` reads. The CPU half is [`CutoutPlan`], kept on each window's render stream.

use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::frame::binder::MainBinder;
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe, vertex_attr};
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{
    BakeTable, CornerTables, CutoutMode, CutoutPlan,
};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use pollster::FutureExt;
use std::mem::offset_of;

/// The atlas, the bake pipeline and this frame's tables.
#[derive(Debug)]
pub(crate) struct CutoutTables {
    /// [`CutoutMode::Shade`] on a device that cannot render into `R32Float` (GL without
    /// `EXT_color_buffer_float`): corners keep the shaded cutout and the atlas is a 1×1 stand-in.
    mode: CutoutMode,
    atlas: wgpu::TextureView,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    bake_pipeline: wgpu::RenderPipeline,
    tables: DynamicBuffer<BakeTable>,
    corners: DynamicBuffer<CornerTables>,
}

impl CutoutTables {
    /// The atlas texel format: one `f32` cutout, read by `textureLoad`.
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

    /// The tables for the quad `shader` (`vs_cutout_bake`, `fs_cutout_bake`).
    pub(crate) fn new(device: &wgpu::Device, shader: &wgpu::ShaderModule) -> Self {
        let mode = if Self::renders_atlas(device) {
            CutoutMode::Bake
        } else {
            CutoutMode::Shade
        };
        let (side, usage) = match mode {
            CutoutMode::Bake => (
                CutoutPlan::ATLAS_SIZE,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
            CutoutMode::Shade => (1, wgpu::TextureUsages::TEXTURE_BINDING),
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
        let bake_layout = pipeline_recipe::pipeline_layout(device, "palantir.quad.cutouts.pl", &[]);
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
                Self::FORMAT,
            )
        }
        .build(device);
        Self {
            mode,
            atlas,
            layout,
            bind_group,
            bake_pipeline,
            tables: DynamicBuffer::vertex(device, "palantir.quad.cutouts.tables", 16),
            corners: DynamicBuffer::vertex(device, "palantir.quad.cutouts.corners", 256),
        }
    }

    /// Whether `device` renders into the atlas format. WebGPU guarantees it; on a downlevel device
    /// wgpu checks a texture's usage against what the adapter reports (`wgpu-core`
    /// `Device::describe_format_features`). A device does not report those features, and the
    /// adapter of a device the app opened is not at hand, so a texture made under an error scope
    /// asks.
    fn renders_atlas(device: &wgpu::Device) -> bool {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let _ = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.quad.cutouts.probe"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        scope.pop().block_on().is_none()
    }

    /// How this device cuts shadow corners; what every window's [`CutoutPlan`] is built with.
    pub(crate) const fn mode(&self) -> CutoutMode {
        self.mode
    }

    /// Group 1 of the shadow pipeline: the atlas.
    pub(crate) const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The per-instance stream `vs_shadow` reads at location 9, parallel to the quads.
    pub(crate) const CORNER_LAYOUT: wgpu::VertexBufferLayout<'static> =
        pipeline_recipe::vertex_layout(
            size_of::<CornerTables>(),
            wgpu::VertexStepMode::Instance,
            &CORNER_ATTRS,
        );

    /// Upload `plan`'s corner tables and bake its tables on `ctx`'s encoder before the shadow pass.
    pub(crate) fn prepare(&mut self, ctx: &mut GpuCtx<'_>, plan: &CutoutPlan) {
        self.corners.upload_instances(ctx, plan.corners());
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
                // Every texel a repainted pixel reads is baked this frame, so keep nothing: a load would pull the atlas into tile memory.
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

    /// Bind the atlas and the corner stream for a shadow draw.
    pub(crate) fn bind<'a>(&'a self, binder: &mut MainBinder<'a>, pass: &mut wgpu::RenderPass<'a>) {
        binder.group(pass, 1, &self.bind_group);
        binder.vertex(pass, 1, &self.corners.buffer);
    }
}

const BAKE_ATTRS: [wgpu::VertexAttribute; 5] = [
    vertex_attr(0, wgpu::VertexFormat::Uint32, offset_of!(BakeTable, table)),
    vertex_attr(1, wgpu::VertexFormat::Float32, offset_of!(BakeTable, r)),
    vertex_attr(2, wgpu::VertexFormat::Float32, offset_of!(BakeTable, sigma)),
    vertex_attr(3, wgpu::VertexFormat::Uint16x2, offset_of!(BakeTable, lo)),
    vertex_attr(4, wgpu::VertexFormat::Uint16x2, offset_of!(BakeTable, hi)),
];

const BAKE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<BakeTable>(),
    wgpu::VertexStepMode::Instance,
    &BAKE_ATTRS,
);

const CORNER_ATTRS: [wgpu::VertexAttribute; 1] = [vertex_attr(
    9,
    wgpu::VertexFormat::Uint32x4,
    offset_of!(CornerTables, 0),
)];

#[cfg(test)]
mod tests {
    use crate::gpu::pipeline::quad_pipeline::cutout_tables::CutoutTables;
    use crate::gpu::test_gpu::headless_test_gpu;

    /// A device that meets WebGPU's formats renders into the atlas, so its shadows bake; the
    /// probe leaves no error behind for the device's handler.
    #[test]
    fn a_webgpu_device_renders_the_atlas() {
        let gpu = headless_test_gpu();
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        assert!(CutoutTables::renders_atlas(&gpu.device));
        assert!(pollster::block_on(scope.pop()).is_none());
    }
}
