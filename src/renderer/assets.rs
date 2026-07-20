//! Cross-frame shared render assets: the image registry and the gradient LUT
//! atlas, bundled so subsystems thread one handle instead
//! of two. Both inner fields are `Rc`-shared, so cloning [`RenderAssets`]
//! is cheap and every clone observes the same state.
//!
//! Lifetime: app-global, shared by every window and the one backend. Distinct
//! from [`crate::scene::record_store::RecordStore`], which retains one window's
//! record payloads until its next record pass.

use crate::renderer::gradient_atlas::handle::GradientAtlas;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::texture_id::TextureIdSource;

#[derive(Clone, Debug)]
pub(crate) struct RenderAssets {
    /// Shared authority for registered images and `GpuView` render targets.
    pub(crate) texture_ids: TextureIdSource,
    /// Image cache. Authoring code stages bytes once via
    /// [`crate::Ui::register_image`] and references the returned handle
    /// in [`crate::Shape::Image`]; this field is reached only from
    /// inside the crate (the `Ui` method + the backend upload path).
    pub(crate) images: ImageRegistry,
    /// Internal gradient LUT cache. Registration is driven from
    /// frontend encoding — users never touch this directly.
    pub(crate) gradients: GradientAtlas,
}

impl RenderAssets {
    pub(crate) fn new() -> Self {
        let texture_ids = TextureIdSource::default();
        Self {
            images: ImageRegistry::new(texture_ids.clone()),
            gradients: GradientAtlas::default(),
            texture_ids,
        }
    }
}

impl Default for RenderAssets {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::image::Image;
    use crate::renderer::assets::RenderAssets;

    #[test]
    fn images_and_gpu_views_share_one_texture_id_authority() {
        let assets = RenderAssets::default();
        let gpu_view_id = assets.texture_ids.reserve();
        let image = Image::from_rgba8(1, 1, vec![0, 0, 0, 0]);
        let image_id = assets.images.register(image).id();

        assert_ne!(gpu_view_id, image_id);
    }
}
