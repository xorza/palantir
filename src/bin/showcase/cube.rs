//! Raw-wgpu demo: a slowly auto-rotating colored cube rendered through a
//! [`GpuView`]. Drag inside the view to orbit it. Shows the full
//! `GpuPaint` surface — lazy `init`, per-frame `paint` into the
//! framework-owned target, a private depth buffer recreated on resize,
//! and continuous repaint driving the animation.

use std::cell::RefCell;
use std::rc::Rc;

use aperture::{
    Configure, GpuFrameCtx, GpuInitCtx, GpuPaint, GpuView, Panel, Sense, Sizing, Text, Ui,
};
use glam::{Mat4, UVec2, Vec3};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    color: [f32; 3],
}

const fn v(pos: [f32; 3], color: [f32; 3]) -> Vertex {
    Vertex { pos, color }
}

/// Eight cube corners, each tinted by its position so faces read as
/// distinct colored gradients.
const VERTICES: [Vertex; 8] = [
    v([-1.0, -1.0, -1.0], [0.10, 0.10, 0.12]),
    v([1.0, -1.0, -1.0], [0.90, 0.20, 0.25]),
    v([1.0, 1.0, -1.0], [0.95, 0.80, 0.20]),
    v([-1.0, 1.0, -1.0], [0.20, 0.80, 0.35]),
    v([-1.0, -1.0, 1.0], [0.20, 0.45, 0.90]),
    v([1.0, -1.0, 1.0], [0.85, 0.30, 0.85]),
    v([1.0, 1.0, 1.0], [0.95, 0.95, 0.95]),
    v([-1.0, 1.0, 1.0], [0.25, 0.85, 0.90]),
];

/// 12 triangles. Culling is off, so winding doesn't matter — depth alone
/// resolves occlusion.
const INDICES: [u16; 36] = [
    0, 1, 2, 0, 2, 3, // -z
    4, 6, 5, 4, 7, 6, // +z
    4, 0, 3, 4, 3, 7, // -x
    1, 5, 6, 1, 6, 2, // +x
    3, 2, 6, 3, 6, 7, // +y
    4, 5, 1, 4, 1, 0, // -y
];

const SHADER: &str = r#"
struct Uniforms { mvp: mat4x4<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs(@location(0) pos: vec3<f32>, @location(1) color: vec3<f32>) -> VsOut {
    var out: VsOut;
    out.clip = u.mvp * vec4<f32>(pos, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// GPU resources, built lazily in [`GpuPaint::init`] (the device isn't
/// available before first paint).
struct CubeGpu {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Private depth attachment, recreated on resize.
    depth: Option<(wgpu::TextureView, UVec2)>,
}

/// The app's persistent renderer. `spin` accumulates the auto-rotation;
/// `yaw`/`pitch` are driven by drag. `'static` (no borrows) so it can
/// live behind the `Rc<RefCell<…>>` the framework holds across frames.
pub struct Cube {
    gpu: Option<CubeGpu>,
    spin: f32,
    yaw: f32,
    pitch: f32,
}

impl Cube {
    pub fn new() -> Self {
        Self {
            gpu: None,
            spin: 0.0,
            yaw: 0.6,
            pitch: 0.5,
        }
    }

    /// Apply a drag delta (logical px) to the orbit angles.
    fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * 0.01;
        self.pitch = (self.pitch + dy * 0.01).clamp(-1.5, 1.5);
    }
}

impl GpuPaint for Cube {
    fn init(&mut self, ctx: &GpuInitCtx<'_>) {
        let device = ctx.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("showcase.cube.shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("showcase.cube.vbuf"),
            contents: bytemuck::cast_slice(&VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("showcase.cube.ibuf"),
            contents: bytemuck::cast_slice(&INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("showcase.cube.mvp"),
            size: 64, // mat4x4<f32>
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("showcase.cube.bgl"),
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
            label: Some("showcase.cube.bg"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("showcase.cube.pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("showcase.cube.pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: ctx.target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        self.gpu = Some(CubeGpu {
            pipeline,
            vertices,
            indices,
            uniform,
            bind_group,
            depth: None,
        });
    }

    fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>) {
        // Slow auto-rotation, framerate-independent via real `dt`
        // (~34°/s). `dt` is ZERO on the first paint, so the cube simply
        // holds its initial pose that frame.
        self.spin += 0.6 * ctx.dt.as_secs_f32();
        let mvp = {
            let size = ctx.size_px.max(UVec2::ONE);
            let aspect = size.x as f32 / size.y as f32;
            // wgpu wants [0,1] clip depth (DirectX/Metal/Vulkan), so use the
            // `directx` RH perspective — the non-deprecated peer of the old
            // `Mat4::perspective_rh`.
            let proj = glam::camera::rh::proj::directx::perspective(
                45f32.to_radians(),
                aspect,
                0.1,
                100.0,
            );
            let view =
                glam::camera::rh::view::look_at_mat4(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
            let model =
                Mat4::from_rotation_y(self.spin + self.yaw) * Mat4::from_rotation_x(self.pitch);
            proj * view * model
        };

        let gpu = self.gpu.as_mut().expect("init ran before paint");
        // Depth matches the color target's size; recreate it when the target
        // is reallocated (i.e. when `size_px` changes — every frame the view
        // is resized).
        let need_depth = gpu.depth.as_ref().map(|(_, s)| *s) != Some(ctx.size_px);
        if need_depth {
            let tex = ctx.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("showcase.cube.depth"),
                size: wgpu::Extent3d {
                    width: ctx.size_px.x.max(1),
                    height: ctx.size_px.y.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            gpu.depth = Some((view, ctx.size_px));
        }
        let depth_view = &gpu.depth.as_ref().expect("depth just ensured").0;

        ctx.queue
            .write_buffer(&gpu.uniform, 0, bytemuck::cast_slice(&mvp.to_cols_array()));

        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("showcase.cube.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.04,
                        g: 0.04,
                        b: 0.06,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        // The target is sized exactly to the view; render into all of it.
        // Viewport sets the NDC→pixel transform; scissor is belt-and-braces.
        let (w, h) = (ctx.size_px.x.max(1), ctx.size_px.y.max(1));
        pass.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, w, h);
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, &gpu.bind_group, &[]);
        pass.set_vertex_buffer(0, gpu.vertices.slice(..));
        pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..INDICES.len() as u32, 0, 0..1);
    }
}

/// Showcase page. `cube` persists across frames in `State` (the device
/// isn't available at construction, so its GPU resources build lazily on
/// first paint).
pub fn build(ui: &mut Ui, cube: &Rc<RefCell<Cube>>) {
    // A GpuView re-renders on every painted frame, so keep frames coming to
    // animate the spin.
    ui.request_repaint();
    Panel::vstack()
        .auto_id()
        .gap(10.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new("Slowly rotating cube — drag inside the view to orbit it.")
                .auto_id()
                .show(ui);

            let paint: Rc<RefCell<dyn GpuPaint>> = cube.clone();
            // GpuView doesn't sense by default — opt into drag so the
            // returned `Response` reports the orbit delta.
            let resp = GpuView::new(paint)
                .sense(Sense::DRAG)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui);
            if let Some(delta) = resp.left.drag.delta() {
                cube.borrow_mut().orbit(delta.x * 0.05, delta.y * 0.05);
            }
        });
}
