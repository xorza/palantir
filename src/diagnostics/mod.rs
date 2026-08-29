//! App-global diagnostic configuration, the GPU measurement handles behind it,
//! and the `frame_stats` overlay one of its flags turns on. Backend collection
//! lives in `renderer::backend`.

pub(crate) mod frame_stats;
pub(crate) mod gpu_pass_stats;

use std::cell::Cell;
use std::rc::Rc;

use crate::diagnostics::gpu_pass_stats::GpuPassStats;

/// Per-overlay flags. Each `bool` toggles one visualization.
/// Default is all-off; flip the flags you want individually.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DebugOverlayConfig {
    /// Draws a 2px red stroke around the damaged region of each
    /// frame. `Skip` frames draw nothing; `Full` outlines the whole
    /// surface; `Partial(rect)` outlines the damage rect.
    pub damage_rect: bool,
    /// Visualize damage on `Partial` frames: before each frame's
    /// damage passes the backend paints a single full-viewport
    /// 40%-translucent black quad over the backbuffer (`LoadOp::Load`,
    /// no scissor) — undamaged pixels fade by 40% per frame; damaged
    /// pixels get dimmed but are then overwritten by the frame's
    /// regular draws, so they stay at full brightness. Across many
    /// frames static regions decay toward black while moving content
    /// stays current. Non-destructive: `Full` frames and frames with
    /// no partial damage skip the dim entirely (one full-screen clear
    /// resets the trail).
    pub dim_undamaged: bool,
    /// Show a frame counter + EMA FPS readout in the top-right,
    /// recorded into `Layer::Debug` by `Ui::frame` after the app's
    /// record callback. Because the text changes every frame, this
    /// forces a `Partial(small rect)` damage even when the rest of
    /// the scene is idle — the readout's rect is unioned into the
    /// damage region; the Main scene's dirty-rect calculation is
    /// unaffected.
    pub frame_stats: bool,
}

/// The app-global overlay flags, and whether they have changed since the
/// host last asked.
///
/// The flags are app-global, so a toggle in one window has to repaint the
/// others — which means the host must *learn* of a change, and only
/// [`Ui::set_debug_overlay`](crate::Ui::set_debug_overlay) can tell it.
/// The write raises the signal and the ask lowers it, so the host holds
/// no copy of its own to keep in step and asks nothing of the flags on a
/// loop tick that changed none.
#[derive(Debug, Default)]
pub(crate) struct OverlayFlags {
    config: Cell<DebugOverlayConfig>,
    changed: Cell<bool>,
}

impl OverlayFlags {
    #[inline]
    pub(crate) fn get(&self) -> DebugOverlayConfig {
        self.config.get()
    }

    /// Writing the flags they already hold is not a change: an app that
    /// assigns the same value every frame must not repaint every window
    /// every frame.
    #[inline]
    pub(crate) fn set(&self, overlay: DebugOverlayConfig) {
        if self.config.replace(overlay) != overlay {
            self.changed.set(true);
        }
    }

    /// Whether the flags changed since this was last asked, clearing the
    /// signal.
    #[inline]
    pub(crate) fn take_change(&self) -> bool {
        self.changed.replace(false)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Diagnostics {
    pub(crate) gpu_pass_stats: GpuPassStats,
    pub(crate) overlay: Rc<OverlayFlags>,
}
