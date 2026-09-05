//! [`TextureId`] — a GPU texture's identity.
//!
//! Lives in `primitives` rather than beside the renderer that consumes it
//! because `scene` needs it too: `ShapeRecord::Image` carries one — for a
//! registered texture or a `GpuView` target alike — from the moment a shape
//! is recorded, long before the renderer
//! sees it. Holding it in `renderer` made `scene` depend on `renderer` for a
//! `u64` newtype — the only such edge, and against the flow of every other
//! dependency between those two.
//!
//! The allocator is [`TextureId::reserve`], here rather than in
//! `renderer`: the counter behind it is process-wide, so it belongs to
//! the id and not to any one host's texture cache.

use crate::common::id_counter::IdCounter;

/// A GPU texture's identity: a process-unique id keying the backend's
/// texture cache and threading through the shape record + draw payload, so
/// a bare `u64` can't be confused with any other. Its texture is sourced
/// from either a registered [`Image`](crate::primitives::image::Image) or a
/// [`GpuView`](crate::widgets::gpu_view::GpuView) render target.
/// `TextureId(0)` is the render path's "no texture" value (the `Zeroable`
/// default of a draw payload) and is never handed out — ids start at `1`.
/// `Pod` so it can live inline on the `bytemuck`-cast draw payload.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, bytemuck::Pod, bytemuck::Zeroable,
)]
pub(crate) struct TextureId(pub(crate) u64);

impl TextureId {
    /// The next unused id, drawn from a process-wide counter.
    ///
    /// Process-wide, not per host. An
    /// [`ImageHandle`](crate::renderer::image_registry::image_handle::ImageHandle)
    /// is an owned value the application can carry anywhere, and the draw
    /// resolves it by this number and nothing else. Two hosts each
    /// counting from one would hand two unrelated images the same id, and
    /// drawing one host's handle in the other would sample whatever that
    /// host had registered first — a wrong picture, silently. One
    /// sequence makes a foreign handle a miss instead, and a miss draws
    /// nothing, which is the defined behaviour.
    ///
    /// Taken once per registered image and once per
    /// [`GpuView`](crate::widgets::gpu_view::GpuView) target, so the
    /// atomic is nowhere near a hot path.
    pub(crate) fn reserve() -> Self {
        static NEXT: IdCounter = IdCounter::new();
        Self(NEXT.reserve())
    }
}
