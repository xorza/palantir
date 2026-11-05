//! Frame times and the bar grid the reel cuts on.

use std::time::Duration;

/// One bar of the track: 4/4 at 80 BPM, four beats of 0.75 s.
pub(crate) const BAR: Duration = Duration::from_secs(3);

/// The time of frame `n` at `fps`, `n · 10⁹ / fps` ns in integer math: a sum of frame steps
/// would drift by its rounding, and frame 120 at 120 fps must be 1 s exactly.
pub(crate) const fn frame_time(n: u64, fps: u32) -> Duration {
    Duration::from_nanos(n * 1_000_000_000 / fps as u64)
}

/// `length` rounded up to whole bars, so a slide ends on a cut of the track.
pub(crate) const fn whole_bars(length: Duration) -> Duration {
    let bars = length.as_nanos().div_ceil(BAR.as_nanos()) as u32;
    Duration::from_secs(BAR.as_secs() * bars as u64)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::timeline::{BAR, frame_time, whole_bars};

    /// 1/120 s is 8,333,333.3 ns, so frame 1 truncates to 8,333,333 ns, frame 120 lands on 1 s
    /// with no drift, and frame 121 on 1,008,333,333 ns. At 60 fps, frame 3 is 50 ms exactly.
    #[test]
    fn frame_times_are_integer_and_do_not_drift() {
        assert_eq!(frame_time(0, 120), Duration::ZERO);
        assert_eq!(frame_time(1, 120), Duration::from_nanos(8_333_333));
        assert_eq!(frame_time(120, 120), Duration::from_secs(1));
        assert_eq!(frame_time(121, 120), Duration::from_nanos(1_008_333_333));
        assert_eq!(frame_time(3, 60), Duration::from_millis(50));
    }

    /// A slide rounds up to whole 3 s bars: 3 s is one, 3.001 s two, 7.5 s three.
    #[test]
    fn a_length_rounds_up_to_whole_bars() {
        assert_eq!(whole_bars(BAR), BAR);
        assert_eq!(whole_bars(Duration::from_millis(3001)), BAR * 2);
        assert_eq!(whole_bars(Duration::from_millis(7500)), BAR * 3);
        assert_eq!(whole_bars(Duration::ZERO), Duration::ZERO);
    }
}
