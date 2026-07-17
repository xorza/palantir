//! GPU side of quads — wgpu pipeline + viewport uniform + instance
//! buffer. Consumes `&[Quad]` (defined frontend-side) and binds the
//! shader at `quad.wgsl` next to this file.

use crate::primitives::brush::Spread;
use crate::primitives::color::ColorF16;
use crate::primitives::fill_wire::FillKind;
use crate::primitives::span::Span;
use crate::primitives::{color::Color, corners::Corners, rect::Rect, size::Size};
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::{
    ColorVariantSpec, PipelineRecipe, StencilVariant, build_pipeline, build_pipeline_layout,
};
use crate::renderer::backend::schedule::{MaskPlan, build_mask_plan};
use crate::renderer::backend::shader_template::{ShaderConstant, specialize};
use crate::renderer::backend::stencil::STENCIL_FORMAT;
use crate::renderer::gradient_atlas::ATLAS_ROWS;
use crate::renderer::quad::{AA_RADIUS, Quad};
use crate::renderer::render_buffer::{MAX_ROUNDED_CLIP_DEPTH, RenderBuffer};
use glam::Vec2;

#[derive(Debug)]
pub(crate) struct QuadPipeline {
    /// Format-independent quad resources. The format-dependent render
    /// pipelines (base + stencil-test twin via [`StencilVariant`], plus
    /// the mask stamp / clear variants) live in
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines),
    /// keyed by swapchain format and passed into every `bind*` call —
    /// `bind` / `bind_clear` / `bind_mask` still own the
    /// `set_pipeline` / `set_bind_group` pair. Group 0 (gradient atlas +
    /// sampler) is owned by
    /// [`GradientResources`](crate::renderer::backend::gradient_resources::GradientResources)
    /// and passed to every `bind*` call.
    instance_buffer: DynamicBuffer<Quad>,
    /// Lazy buffer holding one `Quad` per deduped rounded clip in the
    /// current frame; uploaded by `stage_masks`, drawn by `draw_mask`. Reused
    /// across frames; capacity grows monotonically. `None` until the
    /// first stencil frame.
    mask_buffer: Option<DynamicBuffer<Quad>>,
    /// Retained scratch for the stencil-mask sweep, populated by
    /// [`Self::stage_masks`] and read by the render schedule. Stale on
    /// non-stencil frames; the schedule only reads it when
    /// `use_stencil` is true.
    pub(crate) mask_indices: MaskPlan,
    /// Retained scratch for stencil-mask quads: one entry per chain
    /// level per run of consecutive groups sharing a chain (see
    /// [`build_mask_plan`]); uploaded to `mask_buffer`. Cleared at
    /// the start of each stencil frame; capacity retained.
    masks: Vec<Quad>,
    /// Single-instance buffer holding the partial-repaint pre-clear quad
    /// (full-viewport, opaque, clear color). Drawn before regular groups
    /// inside the damage scissor so `LoadOp::Load` doesn't leak last
    /// frame's AA-fringe pixels into this frame's blends.
    clear_buffer: wgpu::Buffer,
    /// Last `(viewport, color)` written to `clear_buffer`. `None`
    /// before the first call to [`Self::upload_clear`]; thereafter
    /// holds the last upload's inputs so steady-state Partial frames
    /// can short-circuit the `queue.write_buffer`. [`Self::bind_clear`]
    /// asserts `Some` — catches a future refactor that decorrelates
    /// the upload guard in `submit` from the per-pass `PreClear` emit
    /// in the schedule.
    last_clear: Option<(Vec2, Color)>,
    /// Quad shader module — format-independent; the `build_*` methods
    /// read it to build each format's pipelines.
    shader: wgpu::ShaderModule,
}

impl QuadPipeline {
    /// `gradient_bgl` is the group-0 layout owned by
    /// [`GradientResources`](crate::renderer::backend::gradient_resources::GradientResources);
    /// the pipeline composes its layout against it and the matching bind
    /// group arrives at each `bind*` call.
    /// Build the format-independent quad resources. The format-dependent
    /// pipelines are built separately by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variants`] / [`Self::build_mask_stamp`] /
    /// [`Self::build_mask_clear`].
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let wgsl = specialize(
            include_str!("quad.wgsl"),
            &[
                ShaderConstant::float("AA_RADIUS", AA_RADIUS),
                ShaderConstant::float("ATLAS_ROWS", ATLAS_ROWS as f32),
                ShaderConstant::uint("BRUSH_KIND_SOLID", FillKind::SOLID.0),
                ShaderConstant::uint("BRUSH_KIND_LINEAR", FillKind::linear(Spread::Pad).0),
                ShaderConstant::uint("BRUSH_KIND_RADIAL", FillKind::radial(Spread::Pad).0),
                ShaderConstant::uint("BRUSH_KIND_CONIC", FillKind::conic(Spread::Pad).0),
                ShaderConstant::uint("BRUSH_KIND_SHADOW_DROP", FillKind::SHADOW_DROP.0),
                ShaderConstant::uint("BRUSH_KIND_SHADOW_INSET", FillKind::SHADOW_INSET.0),
                ShaderConstant::uint("BRUSH_KIND_TRIANGLE", FillKind::TRIANGLE.0),
                ShaderConstant::uint("FILL_FLAG_FAST", FillKind::FAST_BIT),
                ShaderConstant::uint("FILL_FLAG_WINDOW", FillKind::WINDOW_BIT),
                ShaderConstant::uint("SPREAD_PAD", Spread::Pad as u32),
                ShaderConstant::uint("SPREAD_REPEAT", Spread::Repeat as u32),
                ShaderConstant::uint("SPREAD_REFLECT", Spread::Reflect as u32),
            ],
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aperture.quad.shader"),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });

        let instance_buffer = DynamicBuffer::<Quad>::vertex(device, "aperture.quad.instances", 256);

        let clear_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aperture.quad.clear"),
            size: std::mem::size_of::<Quad>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            instance_buffer,
            mask_buffer: None,
            mask_indices: MaskPlan::default(),
            masks: Vec::new(),
            clear_buffer,
            last_clear: None,
            shader,
        }
    }

    /// Build the base + stencil-test color pipelines against `format` —
    /// the only format-dependent quad objects; the gradient LUT atlas
    /// (texture + bind group + sampler) and the instance / clear buffers
    /// are reused. The distinct mask variants ([`Self::build_mask_stamp`]
    /// / [`Self::build_mask_clear`]) stay separate — different fragment
    /// entry, color writes off. Called by `FormatPipelines` for each
    /// swapchain format.
    pub(crate) fn build_variants(
        &self,
        device: &wgpu::Device,
        gradient_bgl: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        // Gradient atlas at group 0 (viewport rides the shared immediate
        // region, no bind-group slot needed).
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "aperture.quad.pipeline",
                stencil_label: "aperture.quad.pipeline.stencil_test",
                layout_label: "aperture.quad.pl",
                shader: &self.shader,
                bind_group_layouts: &[Some(gradient_bgl)],
                vertex_buffers: &[Some(quad_instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            format,
        )
    }

    /// Mask-stamp variant: stencil `Equal` + `IncrementClamp` at every
    /// pixel the SDF passes (`fs_mask` discards outside), color writes
    /// off, blend inert. Drawn once per chain level at
    /// `stencil_reference = level`: writes `level + 1` only where the
    /// SDF passes AND the stencil already equals `level`, so nested
    /// masks intersect — the outermost stamps ref 0 onto the cleared
    /// stencil, each inner mask deepens only inside its ancestors.
    pub(crate) fn build_mask_stamp(
        &self,
        device: &wgpu::Device,
        gradient_bgl: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let face = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Equal,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::IncrementClamp,
        };
        self.build_mask_pipeline(
            device,
            gradient_bgl,
            format,
            "aperture.quad.pipeline.mask_stamp",
            face,
        )
    }

    /// Mask-clear variant: stencil `Always` + `Replace`, drawn at
    /// `stencil_reference = 0` to reset a stamped chain. One draw of
    /// the chain's *outermost* quad suffices — inner stamps only ever
    /// incremented inside the outer's SDF, so every nonzero stencil
    /// pixel lies under it.
    pub(crate) fn build_mask_clear(
        &self,
        device: &wgpu::Device,
        gradient_bgl: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let face = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Always,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::Replace,
        };
        self.build_mask_pipeline(
            device,
            gradient_bgl,
            format,
            "aperture.quad.pipeline.mask_clear",
            face,
        )
    }

    fn build_mask_pipeline(
        &self,
        device: &wgpu::Device,
        gradient_bgl: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        label: &'static str,
        face: wgpu::StencilFaceState,
    ) -> wgpu::RenderPipeline {
        let layout = build_pipeline_layout(device, "aperture.quad.pl.mask", &[Some(gradient_bgl)]);
        let instance = Some(quad_instance_layout());
        build_pipeline(
            device,
            PipelineRecipe {
                label,
                shader: &self.shader,
                layout: &layout,
                vertex_buffers: std::slice::from_ref(&instance),
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                color_format: format,
                fragment_entry: "fs_mask",
                color_writes: wgpu::ColorWrites::empty(),
                blend: None,
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: STENCIL_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: wgpu::StencilState {
                        front: face,
                        back: face,
                        read_mask: MAX_ROUNDED_CLIP_DEPTH,
                        write_mask: MAX_ROUNDED_CLIP_DEPTH,
                    },
                    bias: wgpu::DepthBiasState::default(),
                }),
            },
        )
    }

    #[profiling::function]
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, quads: &[Quad]) {
        self.instance_buffer.upload_instances(ctx, quads);
    }

    /// Bind pipeline + gradient bind group + instance buffer once per
    /// pass. `use_stencil` selects the stencil-test variant (the
    /// rounded-clip pass) over the no-stencil base. Group 0 is the
    /// shared gradient bind group; viewport rides immediates. Mirrors
    /// the `bind(pass, use_stencil, gradient_bg)` shape of the mesh /
    /// image / curve pipelines.
    pub(crate) fn bind<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        pipelines: &'a StencilVariant,
        use_stencil: bool,
        gradient_bg: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(pipelines.select(use_stencil));
        pass.set_bind_group(0, gradient_bg, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.buffer.slice(..));
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
    pub(crate) fn upload_clear(&mut self, ctx: &mut GpuCtx<'_>, viewport: Vec2, color: Color) {
        // Steady state: viewport + clear color match last frame, so
        // the clear_buffer already holds the right pixels. Skip the
        // belt write entirely on a match.
        if self.last_clear == Some((viewport, color)) {
            return;
        }
        let q = Quad {
            rect: Rect {
                min: glam::Vec2::ZERO,
                size: Size {
                    w: viewport.x,
                    h: viewport.y,
                },
            },
            fill: Color { a: 1.0, ..color }.into(),
            corners: Corners::default(),
            stroke_color: ColorF16::TRANSPARENT,
            stroke_width: 0.0,
            // Solid, sharp, stroke-less, integer-origin (`viewport` is
            // the ceil'd physical size): qualifies for the fragment
            // fast path.
            fill_kind: FillKind::SOLID.with_fast(),
            ..Default::default()
        };
        ctx.write(&self.clear_buffer, 0, bytemuck::bytes_of(&q));
        self.last_clear = Some((viewport, color));
    }

    /// Bind the pipeline + clear vertex buffer for the partial-repaint
    /// pre-clear quad. Caller follows with `viewport.push_into(pass)`
    /// then `pass.draw(0..4, 0..1)` — see the PreClear arm in
    /// `WgpuBackend::render_groups`.
    ///
    /// In `stencil` mode the pass has a stencil attachment, so the
    /// no-stencil base pipeline can't run; uses `stencil_test` at
    /// reference 0 instead — the stencil is cleared to 0 each pass,
    /// so `Equal(0)` matches every pixel and `write_mask=0` keeps
    /// stencil intact.
    pub(crate) fn bind_clear<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        pipelines: &'a StencilVariant,
        use_stencil: bool,
        gradient_bg: &'a wgpu::BindGroup,
    ) {
        debug_assert!(
            self.last_clear.is_some(),
            "bind_clear without upload_clear this frame: the schedule's \
             PreClear emit and submit's upload_clear guard have decorrelated"
        );
        pass.set_pipeline(pipelines.select(use_stencil));
        if use_stencil {
            pass.set_stencil_reference(0);
        }
        pass.set_bind_group(0, gradient_bg, &[]);
        pass.set_vertex_buffer(0, self.clear_buffer.slice(..));
    }

    /// Build the per-group / per-text-batch mask-index maps for the
    /// schedule ([`build_mask_plan`]) and upload the deduped mask
    /// quads. After this call, `self.mask_indices.groups` parallels
    /// `buffer.groups` and `.batches` parallels `buffer.text_batches`,
    /// each entry the mask-quad span for that chain.
    #[profiling::function]
    pub(crate) fn stage_masks(&mut self, ctx: &mut GpuCtx<'_>, buffer: &RenderBuffer) {
        build_mask_plan(buffer, &mut self.mask_indices, &mut self.masks);
        if self.masks.is_empty() {
            return;
        }
        // Lazy-create the mask buffer on the first stencil frame, then
        // reuse across frames (capacity grows monotonically through
        // `DynamicBuffer::upload_instances`).
        let buf = self.mask_buffer.get_or_insert_with(|| {
            DynamicBuffer::<Quad>::vertex(ctx.device, "aperture.quad.masks", 8)
        });
        buf.upload_instances(ctx, &self.masks);
    }

    /// Bind a mask pipeline (stamp or clear — the schedule picks) +
    /// the mask instance buffer. Caller sets `stencil_reference` per
    /// draw (the chain level for stamps, 0 for clears). Group 0 is the
    /// shared gradient bind group; viewport rides immediates,
    /// pre-pushed by the backend.
    pub(crate) fn bind_mask<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        mask_pipeline: &'a wgpu::RenderPipeline,
        gradient_bg: &'a wgpu::BindGroup,
    ) {
        let buf = self.mask_buffer.as_ref().expect("upload_masks first");
        pass.set_pipeline(mask_pipeline);
        pass.set_bind_group(0, gradient_bg, &[]);
        pass.set_vertex_buffer(0, buf.buffer.slice(..));
    }

    /// Draw the single mask `Quad` at `mask_idx` in the mask buffer.
    pub(crate) fn draw_mask(&self, pass: &mut wgpu::RenderPass<'_>, mask_idx: u32) {
        pass.draw(0..4, mask_idx..mask_idx + 1);
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

// Compile-time guard: each attribute's byte offset must match the `Quad`
// field it feeds. `vertex_attr_array!` packs offsets by summing format
// sizes in declaration order, and `array_stride` is pinned to
// `size_of::<Quad>()` — but neither catches a struct field reorder or a
// format/field size mismatch (a same-size swap keeps the stride yet
// mis-routes the data to the shader). `offset_of!` against the actual
// fields closes that gap.
const _: () = {
    use std::mem::offset_of;
    assert!(QUAD_INSTANCE_ATTRS[0].offset == offset_of!(Quad, rect.min) as u64);
    assert!(QUAD_INSTANCE_ATTRS[1].offset == offset_of!(Quad, rect.size) as u64);
    assert!(QUAD_INSTANCE_ATTRS[2].offset == offset_of!(Quad, fill) as u64);
    assert!(QUAD_INSTANCE_ATTRS[3].offset == offset_of!(Quad, corners) as u64);
    assert!(QUAD_INSTANCE_ATTRS[4].offset == offset_of!(Quad, stroke_color) as u64);
    assert!(QUAD_INSTANCE_ATTRS[5].offset == offset_of!(Quad, stroke_width) as u64);
    assert!(QUAD_INSTANCE_ATTRS[6].offset == offset_of!(Quad, fill_kind) as u64);
    assert!(QUAD_INSTANCE_ATTRS[7].offset == offset_of!(Quad, fill_lut_row) as u64);
    assert!(QUAD_INSTANCE_ATTRS[8].offset == offset_of!(Quad, fill_axis) as u64);
};

fn quad_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Quad>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &QUAD_INSTANCE_ATTRS,
    }
}
