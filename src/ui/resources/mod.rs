//! The app-global capabilities a recorder is built over — the shaper, the
//! image and icon registries, the clipboard, the window directory, the
//! diagnostics flags and the user scale.
//!
//! Every field is clone-shared, so two recorders in two windows resolve
//! the same font, the same texture, the same overlay toggle and the same
//! scale.

use std::rc::Rc;

use crate::common::app_setting::AppSetting;
use crate::common::clipboard::Clipboard;
use crate::diagnostics::Diagnostics;
use crate::display::user_scale::UserScale;
use crate::icons::icon_registry::IconRegistry;
use crate::primitives::image::Image;
use crate::renderer::backend::shared_resources::SharedResources;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::renderer::texture_id_source::TextureIdSource;
use crate::renderer::texture_limit::{RegisterImageError, TextureLimit};
use crate::text::shaper::TextShaper;
use crate::window::window_directory::WindowDirectory;

/// Capabilities available to a recorder. Every field is app-global and
/// clone-shared; frame-local scene and layout state remain directly on `Ui`.
#[derive(Clone, Debug)]
pub(crate) struct UiResources {
    text: TextShaper,
    images: ImageRegistry,
    icons: IconRegistry,
    texture_ids: TextureIdSource,
    /// The device ceiling a registered image is measured against, and what
    /// `Ui::max_image_dimension` reports. Held beside the registry rather
    /// than inside it: this is an immutable device constant, and the
    /// gradient atlas takes the same value from the same call site.
    texture_limit: TextureLimit,
    clipboard: Clipboard,
    diagnostics: Diagnostics,
    /// The one scale every window's `Display` is minted from.
    ///
    /// App-global rather than per window: the per-monitor case is already
    /// answered by `Display::system_scale`, which the platform reports per
    /// window, so what is left is a preference — and two windows of one
    /// application disagreeing about a preference is not a picture anyone
    /// asks for.
    user_scale: Rc<AppSetting<UserScale>>,
    windows: WindowDirectory,
}

impl UiResources {
    pub(crate) fn new(shared: SharedResources, clipboard: Clipboard) -> Self {
        Self {
            text: shared.text,
            images: shared.images,
            icons: shared.icons,
            texture_ids: TextureIdSource::default(),
            texture_limit: shared.texture_limit,
            clipboard,
            diagnostics: Diagnostics::new(shared.gpu_pass_stats),
            user_scale: Rc::default(),
            windows: WindowDirectory::default(),
        }
    }

    pub(crate) fn text(&self) -> &TextShaper {
        &self.text
    }

    pub(super) fn icons(&self) -> &IconRegistry {
        &self.icons
    }

    /// The one id authority for registered images and `GpuView` targets.
    pub(super) fn texture_ids(&self) -> &TextureIdSource {
        &self.texture_ids
    }

    pub(crate) fn texture_limit(&self) -> TextureLimit {
        self.texture_limit
    }

    pub(crate) fn clipboard(&self) -> &Clipboard {
        &self.clipboard
    }

    pub(crate) fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    pub(crate) fn user_scale(&self) -> &AppSetting<UserScale> {
        &self.user_scale
    }

    pub(crate) fn windows(&self) -> &WindowDirectory {
        &self.windows
    }

    pub(super) fn register_image(&self, image: &Image) -> Result<ImageHandle, RegisterImageError> {
        self.texture_limit.accepts(image.size)?;
        Ok(ImageHandle::new(
            self.texture_ids.reserve(),
            image,
            self.images.clone(),
        ))
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::common::clipboard::Clipboard;
    use crate::renderer::backend::shared_resources::SharedResources;
    use crate::renderer::texture_limit::TextureLimit;
    use crate::text::shaper::TextShaper;
    use crate::ui::resources::UiResources;

    impl UiResources {
        /// Recorder capabilities that share nothing with any other
        /// recorder: a mono-fallback shaper (deterministic metrics, wrong for
        /// width-follows-label), a memory clipboard, no texture cap, and a
        /// deviceless registry. The real-measurement peer goes through
        /// [`crate::host::shared::HostShared`], which is also what pairs two
        /// recorders onto one text cache.
        pub(crate) fn isolated_mono() -> Self {
            Self::new(
                SharedResources::deviceless(TextShaper::test_mono(), TextureLimit::default()),
                Clipboard::default(),
            )
        }
    }
}

#[cfg(test)]
mod tests;
