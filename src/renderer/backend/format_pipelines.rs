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
use crate::renderer::backend::icon::IconBackend;
use crate::renderer::backend::image_pipeline::ImagePipeline;
use crate::renderer::backend::mesh_pipeline::MeshPipeline;
use crate::renderer::backend::quad_pipeline::{QuadPipeline, QuadVariants};
use crate::renderer::backend::stencil_variant::StencilVariant;
use crate::renderer::backend::text::TextBackend;

/// All render pipelines built against one swapchain color format. Keyed
/// by [`wgpu::TextureFormat`] in the backend so windows on different-format
/// outputs each bind the right set while sharing every other resource.
#[derive(Debug)]
pub(super) struct FormatPipelines {
    pub(super) quad: QuadVariants,
    pub(super) mesh: StencilVariant,
    pub(super) image: StencilVariant,
    /// Icon base + stencil-test pipelines. Same shader and layout as `text`
    /// — the two differ only in which atlas they bind — but a separate
    /// pipeline object, because each carries its own bind-group layout.
    pub(super) icon: StencilVariant,
    pub(super) curve: StencilVariant,
    /// Text base + stencil-test pipelines; selected by `use_stencil` like
    /// the other four. Built from `TextBackend::build_variants`.
    pub(super) text: StencilVariant,
}

/// The format-independent resource structs [`FormatPipelines::new`] reads shaders
/// and layouts off. They live side by side on the backend and are handed over as a
/// set, so a new pipeline kind is one field here rather than one more parameter at
/// every call.
#[derive(Debug)]
pub(super) struct PipelineSources<'a> {
    /// The shared group-0 layout (quad/curve).
    pub(super) gradient_bgl: &'a wgpu::BindGroupLayout,
    pub(super) quad: &'a QuadPipeline,
    pub(super) mesh: &'a MeshPipeline,
    pub(super) image: &'a ImagePipeline,
    pub(super) icon: &'a IconBackend,
    pub(super) curve: &'a CurvePipeline,
    pub(super) text: &'a TextBackend,
}

impl FormatPipelines {
    /// Build every pipeline for `format`, reading shaders + layouts off
    /// the shared, format-independent resource structs.
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        sources: PipelineSources<'_>,
    ) -> Self {
        let PipelineSources {
            gradient_bgl,
            quad,
            mesh,
            image,
            icon,
            curve,
            text,
        } = sources;
        Self {
            quad: quad.build_variants(device, gradient_bgl, format),
            mesh: mesh.build_variants(device, format),
            image: image.build_variants(device, format),
            icon: icon.pass.build_variants(device, format),
            curve: curve.build_variants(device, gradient_bgl, format),
            text: text.pass.build_variants(device, format),
        }
    }
}
