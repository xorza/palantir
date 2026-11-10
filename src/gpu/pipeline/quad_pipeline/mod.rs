//! GPU side of quads: wgpu pipeline and instance buffer over `&[Quad]`; its shader is `quad.wgsl`
//! and the files `QuadPipeline::SHADER` lists, one a concern.

pub(crate) mod cutout_placements;
pub(crate) mod cutout_plan;
mod cutout_tables;
pub(crate) mod quad_draw;

use crate::common::span::Span;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
use crate::gpu::pipeline::format_pipelines::PassPipelines;
use crate::gpu::pipeline::instanced_pipeline::VariantTarget;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::pipeline_recipe::{self, Pipeline, PipelineRecipe, vertex_attr};
use crate::gpu::pipeline::quad_pipeline::cutout_placements::CutoutPlacements;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{CutoutMode, CutoutPlan, ShadowEntry};
use crate::gpu::pipeline::quad_pipeline::cutout_tables::CutoutTables;
use crate::gpu::pipeline::quad_pipeline::quad_draw::{
    QuadDraw, QuadDraws, ShadowCells, ShapeCells,
};
use crate::gpu::pipeline::runs::Runs;
use crate::gpu::pipeline::shader_body::{ShaderBody, ShaderConstant};
use crate::gpu::pipeline::shape_template;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::brush::gradient::Spread;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::wgsl;
use crate::primitives::wgsl::FRAME_GROUP;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::quad_form::QuadForm;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use glam::Vec2;
use std::mem::offset_of;
use std::ops::Range;

/// The label of `draw`'s pipeline.
const fn draw_label(draw: QuadDraw) -> &'static str {
    match draw {
        QuadDraw::Form(QuadForm::BoxSolid) => "palantir.quad.pipeline.box_solid",
        QuadDraw::Form(QuadForm::BoxGradient) => "palantir.quad.pipeline.box_gradient",
        QuadDraw::Form(QuadForm::TriangleSolid) => "palantir.quad.pipeline.triangle_solid",
        QuadDraw::Form(QuadForm::BoxOutside) => "palantir.quad.pipeline.box_outside",
        QuadDraw::Form(QuadForm::TriangleGradient) => "palantir.quad.pipeline.triangle_gradient",
        QuadDraw::Shadow(ShadowEntry::General) => "palantir.quad.pipeline.shadow",
        QuadDraw::Shadow(ShadowEntry::Tables) => "palantir.quad.pipeline.shadow_tables",
        QuadDraw::Shadow(ShadowEntry::TablesWide) => "palantir.quad.pipeline.shadow_tables_wide",
    }
}

/// Every quad pipeline one swapchain format needs: one per [`QuadDraw`], a form of `fs` or a
/// shadow's entry.
#[derive(Debug)]
pub(crate) struct QuadVariants {
    /// Each draw's pipeline, by [`QuadDraw::idx`]. The shadows read the baked cutouts
    /// ([`CutoutTables`]), apart from `fs` to keep the blur integral out of other quads.
    draws: [ClipVariant; QuadDraw::ALL.len()],
}

impl QuadVariants {
    /// No pipeline built.
    pub(crate) const NONE: Self = Self {
        draws: [ClipVariant::NONE; QuadDraw::ALL.len()],
    };

    /// The pipeline a quad of `draw` draws through in a frame of `clipping`, which the frame's
    /// census built.
    pub(crate) const fn get(&self, draw: QuadDraw, clipping: Clipping) -> &Pipeline {
        self.draws[draw.idx() as usize].get(clipping)
    }
}

/// A quad pipeline and the frame group it reads, bound together with the instance stream in a
/// pass of its own, which binds no frame group for it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct QuadBinding<'a> {
    pub(crate) pipeline: &'a Pipeline,
    pub(crate) frame_group: &'a wgpu::BindGroup,
}

impl<'a> QuadBinding<'a> {
    /// Bind the pipeline, the frame group and `buffer` as the instance stream, for a pass of one
    /// draw, which has nothing bound to skip.
    pub(crate) fn bind(self, pass: &mut wgpu::RenderPass<'a>, buffer: &'a wgpu::Buffer) {
        pass.set_pipeline(&self.pipeline.pipeline);
        pass.set_bind_group(FRAME_GROUP, self.frame_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
    }
}

/// A run of quads of one [`QuadDraw`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct QuadRun {
    pub(crate) draw: QuadDraw,
    pub(crate) instances: Span,
}

/// What a frame draws through the quad pipeline besides its quads, laid out after them in the
/// one instance stream, as each draws in a pass or step of its own. Refused: a buffer per kind,
/// four in all, and a one-quad buffer that kept its quad across frames, so a frame that forgot
/// its pre-clear drew the last one's.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct QuadExtras<'a> {
    /// The partial repaint's pre-clear, drawn first inside each damage scissor so `LoadOp::Load`
    /// leaks no last-frame AA fringe.
    pub(crate) clear: Option<Quad>,
    /// The debug overlay's dim over what the frame keeps.
    pub(crate) dim: Option<Quad>,
    /// The debug overlay's damage outlines.
    pub(crate) outlines: &'a [Quad],
}

/// Where the last upload put each of its [`QuadExtras`] in the instance stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ExtraSpans {
    clear: Option<u32>,
    dim: Option<u32>,
    outlines: Span,
}

impl ExtraSpans {
    /// Lay `extras` into `tail`, which follows `base` quads in the stream, in a fixed order:
    /// clear, dim, outlines. An absent extra takes no instance and reads `None`.
    fn lay(base: u32, extras: QuadExtras<'_>, tail: &mut Vec<Quad>) -> Self {
        let QuadExtras {
            clear,
            dim,
            outlines,
        } = extras;
        tail.clear();
        let mut push = |items: &[Quad]| {
            let span = Span::new(base + tail.len() as u32, items.len() as u32);
            tail.extend_from_slice(items);
            span
        };
        Self {
            clear: clear.map(|quad| push(&[quad]).start),
            dim: dim.map(|quad| push(&[quad]).start),
            outlines: push(outlines),
        }
    }
}

/// The partial repaint's pre-clear: full-viewport, opaque `color`, no border or rounding, so AA
/// fringes blend over the clear color.
pub(crate) fn pre_clear(viewport: Vec2, color: RgbaF16) -> Quad {
    debug_assert!(
        color.is_opaque(),
        "a translucent pre-clear blends over last frame"
    );
    Quad {
        fill: color,
        fill_kind: FillKind::SOLID.with_fast(),
        ..Quad::of_rect(Rect::new(0.0, 0.0, viewport.x, viewport.y))
    }
}

/// Format-independent quad resources; per-format pipelines are [`QuadVariants`].
#[derive(Debug)]
pub(crate) struct QuadPipeline {
    instance_buffer: DynamicBuffer<Quad>,
    /// Each uploaded quad's draw, parallel to the instance buffer.
    draws: Vec<QuadDraw>,
    /// The draws in [`Self::draws`].
    taken: QuadDraws,
    /// The frame's [`QuadExtras`], laid out after its quads in the instance buffer.
    tail: Vec<Quad>,
    /// Where the last upload put each extra.
    extras: ExtraSpans,
    shader: wgpu::ShaderModule,
    /// Format-independent, so built once here rather than per format.
    pipeline_layout: wgpu::PipelineLayout,
    /// [`Self::pipeline_layout`] plus the cutout atlas at group 1, for the shadow variant.
    shadow_layout: wgpu::PipelineLayout,
    /// The shadows' baked corner cutouts.
    cutouts: CutoutTables,
    /// How shadows draw (`vs_shadow`): [`ShadowCells::Whole`] only in internals, which build the one-cell reference.
    shadow_cells: ShadowCells,
    /// How the forms draw (`vs_template`): [`ShapeCells::OneQuad`] only in internals, which build
    /// the one-quad reference.
    shape_cells: ShapeCells,
}

impl QuadPipeline {
    /// The vertices `form` draws each instance with.
    pub(crate) const fn form_vertices(&self, form: QuadForm) -> u32 {
        match self.shape_cells {
            ShapeCells::Template => match form {
                QuadForm::TriangleSolid | QuadForm::TriangleGradient => shape_template::vertices(3),
                QuadForm::BoxSolid | QuadForm::BoxGradient | QuadForm::BoxOutside => {
                    shape_template::vertices(4)
                }
            },
            #[cfg(feature = "internals")]
            ShapeCells::OneQuad => 4,
        }
    }

    /// This family's shader body.
    pub(crate) const SHADER: ShaderBody = ShaderBody {
        label: "palantir.quad.shader",
        wgsl: include_str!("quad.wgsl"),
        includes: &[
            wgsl::FRAME,
            wgsl::GRADIENT_LUT,
            wgsl::GRADIENT_FILL,
            include_str!("paint.wgsl"),
            wgsl::SHAPE_TEMPLATE,
            include_str!("template.wgsl"),
            include_str!("reference.wgsl"),
            include_str!("shadow.wgsl"),
            include_str!("cutout_bake.wgsl"),
        ],
        constants: SHADER_CONSTANTS,
    };

    /// Bind `pipeline` over the instance stream and draw the frame's pre-clear quad.
    pub(crate) fn draw_clear<'a>(&'a self, rec: &mut PassRecorder<'a>, pipeline: &'a Pipeline) {
        let at = self
            .extras
            .clear
            .expect("a frame that pre-clears uploads its clear quad");
        self.bind(rec, pipeline);
        rec.draw(0..self.form_vertices(QuadForm::BoxSolid), at..at + 1);
    }

    /// Bind `pipeline` over the instance stream in the main pass, which holds the frame group.
    fn bind<'a>(&'a self, rec: &mut PassRecorder<'a>, pipeline: &'a Pipeline) {
        rec.pipeline(pipeline);
        rec.vertex(0, &self.instance_buffer.buffer);
    }

    /// The instance stream, the frame's quads and its extras.
    pub(crate) const fn instances(&self) -> &wgpu::Buffer {
        &self.instance_buffer.buffer
    }

    /// The instances of the frame's dim quad, which a frame that dims uploads.
    pub(crate) const fn dim(&self) -> Range<u32> {
        let at = self
            .extras
            .dim
            .expect("a frame that dims uploads its dim quad");
        at..at + 1
    }

    /// The instances of the frame's damage outlines.
    pub(crate) const fn outlines(&self) -> Range<u32> {
        self.extras.outlines.u32_range()
    }

    /// Build the format-independent quad resources; per-format pipelines come from [`Self::ensure_draw`].
    pub(crate) fn new(gpu: &Gpu, textures: &TextureBinding) -> Self {
        let shader = Self::SHADER.module(gpu);
        let cutouts = CutoutTables::new(gpu, &shader);

        let instance_buffer = DynamicBuffer::<Quad>::vertex(gpu, "palantir.quad.instances", 256);

        Self {
            instance_buffer,
            draws: Vec::new(),
            taken: QuadDraws::default(),
            tail: Vec::new(),
            extras: ExtraSpans::default(),
            shader,
            // The frame group. One layout serves every form; the fragment entry is no part of it.
            pipeline_layout: pipeline_recipe::pipeline_layout(
                gpu,
                "palantir.quad.pl",
                &[Some(textures.frame())],
            ),
            shadow_layout: pipeline_recipe::pipeline_layout(
                gpu,
                "palantir.quad.shadow.pl",
                &[Some(textures.frame()), Some(cutouts.layout())],
            ),
            cutouts,
            shadow_cells: ShadowCells::Grid,
            shape_cells: ShapeCells::Template,
        }
    }

    /// Build the half of `draw`'s pipelines `target` binds, where it is not built: the only
    /// format-dependent quad objects; the atlas and buffers are reused.
    pub(super) fn ensure_draw(
        &self,
        variants: &mut QuadVariants,
        gpu: &Gpu,
        target: VariantTarget,
        draw: QuadDraw,
    ) {
        let VariantTarget { format, clipping } = target;
        let instance = [Some(INSTANCE_LAYOUT)];
        let label = draw_label(draw);
        let variant = &mut variants.draws[draw.idx() as usize];
        match draw {
            QuadDraw::Form(form) => {
                variant.ensure(
                    gpu,
                    clipping,
                    &PipelineRecipe {
                        vertex_entry: match self.shape_cells {
                            ShapeCells::Template => "vs_template",
                            #[cfg(feature = "internals")]
                            ShapeCells::OneQuad => "vs",
                        },
                        fragment_entry: match self.shape_cells {
                            ShapeCells::Template => "fs_template",
                            #[cfg(feature = "internals")]
                            ShapeCells::OneQuad => "fs",
                        },
                        constants: &[("QUAD_FORM", f64::from(form as u32))],
                        vertex_buffers: &instance,
                        topology: match self.shape_cells {
                            ShapeCells::Template => wgpu::PrimitiveTopology::TriangleList,
                            #[cfg(feature = "internals")]
                            ShapeCells::OneQuad => wgpu::PrimitiveTopology::TriangleStrip,
                        },
                        ..PipelineRecipe::color(label, &self.shader, &self.pipeline_layout, format)
                    },
                );
            }
            QuadDraw::Shadow(entry) => {
                let (fragment_entry, series) = match entry {
                    ShadowEntry::General => ("fs_shadow", false),
                    ShadowEntry::Tables => ("fs_shadow_tables", false),
                    ShadowEntry::TablesWide => ("fs_shadow_tables", true),
                };
                variant.ensure(
                    gpu,
                    clipping,
                    &PipelineRecipe {
                        vertex_entry: "vs_shadow",
                        fragment_entry,
                        constants: &[
                            (
                                "SHADOW_GRID",
                                f64::from(u8::from(self.shadow_cells == ShadowCells::Grid)),
                            ),
                            ("TABLES_SERIES", f64::from(u8::from(series))),
                        ],
                        vertex_buffers: &[Some(INSTANCE_LAYOUT), Some(CutoutTables::CORNER_LAYOUT)],
                        ..PipelineRecipe::color(label, &self.shader, &self.shadow_layout, format)
                    },
                );
            }
        }
    }

    /// Upload the frame's quads and, after them, its `extras`, and work out the draw each quad
    /// takes under `cutouts`.
    pub(crate) fn upload(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        quads: &[Quad],
        cutouts: &CutoutPlan,
        extras: QuadExtras<'_>,
    ) {
        self.extras = ExtraSpans::lay(quads.len() as u32, extras, &mut self.tail);
        debug_assert!(
            quads.iter().chain(&self.tail).all(Quad::has_shape),
            "a box quad left its points or its depth at their default (`Quad::of_rect`)"
        );
        self.instance_buffer.upload_parts(ctx, &[quads, &self.tail]);
        self.taken = quad_draw::fill(&mut self.draws, quads, cutouts.entries(), self.shadow_cells);
    }

    /// The draws the last uploaded frame's quads take.
    pub(crate) const fn taken(&self) -> QuadDraws {
        self.taken
    }

    /// Upload the shadows' corner stream and bake the tables `cutouts` plans, in a pass of its
    /// own on `ctx`'s encoder, before the main pass reads them.
    pub(crate) fn prepare_cutouts(&mut self, ctx: &mut GpuCtx<'_>, cutouts: &CutoutPlan) {
        self.cutouts.prepare(ctx, cutouts);
    }

    pub(crate) const fn cutout_mode(&self) -> CutoutMode {
        self.cutouts.mode()
    }

    /// Where the shadow atlas places each key's table, for a window's plan to build in.
    pub(crate) const fn cutout_placements_mut(&mut self) -> &mut CutoutPlacements {
        self.cutouts.placements_mut()
    }

    /// The runs of `instances`, a group's quads, that each draw through one [`QuadDraw`], in
    /// paint order.
    pub(crate) fn runs(&self, instances: Span) -> Runs<impl Fn(u32) -> QuadDraw + '_> {
        Runs::new(instances, |i| self.draws[i as usize])
    }

    /// Bind the pipeline `run`'s draw picks of `pipelines` over the instance buffer, with the
    /// groups the draw reads past the frame group, and draw it: a shadow reads the cutout tables
    /// and draws the eight cells around its hole.
    pub(crate) fn draw_run<'a>(
        &'a self,
        rec: &mut PassRecorder<'a>,
        pipelines: PassPipelines<'a>,
        run: QuadRun,
    ) {
        let QuadRun { draw, instances } = run;
        if instances.len == 0 {
            return;
        }
        self.bind(rec, pipelines.quad(draw));
        match draw {
            QuadDraw::Form(form) => rec.draw(0..self.form_vertices(form), instances.u32_range()),
            QuadDraw::Shadow(_) => {
                self.cutouts.bind(rec);
                rec.draw(0..SHADOW_GRID_VERTICES, instances.u32_range());
            }
        }
    }
}

/// Vertices per shadow instance: a 3×3 grid less the center, two triangles per cell.
const SHADOW_GRID_VERTICES: u32 = 8 * 6;

// Location 9 is the shadow pipeline's per-instance cutout tables, so the turn takes 10.
/// The quad's header, then its fill's and its border's lanes.
const QUAD_INSTANCE_ATTRS: [wgpu::VertexAttribute; 12] = {
    let h = shape_template::HEADER_ATTRS;
    let at = shape_template::HEADER_LOCATIONS;
    [
        h[0],
        h[1],
        h[2],
        h[3],
        h[4],
        h[5],
        vertex_attr(at, wgpu::VertexFormat::Uint32x2, offset_of!(Quad, fill)),
        vertex_attr(
            at + 1,
            wgpu::VertexFormat::Uint32x2,
            offset_of!(Quad, border_color),
        ),
        vertex_attr(
            at + 2,
            wgpu::VertexFormat::Float32,
            offset_of!(Quad, border_width),
        ),
        vertex_attr(
            at + 3,
            wgpu::VertexFormat::Uint32,
            offset_of!(Quad, fill_kind),
        ),
        vertex_attr(
            at + 4,
            wgpu::VertexFormat::Uint32,
            offset_of!(Quad, fill_lut_row),
        ),
        vertex_attr(
            at + 5,
            wgpu::VertexFormat::Uint32x2,
            offset_of!(Quad, fill_axis),
        ),
    ]
};

const _: () = assert!(offset_of!(Quad, header) == 0);

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<Quad>(),
    wgpu::VertexStepMode::Instance,
    &QUAD_INSTANCE_ATTRS,
);

/// The numbers [`QuadPipeline::SHADER`] takes from the Rust types that own them.
const SHADER_CONSTANTS: &[ShaderConstant] = &[
    ShaderConstant::uint("FILL_TAG_MASK", FillKind::TAG_MASK),
    ShaderConstant::uint("SPREAD_SHIFT", FillKind::SPREAD_SHIFT),
    ShaderConstant::uint("SPREAD_MASK", FillKind::SPREAD_MASK),
    ShaderConstant::uint("FILL_SOLID", FillKind::TAG_SOLID),
    ShaderConstant::uint("FILL_LINEAR", FillKind::TAG_LINEAR),
    ShaderConstant::uint("FILL_RADIAL", FillKind::TAG_RADIAL),
    ShaderConstant::uint("FILL_SHADOW_DROP", FillKind::TAG_SHADOW_DROP),
    ShaderConstant::uint("FILL_SHADOW_INSET", FillKind::TAG_SHADOW_INSET),
    ShaderConstant::uint("FILL_FLAG_TRIANGLE", FillKind::TRIANGLE_BIT),
    ShaderConstant::uint("FILL_FLAG_FAST", FillKind::FAST_BIT),
    ShaderConstant::uint("FILL_FLAG_OUTSIDE", FillKind::OUTSIDE_BIT),
    ShaderConstant::uint("QUAD_FORM_BOX_SOLID", QuadForm::BoxSolid as u32),
    ShaderConstant::uint("QUAD_FORM_BOX_GRADIENT", QuadForm::BoxGradient as u32),
    ShaderConstant::uint("QUAD_FORM_TRIANGLE_SOLID", QuadForm::TriangleSolid as u32),
    ShaderConstant::uint("QUAD_FORM_BOX_OUTSIDE", QuadForm::BoxOutside as u32),
    ShaderConstant::uint(
        "QUAD_FORM_TRIANGLE_GRADIENT",
        QuadForm::TriangleGradient as u32,
    ),
    shape_template::CONSTANT,
    // `Pad` isn't pinned: it is `palantir_apply_spread`'s fallback, also right for unknown modes.
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
];

#[cfg(feature = "internals")]
pub(crate) mod internals {
    #[cfg(test)]
    use crate::gpu::pipeline::clip_variant::Clipping;
    use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
    #[cfg(test)]
    use crate::gpu::pipeline::quad_pipeline::QuadVariants;
    #[cfg(test)]
    use crate::gpu::pipeline::quad_pipeline::quad_draw::QuadDraw;
    use crate::gpu::pipeline::quad_pipeline::quad_draw::{ShadowCells, ShapeCells};

    #[cfg(test)]
    impl QuadVariants {
        /// Whether the half of `draw`'s pipelines `clipping` binds is built.
        pub(crate) const fn is_built(&self, draw: QuadDraw, clipping: Clipping) -> bool {
            self.draws[draw.idx() as usize].is_built(clipping)
        }
    }

    impl QuadPipeline {
        /// The cutout tables the atlas holds from the frames before.
        #[cfg(test)]
        pub(crate) fn held_cutout_tables(&self) -> usize {
            self.cutouts.held_len()
        }

        /// Draw every shadow as one cell of the full form, the reference the grid is compared with. Affects variants built afterwards.
        pub(crate) const fn disable_shadow_grid(&mut self) {
            self.shadow_cells = ShadowCells::Whole;
        }

        /// Draw every box, circle and triangle as one quad that measures its distance at each
        /// pixel, the reference the template is compared with. Affects variants built afterwards.
        pub(crate) const fn disable_shape_template(&mut self) {
            self.shape_cells = ShapeCells::OneQuad;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::span::Span;
    use crate::gpu::pipeline::quad_pipeline::{ExtraSpans, QuadExtras};
    use crate::primitives::geometry::rect::Rect;
    use crate::renderer::quad::Quad;

    /// The extras follow the frame's 3 quads in order: the clear at 3, the outline at 4; no dim
    /// takes no instance. A frame with none lays nothing and its clear reads
    /// `None`, so a pre-clear it forgot cannot draw the last frame's.
    #[test]
    fn extras_follow_the_quads_in_order() {
        let quad = |x: f32| Quad::of_rect(Rect::new(x, 0.0, 1.0, 1.0));
        let mut tail = Vec::new();
        let spans = ExtraSpans::lay(
            3,
            QuadExtras {
                clear: Some(quad(3.0)),
                dim: None,
                outlines: &[quad(4.0)],
            },
            &mut tail,
        );
        assert_eq!(
            spans,
            ExtraSpans {
                clear: Some(3),
                dim: None,
                outlines: Span::new(4, 1),
            }
        );
        assert_eq!(
            tail.iter().map(|q| q.header.rect.min.x).collect::<Vec<_>>(),
            [3.0, 4.0]
        );
        let spans = ExtraSpans::lay(3, QuadExtras::default(), &mut tail);
        assert!(tail.is_empty());
        assert_eq!(spans.clear, None);
        assert_eq!(spans.outlines.len, 0);
    }
}
