//! GPU side of user-supplied colored triangle meshes. Mirrors
//! [`super::quad_pipeline::QuadPipeline`] but draws indexed
//! triangle lists with per-vertex pos+color instead of per-instance
//! quads. Tint is folded into vertex colors at compose time, so this
//! pipeline carries no per-draw uniform beyond the shared viewport.

use crate::primitives::mesh::MeshVertex;
use encase::{ShaderSize, ShaderType, UniformBuffer};
use glam::Vec2;

#[derive(Copy, Clone, Debug, ShaderType)]
struct ViewportUniform {
    size: Vec2,
}

impl ViewportUniform {
    const BYTES: usize = Self::SHADER_SIZE.get() as usize;

    fn encode(&self) -> [u8; Self::BYTES] {
        let mut out = [0u8; Self::BYTES];
        UniformBuffer::new(&mut out[..]).write(self).unwrap();
        out
    }
}

pub(crate) struct MeshPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    viewport_buffer: wgpu::Buffer,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: usize,
    index_buffer: wgpu::Buffer,
    index_capacity: usize,
    stencil_test: Option<wgpu::RenderPipeline>,
    shader: wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    bind_layout: wgpu::BindGroupLayout,
}

impl MeshPipeline {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.mesh.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.mesh.bgl"),
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

        use wgpu::util::DeviceExt;
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("palantir.mesh.viewport"),
            contents: &ViewportUniform { size: Vec2::ZERO }.encode(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.mesh.bg"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("palantir.mesh.pl"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

        let vertex_layout = mesh_vertex_layout();

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("palantir.mesh.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[vertex_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let vertex_capacity = 256;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.mesh.vertices"),
            size: (vertex_capacity * std::mem::size_of::<MeshVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_capacity = 1024;
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.mesh.indices"),
            size: (index_capacity * std::mem::size_of::<u16>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group,
            viewport_buffer,
            vertex_buffer,
            vertex_capacity,
            index_buffer,
            index_capacity,
            stencil_test: None,
            shader,
            color_format: format,
            bind_layout,
        }
    }

    /// Lazy-build the stencil-test variant for rounded-clip frames.
    pub(crate) fn ensure_stencil(&mut self, device: &wgpu::Device) {
        if self.stencil_test.is_some() {
            return;
        }
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("palantir.mesh.pl.stencil"),
            bind_group_layouts: &[Some(&self.bind_layout)],
            immediate_size: 0,
        });
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("palantir.mesh.pipeline.stencil_test"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[mesh_vertex_layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.color_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(super::stencil_test_state()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        self.stencil_test = Some(pipe);
    }

    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: Vec2,
        vertices: &[MeshVertex],
        indices: &[u16],
    ) {
        queue.write_buffer(
            &self.viewport_buffer,
            0,
            &ViewportUniform { size: viewport }.encode(),
        );

        if vertices.is_empty() || indices.is_empty() {
            return;
        }

        if vertices.len() > self.vertex_capacity {
            self.vertex_capacity = vertices.len().next_power_of_two().max(64);
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.mesh.vertices"),
                size: (self.vertex_capacity * std::mem::size_of::<MeshVertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(vertices));

        // The index buffer's binding stride is 2 bytes (u16). wgpu
        // requires copy size to be a multiple of 4 (COPY_BUFFER_ALIGNMENT),
        // so pad the upload to an even count.
        let padded = (indices.len() + 1) & !1;
        if padded > self.index_capacity {
            self.index_capacity = padded.next_power_of_two().max(256);
            self.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.mesh.indices"),
                size: (self.index_capacity * std::mem::size_of::<u16>()) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if indices.len() == padded {
            queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(indices));
        } else {
            // Odd length: copy into a small stack-bounded scratch is
            // overkill; just write the even prefix + the trailing
            // single u16 separately.
            queue.write_buffer(
                &self.index_buffer,
                0,
                bytemuck::cast_slice(&indices[..indices.len() - 1]),
            );
            let tail = [indices[indices.len() - 1], 0u16];
            queue.write_buffer(
                &self.index_buffer,
                ((indices.len() - 1) * std::mem::size_of::<u16>()) as u64,
                bytemuck::cast_slice(&tail),
            );
        }
    }

    /// Bind once per pass, before iterating `meshes` and issuing
    /// `draw_range` per group entry.
    pub(crate) fn bind<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, stencil: bool) {
        if stencil {
            let p = self.stencil_test.as_ref().expect("ensure_stencil first");
            pass.set_pipeline(p);
        } else {
            pass.set_pipeline(&self.pipeline);
        }
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
    }

    /// Issue one indexed draw for a single [`MeshDraw`](crate::renderer::render_buffer::MeshDraw).
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        index_range: std::ops::Range<u32>,
        base_vertex: i32,
    ) {
        if index_range.start == index_range.end {
            return;
        }
        pass.draw_indexed(index_range, base_vertex, 0..1);
    }
}

const MESH_VERTEX_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x4,
];

fn mesh_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<MeshVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &MESH_VERTEX_ATTRS,
    }
}
