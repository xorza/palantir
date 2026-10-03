//! Where a lowered image's texels come from.

use crate::primitives::texture_id::TextureId;
use glam::{UVec2, Vec2};

/// Where a textured rect samples from — the half that actually differs
/// between a registered image and an app-rendered GPU surface. Both
/// composite through the one image pipeline: same paint-rect
/// resolution, fit, sampling, tint, `DrawImagePayload`, and
/// `ImageInstance`. All that separates them is where the encoder gets
/// the `TextureId` and what drives their damage hash, so they are two
/// sources of one draw rather than two draws.
///
/// [`ShapeRecord::Image`]: crate::scene::shapes::record::ShapeRecord::Image
#[derive(Clone, Copy, Debug)]
pub(crate) enum ImageSource {
    /// A registered image. `id` is the registration id behind an
    /// [`ImageHandle`](crate::ImageHandle) — extracted at lowering so the
    /// per-frame record carries no `Rc` (the user's held handle is what
    /// keeps the GPU texture alive). The backend looks `id` up in its
    /// texture cache and skips the draw on a miss. `size` is the
    /// intrinsic dims, baked in at registration so the encoder reads
    /// them with no registry borrow. `generation` counts the handle's
    /// writes: a texture rewritten in place under the same id still moves
    /// the hash and repaints — the registered image's twin of the view's
    /// `epoch` below.
    Texture {
        id: TextureId,
        size: UVec2,
        generation: u32,
    },
    /// An app-rendered GPU surface. The view's stable render-target
    /// `TextureId` and its `paint` callback live in `Ui::gpu_views`,
    /// keyed by the owner node's `WidgetId`, which the encoder reads to
    /// look the view up — kept off the record so the hot `records`
    /// buffer stays small and `Rc`-free. The encoder passes
    /// [`ImageFit`](crate::primitives::image::ImageFit) an all-zero
    /// intrinsic size, which resolves to the base rect at full UV.
    ///
    /// `epoch` is the view's damage version, folded into the shape hash
    /// (which only sees the record, so it rides here): `Ui::gpu_view`
    /// bumps it to the frame id on `repaint(true)` — the rect repaints
    /// and the texture re-renders — and holds it stable on
    /// `repaint(false)`, so a static view stays undamaged and is culled.
    GpuView { epoch: u64 },
}

impl ImageSource {
    /// The intrinsic size [`ImageFit::resolve`](crate::primitives::image::ImageFit::resolve)
    /// fits. A view has none, so it paints its base rect at full UV.
    pub(crate) const fn intrinsic(&self) -> Vec2 {
        match self {
            Self::Texture { size, .. } => Vec2::new(size.x as f32, size.y as f32),
            Self::GpuView { .. } => Vec2::ZERO,
        }
    }
}
