//! One live `GpuView`'s cross-frame bookkeeping.

use crate::primitives::texture_id::TextureId;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;

/// One live `GpuView` in [`Ui::gpu_views`](crate::ui::Ui), keyed by `WidgetId`:
/// the view's stable backend `texture_id` (minted once from the shared render
/// caches, so it cannot collide with images or another window), the app
/// `paint` callback (refreshed
/// every frame), and the redraw `epoch`. This is the only place a `GpuView`'s
/// identity persists across frames; the swept-by-`removed` map is the whole of
/// the `Ui`'s `GpuView` bookkeeping — no `by_texture` index, no resolve (the
/// composer lists targets to paint, the backend frees each the frame it's no
/// longer composited).
#[derive(Debug)]
pub(crate) struct GpuViewEntry {
    pub(crate) texture_id: TextureId,
    pub(crate) paint: GpuPaintRef,
    /// The shape `epoch` stamped on each recorded frame. Bumped to the current
    /// frame id only when the widget requests a repaint; held stable otherwise,
    /// so a static view's shape hash doesn't change and the damage diff treats
    /// it as unchanged (the encoder then culls it, skipping its GPU paint).
    pub(crate) epoch: u64,
}
