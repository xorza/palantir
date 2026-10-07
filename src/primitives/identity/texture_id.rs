//! [`TextureId`] — a GPU texture's identity. Lives in `primitives` because `scene` needs it from record time;
//! holding it in `renderer` would make `scene` depend on `renderer` for a `u64` newtype. The process-wide counter
//! behind [`TextureId::reserve`] belongs to the id, not to any host's texture cache.

use crate::common::id_counter::IdCounter;

/// A GPU texture's identity: a process-unique id keying the backend's texture cache, threaded through the shape
/// record and draw payload. `TextureId(0)` is the render path's "no texture" (the `Zeroable` default) and is never
/// handed out; ids start at `1`. `Pod` so it lives inline on the cast draw payload.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, bytemuck::Pod, bytemuck::Zeroable,
)]
pub(crate) struct TextureId(pub(crate) u64);

impl TextureId {
    /// The next unused id, from a process-wide counter, not per host: an
    /// [`ImageHandle`](crate::renderer::image_registry::image_handle::ImageHandle) can be carried anywhere and the draw
    /// resolves it by this number alone, so two per-host counters would hand unrelated images one id and a foreign
    /// handle would draw the wrong picture instead of missing.
    pub(crate) fn reserve() -> Self {
        static NEXT: IdCounter = IdCounter::new();
        Self(NEXT.reserve())
    }
}
