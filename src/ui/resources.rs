use crate::common::clipboard::Clipboard;
use crate::diagnostics::Diagnostics;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::texture_id::TextureIdSource;
use crate::text::TextShaper;
use crate::window::WindowDirectory;

/// Capabilities available to a recorder. Every field is app-global and
/// clone-shared; frame-local scene and layout state remain directly on `Ui`.
#[derive(Clone, Debug)]
pub(crate) struct UiResources {
    pub(crate) text: TextShaper,
    pub(crate) images: ImageRegistry,
    pub(crate) texture_ids: TextureIdSource,
    pub(crate) clipboard: Clipboard,
    pub(crate) diagnostics: Diagnostics,
    pub(crate) windows: WindowDirectory,
}

impl UiResources {
    pub(crate) fn new(text: TextShaper, clipboard: Clipboard) -> Self {
        let texture_ids = TextureIdSource::default();
        Self {
            text,
            images: ImageRegistry::new(texture_ids.clone()),
            texture_ids,
            clipboard,
            diagnostics: Diagnostics::default(),
            windows: WindowDirectory::default(),
        }
    }
}

impl Default for UiResources {
    fn default() -> Self {
        Self::new(TextShaper::default(), Clipboard::default())
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::image::Image;
    use crate::renderer::texture_id::TextureId;
    use crate::ui::resources::UiResources;

    #[test]
    fn images_and_gpu_views_share_one_texture_id_authority() {
        let resources = UiResources::default();
        let gpu_view_id = resources.texture_ids.reserve();
        let image = Image::from_rgba8(1, 1, vec![0, 0, 0, 0]);
        let image_id = resources.images.register(image).id();

        assert_eq!(gpu_view_id, TextureId(1));
        assert_eq!(image_id, TextureId(2));
    }
}
