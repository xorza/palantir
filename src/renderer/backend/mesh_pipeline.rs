//! GPU side of user-supplied colored triangle meshes. Mirrors
//! [`super::quad_pipeline::QuadPipeline`] but draws indexed
//! triangle lists with per-vertex pos+color and per-instance
//! transform+tint. The vertex stream is content-stable across frames;
//! per-draw state lives in a parallel instance buffer.

use crate::primitives::mesh::MeshVertex;
use crate::renderer::render_buffer::MeshInstance;

pub(crate) struct MeshPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: usize,
    index_buffer: wgpu::Buffer,
    index_capacity: usize,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    stencil_test: Option<wgpu::RenderPipeline>,
    shader: wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    bind_layout: wgpu::BindGroupLayout,
}

impl MeshPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        viewport_buffer: &wgpu::Buffer,
    ) -> Self {
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

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("palantir.mesh.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[mesh_vertex_layout(), mesh_instance_layout()],
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
        let instance_capacity = 64;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.mesh.instances"),
            size: (instance_capacity * std::mem::size_of::<MeshInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group,
            vertex_buffer,
            vertex_capacity,
            index_buffer,
            index_capacity,
            instance_buffer,
            instance_capacity,
            stencil_test: None,
            shader,
            color_format: format,
            bind_layout,
        }
    }

    /// Lazy-build the stencil-test variant for rounded-clip frames.
    #[profiling::function]
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
                buffers: &[mesh_vertex_layout(), mesh_instance_layout()],
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

    #[profiling::function]
    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[MeshVertex],
        indices: &[u16],
        instances: &[MeshInstance],
    ) {
        if vertices.is_empty() || indices.is_empty() || instances.is_empty() {
            return;
        }

        if instances.len() > self.instance_capacity {
            self.instance_capacity = instances.len().next_power_of_two().max(16);
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.mesh.instances"),
                size: (self.instance_capacity * std::mem::size_of::<MeshInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));

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
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
    }

    /// Issue one indexed draw for a single [`MeshDraw`](crate::renderer::render_buffer::MeshDraw).
    /// `instance` indexes into the per-frame instance buffer for the
    /// matching transform + tint.
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        index_range: std::ops::Range<u32>,
        base_vertex: i32,
        instance: u32,
    ) {
        if index_range.start == index_range.end {
            return;
        }
        pass.draw_indexed(index_range, base_vertex, instance..instance + 1);
    }
}

const MESH_VERTEX_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
    0 => Float32x2,
    // `Unorm8x4` normalizes `u8/255 → 0..1` floats on the GPU. The
    // CPU side stores linear-u8 via the linear `From<Color> for
    // ColorU8` impl, so the shader sees linear values directly —
    // no decode, no banding worse than 1/255 (below display step).
    1 => Unorm8x4,
];

fn mesh_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<MeshVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &MESH_VERTEX_ATTRS,
    }
}

// `translate.xy : Float32x2`, `scale : Float32`, `tint : Unorm8x4`.
// Tint storage matches `MeshVertex.color` (linear-u8 premultiplied);
// shader multiplies per-fragment, no decode either side.
const MESH_INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
    2 => Float32x2,
    3 => Float32,
    4 => Unorm8x4,
];

fn mesh_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<MeshInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &MESH_INSTANCE_ATTRS,
    }
}
