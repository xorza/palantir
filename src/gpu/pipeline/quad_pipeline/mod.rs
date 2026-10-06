//! GPU side of quads — wgpu pipeline + instance buffer. Consumes
//! `&[Quad]` (defined frontend-side) and binds the shader at
//! `shader.wgsl` next to this file. The viewport rides the shared
//! immediate region rather than a uniform buffer — see
//! [`ViewportPush`](crate::gpu::surface::viewport::ViewportPush).

pub(crate) mod cutout_plan;
mod cutout_tables;

use crate::common::span::Span;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::pipeline::quad_pipeline::cutout_tables::CutoutTables;
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::pipeline::stencil_variant::ColorVariantSpec;
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::single_quad_buffer::SingleQuadBuffer;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::stencil::Stencil;
use crate::gpu::surface::viewport::RepaintScissors;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::quad::Quad;
use glam::{UVec2, Vec2};
use std::slice;

/// Every quad pipeline one swapchain format needs.
///
/// Four, not one: shadows draw through a fragment entry of their own, and
/// the two mask variants write the stencil instead of colour, through a
/// third. They are built together because they share a layout, and held
/// together because the schedule reaches for whichever the step calls for.
#[derive(Debug)]
pub(crate) struct QuadVariants {
    /// Colour draws — the base and its stencil-test twin.
    pub(crate) color: StencilVariant,
    /// Drop and inset shadows, through `vs_shadow` and `fs_shadow`, which
    /// also read the baked corner cutouts ([`CutoutTables`]). Apart from
    /// `fs` so that the blur integral's registers and code stay out of
    /// every other quad's pipeline.
    pub(crate) shadow: StencilVariant,
    /// Deepens a rounded-clip chain by one level. See
    /// [`Stencil::stamp_state`].
    pub(crate) mask_stamp: wgpu::RenderPipeline,
    /// Resets a stamped chain. See [`Stencil::clear_state`].
    pub(crate) mask_clear: wgpu::RenderPipeline,
}

/// Format-independent quad resources. The format-dependent render
/// pipelines ([`QuadVariants`]) live in
/// [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines),
/// keyed by swapchain format and passed into every `bind*` call.
/// The group-0 bind group, the gradient atlas, is owned by
/// [`GpuGradientAtlas`](crate::gpu::resource::gpu_gradient_atlas::GpuGradientAtlas)
/// and passed to every `bind*` call.
#[derive(Debug)]
pub(crate) struct QuadPipeline {
    instance_buffer: DynamicBuffer<Quad>,
    /// Lazy buffer holding one `Quad` per deduped rounded clip in the
    /// current frame; uploaded by `upload_masks`, drawn by `draw_mask`.
    /// Reused across frames; capacity grows monotonically. `None` until
    /// the first stencil frame.
    mask_buffer: Option<DynamicBuffer<Quad>>,
    /// The partial-repaint pre-clear quad (full-viewport, opaque, clear
    /// color). Drawn before regular groups inside the damage scissor so
    /// `LoadOp::Load` doesn't leak last frame's AA-fringe pixels into
    /// this frame's blends.
    clear: SingleQuadBuffer,
    /// Quad shader module — format-independent; the `build_*` methods
    /// read it to build each format's pipelines.
    shader: wgpu::ShaderModule,
    /// Format-independent, so built once here rather than per format.
    pipeline_layout: wgpu::PipelineLayout,
    /// [`Self::pipeline_layout`] plus the cutout atlas at group 1, for the
    /// shadow variant alone.
    shadow_layout: wgpu::PipelineLayout,
    /// The shadows' baked corner cutouts.
    cutouts: CutoutTables,
    /// Whether the shadow variant draws each shadow as its grid of cells
    /// (`vs_shadow`). Off only in the crate's internals, which build the
    /// one-cell reference the grid is compared with.
    shadow_grid: bool,
}

impl QuadPipeline {
    /// Bind a pipeline, the shared gradient group, and the buffer whose
    /// instances the draws index. The whole of binding a quad pipeline —
    /// the colour draws, the pre-clear quad, the two mask variants and the
    /// debug overlay's quads differ only in which pipeline and which
    /// buffer, never in the steps.
    pub(crate) fn bind_buffer<'a>(
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
    pub(crate) fn upload_clear(&mut self, ctx: &mut GpuCtx<'_>, viewport: Vec2, color: RgbaF32) {
        let q = Quad {
            rect: Rect::new(0.0, 0.0, viewport.x, viewport.y),
            fill: RgbaF32 { a: 1.0, ..color }.into(),
            // Solid, sharp, stroke-less, integer-origin (`viewport` is
            // the ceil'd physical size): qualifies for the fragment
            // fast path.
            fill_kind: FillKind::SOLID.with_fast(),
            ..Default::default()
        };
        self.clear.upload(ctx, q);
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
            self.clear.is_uploaded(),
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
            self.clear.buffer(),
        );
    }

    /// Upload the frame's deduped mask quads, which the schedule's mask
    /// plan indexes by [`Self::draw_mask`].
    pub(crate) fn upload_masks(&mut self, ctx: &mut GpuCtx<'_>, masks: &[Quad]) {
        if masks.is_empty() {
            return;
        }
        // Lazy-create the mask buffer on the first stencil frame, then
        // reuse across frames (capacity grows monotonically through
        // `DynamicBuffer::upload_instances`).
        let buf = self.mask_buffer.get_or_insert_with(|| {
            DynamicBuffer::<Quad>::vertex(ctx.device, "palantir.quad.masks", 8)
        });
        buf.upload_instances(ctx, masks);
    }

    /// Bind a mask pipeline (stamp or clear — the schedule picks) +
    /// the mask instance buffer. Caller sets `stencil_reference` per
    /// draw (the chain level for stamps, 0 for clears). Group 0 is the
    /// shared gradient bind group; the backend pushes the viewport
    /// after the bind.
    pub(crate) fn bind_mask<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        mask_pipeline: &'a wgpu::RenderPipeline,
        gradient_bg: &'a wgpu::BindGroup,
    ) {
        let buf = self.mask_buffer.as_ref().expect("upload_masks first");
        Self::bind_buffer(pass, mask_pipeline, gradient_bg, &buf.buffer);
    }

    /// Draw the single mask `Quad` at `mask_idx` in the mask buffer.
    pub(crate) fn draw_mask(&self, pass: &mut wgpu::RenderPass<'_>, mask_idx: u32) {
        pass.draw(0..4, mask_idx..mask_idx + 1);
    }

    /// Build the format-independent quad resources. The format-dependent
    /// pipelines are built separately by
    /// [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines)
    /// from [`Self::build_variants`].
    pub(crate) fn new(device: &wgpu::Device, textures: &TextureBinding) -> Self {
        let shader = ShaderBody::Quad.module(device);
        let cutouts = CutoutTables::new(device, &shader);

        let instance_buffer = DynamicBuffer::<Quad>::vertex(device, "palantir.quad.instances", 256);

        Self {
            instance_buffer,
            mask_buffer: None,
            clear: SingleQuadBuffer::new(device, "palantir.quad.clear"),
            shader,
            // Gradient atlas at group 0 (viewport rides the shared
            // immediate region, no bind-group slot needed). One layout for
            // all three pipelines: neither the stencil state nor the
            // fragment entry is part of a layout.
            pipeline_layout: PipelineRecipe::pipeline_layout(
                device,
                "palantir.quad.pl",
                &[Some(textures.layout())],
            ),
            shadow_layout: PipelineRecipe::pipeline_layout(
                device,
                "palantir.quad.shadow.pl",
                &[Some(textures.layout()), Some(cutouts.layout())],
            ),
            cutouts,
            shadow_grid: true,
        }
    }

    pub(super) const fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Quad>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &QUAD_INSTANCE_ATTRS,
        }
    }

    /// Build every quad pipeline against `format` — the only
    /// format-dependent quad objects; the gradient LUT atlas (texture +
    /// bind group) and the instance / clear buffers are
    /// reused. Called by `FormatPipelines` for each swapchain format.
    pub(super) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> QuadVariants {
        let instance = Some(Self::instance_layout());
        // The mask pair writes the stencil and no colour: `fs_mask`
        // discards outside the SDF, colour writes are off, and the blend
        // is inert.
        let mask = |label: &'static str, depth_stencil: wgpu::DepthStencilState| {
            PipelineRecipe {
                label,
                shader: &self.shader,
                layout: &self.pipeline_layout,
                vertex_entry: "vs",
                constants: &[],
                vertex_buffers: slice::from_ref(&instance),
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                color_format: format,
                fragment_entry: "fs_mask",
                color_writes: wgpu::ColorWrites::empty(),
                blend: None,
                depth_stencil: Some(depth_stencil),
            }
            .build(device)
        };
        let shadow_buffers = [instance.clone(), Some(CutoutTables::corner_layout())];
        let shadow_constants = [("SHADOW_GRID", f64::from(u8::from(self.shadow_grid)))];
        QuadVariants {
            color: StencilVariant::build(
                device,
                ColorVariantSpec {
                    label: "palantir.quad.pipeline",
                    stencil_label: "palantir.quad.pipeline.stencil_test",
                    shader: &self.shader,
                    vertex_entry: "vs",
                    fragment_entry: "fs",
                    constants: &[],
                    layout: &self.pipeline_layout,
                    vertex_buffers: slice::from_ref(&instance),
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                },
                format,
            ),
            shadow: StencilVariant::build(
                device,
                ColorVariantSpec {
                    label: "palantir.quad.pipeline.shadow",
                    stencil_label: "palantir.quad.pipeline.shadow.stencil_test",
                    shader: &self.shader,
                    vertex_entry: "vs_shadow",
                    fragment_entry: "fs_shadow",
                    constants: &shadow_constants,
                    layout: &self.shadow_layout,
                    vertex_buffers: &shadow_buffers,
                    topology: wgpu::PrimitiveTopology::TriangleList,
                },
                format,
            ),
            mask_stamp: mask("palantir.quad.pipeline.mask_stamp", Stencil::stamp_state()),
            mask_clear: mask("palantir.quad.pipeline.mask_clear", Stencil::clear_state()),
        }
    }

    /// Upload the frame's quads, and bake the cutout tables the shadows a
    /// repaint inside `scissors` of a `viewport` in physical pixels read.
    pub(crate) fn upload(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        quads: &[Quad],
        scissors: &RepaintScissors,
        viewport: UVec2,
    ) {
        self.instance_buffer.upload_instances(ctx, quads);
        self.cutouts.prepare(ctx, quads, scissors, viewport);
    }

    /// Bind pipeline + gradient bind group + instance buffer once per
    /// pass. `use_stencil` selects the stencil-test variant (the
    /// rounded-clip pass) over the no-stencil base. Group 0 is the
    /// shared gradient bind group; viewport rides immediates.
    pub(crate) fn bind<'a>(
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

    /// [`Self::bind`] for the shadow variant, which also reads the cutout
    /// atlas and the per-instance corner tables.
    pub(crate) fn bind_shadows<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        variant: &'a StencilVariant,
        use_stencil: bool,
        gradient_bg: &'a wgpu::BindGroup,
    ) {
        self.bind(pass, variant, use_stencil, gradient_bg);
        self.cutouts.bind(pass);
    }

    /// Draw a contiguous slice of the uploaded instance buffer. Used to
    /// segment quads by scissor region; caller is responsible for
    /// calling [`Self::bind`] once and setting
    /// `RenderPass::set_scissor_rect` before each call.
    pub(crate) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, instances: Span) {
        if instances.len == 0 {
            return;
        }
        pass.draw(0..4, instances.into());
    }

    /// [`Self::draw`] for shadows bound by [`Self::bind_shadows`]: each
    /// instance is its grid's nine cells, two triangles each (`vs_shadow`).
    pub(crate) fn draw_shadows<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, instances: Span) {
        if instances.len == 0 {
            return;
        }
        pass.draw(0..SHADOW_GRID_VERTICES, instances.into());
    }
}

/// Vertices per shadow instance: a 3×3 grid of cells, two triangles each.
const SHADOW_GRID_VERTICES: u32 = 9 * 6;

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

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::gpu::pipeline::quad_pipeline::QuadPipeline;

    impl QuadPipeline {
        /// See `CutoutTables::disable_baking`.
        pub(crate) const fn disable_cutout_tables(&mut self) {
            self.cutouts.disable_baking();
        }

        /// Draw every shadow as one cell of the full form, the reference
        /// the grid is compared with. Takes effect in the variants built
        /// from now on.
        pub(crate) const fn disable_shadow_grid(&mut self) {
            self.shadow_grid = false;
        }
    }
}
