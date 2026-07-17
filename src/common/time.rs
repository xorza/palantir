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
pub(crate) const DEFAULT_REPAINT_COALESCE_DT: Duration = Duration::from_nanos(1_000_000_000 / 120);

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
mod tests {
    use super::*;

    #[test]
    fn coalesce_dt_matches_refresh_interval() {
        // 60 Hz → 16.667 ms, 120 Hz → 8.333 ms, 144 Hz → 6.944 ms
        // (integer-truncated nanos of 1e12 / mHz).
        assert_eq!(
            coalesce_dt_for_refresh(Some(60_000)),
            Duration::from_nanos(16_666_666)
        );
        assert_eq!(
            coalesce_dt_for_refresh(Some(120_000)),
            Duration::from_nanos(8_333_333)
        );
        assert_eq!(
            coalesce_dt_for_refresh(Some(144_000)),
            Duration::from_nanos(6_944_444)
        );
        // 120 Hz reproduces the historical hardcoded default exactly.
        assert_eq!(
            coalesce_dt_for_refresh(Some(120_000)),
            DEFAULT_REPAINT_COALESCE_DT
        );
    }

    #[test]
    fn coalesce_dt_falls_back_when_unknown() {
        assert_eq!(coalesce_dt_for_refresh(None), DEFAULT_REPAINT_COALESCE_DT);
        assert_eq!(
            coalesce_dt_for_refresh(Some(0)),
            DEFAULT_REPAINT_COALESCE_DT
        );
    }

    #[test]
    fn higher_refresh_means_tighter_floor() {
        // Parameterized behavior: a faster panel yields a smaller
        // coalesce window, so fewer near-adjacent wakes collapse.
        let at_60 = coalesce_dt_for_refresh(Some(60_000));
        let at_144 = coalesce_dt_for_refresh(Some(144_000));
        assert!(at_144 < at_60, "{at_144:?} should be < {at_60:?}");
    }
}
