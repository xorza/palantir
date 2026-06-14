//! Cross-frame shared GPU resource caches: the image registry and the
//! gradient LUT atlas, bundled so subsystems thread one handle instead
//! of two. Both inner fields are `Rc`-shared, so cloning [`RenderCaches`]
//! is cheap and every clone observes the same state.
//!
//! Lifetime: same as the renderer (constructed by `WindowRenderer`, dropped when
//! the surface goes away). Distinct from [`crate::forest::frame_arena::FrameArena`]
//! which is per-frame scratch.

use crate::renderer::gpu_view::GpuViewRegistry;
use crate::renderer::gradient_atlas::GradientAtlas;
use crate::renderer::image_registry::{ImageIdSource, ImageRegistry};

#[derive(Clone)]
pub(crate) struct RenderCaches {
    /// Image cache. Authoring code stages bytes once via
    /// [`crate::Ui::register_image`] and references the returned handle
    /// in [`crate::Shape::Image`]; this field is reached only from
    /// inside the crate (the `Ui` method + the backend upload path).
    pub(crate) images: ImageRegistry,
    /// App-driven GPU surfaces (the `GpuView` widget). Shares `images`'s
    /// [`ImageIdSource`] so render-target ids never collide with image ids
    /// in the backend's one texture cache. The backend reconciles + paints
    /// these each frame before the main pass.
    pub(crate) gpu_views: GpuViewRegistry,
    /// Internal gradient LUT cache. Registration is driven from
    /// shape lowering — users never touch this directly.
    pub(crate) gradients: GradientAtlas,
}

impl Default for RenderCaches {
    fn default() -> Self {
        // One id source, shared by both registries, so a `GpuView` target
        // and a registered image can never land on the same id in the
        // backend's single texture cache.
        let ids = ImageIdSource::default();
        Self {
            images: ImageRegistry::new(ids.clone()),
            gpu_views: GpuViewRegistry::new(ids),
            gradients: GradientAtlas::default(),
        }
    }
}
