//! Where a lowered image's texels come from.

use crate::primitives::identity::texture_id::TextureId;
use glam::{UVec2, Vec2};

/// Where a textured rect samples from. A registered image and an app-rendered GPU surface composite through
/// the one image pipeline; only the source of the `TextureId` and the damage hash differ.
///
/// [`ShapeRecord::Image`]: crate::shape::record::ShapeRecord::Image
#[derive(Clone, Copy, Debug)]
pub(crate) enum ImageSource {
    /// A registered image: `id` is extracted at lowering so the record carries no `Rc` (the held handle keeps the GPU
    /// texture alive); the backend skips the draw on a miss. `size` is baked in so the encoder needs no registry
    /// borrow; `generation` counts writes, so an in-place rewrite still moves the hash and repaints.
    Texture {
        id: TextureId,
        size: UVec2,
        generation: u32,
    },
    /// An app-rendered GPU surface: its render-target `TextureId` and `paint` callback live in `Ui::gpu_views`,
    /// keyed by the owner's `WidgetId`, keeping the hot `records` buffer small and `Rc`-free.
    /// `epoch` is the damage version folded into the shape hash: `repaint(true)` bumps it to the frame id,
    /// `repaint(false)` holds it so a static view stays undamaged and is culled.
    GpuView { epoch: u64 },
}

impl ImageSource {
    /// The intrinsic size [`ImageFit::resolve`](crate::primitives::paint::image::ImageFit::resolve) fits; a view has none.
    pub(crate) const fn intrinsic(&self) -> Vec2 {
        match self {
            Self::Texture { size, .. } => Vec2::new(size.x as f32, size.y as f32),
            Self::GpuView { .. } => Vec2::ZERO,
        }
    }
}
