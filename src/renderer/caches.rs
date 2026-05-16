//! Cross-frame shared GPU resource caches: the image registry and the
//! gradient LUT atlas, bundled so subsystems thread one handle instead
//! of two. Both inner fields are `Rc`-shared, so cloning [`RenderCaches`]
//! is cheap and every clone observes the same state.
//!
//! Lifetime: same as the renderer (constructed by `Host`, dropped when
//! the surface goes away). Distinct from [`crate::common::frame_arena::FrameArena`]
//! which is per-frame scratch.

use crate::primitives::image::ImageRegistry;
use crate::renderer::gradient_atlas::GradientAtlas;

#[derive(Clone, Default)]
pub struct RenderCaches {
    /// User-facing image cache. Authoring code calls
    /// `ui.caches.images.register(key, image)` to stage bytes once
    /// and reference the returned handle in [`crate::Shape::Image`].
    pub images: ImageRegistry,
    /// Internal gradient LUT cache. Registration is driven from
    /// shape lowering — users never touch this directly.
    pub(crate) gradients: GradientAtlas,
}
