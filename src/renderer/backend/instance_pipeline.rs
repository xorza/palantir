//! The shape every instanced draw pipeline shares — built once at startup,
//! then bound and replayed per batch.

use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::stencil_variant::StencilVariant;

/// The shape every instanced draw pipeline in the backend has: build once
/// per device, rebuild its color variants per swapchain format, take one
/// upload per frame, bind once per pass, then draw a batch at a time.
///
/// [`QuadPipeline`], [`MeshPipeline`], [`ImagePipeline`] and
/// [`CurvePipeline`] each wrote this shape out under a name of its own —
/// `upload` against `upload_instances`, `draw_range` against `draw_batch`
/// against `draw` — so the five methods were a contract only a reader
/// comparing four files could see. Here the names are one declaration, and
/// a fifth pipeline that misses a step does not compile.
///
/// The four associated types are where the pipelines genuinely differ, and
/// each is named rather than spelled out at the call site. Everything past
/// them — the shared immediate region the viewport rides, the
/// base/stencil-test pair a `use_stencil` flag selects — is the same for
/// all four and is stated once, here.
///
/// [`QuadPipeline`]: crate::renderer::backend::quad_pipeline::QuadPipeline
/// [`MeshPipeline`]: crate::renderer::backend::mesh_pipeline::MeshPipeline
/// [`ImagePipeline`]: crate::renderer::backend::image_pipeline::ImagePipeline
/// [`CurvePipeline`]: crate::renderer::backend::curve_pipeline::CurvePipeline
pub(super) trait InstancePipeline: Sized {
    /// What [`Self::build_variants`] needs past the device and the format:
    /// the gradient atlas's group-0 layout for the pipelines that read it,
    /// `()` for the ones that own group 0 themselves.
    type Layouts<'a>: Copy
    where
        Self: 'a;

    /// What one frame's upload hands over — a slice of instances for the
    /// pipelines with a single stream, a named bundle for the one with
    /// three.
    type Upload<'a>: Copy
    where
        Self: 'a;

    /// What [`Self::bind`] needs past the pass and the variant: the shared
    /// gradient bind group, or `()` where group 0 is set per draw.
    type Bindings<'a>: Copy
    where
        Self: 'a;

    /// What one [`Self::draw`] covers — an instance span, or the frame's
    /// whole per-draw column beside the slice of it this batch owns.
    type Batch<'a>: Copy
    where
        Self: 'a;

    /// The format-independent resources: shader module and the dynamic
    /// buffers the frame uploads into.
    fn new(device: &wgpu::Device) -> Self;

    /// This pipeline's per-instance vertex buffer layout, which
    /// [`Self::build_variants`] reads. Associated rather than free so the
    /// four cannot drift apart by name.
    fn instance_layout() -> wgpu::VertexBufferLayout<'static>;

    /// Build the base and stencil-test color pipelines against `format` —
    /// the only format-dependent objects a pipeline owns. Called by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// once per swapchain format.
    fn build_variants(
        &self,
        device: &wgpu::Device,
        layouts: Self::Layouts<'_>,
        format: wgpu::TextureFormat,
    ) -> StencilVariant;

    /// Sync this frame's instances into the dynamic buffers, once per
    /// submit and ahead of the render pass.
    fn upload(&mut self, ctx: &mut GpuCtx<'_>, upload: Self::Upload<'_>);

    /// Set the pipeline and the buffers that stand for a whole pass.
    /// `use_stencil` selects the stencil-test variant over the base.
    fn bind<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        variant: &'a StencilVariant,
        use_stencil: bool,
        bindings: Self::Bindings<'a>,
    );

    /// Issue the draws for one batch. The caller binds once and sets the
    /// scissor rect before each call.
    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, batch: Self::Batch<'_>);
}
