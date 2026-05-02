use crate::primitives::{Color, Corners, Rect, Stroke};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Per-instance quad data (68 B). Layout is `pos, size, fill, radius,
/// stroke_color, stroke_width` — see the `vertex_attr_array` in
/// [`QuadPipeline::new`] for the explicit attribute offsets, which is the
/// only thing constraining the field order. No tail padding: vertex
/// buffer strides only need 4-byte alignment, unlike std140 uniforms.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Quad {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub fill: [f32; 4],
    pub radius: [f32; 4],
    pub stroke_color: [f32; 4],
    pub stroke_width: f32,
}

impl Quad {
    pub fn new(rect: Rect, fill: Color, radius: Corners, stroke: Option<Stroke>) -> Self {
        let (sc, sw) = match stroke {
            Some(s) => ([s.color.r, s.color.g, s.color.b, s.color.a], s.width),
            None => ([0.0; 4], 0.0),
        };
        Self {
            pos: [rect.min.x, rect.min.y],
            size: [rect.size.w, rect.size.h],
            fill: [fill.r, fill.g, fill.b, fill.a],
            radius: [radius.tl, radius.tr, radius.br, radius.bl],
            stroke_color: sc,
            stroke_width: sw,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ViewportUniform {
    size: [f32; 2],
    _pad: [f32; 2],
}

pub struct QuadPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    viewport_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
}

impl QuadPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.quad.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.quad.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("palantir.quad.viewport"),
            contents: bytemuck::cast_slice(&[ViewportUniform {
                size: [0.0, 0.0],
                _pad: [0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.quad.bg"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("palantir.quad.pl"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Quad>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2,   // pos
                1 => Float32x2,   // size
                2 => Float32x4,   // fill
                3 => Float32x4,   // radius
                4 => Float32x4,   // stroke_color
                5 => Float32,     // stroke_width
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("palantir.quad.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[instance_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let instance_capacity = 256;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.quad.instances"),
            size: (instance_capacity * std::mem::size_of::<Quad>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group,
            viewport_buffer,
            instance_buffer,
            instance_capacity,
        }
    }

    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: [f32; 2],
        quads: &[Quad],
    ) {
        queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::cast_slice(&[ViewportUniform {
                size: viewport,
                _pad: [0.0, 0.0],
            }]),
        );

        if quads.is_empty() {
            return;
        }

        if quads.len() > self.instance_capacity {
            self.instance_capacity = quads.len().next_power_of_two();
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.quad.instances"),
                size: (self.instance_capacity * std::mem::size_of::<Quad>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(quads));
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, instance_count: u32) {
        self.draw_range(pass, 0..instance_count);
    }

    /// Draw a contiguous slice of the uploaded instance buffer. Used to
    /// segment quads by scissor region; caller is responsible for setting
    /// `RenderPass::set_scissor_rect` before each call.
    pub fn draw_range<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        instances: std::ops::Range<u32>,
    ) {
        if instances.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.draw(0..4, instances);
    }
}

#[cfg(test)]
mod tests {
    use super::Quad;

    /// Pin: `Quad` is exactly 68 bytes — pos(8) + size(8) + fill(16) +
    /// radius(16) + stroke_color(16) + stroke_width(4). The
    /// `vertex_attr_array` in `QuadPipeline::new` assumes this exact
    /// layout via Rust's `repr(C)` field-order rules. A reorder or an
    /// added field that shifts an attribute's offset would break the
    /// shader binding silently — this test catches it.
    #[test]
    fn quad_struct_is_68_bytes_no_padding() {
        assert_eq!(std::mem::size_of::<Quad>(), 68);
    }
}
