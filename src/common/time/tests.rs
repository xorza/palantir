use super::*;

/// The coalesce window is one refresh interval, the integer-truncated
/// nanos of 1e12 / mHz: 60 Hz → 16.667 ms, 120 Hz → 8.333 ms, 144 Hz →
/// 6.944 ms. A faster panel gets a smaller window, so fewer near-adjacent
/// wakes collapse. An unknown or zero rate falls back to the default,
/// which is 120 Hz's window exactly.
#[test]
fn coalesce_dt_is_one_refresh_interval() {
    for (millihertz, nanos) in [
        (Some(60_000), 16_666_666),
        (Some(120_000), 8_333_333),
        (Some(144_000), 6_944_444),
        (None, 8_333_333),
        (Some(0), 8_333_333),
    ] {
        assert_eq!(
            coalesce_dt_for_refresh(millihertz),
            Duration::from_nanos(nanos),
            "{millihertz:?} mHz"
        );
    }
    assert_eq!(DEFAULT_REPAINT_COALESCE_DT, Duration::from_nanos(8_333_333));
}
