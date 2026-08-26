//! App-global resource composition for hosts. [`HostShared`] retains recorder
//! resources and the frontend's gradient-atlas handle, then derives the
//! backend's capability bundle from those shared authorities.

use crate::common::clipboard::Clipboard;
use crate::renderer::backend::backend_resources::BackendResources;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;
use crate::ui::resources::UiResources;

#[derive(Debug)]
pub(crate) struct HostShared {
    pub(crate) resources: UiResources,
    pub(crate) gradient_atlas: SharedGradientAtlas,
}

impl HostShared {
    pub(super) fn with_clipboard(
        text: TextShaper,
        clipboard: Clipboard,
        texture_limit: TextureLimit,
    ) -> Self {
        Self {
            resources: UiResources::new(text, clipboard, texture_limit),
            gradient_atlas: SharedGradientAtlas::new(texture_limit),
        }
    }

    pub(super) fn backend_resources(&self) -> BackendResources {
        BackendResources {
            text: self.resources.text.clone(),
            images: self.resources.images.clone(),
            icons: self.resources.icons.clone(),
            gradient_atlas: self.gradient_atlas.clone(),
            gpu_pass_stats: self.resources.diagnostics.gpu_pass_stats.clone(),
        }
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::common::clipboard::Clipboard;
    use crate::host::shared::HostShared;
    use crate::renderer::texture_limit::TextureLimit;
    use crate::text::shaper::TextShaper;

    impl HostShared {
        /// Resources over a memory clipboard, for tests that drive a
        /// `WindowDriver` without a host. Production hosts go through
        /// `HostCore::new`, which supplies the platform clipboard.
        pub(crate) fn new(text: TextShaper, texture_limit: TextureLimit) -> Self {
            Self::with_clipboard(text, Clipboard::default(), texture_limit)
        }
    }
}

#[cfg(test)]
mod tests;
