//! Shared timing constants used across animation, repaint
//! scheduling, and frame pacing.

use std::time::Duration;

/// Base animation step used by the `Ui` frame runtime's `dt` accumulator and
/// as the spring integrator's largest substep. Stiffer springs adapt below it.
pub(crate) const ANIM_SUBSTEP_DT: f32 = 1.0 / 240.0;

/// Per-frame animation delta clamp. Stalled frames freeze motion instead of
/// teleporting; spring validation uses the same bound to cap worst-frame work.
pub(crate) const MAX_ANIM_DT: f32 = 0.1;

/// Fallback repaint-wake coalesce floor, used when the display's
/// refresh rate is unknown — headless, an unmapped window, a monitor
/// that reports no rate, or VRR. The live floor is normally derived
/// per-display by [`coalesce_dt_for_refresh`] from the active
/// `Display::refresh_millihertz`. 1/120 s is a safe middle ground: fast
/// enough not to throttle a 60 Hz panel, slow enough to cap runaway
/// `request_repaint_after` bursts.
const DEFAULT_REPAINT_COALESCE_DT: Duration = Duration::from_nanos(1_000_000_000 / 120);

/// Repaint-wake coalesce floor for a display refreshing at
/// `refresh_millihertz` (winit's `MonitorHandle::refresh_rate_millihertz`,
/// i.e. Hz × 1000). One refresh interval: wakes scheduled closer than
/// this collapse, so the host never wakes faster than the panel can
/// present a frame. `None` or a reported `0` falls back to
/// [`DEFAULT_REPAINT_COALESCE_DT`].
pub(crate) fn coalesce_dt_for_refresh(refresh_millihertz: Option<u32>) -> Duration {
    match refresh_millihertz {
        // period = 1 / (mHz / 1000) s = 1e12 / mHz ns.
        Some(mhz) if mhz > 0 => Duration::from_nanos(1_000_000_000_000 / u64::from(mhz)),
        _ => DEFAULT_REPAINT_COALESCE_DT,
    }
}

#[cfg(test)]
mod tests;
