//! The app-global handles a backend is built over.

use crate::diagnostics::gpu_pass_stats::GpuPassStats;
use crate::icons::icon_registry::IconRegistry;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::image_registry::ImageRegistry;
use crate::text::shaper::TextShaper;

/// The subset of the host's resources the backend connects to, as a view
/// the host takes for it. A view rather than the resources themselves,
/// because the backend sits below the recorder in the module layering and
/// must not name its bundle.
///
/// The backend clones what it keeps: it rasterizes through the shaper,
/// drains the icon registry and the gradient atlas, and publishes into
/// the timing sample. The image registry flows the other way — the
/// backend attaches its texture store to the host's registry through this
/// borrow, before the host mints a recorder, so every later clone carries
/// the store.
#[derive(Debug)]
pub(crate) struct BackendResources<'a> {
    pub(crate) text: &'a TextShaper,
    pub(crate) images: &'a ImageRegistry,
    pub(crate) icons: &'a IconRegistry,
    pub(crate) gradient_atlas: &'a SharedGradientAtlas,
    pub(crate) gpu_pass_stats: &'a GpuPassStats,
}
