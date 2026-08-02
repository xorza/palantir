//! [`TextureIdSource`] — the shared allocator minting
//! [`TextureId`](crate::primitives::texture_id::TextureId)s.
//!
//! Stays in `renderer` (unlike the id itself) because it exists to keep the
//! backend's single texture cache collision-free, which is a renderer
//! concern.

use crate::primitives::texture_id::TextureId;
use std::cell::Cell;
use std::rc::Rc;

/// Shared monotonic source of [`TextureId`]s. The [`ImageRegistry`](crate::renderer::image_registry::ImageRegistry)
/// (CPU images) and each `GpuView` render target (minted via `Ui::gpu_view`)
/// draw from **one** of these so their ids never collide in the backend's
/// single texture cache. `UiResources` creates one and shares it with the
/// registry and every window's `Ui`.
/// Never hands out `TextureId(0)` (the render path's "no texture" value).
#[derive(Clone, Debug, Default)]
pub(crate) struct TextureIdSource(Rc<Cell<u64>>);

impl TextureIdSource {
    /// Mint the next process-unique id.
    pub(crate) fn reserve(&self) -> TextureId {
        let id = self.0.get() + 1;
        self.0.set(id);
        TextureId(id)
    }
}
