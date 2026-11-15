use super::*;

/// The coalesce window is one refresh interval, integer-truncated nanos of 1e12 / mHz: 60 Hz → 16.667 ms, 120 Hz → 8.333 ms, 144 Hz → 6.944 ms. An unknown rate falls back to the default, exactly 120 Hz's window.
#[test]
fn coalesce_dt_is_one_refresh_interval() {
    for (millihertz, nanos) in [
        (Some(60_000), 16_666_666),
        (Some(120_000), 8_333_333),
        (Some(144_000), 6_944_444),
        (None, 8_333_333),
    ] {
        assert_eq!(
            coalesce_dt_for_refresh(millihertz.and_then(NonZeroU32::new)),
            Duration::from_nanos(nanos),
            "{millihertz:?} mHz"
        );
    }
    assert_eq!(DEFAULT_REPAINT_COALESCE_DT, Duration::from_nanos(8_333_333));
}

/// The interval is the least whole number of periods at or above 1/30 s: 2 at 59.94 and 60 Hz,
/// 4 at 120 Hz, 5 at 144 Hz (4.8 rounds up), 1 at 30 Hz, and one period, longer than 1/30 s, at
/// 24 Hz. Each is N × 1e12 / mHz ns, truncated: 2e12 / 59_940 = 33_366_700.03. An unknown rate
/// is 1e9 / 30 ns. The slack is half the coalesce window.
#[test]
fn the_reduced_frame_interval_is_whole_refresh_periods() {
    for (millihertz, nanos) in [
        (Some(59_940), 33_366_700),
        (Some(60_000), 33_333_333),
        (Some(120_000), 33_333_333),
        (Some(144_000), 34_722_222),
        (Some(30_000), 33_333_333),
        (Some(24_000), 41_666_666),
        (None, 33_333_333),
    ] {
        assert_eq!(
            reduced_frame_interval(millihertz.and_then(NonZeroU32::new)),
            Duration::from_nanos(nanos),
            "{millihertz:?} mHz"
        );
    }
    for (millihertz, nanos) in [
        (Some(60_000), 8_333_333),
        (Some(144_000), 3_472_222),
        (None, 4_166_666),
    ] {
        assert_eq!(
            reduced_frame_slack(millihertz.and_then(NonZeroU32::new)),
            Duration::from_nanos(nanos),
            "{millihertz:?} mHz"
        );
    }
}
