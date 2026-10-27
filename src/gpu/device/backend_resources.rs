//! The app-global handles a backend is built over.

use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::icons::icon_registry::IconRegistry;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::text::shaper::TextShaper;
use std::rc::Rc;

/// The subset of the host's resources the backend connects to, as a view: the backend sits below the recorder and must not name its bundle. It clones what it keeps. The image store is here because the host builds it first, for the registry its recorders write through.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BackendResources<'a> {
    pub(crate) text: &'a TextShaper,
    pub(crate) image_store: &'a Rc<WgpuImageStore>,
    pub(crate) icons: &'a IconRegistry,
    pub(crate) gradient_atlas: &'a SharedGradientAtlas,
}
