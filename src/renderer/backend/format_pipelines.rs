//! [`FormatPipelines`] — every format-dependent `wgpu::RenderPipeline`
//! for one swapchain color format, bundled so the rest of the backend's
//! GPU state (shaders, vertex/instance buffers, the glyph + gradient
//! atlases, the image texture cache) stays format-independent and shared.
//!
//! The pipeline objects are the *only* thing that carries the color
//! target's format; pulling them out here lets a single set of resources
//! drive any number of formats — a window on an sRGB output and one on an
//! HDR output share every atlas and buffer, differing only in which
//! `FormatPipelines` their draws bind. Built eagerly (both the base and
//! the stencil-test twin of each kind) so the set is complete the moment
//! it exists.

use crate::renderer::backend::curve_pipeline::CurvePipeline;
use crate::renderer::backend::image_pipeline::ImagePipeline;
use crate::renderer::backend::mesh_pipeline::MeshPipeline;
use crate::renderer::backend::pipeline_utils::StencilVariant;
use crate::renderer::backend::quad_pipeline::QuadPipeline;
use crate::renderer::backend::stencil::stencil_test_state;
use crate::renderer::backend::text::TextBackend;

/// All render pipelines built against one swapchain color format. Keyed
/// by [`Self::color_format`] in the backend so windows on different-format
/// outputs each bind the right set while sharing every other resource.
pub(crate) struct FormatPipelines {
    pub(crate) quad: StencilVariant,
    /// Quad-only stencil mask-write variant (paints the rounded SDF into
    /// the stencil buffer; mesh/image/curve read the mask, never write).
    pub(crate) quad_mask_write: wgpu::RenderPipeline,
    pub(crate) mesh: StencilVariant,
    pub(crate) image: StencilVariant,
    pub(crate) curve: StencilVariant,
    /// Text pipelines indexed by `StencilMode::pipeline_idx` (plain,
    /// stencil-test); built from `TextBackend::build_pipelines`.
    pub(crate) text: Vec<wgpu::RenderPipeline>,
}

impl FormatPipelines {
    /// Build every pipeline for `format`, reading shaders + layouts off
    /// the shared, format-independent resource structs. `gradient_bgl` is
    /// the shared group-0 layout (quad/curve).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        gradient_bgl: &wgpu::BindGroupLayout,
        quad: &QuadPipeline,
        mesh: &MeshPipeline,
        image: &ImagePipeline,
        curve: &CurvePipeline,
        text: &TextBackend,
    ) -> Self {
        Self {
            quad: QuadPipeline::build_variants(device, &quad.shader, gradient_bgl, format),
            quad_mask_write: QuadPipeline::build_mask_write(
                device,
                &quad.shader,
                gradient_bgl,
                format,
            ),
            mesh: MeshPipeline::build_variants(device, &mesh.shader, format),
            image: ImagePipeline::build_variants(device, &image.shader, &image.image_bgl, format),
            curve: CurvePipeline::build_variants(device, &curve.shader, gradient_bgl, format),
            // Index order matches `StencilMode::pipeline_idx`:
            // 0 = Plain, 1 = Stencil.
            text: TextBackend::build_pipelines(
                device,
                &text.atlas_bgl,
                format,
                &[None, Some(stencil_test_state())],
            ),
        }
    }
}
