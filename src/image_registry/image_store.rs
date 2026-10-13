//! The seam between a registered image's CPU lifecycle and the textures behind it.

use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::image::Image;
use std::fmt::Debug;

/// Where a registered image's texels go. The backend implements it over wgpu and owns every texture; a deviceless recorder has no store and discards texels. Methods take `&self` since handles share one store through an `Rc` (the wgpu side keeps its map in a `RefCell`).
///
/// Immediate on every call: `write` creates the texture on first call and copies into staging before returning, `free` drops it (wgpu destroys it once the GPU is done). The queue orders each before the next draw, so nothing waits for a frame boundary and no CPU copy is kept.
pub(crate) trait ImageStore: Debug {
    /// Make `id`'s texture hold `image`'s texels: created at `image.size` on the first write, overwritten after (`ImageHandle::update` asserts the registered size first).
    fn write(&self, id: TextureId, image: &Image);

    /// Free `id`'s texture.
    fn free(&self, id: TextureId);
}
