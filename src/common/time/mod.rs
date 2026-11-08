//! Timing constants for animation, repaint scheduling and frame pacing.

use std::time::Duration;

/// Per-frame animation delta clamp: stalled frames freeze motion instead of teleporting.
pub(crate) const MAX_ANIMATION_DT: f32 = 0.1;

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

/// The most frames a window starts a second while power is reduced, as WebKit limits its
/// rendering update to 30 Hz in Low Power Mode (<https://bugs.webkit.org/show_bug.cgi?id=215745>).
const REDUCED_POWER_HZ: u64 = 30;

/// The least time between two frame starts while power is reduced: the smallest whole number of
/// refresh periods at or above 1/30 s, so each frame lands on a vsync, or 1/30 s where the rate is
/// unknown. Integer-truncated nanoseconds, as [`coalesce_dt_for_refresh`]: a grid of them falls
/// behind the panel by under a nanosecond a frame, which a frame late by a period re-anchors.
pub(crate) fn reduced_power_interval(refresh_millihertz: Option<u32>) -> Duration {
    match refresh_millihertz {
        Some(mhz) if mhz > 0 => {
            let mhz = u64::from(mhz);
            // N periods of 1/(mHz/1000) s reach 1/30 s at N ≥ mHz / 30_000.
            let periods = mhz.div_ceil(REDUCED_POWER_HZ * 1000);
            Duration::from_nanos(periods * 1_000_000_000_000 / mhz)
        }
        _ => Duration::from_nanos(1_000_000_000 / REDUCED_POWER_HZ),
    }
}

/// How much sooner than its slot a frame may start while power is reduced: half a refresh period,
/// or half of [`DEFAULT_REPAINT_COALESCE_DT`] where the rate is unknown. A real clock's timer
/// fires late, and a frame that starts at its slot presents a vsync after it, so a windowed host
/// wakes this much early and the frame still presents at its slot's vsync.
pub(crate) fn reduced_power_slack(refresh_millihertz: Option<u32>) -> Duration {
    coalesce_dt_for_refresh(refresh_millihertz) / 2
}

#[cfg(test)]
mod tests;
