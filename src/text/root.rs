//! The unbounded shape every wrap policy reasons from.

use crate::text::extent::TextExtent;

/// A run's unbounded shape, the root every wrap policy reasons from. It carries
/// the wrapping floor and single-line flag a bounded shape cannot supply. It
/// does not identify a shaped buffer;
/// [`TextSystem`](crate::text::system::TextSystem) derives that key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextRoot {
    pub(super) extent: TextExtent,
    /// Width of the widest unbreakable run; the wrapping path floors a narrower
    /// width with it so text overflows rather than breaking a word. `None` when
    /// shaped without the scan ([`TextWrap::floor_scan`](crate::text::wrap::TextWrap)):
    /// buffers are shared across wrap policies, and `None` (not `0.0`) keeps a
    /// later `WrapWithOverflow` run from reading "no unbreakable segment".
    pub(super) intrinsic_min: Option<f32>,
    /// `true` when the result is one visual line, so `TextSystem::measure` can skip the Clip/Ellipsis resolve.
    pub(super) single_line: bool,
}

impl TextRoot {
    /// The wrap floor; panics with [`WRAP_FLOOR_ERROR`] when shaped without the scan.
    pub(super) const fn wrap_floor(&self) -> f32 {
        self.intrinsic_min.expect(WRAP_FLOOR_ERROR)
    }
}

/// What reading an unscanned wrap floor means, shared with the gated `TestMeasure`.
const WRAP_FLOOR_ERROR: &str = "the wrap floor was never scanned for this shape: TextWrap::floor_scan \
     and the policy asking for it have drifted apart";

#[cfg(test)]
pub(crate) mod internals {
    use super::*;
    use crate::primitives::geometry::size::Size;
    use crate::text::key::TextShapeKey;

    /// Shaping result as the tests read it: the measurement plus the buffer key
    /// its request minted. Flattened, as `shape_run` takes `size` from the
    /// bounded resolve and `intrinsic_min` from the unbounded root.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct TestMeasure {
        pub(crate) size: Size,
        pub(crate) key: Option<TextShapeKey>,
        /// `None` when the policy skips the wrap-floor scan.
        pub(crate) intrinsic_min: Option<f32>,
    }

    impl TestMeasure {
        /// The key of the buffer this run shaped under; panics where none was shaped.
        pub(crate) fn buffer_key(&self) -> TextShapeKey {
            self.key.expect("this fixture shapes a buffer")
        }

        /// The scanned wrap floor; panics like [`TextRoot::wrap_floor`].
        pub(crate) const fn wrap_floor(&self) -> f32 {
            self.intrinsic_min.expect(WRAP_FLOOR_ERROR)
        }

        pub(crate) fn new(root: TextRoot, key: TextShapeKey) -> Self {
            Self {
                size: root.extent.size,
                key: Some(key),
                intrinsic_min: root.intrinsic_min,
            }
        }
    }
}
