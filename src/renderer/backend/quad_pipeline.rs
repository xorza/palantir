//! GPU side of quads — wgpu pipeline + viewport uniform + instance
//! buffer. Consumes `&[Quad]` (defined frontend-side) and binds the
//! shader at `quad.wgsl` next to this file.

use crate::layout::types::span::Span;
use crate::renderer::gpu::quad::Quad;
use encase::{ShaderSize, ShaderType, UniformBuffer};
use glam::Vec2;
use wgpu::util::DeviceExt;

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

pub(crate) struct QuadPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    viewport_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    /// Lazy stencil-aware pipeline variants — built on first need
    /// (first frame with `FrameOutput::has_rounded_clip == true`) so
    /// apps that never round-clip pay nothing. Once built, kept
    /// indefinitely.
    stencil: Option<StencilPipelines>,
    /// Lazy buffer holding one `Quad` per rounded clip in the current
    /// frame; uploaded by `upload_masks`, drawn by `draw_mask`. Reused
    /// across frames; capacity grows monotonically.
    mask_buffer: Option<wgpu::Buffer>,
    mask_capacity: usize,
    /// Cached creation inputs needed to lazy-build `stencil` later.
    shader: wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    bind_layout: wgpu::BindGroupLayout,
}

/// Two pipelines built atop the same shader + viewport bind group as
/// the no-stencil `pipeline`, used in the stencil-attached render pass.
///
/// - `mask_write` paints the rounded SDF shape into the stencil buffer
///   only — color writes disabled — replacing stencil at the masked
///   pixels with `stencil_reference`. Used once per rounded-clipped
///   draw group before its color draws to "stamp" the mask.
/// - `stencil_test` is the regular SDF quad pipeline plus a
///   stencil-test op (`compare = Equal`) so color writes only land on
///   pixels whose stencil matches the active reference. Used for every
///   color draw in the stencil-attached pass — non-rounded groups run
///   it at `stencil_reference = 0`, which always passes against the
///   cleared stencil.
pub(crate) struct StencilPipelines {
    mask_write: wgpu::RenderPipeline,
    stencil_test: wgpu::RenderPipeline,
}

impl QuadPipeline {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
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
            contents: &ViewportUniform { size: Vec2::ZERO }.encode(),
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
                4 => Float32x4,   // stroke.color
                5 => Float32,     // stroke.width
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
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
            stencil: None,
            mask_buffer: None,
            mask_capacity: 0,
            shader,
            color_format: format,
            bind_layout,
        }
    }

    /// Lazy-build the stencil-aware variants. Idempotent; called from
    /// the rounded-clip render path before the first `set_pipeline`.
    pub(crate) fn ensure_stencil(&mut self, device: &wgpu::Device) -> &StencilPipelines {
        if self.stencil.is_none() {
            self.stencil = Some(self.build_stencil_pipelines(device));
        }
        self.stencil.as_ref().expect("just-built above")
    }

    fn build_stencil_pipelines(&self, device: &wgpu::Device) -> StencilPipelines {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("palantir.quad.pl.stencil"),
            bind_group_layouts: &[Some(&self.bind_layout)],
            immediate_size: 0,
        });
        // Both stencil pipelines share the same instance layout — same
        // attributes as the existing `pipeline`. Build it once and pass
        // by reference to each pipeline descriptor.
        let instance_layout_a = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Quad>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2,
                1 => Float32x2,
                2 => Float32x4,
                3 => Float32x4,
                4 => Float32x4,
                5 => Float32,
            ],
        };

        // Mask-write face: stencil compare = Always, op = Replace
        // (writes the dynamic `stencil_reference` to every pixel the
        // SDF would shade). Color writes off → no fragment lighting.
        let mask_face = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Always,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::Replace,
        };
        let mask_write = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("palantir.quad.pipeline.mask"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&instance_layout_a),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.color_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::empty(),
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: super::STENCIL_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState {
                    front: mask_face,
                    back: mask_face,
                    read_mask: 0xff,
                    write_mask: 0xff,
                },
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Stencil-test face: compare Equal against the active
        // reference. Cleared stencil = 0; non-rounded groups draw at
        // ref=0 (always pass); rounded groups draw at ref=N after a
        // mask write established stencil=N inside the rounded shape.
        let test_face = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Equal,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::Keep,
        };
        let stencil_test = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("palantir.quad.pipeline.stencil_test"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&instance_layout_a),
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
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: super::STENCIL_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState {
                    front: test_face,
                    back: test_face,
                    read_mask: 0xff,
                    write_mask: 0x00,
                },
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        StencilPipelines {
            mask_write,
            stencil_test,
        }
    }

    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: Vec2,
        quads: &[Quad],
    ) {
        queue.write_buffer(
            &self.viewport_buffer,
            0,
            &ViewportUniform { size: viewport }.encode(),
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

    /// Bind pipeline + viewport bind group + instance buffer once per
    /// pass. Call before the per-group `draw_range` loop so we don't
    /// re-issue these every group.
    pub(crate) fn bind<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
    }

    /// Draw a contiguous slice of the uploaded instance buffer. Used to
    /// segment quads by scissor region; caller is responsible for
    /// calling [`Self::bind`] once and setting
    /// `RenderPass::set_scissor_rect` before each call.
    pub(crate) fn draw_range(&self, pass: &mut wgpu::RenderPass<'_>, instances: Span) {
        if instances.len == 0 {
            return;
        }
        pass.draw(0..4, instances.into());
    }

    /// Upload `masks` (one `Quad` per rounded clip in the frame) to the
    /// stencil-mask vertex buffer. Grows the buffer to the next power
    /// of two when needed.
    pub(crate) fn upload_masks(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        masks: &[Quad],
    ) {
        if masks.is_empty() {
            return;
        }
        if masks.len() > self.mask_capacity {
            self.mask_capacity = masks.len().next_power_of_two().max(8);
            self.mask_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.quad.masks"),
                size: (self.mask_capacity * std::mem::size_of::<Quad>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let buf = self
            .mask_buffer
            .as_ref()
            .expect("mask_buffer just allocated");
        queue.write_buffer(buf, 0, bytemuck::cast_slice(masks));
    }

    /// Bind the stencil-test (color) pipeline + main instance buffer.
    /// Used once before the per-group draw loop in the stencil path.
    pub(crate) fn bind_stencil_test<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let stencil = self.stencil.as_ref().expect("ensure_stencil first");
        pass.set_pipeline(&stencil.stencil_test);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
    }

    /// Bind the mask-write pipeline + mask instance buffer. Caller sets
    /// `stencil_reference` per draw (1 to write the mask, 0 to clear).
    pub(crate) fn bind_mask_write<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let stencil = self.stencil.as_ref().expect("ensure_stencil first");
        let buf = self.mask_buffer.as_ref().expect("upload_masks first");
        pass.set_pipeline(&stencil.mask_write);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, buf.slice(..));
    }

    /// Draw the single mask `Quad` at `mask_idx` in the mask buffer.
    pub(crate) fn draw_mask(&self, pass: &mut wgpu::RenderPass<'_>, mask_idx: u32) {
        pass.draw(0..4, mask_idx..mask_idx + 1);
    }
}
