//! GPU side of native parametric strokes (cubic beziers + circular
//! arcs — see `CurveInstance::kind`). One `draw` per scissor
//! group covers every `CurveInstance` in the group's `CurveBatch` —
//! the vertex shader subdivides each instance into
//! [`SEGMENTS_PER_INSTANCE`](crate::renderer::render_buffer::SEGMENTS_PER_INSTANCE)
//! chords (96 vertices per instance, no index buffer) and offsets the
//! strip perpendicular to the tangent for stroking + AA.
//!
//! Same stencil-variant pattern as [`MeshPipeline`] /
//! [`ImagePipeline`]: rounded-clip frames use a stencil-test pipeline,
//! plain frames use the unconditional one.
//!
//! [`MeshPipeline`]: crate::renderer::backend::mesh_pipeline::MeshPipeline
//! [`ImagePipeline`]: crate::renderer::backend::image_pipeline::ImagePipeline

use crate::primitives::brush::Spread;
use crate::primitives::fill_wire::FillKind;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::{ColorVariantSpec, StencilVariant};
use crate::renderer::backend::shader_template::{ShaderConstant, specialize};
use crate::renderer::gradient_atlas::ATLAS_ROWS;
use crate::renderer::render_buffer::{
    CURVE_KIND_ARC, CURVE_KIND_CUBIC, CURVE_KIND_JOIN_BEVEL, CURVE_KIND_JOIN_MITER,
    CURVE_KIND_JOIN_ROUND, CURVE_KIND_SEGMENT, CurveInstance, HALF_FRINGE, MITER_LIMIT,
    SEGMENTS_PER_INSTANCE,
};
use crate::shape::LineCap;

/// Vertex count per instance — every instance is a 16-segment strip,
/// 6 vertices per segment (two triangles), no index buffer.
const VERTICES_PER_INSTANCE: u32 = 6 * SEGMENTS_PER_INSTANCE;

#[derive(Debug)]
pub(crate) struct CurvePipeline {
    instance_buffer: DynamicBuffer<CurveInstance>,
    /// Curve shader module — format-independent; [`Self::build_variants`]
    /// reads it to build each format's pipelines.
    shader: wgpu::ShaderModule,
}

impl CurvePipeline {
    /// Format-independent curve resources; the pipelines are built by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variant`].
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let wgsl = specialize(
            include_str!("curve.wgsl"),
            &[
                ShaderConstant::float("ATLAS_ROWS", ATLAS_ROWS as f32),
                ShaderConstant::uint("SEGMENTS_PER_INSTANCE", SEGMENTS_PER_INSTANCE),
                ShaderConstant::float("HALF_FRINGE", HALF_FRINGE),
                ShaderConstant::float("MITER_LIMIT", MITER_LIMIT),
                ShaderConstant::uint("CAP_BUTT", LineCap::Butt as u32),
                ShaderConstant::uint("CAP_SQUARE", LineCap::Square as u32),
                ShaderConstant::uint("CAP_ROUND", LineCap::Round as u32),
                ShaderConstant::uint("KIND_CUBIC", CURVE_KIND_CUBIC),
                ShaderConstant::uint("KIND_ARC", CURVE_KIND_ARC),
                ShaderConstant::uint("KIND_SEGMENT", CURVE_KIND_SEGMENT),
                ShaderConstant::uint("KIND_JOIN_ROUND", CURVE_KIND_JOIN_ROUND),
                ShaderConstant::uint("KIND_JOIN_BEVEL", CURVE_KIND_JOIN_BEVEL),
                ShaderConstant::uint("KIND_JOIN_MITER", CURVE_KIND_JOIN_MITER),
                ShaderConstant::uint("BRUSH_KIND_SOLID", FillKind::SOLID.0),
                ShaderConstant::uint("BRUSH_KIND_LINEAR", FillKind::linear(Spread::Pad).0),
            ],
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aperture.curve.shader"),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });

        let instance_buffer =
            DynamicBuffer::<CurveInstance>::vertex(device, "aperture.curve.instances", 64);

        Self {
            instance_buffer,
            shader,
        }
    }

    /// Build the base + stencil-test color pipelines against `format`.
    /// Caller passes the shared `gradient_bgl` (owned by
    /// `GradientResources`) so the layout matches; the instance buffer
    /// is format-independent. Called by `FormatPipelines` per format.
    pub(crate) fn build_variants(
        &self,
        device: &wgpu::Device,
        gradient_bgl: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        // Gradient at group 0 — viewport rides the shared immediate
        // region, no bind-group slot needed for it.
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "aperture.curve.pipeline",
                stencil_label: "aperture.curve.pipeline.stencil_test",
                layout_label: "aperture.curve.pl",
                shader: &self.shader,
                bind_group_layouts: &[Some(gradient_bgl)],
                vertex_buffers: &[Some(curve_instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleList,
            },
            format,
        )
    }

    #[profiling::function]
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, instances: &[CurveInstance]) {
        self.instance_buffer.upload_instances(ctx, instances);
    }

    /// Bind once per pass, before issuing one [`Self::draw`] per
    /// `CurveBatch`. Viewport rides the shared immediate region;
    /// `gradient_bg` is the group-0 handle owned by `GradientResources`
    /// (one allocation, used by both the quad and curve pipelines).
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

    /// Issue one non-indexed instanced draw covering every instance in
    /// the span (no index buffer — `vertex_index` maps directly to the
    /// 6 corners of each of `SEGMENTS_PER_INSTANCE` quads). This is the
    /// "one draw call per scissor group" terminus — the entire
    /// `CurveBatch` lands as a single GPU draw call.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, instances: std::ops::Range<u32>) {
        if instances.start == instances.end {
            return;
        }
        pass.draw(0..VERTICES_PER_INSTANCE, instances);
    }
}

// `p0/p1/p2/p3 : Float32x2`, `t_range : Float32x2`, `width : Float32`,
// `color0/color1 : Unorm8x4` (linear-u8, t=0 / t=1 stroke colours),
// `cap : Uint32` (per-end caps packed: bits 0..8 start, 8..16 end),
// `fill_kind : Uint32` (0 = solid, 1 = linear),
// `fill_lut_row : Uint32` (gradient atlas row when fill_kind != 0),
// `kind : Uint32` (basis tag — geometry-lane interpretation).
const CURVE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 12] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x2,
    3 => Float32x2,
    4 => Float32x2,
    5 => Float32,
    6 => Unorm8x4,
    7 => Unorm8x4,
    8 => Uint32,
    9 => Uint32,
    10 => Uint32,
    11 => Uint32,
];

// Compile-time guard: attribute offsets must match the `CurveInstance`
// fields they feed. `array_stride == size_of` alone wouldn't catch a
// same-size field reorder or a format/field size mismatch; `offset_of!`
// does. Attr 4 (`Float32x2`) spans the adjacent `t0`,`t1` pair — anchored
// at `t0`, bracketed by the `width` check that follows.
const _: () = {
    use std::mem::offset_of;
    assert!(CURVE_INSTANCE_ATTRS[0].offset == offset_of!(CurveInstance, p0) as u64);
    assert!(CURVE_INSTANCE_ATTRS[1].offset == offset_of!(CurveInstance, p1) as u64);
    assert!(CURVE_INSTANCE_ATTRS[2].offset == offset_of!(CurveInstance, p2) as u64);
    assert!(CURVE_INSTANCE_ATTRS[3].offset == offset_of!(CurveInstance, p3) as u64);
    assert!(CURVE_INSTANCE_ATTRS[4].offset == offset_of!(CurveInstance, t0) as u64);
    assert!(CURVE_INSTANCE_ATTRS[5].offset == offset_of!(CurveInstance, width) as u64);
    assert!(CURVE_INSTANCE_ATTRS[6].offset == offset_of!(CurveInstance, color0) as u64);
    assert!(CURVE_INSTANCE_ATTRS[7].offset == offset_of!(CurveInstance, color1) as u64);
    assert!(CURVE_INSTANCE_ATTRS[8].offset == offset_of!(CurveInstance, cap) as u64);
    assert!(CURVE_INSTANCE_ATTRS[9].offset == offset_of!(CurveInstance, fill_kind) as u64);
    assert!(CURVE_INSTANCE_ATTRS[10].offset == offset_of!(CurveInstance, fill_lut_row) as u64);
    assert!(CURVE_INSTANCE_ATTRS[11].offset == offset_of!(CurveInstance, kind) as u64);
};

fn curve_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<CurveInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &CURVE_INSTANCE_ATTRS,
    }
}
