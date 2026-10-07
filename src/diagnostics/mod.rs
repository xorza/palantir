//! App-global diagnostic configuration, GPU measurement handles, and the `frame_stats` overlay. Backend collection lives in `gpu`.

pub(crate) mod frame_stats;
pub(crate) mod gpu_pass_stats;

use std::rc::Rc;

use crate::common::app_setting::AppSetting;
use crate::diagnostics::gpu_pass_stats::GpuPassStats;

/// Per-overlay flags, all off by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DebugOverlayConfig {
    /// A 2px red stroke around each frame's damage: `Skip` nothing, `Full` the surface, `Partial(rect)` the rect.
    pub damage_rect: bool,
    /// On `Partial` frames, a full-viewport 40%-translucent black quad is painted over the backbuffer before the damage passes (`LoadOp::Load`, no scissor), so static regions decay toward black while redrawn content stays current. `Full` frames skip it.
    pub dim_undamaged: bool,
    /// A frame counter and EMA FPS readout in the top-right, recorded into `Layer::Debug` after the app's record callback. It changes every frame, forcing a small `Partial` damage rect even when idle.
    pub frame_stats: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Diagnostics {
    pub(crate) gpu_pass_stats: GpuPassStats,
    /// App-global, so a toggle must repaint the other windows via the [`AppSetting`] signal, which only [`Ui::set_debug_overlay`](crate::Ui::set_debug_overlay) raises.
    pub(crate) overlay: Rc<AppSetting<DebugOverlayConfig>>,
}
