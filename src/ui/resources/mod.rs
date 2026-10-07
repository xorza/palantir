//! The app-global resources of one host, shared by every window's recorder, the one frontend and the one backend:
//! shaper, image and icon registries, gradient atlas, clipboard, window directory, diagnostics and user scale. Every
//! field is clone-shared, so recorders in two windows resolve the same font, texture and scale.

use std::rc::Rc;

use crate::common::app_setting::AppSetting;
use crate::common::clipboard::Clipboard;
use crate::diagnostics::Diagnostics;
use crate::display::user_scale::UserScale;
use crate::icons::icon_registry::IconRegistry;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::image::Image;
use crate::renderer::error::ImageTooLarge;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;
use crate::window::window_directory::WindowDirectory;

#[derive(Clone, Debug)]
pub(crate) struct UiResources {
    text: TextShaper,
    images: ImageRegistry,
    icons: IconRegistry,
    /// The frontend bakes gradients into it and the backend uploads them; held here so this struct is the one
    /// list of handles a host shares.
    gradient_atlas: SharedGradientAtlas,
    /// The device ceiling a registered image is measured against (`Ui::max_image_dimension`), an immutable
    /// constant held beside the registry; the gradient atlas takes it from the same call site.
    texture_limit: TextureLimit,
    clipboard: Clipboard,
    diagnostics: Diagnostics,
    /// The one scale every window's `Display` is minted from; app-global because per-monitor scale is already
    /// `Display::system_scale`, leaving a preference windows should not disagree about.
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

    pub(crate) const fn text(&self) -> &TextShaper {
        &self.text
    }

    pub(crate) const fn images(&self) -> &ImageRegistry {
        &self.images
    }

    pub(crate) const fn icons(&self) -> &IconRegistry {
        &self.icons
    }

    pub(crate) const fn gradient_atlas(&self) -> &SharedGradientAtlas {
        &self.gradient_atlas
    }

    pub(crate) const fn texture_limit(&self) -> TextureLimit {
        self.texture_limit
    }

    pub(crate) const fn clipboard(&self) -> &Clipboard {
        &self.clipboard
    }

    pub(crate) const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    pub(crate) fn user_scale(&self) -> &AppSetting<UserScale> {
        &self.user_scale
    }

    pub(crate) const fn windows(&self) -> &WindowDirectory {
        &self.windows
    }

    pub(super) fn load_image(&self, image: &Image) -> Result<ImageHandle, ImageTooLarge> {
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
        /// Recorder capabilities that share nothing with another recorder: a mono-fallback shaper (deterministic metrics),
        /// a memory clipboard, and no texture cap.
        pub(crate) fn isolated_mono() -> Self {
            Self::new(
                TextShaper::test_mono(),
                Clipboard::memory(),
                TextureLimit::default(),
            )
        }

        /// [`Self::isolated_mono`] with real shaping over the bundled faces, for anything that sizes to its text.
        pub(crate) fn isolated_text() -> Self {
            Self::new(
                TextShaper::new(),
                Clipboard::memory(),
                TextureLimit::default(),
            )
        }
    }
}

#[cfg(test)]
mod tests;
