//! [`Memory`]: what a `Ui` keeps for each widget between frames, swept as one.

use crate::animation::AnimationMap;
use crate::gpu::gpu_paint::gpu_views::GpuViews;
use crate::primitives::identity::widget_id::WidgetIdSet;
use crate::ui::state::StateMap;

/// What a `Ui` keeps for its widgets between frames: their state rows, their animation rows and
/// their live `GpuView`s, each keyed by the widget's id. One [`Self::sweep`] drops what the widgets
/// that left the tree kept, so no store is missed, as egui's `Memory` is one owner of a frame's
/// retained state.
#[derive(Debug, Default)]
pub(super) struct Memory {
    /// Cross-frame widget state, keyed by `WidgetId`.
    pub(super) state: StateMap,
    pub(super) animation: AnimationMap,
    /// Live `GpuView`s. The shape records only the redraw epoch; the encoder looks the view up by
    /// the node's `WidgetId`.
    pub(super) gpu_views: GpuViews,
}

impl Memory {
    /// Drop what each widget in `removed` kept, and each animation row frame `render_frame_id`
    /// did not touch.
    pub(super) fn sweep(&mut self, removed: &WidgetIdSet, render_frame_id: u64) {
        let Self {
            state,
            animation,
            gpu_views,
        } = self;
        animation.sweep(render_frame_id);
        state.sweep_removed(removed);
        gpu_views.sweep_removed(removed);
    }
}
