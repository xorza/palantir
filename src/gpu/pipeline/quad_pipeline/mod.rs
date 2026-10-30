//! GPU side of quads: wgpu pipeline and instance buffer over `&[Quad]`; shader in `shader.wgsl`.

pub(crate) mod cutout_plan;
mod cutout_tables;
pub(crate) mod quad_draw;
pub(crate) mod quad_form;

use crate::common::span::Span;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::frame::binder::MainBinder;
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe, vertex_attr};
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{CutoutMode, CutoutPlan, ShadowEntry};
use crate::gpu::pipeline::quad_pipeline::cutout_tables::CutoutTables;
use crate::gpu::pipeline::quad_pipeline::quad_draw::QuadDraw;
use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
use crate::gpu::pipeline::runs::Runs;
use crate::gpu::pipeline::shader_body::{GRADIENT, ShaderBody, ShaderConstant};
use crate::gpu::pipeline::stencil_variant::{StencilMode, StencilVariant};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::single_quad_buffer::SingleQuadBuffer;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::stencil::Stencil;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::brush::gradient::Spread;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::quad::Quad;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use glam::Vec2;
use std::mem::offset_of;
use std::slice;

/// Every quad pipeline one swapchain format needs: color per [`QuadForm`], shadows per edge form, and two stencil-writing mask variants.
#[derive(Debug)]
pub(crate) struct QuadVariants {
    /// Color draws per [`QuadForm`], each the base and its stencil-test twin.
    solid: StencilVariant,
    gradient: StencilVariant,
    triangle: StencilVariant,
    /// Backdrop panes, `fs_backdrop` over [`QuadPipeline`]'s backdrop layout.
    backdrop: StencilVariant,
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
    /// The pipelines a quad of `draw` draws through.
    pub(crate) const fn of(&self, draw: QuadDraw) -> &StencilVariant {
        match draw {
            QuadDraw::Form(QuadForm::Solid) => &self.solid,
            QuadDraw::Form(QuadForm::Gradient) => &self.gradient,
            QuadDraw::Form(QuadForm::Triangle) => &self.triangle,
            QuadDraw::Form(QuadForm::Backdrop) => &self.backdrop,
            QuadDraw::Shadow(ShadowEntry::Tables) => &self.shadow_tables,
            QuadDraw::Shadow(ShadowEntry::TablesWide) => &self.shadow_tables_wide,
            QuadDraw::Shadow(ShadowEntry::General) => &self.shadow_general,
        }
    }
}

/// A format's quad pipelines for one pass: each draw picks its own, with the pass's stencil test.
#[derive(Clone, Copy, Debug)]
pub(crate) struct QuadPass<'a> {
    variants: &'a QuadVariants,
    stencil: StencilMode,
}

impl<'a> QuadPass<'a> {
    pub(crate) const fn new(variants: &'a QuadVariants, stencil: StencilMode) -> Self {
        Self { variants, stencil }
    }

    /// The pipeline a quad of `draw` draws through in this pass.
    pub(crate) const fn pipeline(self, draw: QuadDraw) -> &'a wgpu::RenderPipeline {
        self.variants.of(draw).select(self.stencil)
    }
}

/// A quad pipeline and the gradient group it reads at group 0, bound together
/// with the instance stream before any quad draw.
#[derive(Clone, Copy, Debug)]
pub(crate) struct QuadBinding<'a> {
    pub(crate) pipeline: &'a wgpu::RenderPipeline,
    pub(crate) gradient_bind_group: &'a wgpu::BindGroup,
}

impl<'a> QuadBinding<'a> {
    /// Bind the pipeline, the gradient group and `buffer` as the instance stream, for a pass of one
    /// draw, which has nothing bound to skip.
    pub(crate) fn bind(self, pass: &mut wgpu::RenderPass<'a>, buffer: &'a wgpu::Buffer) {
        pass.set_pipeline(self.pipeline);
        pass.set_bind_group(0, self.gradient_bind_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
    }

    /// [`Self::bind`] over `layout` in the main pass, through `binder`, which skips what is bound.
    fn bind_through(
        self,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        layout: &'a wgpu::PipelineLayout,
        buffer: &'a wgpu::Buffer,
    ) {
        binder.pipeline(pass, self.pipeline, layout);
        binder.group(pass, 0, self.gradient_bind_group);
        binder.vertex(pass, 0, buffer);
    }
}

/// A run of quads of one [`QuadDraw`], the pass's pipelines it picks its own from, and the groups
/// it reads.
#[derive(Clone, Copy, Debug)]
pub(crate) struct QuadRun<'a> {
    pub(crate) draw: QuadDraw,
    pub(crate) instances: Span,
    pub(crate) pipelines: QuadPass<'a>,
    pub(crate) gradient_bind_group: &'a wgpu::BindGroup,
    /// The frame's backdrop results, which a run of [`QuadForm::Backdrop`] reads at group 2.
    pub(crate) backdrops: Option<&'a wgpu::BindGroup>,
}

/// Format-independent quad resources; per-format pipelines are [`QuadVariants`].
#[derive(Debug)]
pub(crate) struct QuadPipeline {
    instance_buffer: DynamicBuffer<Quad>,
    /// Each uploaded quad's draw, parallel to the instance buffer.
    draws: Vec<QuadDraw>,
    /// Lazy buffer of one `Quad` per deduped rounded clip this frame; grows monotonically. `None` until the first stencil frame.
    mask_buffer: Option<DynamicBuffer<Quad>>,
    /// The partial-repaint pre-clear quad, drawn first inside the damage scissor so `LoadOp::Load` doesn't leak last frame's AA fringe.
    clear: SingleQuadBuffer,
    shader: wgpu::ShaderModule,
    /// Format-independent, so built once here rather than per format.
    pipeline_layout: wgpu::PipelineLayout,
    /// [`Self::pipeline_layout`] plus the cutout atlas at group 1, for the shadow variant.
    shadow_layout: wgpu::PipelineLayout,
    /// [`Self::pipeline_layout`] plus the frame's backdrop results at group 2, for the backdrop
    /// variant.
    backdrop_layout: wgpu::PipelineLayout,
    /// The shadows' baked corner cutouts.
    cutouts: CutoutTables,
    /// Whether shadows draw as a grid of cells (`vs_shadow`). Off only in internals, which build the one-cell reference.
    shadow_grid: bool,
}

impl QuadPipeline {
    /// This family's shader body.
    pub(crate) const SHADER: ShaderBody = ShaderBody {
        label: "palantir.quad.shader",
        wgsl: include_str!("shader.wgsl"),
        includes: &[GRADIENT],
        constants: shader_constants,
    };

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

    /// Bind `binding` over the pre-clear quad. Under [`StencilMode::Test`], the test at reference 0 matches every pixel and leaves the stencil intact.
    pub(crate) fn bind_clear<'a>(
        &'a self,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        binding: QuadBinding<'a>,
    ) {
        debug_assert!(
            self.clear.is_uploaded(),
            "bind_clear without upload_clear this frame: the schedule's \
             PreClear emit and submit's upload_clear guard have decorrelated"
        );
        // Sets no stencil reference: the schedule dedupes `SetStencilRef` assuming no draw arm sets one, and the ref is already 0.
        binding.bind_through(binder, pass, &self.pipeline_layout, self.clear.buffer());
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

    /// Bind a mask pipeline (stamp or clear) over the mask buffer; the caller sets `stencil_reference` per draw.
    pub(crate) fn bind_mask<'a>(
        &'a self,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        binding: QuadBinding<'a>,
    ) {
        let buf = self.mask_buffer.as_ref().expect("upload_masks first");
        binding.bind_through(binder, pass, &self.pipeline_layout, &buf.buffer);
    }

    pub(crate) fn draw_mask(&self, pass: &mut wgpu::RenderPass<'_>, mask_idx: u32) {
        pass.draw(0..4, mask_idx..mask_idx + 1);
    }

    /// Build the format-independent quad resources; per-format pipelines come from [`Self::build_variants`].
    pub(crate) fn new(
        device: &wgpu::Device,
        textures: &TextureBinding,
        backdrops: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = Self::SHADER.module(device);
        let cutouts = CutoutTables::new(device, &shader);

        let instance_buffer = DynamicBuffer::<Quad>::vertex(device, "palantir.quad.instances", 256);

        Self {
            instance_buffer,
            draws: Vec::new(),
            mask_buffer: None,
            clear: SingleQuadBuffer::new(device, "palantir.quad.clear"),
            shader,
            // Gradient atlas at group 0. One layout serves all pipelines; stencil state and fragment entry are not part of it.
            pipeline_layout: pipeline_recipe::pipeline_layout(
                device,
                "palantir.quad.pl",
                &[Some(textures.layout())],
            ),
            shadow_layout: pipeline_recipe::pipeline_layout(
                device,
                "palantir.quad.shadow.pl",
                &[Some(textures.layout()), Some(cutouts.layout())],
            ),
            backdrop_layout: pipeline_recipe::pipeline_layout(
                device,
                "palantir.quad.backdrop.pl",
                &[Some(textures.layout()), None, Some(backdrops)],
            ),
            cutouts,
            shadow_grid: true,
        }
    }

    /// Build every quad pipeline against `format`, the only format-dependent quad objects; the atlas and buffers are reused.
    pub(super) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> QuadVariants {
        let instance = Some(INSTANCE_LAYOUT);
        // The mask pair writes stencil only: `fs_mask` discards outside the SDF, color writes are off, the blend is inert.
        let mask = |label: &'static str, depth_stencil: wgpu::DepthStencilState| {
            PipelineRecipe {
                vertex_buffers: slice::from_ref(&instance),
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                fragment_entry: "fs_mask",
                color_writes: wgpu::ColorWrites::empty(),
                blend: None,
                depth_stencil: Some(depth_stencil),
                ..PipelineRecipe::color(label, &self.shader, &self.pipeline_layout, format)
            }
            .build(device)
        };
        let shadow_buffers = [instance.clone(), Some(CutoutTables::CORNER_LAYOUT)];
        let grid = f64::from(u8::from(self.shadow_grid));
        let shadow = |label, stencil_label, fragment_entry, series: bool| {
            StencilVariant::build(
                device,
                PipelineRecipe {
                    vertex_entry: "vs_shadow",
                    fragment_entry,
                    constants: &[
                        ("SHADOW_GRID", grid),
                        ("TABLES_SERIES", f64::from(u8::from(series))),
                    ],
                    vertex_buffers: &shadow_buffers,
                    ..PipelineRecipe::color(label, &self.shader, &self.shadow_layout, format)
                },
                stencil_label,
            )
        };
        let color = |label, stencil_label, form: QuadForm| {
            StencilVariant::build(
                device,
                PipelineRecipe {
                    constants: &[("QUAD_FORM", f64::from(form as u32))],
                    vertex_buffers: slice::from_ref(&instance),
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..PipelineRecipe::color(label, &self.shader, &self.pipeline_layout, format)
                },
                stencil_label,
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
            backdrop: StencilVariant::build(
                device,
                PipelineRecipe {
                    fragment_entry: "fs_backdrop",
                    constants: &[(
                        "DITHER_SRGB8",
                        f64::from(u8::from(
                            format.is_srgb() && format.target_pixel_byte_cost() == Some(4),
                        )),
                    )],
                    vertex_buffers: slice::from_ref(&instance),
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..PipelineRecipe::color(
                        "palantir.quad.pipeline.backdrop",
                        &self.shader,
                        &self.backdrop_layout,
                        format,
                    )
                },
                "palantir.quad.pipeline.backdrop.stencil_test",
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

    /// Upload the frame's quads, and work out the draw each takes under `cutouts`.
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, quads: &[Quad], cutouts: &CutoutPlan) {
        self.instance_buffer.upload_instances(ctx, quads);
        quad_draw::fill(&mut self.draws, quads, cutouts.entries(), self.shadow_grid);
    }

    /// Upload the shadows' corner stream and bake the tables `cutouts` plans, in a pass of its
    /// own on `ctx`'s encoder, before the main pass reads them.
    pub(crate) fn prepare_cutouts(&mut self, ctx: &mut GpuCtx<'_>, cutouts: &CutoutPlan) {
        self.cutouts.prepare(ctx, cutouts);
    }

    pub(crate) const fn cutout_mode(&self) -> CutoutMode {
        self.cutouts.mode()
    }

    /// The runs of `instances`, a group's quads, that each draw through one [`QuadDraw`], in
    /// paint order.
    pub(crate) fn runs(&self, instances: Span) -> Runs<impl Fn(u32) -> QuadDraw + '_> {
        Runs::new(instances, |i| self.draws[i as usize])
    }

    /// Bind the pipeline `run`'s draw picks over the instance buffer, with the groups the draw
    /// reads, and draw it:
    /// a shadow reads the cutout tables and draws the eight cells around its hole, a backdrop pane
    /// reads the frame's results.
    pub(crate) fn draw_run<'a>(
        &'a self,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        run: QuadRun<'a>,
    ) {
        let QuadRun {
            draw,
            instances,
            pipelines,
            gradient_bind_group,
            backdrops,
        } = run;
        if instances.len == 0 {
            return;
        }
        let binding = QuadBinding {
            pipeline: pipelines.pipeline(draw),
            gradient_bind_group,
        };
        let buffer = &self.instance_buffer.buffer;
        match draw {
            QuadDraw::Form(QuadForm::Backdrop) => {
                binding.bind_through(binder, pass, &self.backdrop_layout, buffer);
                let backdrops = backdrops.expect("a frame that draws a pane prepared its results");
                binder.group(pass, 2, backdrops);
                pass.draw(0..4, instances.into());
            }
            QuadDraw::Form(_) => {
                binding.bind_through(binder, pass, &self.pipeline_layout, buffer);
                pass.draw(0..4, instances.into());
            }
            QuadDraw::Shadow(_) => {
                binding.bind_through(binder, pass, &self.shadow_layout, buffer);
                self.cutouts.bind(binder, pass);
                pass.draw(0..SHADOW_GRID_VERTICES, instances.into());
            }
        }
    }
}

/// Vertices per shadow instance: a 3×3 grid less the center, two triangles per cell.
const SHADOW_GRID_VERTICES: u32 = 8 * 6;

// Location 9 is the shadow pipeline's per-instance cutout tables, so the turn takes 10.
const QUAD_INSTANCE_ATTRS: [wgpu::VertexAttribute; 10] = [
    vertex_attr(0, wgpu::VertexFormat::Float32x2, offset_of!(Quad, rect.min)),
    vertex_attr(
        1,
        wgpu::VertexFormat::Float32x2,
        offset_of!(Quad, rect.size),
    ),
    vertex_attr(2, wgpu::VertexFormat::Uint32x2, offset_of!(Quad, fill)),
    vertex_attr(3, wgpu::VertexFormat::Uint32x2, offset_of!(Quad, corners)),
    vertex_attr(
        4,
        wgpu::VertexFormat::Uint32x2,
        offset_of!(Quad, stroke_color),
    ),
    vertex_attr(
        5,
        wgpu::VertexFormat::Float32,
        offset_of!(Quad, stroke_width),
    ),
    vertex_attr(6, wgpu::VertexFormat::Uint32, offset_of!(Quad, fill_kind)),
    vertex_attr(
        7,
        wgpu::VertexFormat::Uint32,
        offset_of!(Quad, fill_lut_row),
    ),
    vertex_attr(8, wgpu::VertexFormat::Uint32x2, offset_of!(Quad, fill_axis)),
    vertex_attr(10, wgpu::VertexFormat::Float32, offset_of!(Quad, rotation)),
];

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<Quad>(),
    wgpu::VertexStepMode::Instance,
    &QUAD_INSTANCE_ATTRS,
);

/// The numbers [`QuadPipeline::SHADER`] takes from the Rust types that own them.
fn shader_constants() -> Vec<ShaderConstant> {
    vec![
        ShaderConstant::uint("FILL_TAG_MASK", FillKind::TAG_MASK),
        ShaderConstant::uint("SPREAD_SHIFT", FillKind::SPREAD_SHIFT),
        ShaderConstant::uint("SPREAD_MASK", FillKind::SPREAD_MASK),
        ShaderConstant::uint("BRUSH_KIND_LINEAR", FillKind::TAG_LINEAR),
        ShaderConstant::uint("BRUSH_KIND_RADIAL", FillKind::TAG_RADIAL),
        ShaderConstant::uint("BRUSH_KIND_SHADOW_DROP", FillKind::TAG_SHADOW_DROP),
        ShaderConstant::uint("BRUSH_KIND_SHADOW_INSET", FillKind::TAG_SHADOW_INSET),
        ShaderConstant::uint("BRUSH_KIND_TRIANGLE", FillKind::TAG_TRIANGLE),
        ShaderConstant::uint("FILL_FLAG_FAST", FillKind::FAST_BIT),
        ShaderConstant::uint("FILL_FLAG_WINDOW", FillKind::WINDOW_BIT),
        ShaderConstant::uint("QUAD_FORM_SOLID", QuadForm::Solid as u32),
        ShaderConstant::uint("QUAD_FORM_GRADIENT", QuadForm::Gradient as u32),
        ShaderConstant::uint("QUAD_FORM_TRIANGLE", QuadForm::Triangle as u32),
        // `Pad` isn't pinned: it is `apply_spread`'s fallback, also right for unknown modes.
        ShaderConstant::uint("SPREAD_REPEAT", Spread::Repeat as u32),
        ShaderConstant::uint("SPREAD_REFLECT", Spread::Reflect as u32),
        ShaderConstant::float("SHADOW_REACH_SIGMAS", ShadowGeometry::REACH_SIGMAS),
        ShaderConstant::float("CUTOUT_MIN_SIGMA", CutoutPlan::MIN_SIGMA),
        ShaderConstant::float("FILTER_SERIES_MIN_SIGMA", CutoutPlan::SERIES_MIN_SIGMA),
        ShaderConstant::float("CUTOUT_TEXELS_PER_SIGMA", CutoutPlan::TEXELS_PER_SIGMA),
        ShaderConstant::float("CUTOUT_ATLAS_SIZE", CutoutPlan::ATLAS_SIZE as f32),
        ShaderConstant::uint("CUTOUT_CELL", CutoutPlan::CELL),
        ShaderConstant::uint("NO_CUTOUT_TABLE", CutoutPlan::NONE),
        ShaderConstant::uint("CUTOUT_NODES", CutoutPlan::SHADED_NODES),
        ShaderConstant::uint("CUTOUT_BAKE_NODES", CutoutPlan::BAKED_NODES),
    ]
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::pipeline::quad_pipeline::QuadPipeline;

    impl QuadPipeline {
        /// The cutout tables the atlas holds from the frames before.
        #[cfg(test)]
        pub(crate) fn held_cutout_tables(&self) -> usize {
            self.cutouts.held_len()
        }

        /// Draw every shadow as one cell of the full form, the reference the grid is compared with. Affects variants built afterwards.
        pub(crate) const fn disable_shadow_grid(&mut self) {
            self.shadow_grid = false;
        }
    }
}
