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
