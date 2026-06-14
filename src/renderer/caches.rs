//! Cross-frame shared GPU resource caches: the image registry and the
//! gradient LUT atlas, bundled so subsystems thread one handle instead
//! of two. Both inner fields are `Rc`-shared, so cloning [`RenderCaches`]
//! is cheap and every clone observes the same state.
//!
//! Lifetime: same as the renderer (constructed by `WindowRenderer`, dropped when
//! the surface goes away). Distinct from [`crate::forest::frame_arena::FrameArena`]
//! which is per-frame scratch.

use crate::renderer::gradient_atlas::GradientAtlas;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::texture_id::TextureIdSource;

#[derive(Clone)]
pub(crate) struct RenderCaches {
    /// Image cache. Authoring code stages bytes once via
    /// [`crate::Ui::register_image`] and references the returned handle
    /// in [`crate::Shape::Image`]; this field is reached only from
    /// inside the crate (the `Ui` method + the backend upload path).
    pub(crate) images: ImageRegistry,
    /// Internal gradient LUT cache. Registration is driven from
    /// shape lowering — users never touch this directly.
    pub(crate) gradients: GradientAtlas,
}

impl RenderCaches {
    /// Build the caches with `images` minting from `ids` — the shared
    /// [`TextureIdSource`] owned by [`HostContext`](crate::context::HostContext),
    /// also handed to each window's `GpuViewRegistry`, so a registered image
    /// and a `GpuView` target can never land on the same id in the one
    /// backend texture cache.
    pub(crate) fn new(ids: TextureIdSource) -> Self {
        Self {
            images: ImageRegistry::new(ids),
            gradients: GradientAtlas::default(),
        }
    }
}
