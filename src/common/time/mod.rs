//! Timing constants for animation, repaint scheduling and frame pacing.

use std::time::Duration;

/// Smallest delta the `dt` accumulator spends; shorter frames carry, so an unthrottled repaint loop can't push animation in deltas below the f32 ULP.
pub(crate) const ANIM_SUBSTEP_DT: f32 = 1.0 / 240.0;

/// Per-frame animation delta clamp: stalled frames freeze motion instead of teleporting.
pub(crate) const MAX_ANIM_DT: f32 = 0.1;

/// Fallback repaint-wake coalesce floor when the refresh rate is unknown (headless, unmapped window, no rate, VRR); otherwise [`coalesce_dt_for_refresh`] derives it from `Display::refresh_millihertz`. 1/120 s doesn't throttle a 60 Hz panel and caps `request_repaint_after` bursts.
const DEFAULT_REPAINT_COALESCE_DT: Duration = Duration::from_nanos(1_000_000_000 / 120);

/// Wake coalesce floor for a display at `refresh_millihertz` (Hz × 1000, winit's `refresh_rate_millihertz`): one refresh interval, so the host never wakes faster than the panel presents. `None` or `0` falls back to [`DEFAULT_REPAINT_COALESCE_DT`].
pub(crate) fn coalesce_dt_for_refresh(refresh_millihertz: Option<u32>) -> Duration {
    match refresh_millihertz {
        // period = 1 / (mHz / 1000) s = 1e12 / mHz ns.
        Some(mhz) if mhz > 0 => Duration::from_nanos(1_000_000_000_000 / u64::from(mhz)),
        _ => DEFAULT_REPAINT_COALESCE_DT,
    }
}

#[cfg(test)]
mod tests;
