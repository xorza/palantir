//! Where a lowered image's texels come from.

use crate::primitives::geometry::size::Size;
use crate::primitives::identity::texture_id::TextureId;
use glam::UVec2;

/// Where a textured rect samples from. A registered image and an app-rendered GPU surface composite through
/// the one image pipeline; only the source of the `TextureId` and the damage hash differ.
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
    /// An app-rendered GPU surface: `id` is its render target, which `Ui::gpu_views` minted; its `paint`
    /// callback stays there, keyed by the owner's `WidgetId`, keeping the hot `records` buffer small and
    /// `Rc`-free. `epoch` is the damage version folded into the shape hash: `repaint(true)` bumps it to
    /// the frame id, `repaint(false)` holds it so a static view stays undamaged and is culled.
    GpuView { id: TextureId, epoch: u64 },
}

impl ImageSource {
    /// The intrinsic size [`ImagePlacement::resolve`](crate::primitives::paint::image::ImagePlacement::resolve) fits; a view has none.
    pub(crate) const fn intrinsic(&self) -> Size {
        match self {
            Self::Texture { size, .. } => Size::new(size.x as f32, size.y as f32),
            Self::GpuView { .. } => Size::ZERO,
        }
    }
}
