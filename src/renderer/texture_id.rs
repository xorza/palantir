//! [`TextureId`] — the renderer's GPU-texture identity — and
//! [`TextureIdSource`], the shared allocator that mints them.
//!
//! `TextureId` is a frontend↔backend contract type: it keys the backend's
//! one texture cache and rides `DrawImagePayload` / `ImageDrawRow`, so it
//! lives at the renderer level rather than inside any one consumer. Both
//! [`ImageRegistry`](crate::renderer::image_registry::ImageRegistry) (CPU
//! images) and each `GpuView` render target (minted via `Ui::gpu_view` into
//! its `Ui::gpu_views` entry) draw from one shared [`TextureIdSource`], so
//! their ids never collide in that cache.

use std::cell::Cell;
use std::rc::Rc;

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

/// Shared monotonic source of [`TextureId`]s. The [`ImageRegistry`](crate::renderer::image_registry::ImageRegistry)
/// (CPU images) and each `GpuView` render target (minted via `Ui::gpu_view`)
/// draw from **one** of these so their ids never collide in the backend's
/// single texture cache. `WindowRenderer`/`RenderCaches` build one and clone
/// it into the registry + every window's `Ui`; cloning shares the counter.
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
