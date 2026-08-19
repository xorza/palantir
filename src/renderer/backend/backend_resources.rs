//! The app-global render handles a backend is built from.

use crate::diagnostics::gpu_pass_stats::GpuPassStats;
use crate::icons::icon_registry::IconRegistry;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::image_registry::ImageRegistry;
use crate::text::shaper::TextShaper;

#[derive(Debug)]
pub(crate) struct BackendResources {
    pub(crate) text: TextShaper,
    pub(crate) images: ImageRegistry,
    pub(crate) icons: IconRegistry,
    pub(crate) gradient_atlas: SharedGradientAtlas,
    pub(crate) gpu_pass_stats: GpuPassStats,
}
