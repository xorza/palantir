//! GPU side of quads — wgpu pipeline + viewport uniform + instance
//! buffer. Consumes `&[Quad]` (defined frontend-side) and binds the
//! shader at `quad.wgsl` next to this file.

use super::pipeline_utils::{
    PipelineRecipe, build_pipeline, build_pipeline_layout, grow_instance_buffer,
};
use crate::primitives::color::ColorF16;
use crate::primitives::span::Span;
use crate::primitives::{color::Color, corners::Corners, rect::Rect, size::Size};
use crate::renderer::gradient_atlas::GradientAtlas;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::DrawGroup;
use glam::Vec2;

pub(crate) struct QuadPipeline {
    /// The no-stencil base pipeline. Reached only via methods —
    /// `bind`, `draw_clear`, and `bind_debug` (the debug-overlay
    /// entrypoint) own the `set_pipeline` / `set_bind_group` pair so
    /// the public surface is "what to do", not "what to bind."
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    /// Lazy stencil-aware pipeline variants — built on first need
    /// (first frame with `FrameOutput::has_rounded_clip == true`) so
    /// apps that never round-clip pay nothing. Once built, kept
    /// indefinitely.
    stencil: Option<StencilPipelines>,
    /// Lazy buffer holding one `Quad` per rounded clip in the current
    /// frame; uploaded by `stage_masks`, drawn by `draw_mask`. Reused
    /// across frames; capacity grows monotonically.
    mask_buffer: Option<wgpu::Buffer>,
    mask_capacity: usize,
    /// Retained scratch for the stencil-mask sweep. `Some(j)` at index
    /// `i` says "group `i`'s mask is mask quad `j` in the upload
    /// buffer." Sized to `buffer.groups.len()` each frame; capacity
    /// retained across frames so steady-state runs alloc-free.
    /// Populated by [`Self::stage_masks`], read by the render schedule.
    /// Empty slice on non-stencil frames; the schedule only reads it
    /// when `use_stencil` is true.
    pub(crate) mask_indices: Vec<Option<u32>>,
    /// Retained scratch for stencil-mask quads. One entry per rounded
    /// clip group; uploaded to `mask_buffer`. Cleared at the start of
    /// each stencil frame; capacity retained.
    masks: Vec<Quad>,
    /// Single-instance buffer holding the partial-repaint pre-clear quad
    /// (full-viewport, opaque, clear color). Drawn before regular groups
    /// inside the damage scissor so `LoadOp::Load` doesn't leak last
    /// frame's AA-fringe pixels into this frame's blends.
    clear_buffer: wgpu::Buffer,
    /// Set true by [`Self::upload_clear`], reset by [`Self::post_record`].
    /// [`Self::draw_clear`] asserts it's true — catches a future
    /// refactor that decorrelates the upload guard in `submit` from
    /// the per-pass `PreClear` emit in the schedule.
    clear_buffer_dirty: bool,
    /// Cached creation inputs needed to lazy-build `stencil` later.
    shader: wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    bind_layout: wgpu::BindGroupLayout,
    /// LUT atlas texture for gradient brushes. 256 cols × 256 rows of
    /// `Rgba8UnormSrgb`. Sampled at fragment time by the brush-slot
    /// path in `quad.wgsl`; sRGB-format so the GPU sampler returns
    /// linear-RGB to match the existing premultiplied blend convention
    /// (see `CLAUDE.md` "Colour pipeline"). Uploaded each frame by
    /// `upload_gradients`, bound via the pipeline's bind group entry 1.
    gradient_texture: wgpu::Texture,
    /// Kept alive alongside `gradient_texture`: the bind group holds a
    /// borrow that has to stay valid as long as the pipeline can be
    /// drawn against. Not read directly — accessed via the bind group
    /// the GPU sees at draw time.
    #[allow(dead_code)]
    gradient_texture_view: wgpu::TextureView,
    /// Same: held by the bind group, not read directly.
    #[allow(dead_code)]
    gradient_sampler: wgpu::Sampler,
}

/// Side of the gradient LUT atlas texture (square: 256 × 256).
/// Must equal `ATLAS_ROWS_F` in `quad.wgsl` — the shader divides the
/// row index by this constant to compute the sample `v` coord.
const GRADIENT_ATLAS_SIDE: u32 = 256;
const _: () = assert!(
    GRADIENT_ATLAS_SIDE == 256,
    "shader ATLAS_ROWS_F is hardcoded to 256.0; update quad.wgsl if you change this"
);

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
struct StencilPipelines {
    mask_write: wgpu::RenderPipeline,
    stencil_test: wgpu::RenderPipeline,
}

impl QuadPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        viewport_buffer: &wgpu::Buffer,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.quad.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.quad.bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Gradient LUT atlas: sampled at fragment time once the
                // brush slot is wired into the shader (slice-2 step 3).
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let gradient_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.quad.gradient_atlas"),
            size: wgpu::Extent3d {
                width: GRADIENT_ATLAS_SIDE,
                height: GRADIENT_ATLAS_SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Linear format: sampler returns the stored bytes as
            // `u8/255` floats with no decode. The LUT bake quantizes
            // linear-RGB values directly to `ColorU8` via the linear
            // `From<Color>` impl, so the GPU sees ready-to-blend
            // linear values.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let gradient_texture_view = gradient_texture.create_view(&Default::default());
        // Linear filter inside a row (smooth gradient interpolation).
        // Clamp addressing — spread modes (Pad/Repeat/Reflect) are
        // applied shader-side on `t` before the sample, so the GPU
        // sampler never sees t outside 0..1.
        let gradient_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("palantir.quad.gradient_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.quad.bg"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: viewport_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&gradient_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&gradient_sampler),
                },
            ],
        });

        let pipeline_layout =
            build_pipeline_layout(device, "palantir.quad.pl", &[Some(&bind_layout)]);
        let pipeline = build_pipeline(
            device,
            PipelineRecipe {
                label: "palantir.quad.pipeline",
                shader: &shader,
                layout: &pipeline_layout,
                vertex_buffers: &[quad_instance_layout()],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                color_format: format,
                fragment_entry: "fs",
                color_writes: wgpu::ColorWrites::ALL,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                depth_stencil: None,
            },
        );

        let instance_capacity = 256;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.quad.instances"),
            size: (instance_capacity * std::mem::size_of::<Quad>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let clear_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.quad.clear"),
            size: std::mem::size_of::<Quad>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group,
            instance_buffer,
            instance_capacity,
            stencil: None,
            mask_buffer: None,
            mask_capacity: 0,
            mask_indices: Vec::new(),
            masks: Vec::new(),
            clear_buffer,
            clear_buffer_dirty: false,
            shader,
            color_format: format,
            bind_layout,
            gradient_texture,
            gradient_texture_view,
            gradient_sampler,
        }
    }

    /// Sync the gradient LUT atlas from CPU to GPU if anything changed.
    /// Idle frames (no new gradients) hit the early `None` return and
    /// do nothing. Dirty frames upload the entire 256 KB atlas in a
    /// single `write_texture` call — see the dirty-tracking note in
    /// `GradientCpuAtlas` for why per-row uploads aren't worth the API
    /// overhead. Called from `WgpuBackend::submit` before the render
    /// pass starts.
    #[profiling::function]
    pub(crate) fn upload_gradients(&self, queue: &wgpu::Queue, atlas: &GradientAtlas) {
        atlas.flush_with(|bytes| {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.gradient_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(GRADIENT_ATLAS_SIDE * 4),
                    rows_per_image: Some(GRADIENT_ATLAS_SIDE),
                },
                wgpu::Extent3d {
                    width: GRADIENT_ATLAS_SIDE,
                    height: GRADIENT_ATLAS_SIDE,
                    depth_or_array_layers: 1,
                },
            );
        });
    }

    /// Lazy-build the stencil-aware variants. Idempotent; called from
    /// the rounded-clip render path before the first `set_pipeline`.
    #[profiling::function]
    pub(crate) fn ensure_stencil(&mut self, device: &wgpu::Device) {
        if self.stencil.is_none() {
            self.stencil = Some(self.build_stencil_pipelines(device));
        }
    }

    fn build_stencil_pipelines(&self, device: &wgpu::Device) -> StencilPipelines {
        let layout = build_pipeline_layout(
            device,
            "palantir.quad.pl.stencil",
            &[Some(&self.bind_layout)],
        );
        let instance = quad_instance_layout();
        let vertex_buffers = std::slice::from_ref(&instance);

        // Mask-write: stencil Replace at every pixel the SDF passes
        // (`fs_mask` discards outside). Color writes off, blend inert.
        let mask_face = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Always,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::Replace,
        };
        let mask_write = build_pipeline(
            device,
            PipelineRecipe {
                label: "palantir.quad.pipeline.mask",
                shader: &self.shader,
                layout: &layout,
                vertex_buffers,
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                color_format: self.color_format,
                fragment_entry: "fs_mask",
                color_writes: wgpu::ColorWrites::empty(),
                blend: None,
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: super::stencil::STENCIL_FORMAT,
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
            },
        );

        // Stencil-test: same `fs` as the no-stencil pipeline, plus the
        // shared `stencil_test_state` so the four stencil-test
        // pipelines can't drift.
        let stencil_test = build_pipeline(
            device,
            PipelineRecipe {
                label: "palantir.quad.pipeline.stencil_test",
                shader: &self.shader,
                layout: &layout,
                vertex_buffers,
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                color_format: self.color_format,
                fragment_entry: "fs",
                color_writes: wgpu::ColorWrites::ALL,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                depth_stencil: Some(super::stencil::stencil_test_state()),
            },
        );

        StencilPipelines {
            mask_write,
            stencil_test,
        }
    }

    #[profiling::function]
    pub(crate) fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, quads: &[Quad]) {
        if quads.is_empty() {
            return;
        }

        grow_instance_buffer(
            device,
            &mut self.instance_buffer,
            &mut self.instance_capacity,
            quads.len(),
            std::mem::size_of::<Quad>(),
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            "palantir.quad.instances",
            8,
        );
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

    /// Bind pipeline + viewport bind group **without** the instance
    /// buffer. The caller (today: `DebugOverlay`) sets its own vertex
    /// buffer next. Lets the debug-overlay quads ride the no-stencil
    /// quad pipeline without exposing the pipeline / bind-group
    /// fields directly — kills the prior `pub(crate)` leak.
    pub(crate) fn bind_debug<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
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

    /// Upload the partial-repaint pre-clear quad: full-viewport rect
    /// filled with `color` (alpha forced to 1), no stroke, no
    /// rounding. Drawn inside the damage scissor before regular
    /// groups so AA fringes blend over the clear color, not over
    /// last frame's pixels. Alpha is forced because a translucent
    /// pre-clear would blend against last frame's pixels and defeat
    /// the fringe-fix.
    #[profiling::function]
    pub(crate) fn upload_clear(&mut self, queue: &wgpu::Queue, viewport: Vec2, color: Color) {
        let q = Quad {
            rect: Rect {
                min: glam::Vec2::ZERO,
                size: Size {
                    w: viewport.x,
                    h: viewport.y,
                },
            },
            fill: Color { a: 1.0, ..color }.into(),
            radius: Corners::default(),
            stroke_color: ColorF16::TRANSPARENT,
            stroke_width: 0.0,
            ..Default::default()
        };
        queue.write_buffer(&self.clear_buffer, 0, bytemuck::bytes_of(&q));
        self.clear_buffer_dirty = true;
    }

    /// Bind the appropriate pipeline + clear buffer and draw one
    /// instance. In `stencil` mode the pass has a stencil attachment,
    /// so the no-stencil base pipeline can't run; uses `stencil_test`
    /// at reference 0 instead — the stencil is cleared to 0 each pass,
    /// so `Equal(0)` matches every pixel and `write_mask=0` keeps
    /// stencil intact.
    pub(crate) fn draw_clear<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, stencil: bool) {
        debug_assert!(
            self.clear_buffer_dirty,
            "draw_clear without upload_clear this frame: the schedule's \
             PreClear emit and submit's upload_clear guard have decorrelated"
        );
        if stencil {
            let s = self.stencil.as_ref().expect("ensure_stencil first");
            pass.set_pipeline(&s.stencil_test);
            pass.set_stencil_reference(0);
        } else {
            pass.set_pipeline(&self.pipeline);
        }
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.clear_buffer.slice(..));
        pass.draw(0..4, 0..1);
    }

    /// Reset per-frame state. Called from `WgpuBackend::submit` after
    /// `queue.submit` so the next frame starts with a clean slate.
    /// Today only resets `clear_buffer_dirty` so the assert in
    /// `draw_clear` actually catches "upload_clear was never called
    /// this frame."
    pub(crate) fn post_record(&mut self) {
        self.clear_buffer_dirty = false;
    }

    /// Build the per-group mask-index map for the schedule and upload
    /// one mask quad per rounded-clip group in `groups`. Caller must
    /// have run [`Self::ensure_stencil`] earlier this frame. After
    /// this call, `self.mask_indices` parallels `groups`: `Some(j)`
    /// at index `i` says "group `i`'s mask is mask quad `j`."
    #[profiling::function]
    pub(crate) fn stage_masks(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        groups: &[DrawGroup],
    ) {
        debug_assert!(
            self.stencil.is_some(),
            "stage_masks requires ensure_stencil to have run this frame"
        );
        self.mask_indices.clear();
        self.mask_indices.resize(groups.len(), None);
        self.masks.clear();
        for (i, g) in groups.iter().enumerate() {
            if g.scissor.is_some()
                && let Some(r) = g.rounded_clip
            {
                self.mask_indices[i] = Some(self.masks.len() as u32);
                self.masks.push(Self::mask_instance(r.mask_rect, r.radius));
            }
        }
        if self.masks.is_empty() {
            return;
        }
        if self.masks.len() > self.mask_capacity {
            self.mask_capacity = self.masks.len().next_power_of_two().max(8);
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
        queue.write_buffer(buf, 0, bytemuck::cast_slice(&self.masks));
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

    /// Build a `Quad` instance for the stencil mask-write pipeline.
    /// Only `rect` + `radius` reach the SDF in `fs_mask`; color/stroke
    /// are ignored (mask pipeline disables color writes), so we pass
    /// defaults.
    fn mask_instance(rect: Rect, radius: Corners) -> Quad {
        Quad {
            rect,
            fill: Color::default().into(),
            radius,
            stroke_color: ColorF16::TRANSPARENT,
            stroke_width: 0.0,
            ..Default::default()
        }
    }
}

const QUAD_INSTANCE_ATTRS: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
    0 => Float32x2,   // pos
    1 => Float32x2,   // size
    2 => Uint32x2,    // fill (packed 4x f16: r|g|b|a)
    3 => Uint32x2,    // radius (packed 4x f16: tl|tr|br|bl)
    4 => Uint32x2,    // stroke.color (packed 4x f16)
    5 => Float32,     // stroke.width
    6 => Uint32,      // fill_kind (low byte: kind, bits 8..16: spread)
    7 => Uint32,      // fill_lut_row
    8 => Uint32x2,    // fill_axis (packed 4x f16: lane0|lane1|lane2|lane3)
];

fn quad_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Quad>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &QUAD_INSTANCE_ATTRS,
    }
}
