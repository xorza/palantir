//! The app-global resources of one host, shared by every window's recorder: shaper, image, icon
//! and effect registries, clipboard, window directory, and the settings every window shares, so recorders in
//! two windows resolve the same font, texture and scale.

use std::num::NonZeroU32;
use std::rc::Rc;

use crate::clipboard::Clipboard;
use crate::effect::effect_registry::EffectRegistry;
use crate::icons::icon_registry::IconRegistry;
use crate::image_registry::ImageRegistry;
use crate::image_registry::error::ImageTooLarge;
use crate::image_registry::image_handle::ImageHandle;
use crate::image_registry::texture_limit::TextureLimit;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::image::Image;
use crate::text::shaper::TextShaper;
use crate::theme::color_scheme::ColorScheme;
use crate::ui::resources::app_settings::AppSettings;
use crate::ui::resources::shared_device::SharedDevice;
use crate::window::window_directory::WindowDirectory;

pub(crate) mod app_settings;
pub(crate) mod shared_device;

/// One `Rc` over what a host shares, cloned onto each window's `Ui`. Refused: a `HostCpu` that owns
/// them, as `&mut Ui` has no lifetime, and lending them needs `Ui<'res>` at each of the 356 widget
/// signatures; an `Rc` per resource, for one concept. Refcounts of their own stay for a handle an app
/// holds or drops (`ImageHandle`, `IconSet`, `Clipboard`, a `GpuView`'s paint), a `TextShaper` two
/// hosts may share, a `Theme` too costly to copy that outlives a `&mut Ui` reborrow, and what a thread
/// or a wgpu callback reads.
#[derive(Clone, Debug)]
pub(crate) struct UiResources(Rc<SharedResources>);

/// What the windows of a host share. A handle an app or the backend holds apart, as [`TextShaper`]
/// and [`Clipboard`], stays a refcounted handle of its own.
#[derive(Debug)]
struct SharedResources {
    text: TextShaper,
    images: ImageRegistry,
    icons: IconRegistry,
    effects: EffectRegistry,
    /// The ceiling a registered image is measured against (`Ui::max_image_dimension`) and the
    /// attach epoch; no ceiling until a device attaches.
    device: SharedDevice,
    clipboard: Clipboard,
    settings: AppSettings,
    windows: WindowDirectory,
}

impl UiResources {
    /// Resources whose images go to `images`, the registry over the host's texture store; the
    /// default registry is a deviceless recorder's.
    pub(crate) fn new(
        text: TextShaper,
        clipboard: Clipboard,
        images: ImageRegistry,
        effects: EffectRegistry,
    ) -> Self {
        Self(Rc::new(SharedResources {
            text,
            images,
            icons: IconRegistry::default(),
            effects,
            device: SharedDevice::default(),
            clipboard,
            settings: AppSettings::default(),
            windows: WindowDirectory::default(),
        }))
    }

    pub(crate) fn text(&self) -> &TextShaper {
        &self.0.text
    }

    pub(crate) fn effects(&self) -> &EffectRegistry {
        &self.0.effects
    }

    pub(crate) fn icons(&self) -> &IconRegistry {
        &self.0.icons
    }

    pub(crate) fn device(&self) -> &SharedDevice {
        &self.0.device
    }

    /// The recorders' side of a device attach, at startup and after a loss alike: note the
    /// device's `texture_limit` in a new epoch.
    pub(crate) fn attach_device(&self, texture_limit: TextureLimit) {
        self.0.device.attach(texture_limit);
    }

    pub(crate) fn clipboard(&self) -> &Clipboard {
        &self.0.clipboard
    }

    /// The settings every window shares.
    pub(crate) fn settings(&self) -> &AppSettings {
        &self.0.settings
    }

    /// The scheme every window shows now: the preference resolved against the system's.
    pub(crate) fn color_scheme(&self) -> ColorScheme {
        self.0.settings.color_scheme()
    }

    pub(crate) fn windows(&self) -> &WindowDirectory {
        &self.0.windows
    }

    /// The longest image side the device holds, `None` before a device attaches.
    pub(super) fn max_image_dimension(&self) -> Option<NonZeroU32> {
        self.0.device.texture_limit().max_dimension()
    }

    pub(super) fn load_image(&self, image: &Image) -> Result<ImageHandle, ImageTooLarge> {
        self.0.device.texture_limit().accepts(image.size)?;
        Ok(ImageHandle::new(
            TextureId::reserve(),
            image,
            self.0.images.clone(),
        ))
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::clipboard::Clipboard;
    use crate::effect::effect_registry::EffectRegistry;
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
                EffectRegistry::default(),
            )
        }

        /// [`Self::isolated_mono`] with real shaping over the bundled faces, for anything that sizes to its text.
        pub(crate) fn isolated_text() -> Self {
            Self::new(
                TextShaper::new(),
                Clipboard::memory(),
                ImageRegistry::default(),
                EffectRegistry::default(),
            )
        }
    }
}

#[cfg(test)]
mod tests;
