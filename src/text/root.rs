//! The unbounded shape every wrap policy reasons from.

use crate::primitives::size::Size;

/// A run's *unbounded* shape — the root every wrap policy reasons from.
///
/// Carries the two facts only an unbounded shape can supply, which is why
/// they live here and not on the width-resolved result: a bounded shape
/// cannot report a wrapping floor it never scanned for, and its line count
/// answers a different question. Nothing here identifies a shaped buffer;
/// the buffer key is derived by [`TextSystem`](crate::text::system::TextSystem)
/// from the request that produced it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextRoot {
    pub(super) size: Size,
    /// Width of the widest unbreakable run (typically the longest word).
    /// The wrapping path uses this as the floor when a parent commits a
    /// narrower width: text overflows rather than breaking inside a word.
    ///
    /// `None` when the run was shaped without the scan that produces it —
    /// see [`TextWrap::floor_scan`](crate::text::wrap::TextWrap). A shaped
    /// buffer is shared by every run with the same text and face regardless
    /// of wrap policy, so one that never asked for the floor can be the one
    /// that populates the cache entry. Storing the absence rather than `0.0`
    /// is what keeps that from silently reading as "no unbreakable segment"
    /// to a later `WrapWithOverflow` run over the same string.
    pub(super) intrinsic_min: Option<f32>,
    /// `true` when the shaped result is one visual line. Gates
    /// `TextSystem::measure`'s fitting-truncate skip: a single-line run
    /// whose natural width fits the committed width needs no Clip/Ellipsis
    /// resolve — the unbounded root stands in.
    pub(super) single_line: bool,
}

impl TextRoot {
    /// Successful empty-text shape. Its floor is genuinely zero rather
    /// than unscanned — there is nothing to scan.
    pub(super) const ZERO: Self = Self {
        size: Size::ZERO,
        intrinsic_min: Some(0.0),
        single_line: true,
    };

    /// The wrap floor, for the one policy that reads it.
    ///
    /// Panics if the run was shaped without the scan: that means
    /// [`TextWrap::floor_scan`](crate::text::wrap::TextWrap) and the policy
    /// actually asking have drifted apart, which is a wiring bug rather
    /// than bad data.
    pub(super) fn wrap_floor(&self) -> f32 {
        self.intrinsic_min
            .expect("WrapWithOverflow must shape its root with the wrap-floor scan")
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    // Wider than `cfg(test)`: an `internals`-only build reaches
    // `TestShape`/`TestMeasure` through the shaper's gated helpers but
    // runs none of the in-tree tests, so the assertion-side accessors
    // have no caller there.
    #![allow(dead_code)]
    use super::*;
    use crate::text::key::TextShapeKey;

    /// Shaping result as the in-tree tests read it: a production
    /// [`TextRoot`] flattened alongside the shaped-buffer key its request
    /// minted. Production derives that key in
    /// [`TextSystem`](crate::text::system::TextSystem) rather than carrying
    /// it on the measurement, but tests assert on buffer identity, so the
    /// helpers hand both back together.
    ///
    /// **Flattened rather than holding a [`TextRoot`], because it is not
    /// always one.** `TextSystem::shape_run` fills `size` from the
    /// *width-bounded* resolve while `intrinsic_min` and `single_line`
    /// come from the unbounded root, which is exactly the pair a bounded
    /// shape cannot answer for itself. Storing a `root: TextRoot` would
    /// give that hybrid a name promising it came from one shape.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct TestMeasure {
        pub(crate) size: Size,
        pub(crate) key: TextShapeKey,
        /// `None` when the run was shaped by a policy that skips the
        /// wrap-floor scan — see [`TextRoot::intrinsic_min`].
        pub(crate) intrinsic_min: Option<f32>,
        pub(crate) single_line: bool,
    }

    impl TestMeasure {
        /// The scanned wrap floor, for tests that assert on it. Panics
        /// if the run was shaped without the scan — mirrors
        /// [`TextRoot::wrap_floor`].
        pub(crate) fn wrap_floor(&self) -> f32 {
            self.intrinsic_min
                .expect("this measurement was shaped without the wrap-floor scan")
        }

        pub(crate) fn new(root: TextRoot, key: TextShapeKey) -> Self {
            Self {
                size: root.size,
                key,
                intrinsic_min: root.intrinsic_min,
                single_line: root.single_line,
            }
        }
    }
}
