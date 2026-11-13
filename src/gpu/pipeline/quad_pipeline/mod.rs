//! GPU side of quads: wgpu pipeline and instance buffer over `&[QuadInstance]`; its shader is `quad.wgsl`
//! and the files `QuadPipeline::SHADER` lists, one a concern.

pub(crate) mod cutout_placements;
pub(crate) mod cutout_plan;
mod cutout_tables;
pub(crate) mod quad_draw;

use crate::common::span::Span;
use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::page_slots::PageSlots;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
use crate::gpu::pipeline::format_pipelines::PassPipelines;
use crate::gpu::pipeline::instanced_pipeline::VariantTarget;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::quad_pipeline::cutout_placements::CutoutPlacements;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{CutoutPlan, ShadowEntry};
use crate::gpu::pipeline::quad_pipeline::cutout_tables::CutoutTables;
use crate::gpu::pipeline::quad_pipeline::quad_draw::{QuadDraw, QuadDraws, ShapeCells};
use crate::gpu::pipeline::runs::Runs;
use crate::gpu::pipeline::shape_template::{self, TemplateIndices};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::gpu::resource::pipeline_recipe::{self, Pipeline, PipelineRecipe, vertex_attr};
use crate::gpu::resource::shader_body::{ShaderBody, ShaderConstant};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::brush::gradient::Spread;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::wgsl;
use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
use crate::renderer::render_buffer::quad_draw_row::{QuadDrawRow, QuadInstance};
use crate::renderer::render_buffer::shadow_instance::{ShadowInstance, ShadowKind};
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::template_cover::TemplateCover;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use glam::Vec2;
use soa_rs::Soa;
use std::mem::offset_of;
use std::ops::Range;

/// The label of `draw`'s pipeline.
const fn draw_label(draw: QuadDraw) -> &'static str {
    match draw {
        QuadDraw::Fill(form) => FILL_LABELS[form.idx() as usize],
        QuadDraw::Shadow(ShadowEntry::General) => "palantir.quad.pipeline.shadow",
        QuadDraw::Shadow(ShadowEntry::Tables) => "palantir.quad.pipeline.shadow_tables",
        QuadDraw::Shadow(ShadowEntry::TablesWide) => "palantir.quad.pipeline.shadow_tables_wide",
    }
}

/// Each fill form's pipeline label, in [`FillForm::ALL`] order.
const FILL_LABELS: [&str; FillForm::ALL.len()] = [
    "palantir.quad.pipeline.box_solid",
    "palantir.quad.pipeline.box_gradient",
    "palantir.quad.pipeline.triangle_solid",
    "palantir.quad.pipeline.triangle_gradient",
];

/// Every quad pipeline one swapchain format needs: one a fill's form, which reads no page, and
/// one a shadow's entry for each set of open slots of the mask pages its cutout tables lie on.
#[derive(Debug)]
pub(crate) struct QuadVariants {
    /// Each fill's pipeline, by [`FillForm::idx`].
    fills: [ClipVariant; FillForm::ALL.len()],
    /// Each shadow entry's pipelines, by [`ShadowEntry`], then by the open mask slots'
    /// [`PageSlots::idx`]. Every entry reads the baked cutouts ([`CutoutTables`]), the general one
    /// for the corners of a shadow that hold a table beside one that holds none; apart from `fs`
    /// to keep the blur integral out of other quads.
    shadows: [[ClipVariant; PageSlots::COUNT]; ShadowEntry::COUNT],
}

impl QuadVariants {
    /// No pipeline built.
    pub(crate) const NONE: Self = Self {
        fills: [const { ClipVariant::NONE }; FillForm::ALL.len()],
        shadows: [const { [const { ClipVariant::NONE }; PageSlots::COUNT] }; ShadowEntry::COUNT],
    };

    /// `draw`'s pipelines over the open mask `slots`.
    const fn variant(&self, draw: QuadDraw, slots: PageSlots) -> &ClipVariant {
        match draw {
            QuadDraw::Fill(form) => &self.fills[form.idx() as usize],
            QuadDraw::Shadow(entry) => &self.shadows[entry as usize][slots.idx()],
        }
    }

    const fn variant_mut(&mut self, draw: QuadDraw, slots: PageSlots) -> &mut ClipVariant {
        match draw {
            QuadDraw::Fill(form) => &mut self.fills[form.idx() as usize],
            QuadDraw::Shadow(entry) => &mut self.shadows[entry as usize][slots.idx()],
        }
    }

    /// The pipeline a quad of `draw` draws through in a frame of `clipping` over the open mask
    /// `slots`, which the frame's census built.
    pub(crate) const fn get(
        &self,
        draw: QuadDraw,
        clipping: Clipping,
        slots: PageSlots,
    ) -> &Pipeline {
        self.variant(draw, slots).get(clipping)
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
    pub(crate) clear: Option<QuadInstance>,
    /// The debug overlay's dim over what the frame keeps.
    pub(crate) dim: Option<QuadInstance>,
    /// The debug overlay's damage outlines.
    pub(crate) outlines: &'a [QuadInstance],
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
    fn lay(base: u32, extras: QuadExtras<'_>, tail: &mut Vec<QuadInstance>) -> Self {
        let QuadExtras {
            clear,
            dim,
            outlines,
        } = extras;
        tail.clear();
        let mut push = |items: &[QuadInstance]| {
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
pub(crate) fn pre_clear(viewport: Vec2, color: RgbaF16) -> QuadInstance {
    debug_assert!(
        color.is_opaque(),
        "a translucent pre-clear blends over last frame"
    );
    QuadInstance {
        fill: color,
        ..QuadInstance::of_rect(Rect::new(0.0, 0.0, viewport.x, viewport.y))
    }
}

/// Format-independent quad resources; per-format pipelines are [`QuadVariants`].
#[derive(Debug)]
pub(crate) struct QuadPipeline {
    instance_buffer: DynamicBuffer<QuadInstance>,
    /// Each uploaded quad's draw, parallel to the instance buffer.
    draws: Vec<QuadDraw>,
    /// The draws in [`Self::draws`].
    taken: QuadDraws,
    /// The frame's [`QuadExtras`], laid out after its quads in the instance buffer.
    tail: Vec<QuadInstance>,
    /// Where the last upload put each extra.
    extras: ExtraSpans,
    shader: wgpu::ShaderModule,
    /// Format-independent, so built once here rather than per format.
    pipeline_layout: wgpu::PipelineLayout,
    /// [`Self::pipeline_layout`] plus the mask pages at group 1, for the shadow variant.
    shadow_layout: wgpu::PipelineLayout,
    /// The shadows' baked corner cutouts.
    cutouts: CutoutTables,
    /// How shadows draw (`vs_shadow`): [`ShapeCells::OneQuad`] only in internals, which build
    /// the one-quad reference.
    shadow_cells: ShapeCells,
    /// How the forms draw (`vs_template`): [`ShapeCells::OneQuad`] only in internals, which build
    /// the one-quad reference.
    shape_cells: ShapeCells,
    /// The template's triangles over its points, which every form but the one-quad reference
    /// draws.
    indices: TemplateIndices,
}

impl QuadPipeline {
    /// This family's shader body.
    pub(crate) const SHADER: ShaderBody = ShaderBody {
        label: "palantir.quad.shader",
        wgsl: include_str!("quad.wgsl"),
        includes: &[
            wgsl::FRAME,
            wgsl::ATLAS_SLOTS,
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
        self.draw_boxes(rec, pipeline, at..at + 1);
    }

    /// Bind `pipeline`, a solid box's, over the instance stream and draw `instances` of it.
    pub(crate) fn draw_boxes<'a>(
        &'a self,
        rec: &mut PassRecorder<'a>,
        pipeline: &'a Pipeline,
        instances: Range<u32>,
    ) {
        self.bind(rec, pipeline);
        self.draw_fill(rec, FillForm::BOX_SOLID, instances);
    }

    /// Draw `instances` of `form` through the pipeline bound.
    fn draw_fill<'a>(&'a self, rec: &mut PassRecorder<'a>, form: FillForm, instances: Range<u32>) {
        self.draw_shapes(rec, self.shape_cells, form.shape(), instances);
    }

    /// Draw `instances` of `form` as `cells` says: its whole template, indexed, or one quad.
    fn draw_shapes<'a>(
        &'a self,
        rec: &mut PassRecorder<'a>,
        cells: ShapeCells,
        form: ShapeForm,
        instances: Range<u32>,
    ) {
        match cells {
            ShapeCells::Template => {
                self.indices
                    .draw(rec, form, TemplateCover::Whole, instances);
            }
            #[cfg(feature = "internals")]
            ShapeCells::OneQuad => rec.draw(0..4, instances),
        }
    }

    /// Bind `pipeline` over the instance stream in the main pass, which holds the frame group.
    fn bind<'a>(&'a self, rec: &mut PassRecorder<'a>, pipeline: &'a Pipeline) {
        rec.pipeline(pipeline);
        rec.vertex(0, &self.instance_buffer.buffer);
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

    /// Build the format-independent quad resources, the shadows reading their cutout tables from
    /// `atlas`'s mask pages; per-format pipelines come from [`Self::ensure_draw`].
    pub(crate) fn new(gpu: &Gpu, textures: &TextureBinding, atlas: &AtlasTextures) -> Self {
        let shader = Self::SHADER.module(gpu);
        let cutouts = CutoutTables::new(gpu, &shader, atlas);

        let instance_buffer =
            DynamicBuffer::<QuadInstance>::vertex(gpu, "palantir.quad.instances", 256);

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
            shadow_cells: ShapeCells::Template,
            shape_cells: ShapeCells::Template,
            indices: TemplateIndices::new(gpu),
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
        let VariantTarget {
            format,
            clipping,
            slots,
        } = target;
        let instance = [Some(INSTANCE_LAYOUT)];
        let label = draw_label(draw);
        let variant = variants.variant_mut(draw, slots.mask);
        match draw {
            QuadDraw::Fill(form) => {
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
                        constants: &[
                            ("SHAPE_FORM", f64::from(form.shape() as u32)),
                            ("FILL_BRUSH", f64::from(form.brush() as u32)),
                        ],
                        vertex_buffers: &instance,
                        topology: self.shape_cells.topology(),
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
                                "SHADOW_TEMPLATE",
                                f64::from(u8::from(self.shadow_cells == ShapeCells::Template)),
                            ),
                            ("TABLES_SERIES", f64::from(u8::from(series))),
                            ("CUTOUT_SLOTS", f64::from(slots.mask.bits())),
                        ],
                        vertex_buffers: &[
                            Some(SHADOW_INSTANCE_LAYOUT),
                            Some(CutoutTables::CORNER_LAYOUT),
                        ],
                        topology: self.shadow_cells.topology(),
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
        quads: &Soa<QuadDrawRow>,
        cutouts: &CutoutPlan,
        extras: QuadExtras<'_>,
    ) {
        self.extras = ExtraSpans::lay(quads.len() as u32, extras, &mut self.tail);
        debug_assert!(
            quads
                .key()
                .iter()
                .zip(quads.instance())
                .all(|(key, quad)| key.holds_shape(&quad.header)),
            "a box quad left its points or its depth at their default (`QuadInstance::of_rect`)"
        );
        debug_assert!(
            self.tail.iter().all(|quad| quad.header.is_box()),
            "an extra is a box (`QuadInstance::of_rect`)"
        );
        self.instance_buffer
            .upload_parts(ctx, &[quads.instance(), &self.tail]);
        self.taken = quad_draw::fill(
            &mut self.draws,
            quads.key(),
            cutouts.entries(),
            self.shadow_cells,
        );
    }

    /// The draws the last uploaded frame's quads take.
    pub(crate) const fn taken(&self) -> QuadDraws {
        self.taken
    }

    /// Upload the shadows' corner stream and bake the tables `cutouts` plans into `atlas`'s mask
    /// pages, a pass a page on `ctx`'s encoder, after the pool's commit and before the main pass
    /// reads them.
    pub(crate) fn prepare_cutouts(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        cutouts: &CutoutPlan,
        atlas: &AtlasTextures,
    ) {
        self.cutouts.prepare(ctx, cutouts, atlas);
    }

    /// Make the cutout tables' group follow `atlas`, outside a frame's upload.
    pub(crate) fn follow_cutouts(&mut self, gpu: &Gpu, atlas: &AtlasTextures) {
        self.cutouts.follow(gpu, atlas);
    }

    /// Drop the cutout tables `pool` evicted this submit.
    pub(crate) fn end_frame(&mut self, pool: &mut AtlasPool) {
        self.cutouts.end_frame(pool);
    }

    /// Where the mask pages hold each key's table, for a window's plan to build in.
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
    /// and draws a box's template, its rim around its hole (`vs_shadow`).
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
            QuadDraw::Fill(form) => self.draw_fill(rec, form, instances.u32_range()),
            QuadDraw::Shadow(_) => {
                self.cutouts.bind(rec);
                self.draw_shapes(
                    rec,
                    self.shadow_cells,
                    ShapeForm::Box,
                    instances.u32_range(),
                );
            }
        }
    }
}

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
        vertex_attr(
            at,
            wgpu::VertexFormat::Uint32x2,
            offset_of!(QuadInstance, fill),
        ),
        vertex_attr(
            at + 1,
            wgpu::VertexFormat::Uint32x2,
            offset_of!(QuadInstance, border_color),
        ),
        vertex_attr(
            at + 2,
            wgpu::VertexFormat::Float32,
            offset_of!(QuadInstance, border_width),
        ),
        vertex_attr(
            at + 3,
            wgpu::VertexFormat::Uint32,
            offset_of!(QuadInstance, fill_kind),
        ),
        vertex_attr(
            at + 4,
            wgpu::VertexFormat::Uint32,
            offset_of!(QuadInstance, fill_lut_row),
        ),
        vertex_attr(
            at + 5,
            wgpu::VertexFormat::Uint32x2,
            offset_of!(QuadInstance, fill_axis),
        ),
    ]
};

const _: () = assert!(offset_of!(QuadInstance, header) == 0);

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<QuadInstance>(),
    wgpu::VertexStepMode::Instance,
    &QUAD_INSTANCE_ATTRS,
);

/// A shadow's header, then its own lanes, over the same stream at the same stride; the cutout
/// tables follow at the next location (`CutoutTables::CORNER_LAYOUT`).
const SHADOW_INSTANCE_ATTRS: [wgpu::VertexAttribute; 9] = {
    let h = shape_template::HEADER_ATTRS;
    let at = shape_template::HEADER_LOCATIONS;
    [
        h[0],
        h[1],
        h[2],
        h[3],
        h[4],
        h[5],
        vertex_attr(
            at,
            wgpu::VertexFormat::Uint32x2,
            offset_of!(ShadowInstance, color),
        ),
        vertex_attr(
            at + 1,
            wgpu::VertexFormat::Uint32,
            offset_of!(ShadowInstance, kind),
        ),
        vertex_attr(
            at + 2,
            wgpu::VertexFormat::Uint32x2,
            offset_of!(ShadowInstance, geometry),
        ),
    ]
};

const SHADOW_INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<ShadowInstance>(),
    wgpu::VertexStepMode::Instance,
    &SHADOW_INSTANCE_ATTRS,
);

/// The numbers [`QuadPipeline::SHADER`] takes from the Rust types that own them.
const SHADER_CONSTANTS: &[ShaderConstant] = &[
    ShaderConstant::uint("FILL_TAG_MASK", FillKind::TAG_MASK),
    ShaderConstant::uint("SPREAD_SHIFT", FillKind::SPREAD_SHIFT),
    ShaderConstant::uint("SPREAD_MASK", FillKind::SPREAD_MASK),
    ShaderConstant::uint("FILL_LINEAR", FillKind::TAG_LINEAR),
    ShaderConstant::uint("FILL_RADIAL", FillKind::TAG_RADIAL),
    ShaderConstant::uint("SHADOW_DROP", ShadowKind::DROP.bits()),
    ShaderConstant::uint("SHADOW_INSET", ShadowKind::INSET.bits()),
    ShaderConstant::uint("SHAPE_FORM_TRIANGLE", ShapeForm::Triangle as u32),
    ShaderConstant::uint("FILL_BRUSH_GRADIENT", FillBrush::Gradient as u32),
    shape_template::CONSTANT,
    // `Pad` isn't pinned: it is `palantir_apply_spread`'s fallback, also right for unknown modes.
    ShaderConstant::uint("SPREAD_REPEAT", Spread::Repeat as u32),
    ShaderConstant::uint("SPREAD_REFLECT", Spread::Reflect as u32),
    ShaderConstant::float("SHADOW_REACH_SIGMAS", ShadowGeometry::REACH_SIGMAS),
    ShaderConstant::float("CUTOUT_MIN_SIGMA", CutoutPlan::MIN_SIGMA),
    ShaderConstant::float("FILTER_SERIES_MIN_SIGMA", CutoutPlan::SERIES_MIN_SIGMA),
    ShaderConstant::float("CUTOUT_TEXELS_PER_SIGMA", CutoutPlan::TEXELS_PER_SIGMA),
    ShaderConstant::uint("CUTOUT_COORD_BITS", CutoutPlan::COORD_BITS),
    ShaderConstant::uint("CUTOUT_SIDE_SHIFT", CutoutPlan::SIDE_SHIFT),
    ShaderConstant::uint("CUTOUT_PAGE_SHIFT", CutoutPlan::PAGE_SHIFT),
    ShaderConstant::uint("NO_CUTOUT_TABLE", CutoutPlan::NONE),
    ShaderConstant::uint("CUTOUT_NODES", CutoutPlan::SHADED_NODES),
    ShaderConstant::uint("CUTOUT_BAKE_NODES", CutoutPlan::BAKED_NODES),
];

#[cfg(feature = "internals")]
pub(crate) mod internals {
    #[cfg(test)]
    use crate::gpu::atlas::page_slots::PageSlots;
    #[cfg(test)]
    use crate::gpu::pipeline::clip_variant::Clipping;
    use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
    #[cfg(test)]
    use crate::gpu::pipeline::quad_pipeline::QuadVariants;
    #[cfg(test)]
    use crate::gpu::pipeline::quad_pipeline::quad_draw::QuadDraw;
    use crate::gpu::pipeline::quad_pipeline::quad_draw::ShapeCells;

    #[cfg(test)]
    impl QuadVariants {
        /// Whether the half of `draw`'s pipelines over the open mask `slots` `clipping` binds is
        /// built.
        pub(crate) const fn is_built(
            &self,
            draw: QuadDraw,
            clipping: Clipping,
            slots: PageSlots,
        ) -> bool {
            self.variant(draw, slots).is_built(clipping)
        }
    }

    impl QuadPipeline {
        /// The cutout tables the mask pages hold baked in part or whole.
        #[cfg(test)]
        pub(crate) fn baked_cutout_tables(&self) -> usize {
            self.cutouts.baked_len()
        }

        /// Draw every shadow as one quad of the full form, the reference the template is compared
        /// with. Affects variants built afterwards.
        pub(crate) const fn disable_shadow_template(&mut self) {
            self.shadow_cells = ShapeCells::OneQuad;
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
    use crate::gpu::pipeline::quad_pipeline::{ExtraSpans, QuadExtras, QuadPipeline};
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::paint::color::RgbaF32;
    use crate::renderer::render_buffer::quad_draw_row::QuadInstance;

    /// The extras follow the frame's 3 quads in order: the clear at 3, the outline at 4; no dim
    /// takes no instance. A frame with none lays nothing and its clear reads
    /// `None`, so a pre-clear it forgot cannot draw the last frame's.
    #[test]
    fn extras_follow_the_quads_in_order() {
        let quad = |x: f32| QuadInstance::of_rect(Rect::new(x, 0.0, 1.0, 1.0));
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

    /// A shadow builds a pipeline for each set of open mask slots, the set it is asked for alone,
    /// and every set compiles: after the sets up to one, those and no later ones are built. So do
    /// the wide entry and the general one, which reads the tables its shadow's other corners hold:
    /// built over slot 0, neither is built over slot 1 until asked. A fill reads no page, so its
    /// one pipeline serves every set.
    #[test]
    fn a_table_shadow_builds_one_pipeline_a_set_of_mask_slots() {
        use crate::gpu::atlas::atlas_textures::AtlasTextures;
        use crate::gpu::atlas::page_slots::{AtlasSlots, PageSlots};
        use crate::gpu::pipeline::clip_variant::Clipping;
        use crate::gpu::pipeline::instanced_pipeline::VariantTarget;
        use crate::gpu::pipeline::quad_pipeline::QuadVariants;
        use crate::gpu::pipeline::quad_pipeline::cutout_plan::ShadowEntry;
        use crate::gpu::pipeline::quad_pipeline::quad_draw::QuadDraw;
        use crate::gpu::resource::texture_binding::TextureBinding;
        use crate::gpu::test_gpu::headless_test_gpu;
        use crate::gpu::texel::TargetFormat;
        use crate::renderer::render_buffer::fill_form::FillForm;

        let gpu = headless_test_gpu();
        let program =
            QuadPipeline::new(&gpu, &TextureBinding::new(&gpu), &AtlasTextures::new(&gpu));
        let mut variants = QuadVariants::NONE;
        let tables = QuadDraw::Shadow(ShadowEntry::Tables);
        let fill = QuadDraw::Fill(FillForm::BOX_SOLID);
        let target = |mask| VariantTarget {
            format: TargetFormat::RGBA8_SRGB.get(),
            clipping: Clipping::Off,
            slots: AtlasSlots {
                mask,
                color: PageSlots::FIRST,
            },
        };
        for slots in PageSlots::all() {
            program.ensure_draw(&mut variants, &gpu, target(slots), tables);
            for other in PageSlots::all() {
                assert_eq!(
                    variants.is_built(tables, Clipping::Off, other),
                    other.idx() <= slots.idx(),
                    "built through {slots:?}, asking {other:?}"
                );
            }
        }
        let second = PageSlots::new(0b10);
        for entry in [ShadowEntry::TablesWide, ShadowEntry::General] {
            let shadow = QuadDraw::Shadow(entry);
            program.ensure_draw(&mut variants, &gpu, target(PageSlots::FIRST), shadow);
            assert!(variants.is_built(shadow, Clipping::Off, PageSlots::FIRST));
            assert!(
                !variants.is_built(shadow, Clipping::Off, second),
                "{entry:?} over slot 0 reads no table on slot 1"
            );
            program.ensure_draw(&mut variants, &gpu, target(second), shadow);
            assert!(variants.is_built(shadow, Clipping::Off, second));
        }
        program.ensure_draw(&mut variants, &gpu, target(PageSlots::FIRST), fill);
        for slots in PageSlots::all() {
            assert!(variants.is_built(fill, Clipping::Off, slots));
        }
    }

    /// Each fill form's pipeline is labeled by its own shape and brush, so a capture names the
    /// form it shows.
    #[test]
    fn each_fill_form_labels_its_shape_and_brush() {
        use crate::gpu::pipeline::quad_pipeline::draw_label;
        use crate::gpu::pipeline::quad_pipeline::quad_draw::QuadDraw;
        use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
        use crate::renderer::render_buffer::shape_form::ShapeForm;

        for form in FillForm::ALL {
            let shape = match form.shape() {
                ShapeForm::Box => "box",
                ShapeForm::Triangle => "triangle",
                ShapeForm::Rect => unreachable!("no fill draws a rect"),
            };
            let brush = match form.brush() {
                FillBrush::Solid => "solid",
                FillBrush::Gradient => "gradient",
            };
            assert_eq!(
                draw_label(QuadDraw::Fill(form)),
                format!("palantir.quad.pipeline.{shape}_{brush}"),
            );
        }
    }

    /// A fill is transparent, and its template's interior collapses, at an alpha of zero of
    /// either sign whatever its color; the smallest f16 above zero, `2⁻²⁴ ≈ 5.96e-8`, and an opaque fill
    /// paint. On the GPU, over the packing the instance carries.
    #[test]
    fn a_transparent_fill_is_its_alpha_at_zero() {
        const PROBE: &str = "
@group(0) @binding(14) var<storage, read> probe_in: array<vec2<u32>>;
@group(0) @binding(15) var<storage, read_write> probe_out: array<u32>;
@compute @workgroup_size(1)
fn probe(@builtin(global_invocation_id) id: vec3<u32>) {
    probe_out[id.x] = u32(fill_is_transparent(probe_in[id.x]));
}
";
        let pack = |color: RgbaF32| bytemuck::cast::<_, [u32; 2]>(color.to_f16());
        let smallest = RgbaF32::new(0.0, 0.0, 0.0, 5.960_464_5e-8);
        let cases = [
            (RgbaF32::TRANSPARENT, 1),
            (RgbaF32::new(1.0, 0.5, 0.25, 0.0), 1),
            (RgbaF32::new(0.0, 0.0, 0.0, -0.0), 1),
            (smallest, 0),
            (RgbaF32::new(0.2, 0.4, 0.6, 1.0), 0),
        ];
        let inputs: Vec<[u32; 2]> = cases.iter().map(|&(color, _)| pack(color)).collect();
        let got: Vec<u32> = QuadPipeline::SHADER.probe(PROBE, &inputs);
        let want: Vec<u32> = cases.iter().map(|&(_, want)| want).collect();
        assert_eq!(got, want);
    }
}
