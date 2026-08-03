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
//! The allocator that mints ids is
//! [`TextureIdSource`](crate::renderer::texture_id_source::TextureIdSource);
//! it stays in `renderer`, which owns the texture cache these ids key.

/// A GPU texture's identity: a process-unique id keying the backend's
/// texture cache and threading through the shape record + draw payload, so
/// a bare `u64` can't be confused with any other. Its texture is sourced
/// from either a registered [`Image`](crate::primitives::image::Image) or a
/// [`GpuView`](crate::widgets::gpu_view::GpuView) render target.
/// `TextureId(0)` is the render path's "no texture" value (the `Zeroable`
/// default of a draw payload) and is never handed out — ids start at `1`.
/// `Pod` so it can live inline on the `bytemuck`-cast draw payload.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextureId(pub(crate) u64);
