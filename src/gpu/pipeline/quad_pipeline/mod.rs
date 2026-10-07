//! GPU side of quads: wgpu pipeline and instance buffer over `&[Quad]`; shader in `shader.wgsl`.

pub(crate) mod cutout_plan;
mod cutout_tables;
pub(crate) mod quad_form;
pub(crate) mod runs;

use crate::common::span::Span;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{CutoutPlan, ShadowEntry};
use crate::gpu::pipeline::quad_pipeline::cutout_tables::CutoutTables;
use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
use crate::gpu::pipeline::quad_pipeline::runs::Runs;
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::pipeline::stencil_variant::ColorVariantSpec;
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::single_quad_buffer::SingleQuadBuffer;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::stencil::Stencil;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::quad::Quad;
use glam::Vec2;
use std::slice;

/// Every quad pipeline one swapchain format needs: colour per [`QuadForm`], shadows per edge form, and two stencil-writing mask variants.
#[derive(Debug)]
pub(crate) struct QuadVariants {
    /// Colour draws per [`QuadForm`], each the base and its stencil-test twin.
    solid: StencilVariant,
    gradient: StencilVariant,
    triangle: StencilVariant,
    /// Drop and inset shadows, reading the baked cutouts ([`CutoutTables`]); apart from `fs` to keep the blur integral out of other quads.
    shadow_general: StencilVariant,
    /// Shadows whose every drawn corner reads a table (`fs_shadow_tables`).
    shadow_tables: StencilVariant,
    /// [`Self::shadow_tables`] with the series form of `filter_cdf`, for blurs from [`CutoutPlan::SERIES_MIN_SIGMA`] up.
    shadow_tables_wide: StencilVariant,
    pub(crate) mask_stamp: wgpu::RenderPipeline,
    pub(crate) mask_clear: wgpu::RenderPipeline,
}

impl QuadVariants {
    pub(crate) const fn color(&self, form: QuadForm) -> &StencilVariant {
        match form {
            QuadForm::Solid => &self.solid,
            QuadForm::Gradient => &self.gradient,
            QuadForm::Triangle => &self.triangle,
        }
    }

    pub(crate) const fn shadow(&self, entry: ShadowEntry) -> &StencilVariant {
        match entry {
            ShadowEntry::Tables => &self.shadow_tables,
            ShadowEntry::TablesWide => &self.shadow_tables_wide,
            ShadowEntry::General => &self.shadow_general,
        }
    }
}

/// Format-independent quad resources; per-format pipelines are [`QuadVariants`].
#[derive(Debug)]
pub(crate) struct QuadPipeline {
    instance_buffer: DynamicBuffer<Quad>,
    /// Lazy buffer of one `Quad` per deduped rounded clip this frame; grows monotonically. `None` until the first stencil frame.
    mask_buffer: Option<DynamicBuffer<Quad>>,
    /// The partial-repaint pre-clear quad, drawn first inside the damage scissor so `LoadOp::Load` doesn't leak last frame's AA fringe.
    clear: SingleQuadBuffer,
    shader: wgpu::ShaderModule,
    /// Format-independent, so built once here rather than per format.
    pipeline_layout: wgpu::PipelineLayout,
    /// [`Self::pipeline_layout`] plus the cutout atlas at group 1, for the shadow variant.
    shadow_layout: wgpu::PipelineLayout,
    /// The shadows' baked corner cutouts.
    cutouts: CutoutTables,
    /// [`QuadForm`] per uploaded quad.
    forms: Vec<QuadForm>,
    /// Whether shadows draw as a grid of cells (`vs_shadow`). Off only in internals, which build the one-cell reference.
    shadow_grid: bool,
}

impl QuadPipeline {
    /// Bind a pipeline, the shared gradient group and the instance buffer.
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

    /// Upload the pre-clear quad: full-viewport, opaque `color`, no stroke or rounding, so AA fringes blend over the clear color.
    pub(crate) fn upload_clear(&mut self, ctx: &mut GpuCtx<'_>, viewport: Vec2, color: RgbaF16) {
        debug_assert!(
            color.is_opaque(),
            "a translucent pre-clear blends over last frame"
        );
        let q = Quad {
            rect: Rect::new(0.0, 0.0, viewport.x, viewport.y),
            fill: color,
            fill_kind: FillKind::SOLID.with_fast(),
            ..Default::default()
        };
        self.clear.upload(ctx, q);
    }

    /// Bind the pipeline and clear buffer for the pre-clear quad. Under `stencil`, `stencil_test` at reference 0 matches every pixel and leaves the stencil intact.
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
        // Sets no stencil reference: the schedule dedupes `SetStencilRef` assuming no draw arm sets one, and the ref is already 0.
        Self::bind_buffer(
            pass,
            pipelines.select(use_stencil),
            gradient_bg,
            self.clear.buffer(),
        );
    }

    pub(crate) fn upload_masks(&mut self, ctx: &mut GpuCtx<'_>, masks: &[Quad]) {
        if masks.is_empty() {
            return;
        }
        let buf = self.mask_buffer.get_or_insert_with(|| {
            DynamicBuffer::<Quad>::vertex(ctx.device, "palantir.quad.masks", 8)
        });
        buf.upload_instances(ctx, masks);
    }

    /// Bind a mask pipeline (stamp or clear) and the mask buffer; the caller sets `stencil_reference` per draw.
    pub(crate) fn bind_mask<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        mask_pipeline: &'a wgpu::RenderPipeline,
        gradient_bg: &'a wgpu::BindGroup,
    ) {
        let buf = self.mask_buffer.as_ref().expect("upload_masks first");
        Self::bind_buffer(pass, mask_pipeline, gradient_bg, &buf.buffer);
    }

    pub(crate) fn draw_mask(&self, pass: &mut wgpu::RenderPass<'_>, mask_idx: u32) {
        pass.draw(0..4, mask_idx..mask_idx + 1);
    }

    /// Build the format-independent quad resources; per-format pipelines come from [`Self::build_variants`].
    pub(crate) fn new(device: &wgpu::Device, textures: &TextureBinding) -> Self {
        let shader = ShaderBody::Quad.module(device);
        let cutouts = CutoutTables::new(device, &shader);

        let instance_buffer = DynamicBuffer::<Quad>::vertex(device, "palantir.quad.instances", 256);

        Self {
            instance_buffer,
            mask_buffer: None,
            clear: SingleQuadBuffer::new(device, "palantir.quad.clear"),
            shader,
            // Gradient atlas at group 0. One layout serves all pipelines; stencil state and fragment entry are not part of it.
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
            forms: Vec::new(),
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

    /// Build every quad pipeline against `format`, the only format-dependent quad objects; the atlas and buffers are reused.
    pub(super) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> QuadVariants {
        let instance = Some(Self::instance_layout());
        // The mask pair writes stencil only: `fs_mask` discards outside the SDF, colour writes are off, the blend is inert.
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
        let grid = f64::from(u8::from(self.shadow_grid));
        let shadow = |label, stencil_label, fragment_entry, series: bool| {
            StencilVariant::build(
                device,
                ColorVariantSpec {
                    label,
                    stencil_label,
                    shader: &self.shader,
                    vertex_entry: "vs_shadow",
                    fragment_entry,
                    constants: &[
                        ("SHADOW_GRID", grid),
                        ("TABLES_SERIES", f64::from(u8::from(series))),
                    ],
                    layout: &self.shadow_layout,
                    vertex_buffers: &shadow_buffers,
                    topology: wgpu::PrimitiveTopology::TriangleList,
                },
                format,
            )
        };
        let color = |label, stencil_label, form: QuadForm| {
            StencilVariant::build(
                device,
                ColorVariantSpec {
                    label,
                    stencil_label,
                    shader: &self.shader,
                    vertex_entry: "vs",
                    fragment_entry: "fs",
                    constants: &[("QUAD_FORM", f64::from(form as u32))],
                    layout: &self.pipeline_layout,
                    vertex_buffers: slice::from_ref(&instance),
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                },
                format,
            )
        };
        QuadVariants {
            solid: color(
                "palantir.quad.pipeline.solid",
                "palantir.quad.pipeline.solid.stencil_test",
                QuadForm::Solid,
            ),
            gradient: color(
                "palantir.quad.pipeline.gradient",
                "palantir.quad.pipeline.gradient.stencil_test",
                QuadForm::Gradient,
            ),
            triangle: color(
                "palantir.quad.pipeline.triangle",
                "palantir.quad.pipeline.triangle.stencil_test",
                QuadForm::Triangle,
            ),
            shadow_general: shadow(
                "palantir.quad.pipeline.shadow",
                "palantir.quad.pipeline.shadow.stencil_test",
                "fs_shadow",
                false,
            ),
            shadow_tables: shadow(
                "palantir.quad.pipeline.shadow_tables",
                "palantir.quad.pipeline.shadow_tables.stencil_test",
                "fs_shadow_tables",
                false,
            ),
            shadow_tables_wide: shadow(
                "palantir.quad.pipeline.shadow_tables_wide",
                "palantir.quad.pipeline.shadow_tables_wide.stencil_test",
                "fs_shadow_tables",
                true,
            ),
            mask_stamp: mask("palantir.quad.pipeline.mask_stamp", Stencil::stamp_state()),
            mask_clear: mask("palantir.quad.pipeline.mask_clear", Stencil::clear_state()),
        }
    }

    /// Upload the frame's quads and bake the cutout tables planned for them.
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, quads: &[Quad], cutouts: &CutoutPlan) {
        self.instance_buffer.upload_instances(ctx, quads);
        self.forms.clear();
        self.forms
            .extend(quads.iter().map(|quad| QuadForm::of(quad.fill_kind)));
        self.cutouts.prepare(ctx, cutouts);
    }

    pub(crate) const fn bakes_cutouts(&self) -> bool {
        self.cutouts.bakes()
    }

    /// Bind pipeline, gradient group and instance buffer once per pass. `use_stencil` selects the stencil-test variant.
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

    /// [`Self::bind`] for the shadow variant, which also reads the cutout atlas and corner tables.
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

    /// Draw a contiguous slice of the instance buffer. The caller binds once and sets the scissor before each call.
    pub(crate) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, instances: Span) {
        if instances.len == 0 {
            return;
        }
        pass.draw(0..4, instances.into());
    }

    /// The runs of `instances`, a shadow step's span, that draw through one [`ShadowEntry`], in paint order.
    pub(crate) fn shadow_runs(&self, instances: Span) -> Runs<'_, ShadowEntry> {
        let entries = self.shadow_grid.then(|| self.cutouts.entries());
        Runs::new(instances, entries, ShadowEntry::General)
    }

    /// The runs of `instances`, a quad step's span, that each draw through one [`QuadForm`], in paint order.
    pub(crate) fn quad_runs(&self, instances: Span) -> Runs<'_, QuadForm> {
        Runs::new(instances, Some(&self.forms), QuadForm::Solid)
    }

    /// [`Self::draw`] for shadows from [`Self::bind_shadows`]: each instance is the eight cells around the hole, two triangles each.
    pub(crate) fn draw_shadows<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, instances: Span) {
        if instances.len == 0 {
            return;
        }
        pass.draw(0..SHADOW_GRID_VERTICES, instances.into());
    }
}

/// Vertices per shadow instance: a 3×3 grid less the centre, two triangles per cell.
const SHADOW_GRID_VERTICES: u32 = 8 * 6;

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

// Compile-time guard: each attribute offset must match its `Quad` field; a reorder or same-size swap would mis-route data without changing the stride.
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
        /// Draw every shadow as one cell of the full form, the reference the grid is compared with. Affects variants built afterwards.
        pub(crate) const fn disable_shadow_grid(&mut self) {
            self.shadow_grid = false;
        }
    }
}
