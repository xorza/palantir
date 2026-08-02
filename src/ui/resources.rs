use crate::common::clipboard::Clipboard;
use crate::diagnostics::Diagnostics;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::texture_id_source::TextureIdSource;
use crate::text::TextShaper;
use crate::window::WindowDirectory;
use std::num::NonZeroU32;

/// Capabilities available to a recorder. Every field is app-global and
/// clone-shared; frame-local scene and layout state remain directly on `Ui`.
#[derive(Clone, Debug)]
pub(crate) struct UiResources {
    pub(crate) text: TextShaper,
    pub(crate) images: ImageRegistry,
    pub(super) texture_ids: TextureIdSource,
    pub(crate) clipboard: Clipboard,
    pub(crate) diagnostics: Diagnostics,
    pub(crate) windows: WindowDirectory,
}

impl UiResources {
    pub(crate) fn new(
        text: TextShaper,
        clipboard: Clipboard,
        max_texture_dimension_2d: Option<NonZeroU32>,
    ) -> Self {
        let texture_ids = TextureIdSource::default();
        Self {
            text,
            images: ImageRegistry::new(texture_ids.clone(), max_texture_dimension_2d),
            texture_ids,
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
        Self::new(TextShaper::test_mono(), Clipboard::default(), None)
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::image::Image;
    use crate::primitives::texture_id::TextureId;
    use crate::ui::resources::UiResources;

    #[test]
    fn images_and_gpu_views_share_one_texture_id_authority() {
        let resources = UiResources::isolated_mono();
        let gpu_view_id = resources.texture_ids.reserve();
        let image = Image::from_rgba8(1, 1, vec![0, 0, 0, 0]);
        let image_id = resources.images.register(image).unwrap().id();

        assert_eq!(gpu_view_id, TextureId(1));
        assert_eq!(image_id, TextureId(2));
    }
}
