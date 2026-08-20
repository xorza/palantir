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
    /// recorder: a mono-fallback shaper (no font loading, deterministic
    /// metrics, wrong for width-follows-label), a memory clipboard, and
    /// no texture cap. The cosmic-shaping peer goes through
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
mod tests {
    use crate::common::clipboard::Clipboard;
    use crate::primitives::image::Image;
    use crate::primitives::texture_id::TextureId;
    use crate::renderer::texture_limit::TextureLimit;
    use crate::text::shaper::TextShaper;
    use crate::ui::Ui;
    use crate::ui::resources::UiResources;
    use std::num::NonZeroU32;

    /// The bundle's ceiling is the one `Ui::register_image` enforces and
    /// the one it reports, and a rejection stops before the registry — so
    /// it queues no upload and consumes no id.
    #[test]
    fn the_bundle_ceiling_gates_registration_and_is_what_ui_reports() {
        let resources = UiResources::new(
            TextShaper::test_mono(),
            Clipboard::default(),
            TextureLimit::from_device(NonZeroU32::new(4).unwrap()),
        );
        let images = resources.images.clone();
        let ui = Ui::new(resources);
        assert_eq!(ui.max_image_dimension(), NonZeroU32::new(4));

        let accepted = ui.register_image(img(4, 4)).unwrap();
        assert_eq!(accepted.id(), TextureId(1));
        assert_eq!(
            ui.register_image(img(5, 1)).unwrap_err().max_dimension,
            4,
            "an over-limit source is rejected against the bundle's ceiling",
        );
        let next = ui.register_image(img(1, 1)).unwrap();
        assert_eq!(next.id(), TextureId(2), "a rejection consumes no id");

        let mut uploaded = Vec::new();
        images.drain_pending(|id, _| uploaded.push(id));
        assert_eq!(
            uploaded,
            vec![accepted.id(), next.id()],
            "and queues no upload",
        );
    }

    fn img(w: u32, h: u32) -> Image {
        Image::from_rgba8(w, h, vec![0u8; (w * h * 4) as usize])
    }

    #[test]
    fn images_and_gpu_views_share_one_texture_id_authority() {
        let resources = UiResources::isolated_mono();
        let gpu_view_id = resources.texture_ids.reserve();
        let image = Image::from_rgba8(1, 1, vec![0, 0, 0, 0]);
        let image_id = resources.images.register(image).id();

        assert_eq!(gpu_view_id, TextureId(1));
        assert_eq!(image_id, TextureId(2));
    }
}
