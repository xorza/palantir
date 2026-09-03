//! The app-global capabilities a recorder is built over — the shaper, the
//! image and icon registries, the clipboard, the window directory and the
//! diagnostics flags.
//!
//! Every field is clone-shared, so two recorders in two windows resolve
//! the same font, the same texture and the same overlay toggle.

use crate::common::clipboard::Clipboard;
use crate::diagnostics::Diagnostics;
use crate::icons::icon_registry::IconRegistry;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::texture_id_source::TextureIdSource;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;
use crate::window::window_directory::WindowDirectory;

/// Capabilities available to a recorder. Every field is app-global and
/// clone-shared; frame-local scene and layout state remain directly on `Ui`.
#[derive(Clone, Debug)]
pub(crate) struct UiResources {
    pub(crate) text: TextShaper,
    pub(crate) images: ImageRegistry,
    pub(crate) icons: IconRegistry,
    pub(super) texture_ids: TextureIdSource,
    /// The device ceiling a registered image is measured against, and what
    /// `Ui::max_image_dimension` reports. Held beside the registry rather
    /// than inside it: the registry owns an `Rc`-shared upload/release
    /// queue, this is an immutable device constant, and the gradient atlas
    /// takes the same value from the same call site.
    pub(crate) texture_limit: TextureLimit,
    pub(crate) clipboard: Clipboard,
    pub(crate) diagnostics: Diagnostics,
    pub(crate) windows: WindowDirectory,
}

impl UiResources {
    pub(crate) fn new(text: TextShaper, clipboard: Clipboard, texture_limit: TextureLimit) -> Self {
        let texture_ids = TextureIdSource::default();
        Self {
            text,
            images: ImageRegistry::new(texture_ids.clone()),
            icons: IconRegistry::default(),
            texture_ids,
            texture_limit,
            clipboard,
            diagnostics: Diagnostics::default(),
            windows: WindowDirectory::default(),
        }
    }
}

#[cfg(any(test, feature = "internals"))]
impl UiResources {
    /// Recorder capabilities that share nothing with any other
    /// recorder: a mono-fallback shaper (deterministic metrics, wrong for
    /// width-follows-label), a memory clipboard, and no texture cap. The
    /// real-measurement peer goes through
    /// [`crate::host::shared::HostShared`], which is also what pairs two
    /// recorders onto one text cache.
    pub(crate) fn isolated_mono() -> Self {
        Self::new(
            TextShaper::test_mono(),
            Clipboard::default(),
            TextureLimit::default(),
        )
    }
}

#[cfg(test)]
mod tests;
