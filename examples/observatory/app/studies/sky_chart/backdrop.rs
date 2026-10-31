//! [`SkyBackdrop`]: the night sky under the chart, drawn with raw wgpu into a `GpuView`.

#![expect(
    clippy::disallowed_types,
    reason = "the `GpuView` escape hatch is raw wgpu by definition, so naming its types is the point"
)]

use palantir::glam::Vec2;
use palantir::{GpuFrameCtx, GpuInitCtx, GpuPaint};

/// The shader: a full-screen triangle that reads each fragment's galactic latitude.
pub(crate) const SHADER: &str = include_str!("sky.wgsl");

/// What the shader reads, as `sky.wgsl`'s `Sky` lays it out.
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub(crate) struct SkyUniform {
    pub(crate) view_min: [f32; 2],
    pub(crate) view_size: [f32; 2],
    pub(crate) chart: [f32; 2],
    pub(crate) night: f32,
    pad: f32,
}

impl SkyUniform {
    /// The part of the chart from `min` across `size` in view, in chart pixels.
    pub(crate) const fn new(min: Vec2, size: Vec2, chart: Vec2, night: bool) -> Self {
        Self {
            view_min: min.to_array(),
            view_size: size.to_array(),
            chart: chart.to_array(),
            night: if night { 1.0 } else { 0.0 },
            pad: 0.0,
        }
    }
}

#[derive(Debug)]
struct Gpu {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// The backdrop's GPU half, built on its first paint, and the view it draws next.
#[derive(Debug, Default)]
pub(crate) struct SkyBackdrop {
    gpu: Option<Gpu>,
    pub(crate) sky: SkyUniform,
}

impl GpuPaint for SkyBackdrop {
    fn init(&mut self, ctx: &GpuInitCtx<'_>) {
        let device = ctx.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("observatory.sky.shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("observatory.sky.uniform"),
            size: size_of::<SkyUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("observatory.sky.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("observatory.sky.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("observatory.sky.pl"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("observatory.sky.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: ctx.target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        self.gpu = Some(Gpu {
            pipeline,
            uniform,
            bind_group,
        });
    }

    fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>) {
        let gpu = self.gpu.as_ref().expect("init ran before paint");
        ctx.queue
            .write_buffer(&gpu.uniform, 0, bytemuck::bytes_of(&self.sky));
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("observatory.sky.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let (w, h) = (ctx.physical_size.x.max(1), ctx.physical_size.y.max(1));
        pass.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, w, h);
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, &gpu.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use naga::front::wgsl;
    use naga::valid::{Capabilities, ValidationFlags, Validator};

    use crate::app::studies::sky_chart::backdrop::{SHADER, SkyUniform};

    /// The shader parses and validates as WGSL, and the uniform is the 32 bytes its `Sky` holds.
    #[test]
    fn the_shader_validates_and_the_uniform_matches_it() {
        let module = wgsl::parse_str(SHADER).unwrap_or_else(|error| {
            panic!("{}", error.emit_to_string(SHADER));
        });
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&module)
            .expect("the sky shader validates");
        assert_eq!(size_of::<SkyUniform>(), 32);
    }
}
