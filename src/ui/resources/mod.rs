//! The app-global resources of one host, shared by every window's recorder, the one frontend and the one backend:
//! shaper, image and icon registries, clipboard, window directory, and the
//! settings every window shares. Every field is clone-shared, so recorders in two windows resolve the same font, texture and scale.

use std::num::NonZeroU32;
use std::rc::Rc;

use crate::clipboard::Clipboard;
use crate::icons::icon_registry::IconRegistry;
use crate::image_registry::ImageRegistry;
use crate::image_registry::error::ImageTooLarge;
use crate::image_registry::image_handle::ImageHandle;
use crate::image_registry::texture_limit::TextureLimit;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::image::Image;
use crate::renderer::shared_device::SharedDevice;
use crate::text::shaper::TextShaper;
use crate::theme::color_scheme::ColorScheme;
use crate::ui::resources::app_settings::AppSettings;
use crate::window::window_directory::WindowDirectory;

pub(crate) mod app_settings;

/// Shared by `Rc` on each window's `Ui`. Refused: a `CpuCore` that owns them, as `&mut Ui` has no
/// lifetime, and lending them needs `Ui<'res>` at each of the 356 widget signatures.
#[derive(Clone, Debug)]
pub(crate) struct UiResources {
    text: TextShaper,
    images: ImageRegistry,
    icons: IconRegistry,
    /// The ceiling a registered image is measured against (`Ui::max_image_dimension`) and the
    /// attach epoch, shared with every clone and the frontend; no ceiling until a device attaches.
    device: Rc<SharedDevice>,
    clipboard: Clipboard,
    settings: Rc<AppSettings>,
    windows: WindowDirectory,
}

impl UiResources {
    /// Resources whose images go to `images`, the registry over the host's texture store; the
    /// default registry is a deviceless recorder's.
    pub(crate) fn new(text: TextShaper, clipboard: Clipboard, images: ImageRegistry) -> Self {
        Self {
            text,
            images,
            icons: IconRegistry::default(),
            device: Rc::default(),
            clipboard,
            settings: Rc::default(),
            windows: WindowDirectory::default(),
        }
    }

    pub(crate) const fn text(&self) -> &TextShaper {
        &self.text
    }

    pub(crate) const fn icons(&self) -> &IconRegistry {
        &self.icons
    }

    pub(crate) const fn device(&self) -> &Rc<SharedDevice> {
        &self.device
    }

    /// The recorders' side of a device attach, at startup and after a loss alike: note the
    /// device's `texture_limit` in a new epoch.
    pub(crate) fn attach_device(&self, texture_limit: TextureLimit) {
        self.device.attach(texture_limit);
    }

    pub(crate) const fn clipboard(&self) -> &Clipboard {
        &self.clipboard
    }

    /// The settings every window shares.
    pub(crate) fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// The scheme every window shows now: the preference resolved against the system's.
    pub(crate) fn color_scheme(&self) -> ColorScheme {
        self.settings.color_scheme()
    }

    pub(crate) const fn windows(&self) -> &WindowDirectory {
        &self.windows
    }

    /// The longest image side the device holds, `None` before a device attaches.
    pub(super) fn max_image_dimension(&self) -> Option<NonZeroU32> {
        self.device.texture_limit().max_dimension()
    }

    pub(super) fn load_image(&self, image: &Image) -> Result<ImageHandle, ImageTooLarge> {
        self.device.texture_limit().accepts(image.size)?;
        Ok(ImageHandle::new(
            TextureId::reserve(),
            image,
            self.images.clone(),
        ))
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::clipboard::Clipboard;
    use crate::image_registry::ImageRegistry;
    use crate::text::shaper::TextShaper;
    use crate::ui::resources::UiResources;

    impl UiResources {
        /// Recorder capabilities that share nothing with another recorder: a mono-fallback shaper (deterministic metrics),
        /// a memory clipboard, and no texture cap.
        pub(crate) fn isolated_mono() -> Self {
            Self::new(
                TextShaper::test_mono(),
                Clipboard::memory(),
                ImageRegistry::default(),
            )
        }

        /// [`Self::isolated_mono`] with real shaping over the bundled faces, for anything that sizes to its text.
        pub(crate) fn isolated_text() -> Self {
            Self::new(
                TextShaper::new(),
                Clipboard::memory(),
                ImageRegistry::default(),
            )
        }
    }
}

#[cfg(test)]
mod tests;
