//! The app-global handles a backend is built over.

use crate::diagnostics::gpu_pass_stats::GpuPassStats;
use crate::icons::icon_registry::IconRegistry;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::image_registry::ImageRegistry;
use crate::text::shaper::TextShaper;

/// The subset of the host's resources the backend connects to, as a view: the backend sits below the recorder and must not name its bundle. It clones what it keeps and publishes into the timing sample; the image registry flows the other way, taking the backend's texture store through this borrow before the host mints a recorder.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BackendResources<'a> {
    pub(crate) text: &'a TextShaper,
    pub(crate) images: &'a ImageRegistry,
    pub(crate) icons: &'a IconRegistry,
    pub(crate) gradient_atlas: &'a SharedGradientAtlas,
    pub(crate) gpu_pass_stats: &'a GpuPassStats,
}
