//! The app-global resources of one host — what every window's recorder,
//! the one frontend and the one backend are built over: the shaper, the
//! image and icon registries, the gradient atlas, the clipboard, the window
//! directory, the diagnostics flags and the user scale.
//!
//! Every field is clone-shared, so two recorders in two windows resolve
//! the same font, the same texture, the same overlay toggle and the same
//! scale, and the backend drains the registries those recorders fill.
//! The one constructor mints every handle; a bundle no backend is built
//! over is a standalone CPU recorder, and needs nothing further.

use std::rc::Rc;

use crate::common::app_setting::AppSetting;
use crate::common::clipboard::Clipboard;
use crate::diagnostics::Diagnostics;
use crate::display::user_scale::UserScale;
use crate::icons::icon_registry::IconRegistry;
use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::renderer::texture_limit::{RegisterImageError, TextureLimit};
use crate::text::shaper::TextShaper;
use crate::window::window_directory::WindowDirectory;

/// The host's app-global handles. Every field is clone-shared; frame-local
/// scene and layout state remain directly on `Ui`.
#[derive(Clone, Debug)]
pub(crate) struct UiResources {
    text: TextShaper,
    images: ImageRegistry,
    icons: IconRegistry,
    /// The frontend bakes gradients into it and the backend uploads them.
    /// Held here, where no recorder reads it, so the one list of handles a
    /// host shares is this struct and not this struct plus a loose atlas.
    gradient_atlas: SharedGradientAtlas,
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
    pub(crate) fn new(text: TextShaper, clipboard: Clipboard, texture_limit: TextureLimit) -> Self {
        Self {
            text,
            images: ImageRegistry::default(),
            icons: IconRegistry::default(),
            gradient_atlas: SharedGradientAtlas::new(texture_limit),
            texture_limit,
            clipboard,
            diagnostics: Diagnostics::default(),
            user_scale: Rc::default(),
            windows: WindowDirectory::default(),
        }
    }

    pub(crate) fn text(&self) -> &TextShaper {
        &self.text
    }

    pub(crate) fn images(&self) -> &ImageRegistry {
        &self.images
    }

    pub(crate) fn icons(&self) -> &IconRegistry {
        &self.icons
    }

    pub(crate) fn gradient_atlas(&self) -> &SharedGradientAtlas {
        &self.gradient_atlas
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
            TextureId::reserve(),
            image,
            self.images.clone(),
        ))
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::common::clipboard::Clipboard;
    use crate::renderer::texture_limit::TextureLimit;
    use crate::text::shaper::TextShaper;
    use crate::ui::resources::UiResources;

    impl UiResources {
        /// Recorder capabilities that share nothing with any other
        /// recorder: a mono-fallback shaper (deterministic metrics, wrong for
        /// width-follows-label), a memory clipboard, and no texture cap. The
        /// real-measurement peer is [`UiResources::new`] over a shaper of
        /// the test's own, which is also what pairs two recorders onto one
        /// text cache.
        pub(crate) fn isolated_mono() -> Self {
            Self::new(
                TextShaper::test_mono(),
                Clipboard::default(),
                TextureLimit::default(),
            )
        }
    }
}

#[cfg(test)]
mod tests;
