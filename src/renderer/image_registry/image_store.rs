//! The seam between a registered image's CPU lifecycle and the textures
//! behind it.

use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use std::fmt::Debug;

/// Where a registered image's texels go.
///
/// The backend implements it over wgpu and owns every texture. A
/// deviceless recorder has no implementation at all: its registry holds
/// no store and discards the texels. Every method takes `&self` because
/// the handles that call in share one store through an `Rc`, so the wgpu
/// side keeps its map behind a `RefCell`.
///
/// Immediate on every call. `create` fills the texture before it returns,
/// `write` copies into wgpu's staging before it returns, and `free` drops
/// the texture, which wgpu destroys once the GPU is done with it. The
/// queue orders each of them before the next frame's draw, so nothing
/// here waits for a frame boundary and the registry retains no CPU copy.
pub(crate) trait ImageStore: Debug {
    /// Create the texture for `id` and fill it with `image`'s texels.
    fn create(&self, id: TextureId, image: &Image);

    /// Overwrite every texel of `id`'s texture. `image` is the registered
    /// size — `ImageHandle::update` asserts it before calling in.
    fn write(&self, id: TextureId, image: &Image);

    /// Free `id`'s texture.
    fn free(&self, id: TextureId);
}
