//! GPU side of quads — wgpu pipeline + instance buffer. Consumes
//! `&[Quad]` (defined frontend-side) and binds the shader at
//! `quad.wgsl` next to this file. The viewport rides the shared
//! immediate region rather than a uniform buffer — see
//! [`ViewportPush`](crate::renderer::backend::viewport::ViewportPush).

use crate::common::tracy;
use crate::primitives::brush::gradient::Spread;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::span::Span;
use crate::primitives::{color::Color, rect::Rect};
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_recipe::PipelineRecipe;
use crate::renderer::backend::schedule::{MaskPlan, build_mask_plan};
use crate::renderer::backend::shader_template::{self, ShaderConstant};
use crate::renderer::backend::stencil::Stencil;
use crate::renderer::backend::stencil_variant::ColorVariantSpec;
use crate::renderer::backend::stencil_variant::StencilVariant;
use crate::renderer::quad::{AA_RADIUS, Quad};
use crate::renderer::render_buffer::RenderBuffer;
use glam::Vec2;

/// Every quad pipeline one swapchain format needs.
///
/// Three, not one: the two mask variants write the stencil instead of
/// colour, through a fragment entry of their own. They are built
/// together because they share a layout, and held together because the
/// schedule reaches for whichever the step calls for.
#[derive(Debug)]
pub(super) struct QuadVariants {
    /// Colour draws — the base and its stencil-test twin.
    pub(super) color: StencilVariant,
    /// Deepens a rounded-clip chain by one level. See
    /// [`Stencil::stamp_state`].
    pub(super) mask_stamp: wgpu::RenderPipeline,
    /// Resets a stamped chain. See [`Stencil::clear_state`].
    pub(super) mask_clear: wgpu::RenderPipeline,
}

#[derive(Debug)]
pub(super) struct QuadPipeline {
    /// Format-independent quad resources. The format-dependent render
    /// pipelines ([`QuadVariants`]) live in
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines),
    /// keyed by swapchain format and passed into every `bind*` call.
    /// Group 0 (gradient atlas + sampler) is owned by
    /// [`GpuGradientAtlas`](crate::renderer::backend::gpu_gradient_atlas::GpuGradientAtlas)
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
    pub(super) mask_indices: MaskPlan,
    /// Retained scratch for stencil-mask quads: one entry per chain
    /// level per run of consecutive groups sharing a chain (see
    /// [`build_mask_plan`]); uploaded to `mask_buffer`. Cleared at
    /// the start of each stencil frame; capacity retained.
    masks: Vec<Quad>,
    /// Single-instance buffer holding the partial-repaint pre-clear quad
    /// (full-viewport, opaque, clear color). Drawn before regular groups
    /// inside the damage scissor so `LoadOp::Load` doesn't leak last
    /// frame's AA-fringe pixels into this frame's blends.
    clear_buffer: DynamicBuffer<Quad>,
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
    /// Bind a pipeline, the shared gradient group, and the buffer whose
    /// instances the draws index. The whole of binding a quad pipeline —
    /// the colour draws, the pre-clear quad and the two mask variants
    /// differ only in which pipeline and which buffer, never in the
    /// steps.
    fn bind_buffer<'a>(
        pass: &mut wgpu::RenderPass<'a>,
        pipeline: &'a wgpu::RenderPipeline,
        gradient_bg: &'a wgpu::BindGroup,
        buffer: &'a wgpu::Buffer,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, gradient_bg, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
    }

    /// Upload the partial-repaint pre-clear quad: full-viewport rect
    /// filled with `color` (alpha forced to 1), no stroke, no
    /// rounding. Drawn inside the damage scissor before regular
    /// groups so AA fringes blend over the clear color, not over
    /// last frame's pixels. Alpha is forced because a translucent
    /// pre-clear would blend against last frame's pixels and defeat
    /// the fringe-fix.
    pub(super) fn upload_clear(&mut self, ctx: &mut GpuCtx<'_>, viewport: Vec2, color: Color) {
        // Steady state: viewport + clear color match last frame, so
        // the clear_buffer already holds the right pixels. Skip the
        // belt write entirely on a match.
        if self.last_clear == Some((viewport, color)) {
            return;
        }
        let q = Quad {
            rect: Rect::new(0.0, 0.0, viewport.x, viewport.y),
            fill: Color { a: 1.0, ..color }.into(),
            // Solid, sharp, stroke-less, integer-origin (`viewport` is
            // the ceil'd physical size): qualifies for the fragment
            // fast path.
            fill_kind: FillKind::SOLID.with_fast(),
            ..Default::default()
        };
        self.clear_buffer.upload_instances(ctx, &[q]);
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
    pub(super) fn bind_clear<'a>(
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
        // Deliberately does **not** set a stencil reference, even under
        // `use_stencil`. The schedule dedupes `SetStencilRef` on the
        // strength of no draw arm setting one of its own, and the ref is
        // provably 0 here anyway: a pass opens at 0 per the WebGPU spec,
        // `for_each_step`'s tail `clear_active` returns every walk to 0,
        // and `PreClear` is a walk's first step. Re-adding a defensive
        // `set_stencil_reference(0)` would falsify that invariant, not
        // guard it.
        Self::bind_buffer(
            pass,
            pipelines.select(use_stencil),
            gradient_bg,
            &self.clear_buffer.buffer,
        );
    }

    /// Build the per-group / per-text-batch mask-index maps for the
    /// schedule ([`build_mask_plan`]) and upload the deduped mask
    /// quads. After this call, `self.mask_indices.groups` parallels
    /// `buffer.groups` and `.batches` parallels `buffer.text_batches`,
    /// each entry the mask-quad span for that chain.
    pub(super) fn stage_masks(&mut self, ctx: &mut GpuCtx<'_>, buffer: &RenderBuffer) {
        tracy::zone!();
        build_mask_plan(buffer, &mut self.mask_indices, &mut self.masks);
        if self.masks.is_empty() {
            return;
        }
        // Lazy-create the mask buffer on the first stencil frame, then
        // reuse across frames (capacity grows monotonically through
        // `DynamicBuffer::upload_instances`).
        let buf = self.mask_buffer.get_or_insert_with(|| {
            DynamicBuffer::<Quad>::vertex(ctx.device, "palantir.quad.masks", 8)
        });
        buf.upload_instances(ctx, &self.masks);
    }

    /// Bind a mask pipeline (stamp or clear — the schedule picks) +
    /// the mask instance buffer. Caller sets `stencil_reference` per
    /// draw (the chain level for stamps, 0 for clears). Group 0 is the
    /// shared gradient bind group; viewport rides immediates,
    /// pre-pushed by the backend.
    pub(super) fn bind_mask<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        mask_pipeline: &'a wgpu::RenderPipeline,
        gradient_bg: &'a wgpu::BindGroup,
    ) {
        let buf = self.mask_buffer.as_ref().expect("stage_masks first");
        Self::bind_buffer(pass, mask_pipeline, gradient_bg, &buf.buffer);
    }

    /// Draw the single mask `Quad` at `mask_idx` in the mask buffer.
    pub(super) fn draw_mask(&self, pass: &mut wgpu::RenderPass<'_>, mask_idx: u32) {
        pass.draw(0..4, mask_idx..mask_idx + 1);
    }
    /// `gradient_bgl` is the group-0 layout owned by
    /// [`GpuGradientAtlas`](crate::renderer::backend::gpu_gradient_atlas::GpuGradientAtlas);
    /// the pipeline composes its layout against it and the matching bind
    /// group arrives at each `bind*` call.
    /// Build the format-independent quad resources. The format-dependent
    /// pipelines are built separately by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variants`].
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let wgsl = shader_template::specialize(
            shader_template::QUAD_WGSL,
            &[
                ShaderConstant::float("AA_RADIUS", AA_RADIUS),
                // The family tags, not whole packed words: the shader
                // compares them against `fill_kind & 0xFF`, and
                // `FillKind::linear(Spread::Pad).0` only happened to
                // equal the tag because `Pad` is zero.
                ShaderConstant::uint("BRUSH_KIND_SOLID", FillKind::TAG_SOLID),
                ShaderConstant::uint("BRUSH_KIND_LINEAR", FillKind::TAG_LINEAR),
                ShaderConstant::uint("BRUSH_KIND_RADIAL", FillKind::TAG_RADIAL),
                ShaderConstant::uint("BRUSH_KIND_CONIC", FillKind::TAG_CONIC),
                ShaderConstant::uint("BRUSH_KIND_SHADOW_DROP", FillKind::TAG_SHADOW_DROP),
                ShaderConstant::uint("BRUSH_KIND_SHADOW_INSET", FillKind::TAG_SHADOW_INSET),
                ShaderConstant::uint("BRUSH_KIND_TRIANGLE", FillKind::TAG_TRIANGLE),
                ShaderConstant::uint("FILL_FLAG_FAST", FillKind::FAST_BIT),
                ShaderConstant::uint("FILL_FLAG_WINDOW", FillKind::WINDOW_BIT),
                // `Pad` is not pinned: it is `apply_spread`'s fallback,
                // which is also the right answer for a mode the shader
                // does not know, so nothing there compares against it.
                ShaderConstant::uint("SPREAD_REPEAT", Spread::Repeat as u32),
                ShaderConstant::uint("SPREAD_REFLECT", Spread::Reflect as u32),
            ],
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.quad.shader"),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });

        let instance_buffer = DynamicBuffer::<Quad>::vertex(device, "palantir.quad.instances", 256);

        let clear_buffer = DynamicBuffer::<Quad>::vertex(device, "palantir.quad.clear", 1);

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

    pub(super) fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Quad>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &QUAD_INSTANCE_ATTRS,
        }
    }

    /// Build every quad pipeline against `format` — the only
    /// format-dependent quad objects; the gradient LUT atlas (texture +
    /// bind group + sampler) and the instance / clear buffers are
    /// reused. Called by `FormatPipelines` for each swapchain format.
    pub(super) fn build_variants(
        &self,
        device: &wgpu::Device,
        gradient_bgl: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> QuadVariants {
        // Gradient atlas at group 0 (viewport rides the shared immediate
        // region, no bind-group slot needed). One layout for all three
        // pipelines: neither the stencil state nor the fragment entry is
        // part of a layout, so building one apiece would be three
        // identical objects.
        let layout =
            PipelineRecipe::pipeline_layout(device, "palantir.quad.pl", &[Some(gradient_bgl)]);
        let instance = Some(Self::instance_layout());
        // The mask pair writes the stencil and no colour: `fs_mask`
        // discards outside the SDF, colour writes are off, and the blend
        // is inert.
        let mask = |label: &'static str, depth_stencil: wgpu::DepthStencilState| {
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
                depth_stencil: Some(depth_stencil),
            }
            .build(device)
        };
        QuadVariants {
            color: StencilVariant::build(
                device,
                ColorVariantSpec {
                    label: "palantir.quad.pipeline",
                    stencil_label: "palantir.quad.pipeline.stencil_test",
                    shader: &self.shader,
                    layout: &layout,
                    vertex_buffers: std::slice::from_ref(&instance),
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                },
                format,
            ),
            mask_stamp: mask("palantir.quad.pipeline.mask_stamp", Stencil::stamp_state()),
            mask_clear: mask("palantir.quad.pipeline.mask_clear", Stencil::clear_state()),
        }
    }

    pub(super) fn upload(&mut self, ctx: &mut GpuCtx<'_>, quads: &[Quad]) {
        self.instance_buffer.upload_instances(ctx, quads);
    }

    /// Bind pipeline + gradient bind group + instance buffer once per
    /// pass. `use_stencil` selects the stencil-test variant (the
    /// rounded-clip pass) over the no-stencil base. Group 0 is the
    /// shared gradient bind group; viewport rides immediates.
    pub(super) fn bind<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        variant: &'a StencilVariant,
        use_stencil: bool,
        gradient_bg: &'a wgpu::BindGroup,
    ) {
        Self::bind_buffer(
            pass,
            variant.select(use_stencil),
            gradient_bg,
            &self.instance_buffer.buffer,
        );
    }

    /// Draw a contiguous slice of the uploaded instance buffer. Used to
    /// segment quads by scissor region; caller is responsible for
    /// calling [`Self::bind`] once and setting
    /// `RenderPass::set_scissor_rect` before each call.
    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, instances: Span) {
        if instances.len == 0 {
            return;
        }
        pass.draw(0..4, instances.into());
    }
}

const QUAD_INSTANCE_ATTRS: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Uint32x2,
    3 => Uint32x2,
    4 => Uint32x2,
    5 => Float32,
    6 => Uint32,
    7 => Uint32,
    8 => Uint32x2,
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
