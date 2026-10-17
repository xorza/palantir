//! The user-scale range of one `fvar` variation axis, as a font file declares it.

use swash::Variation;

/// The bounds and default of one variation axis. A loaded font is untrusted data, so the
/// order `min ≤ default ≤ max` the axis record owes is checked here, not assumed.
#[derive(Clone, Copy, Debug)]
pub(super) struct AxisRange {
    min: f32,
    default: f32,
    max: f32,
}

impl AxisRange {
    /// `value` clamped into the axis, or `None` for an axis out of order: the face stays at its
    /// default instance on that axis, as FreeType pins an invalid axis to its default.
    pub(super) const fn coordinate(self, value: f32) -> Option<f32> {
        if !(self.min <= self.default && self.default <= self.max) {
            return None;
        }
        Some(if value < self.min {
            self.min
        } else if value > self.max {
            self.max
        } else {
            value
        })
    }
}

impl From<Variation<'_>> for AxisRange {
    fn from(axis: Variation<'_>) -> Self {
        Self {
            min: axis.min_value(),
            default: axis.default_value(),
            max: axis.max_value(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn range(min: f32, default: f32, max: f32) -> AxisRange {
        AxisRange { min, default, max }
    }

    /// In order, a weight inside the axis passes through and one outside lands on the nearer
    /// bound; an axis of one point holds every weight there.
    #[test]
    fn an_ordered_axis_clamps_the_weight() {
        let wght = range(100.0, 400.0, 900.0);
        assert_eq!(wght.coordinate(700.0), Some(700.0));
        assert_eq!(wght.coordinate(100.0), Some(100.0));
        assert_eq!(wght.coordinate(50.0), Some(100.0));
        assert_eq!(wght.coordinate(1000.0), Some(900.0));

        let point = range(400.0, 400.0, 400.0);
        assert_eq!(point.coordinate(100.0), Some(400.0));
        assert_eq!(point.coordinate(900.0), Some(400.0));
    }

    /// Each broken order leaves the axis alone instead of panicking: inverted bounds, which
    /// `f32::clamp` panics on, and a default outside bounds that are in order.
    #[test]
    fn an_axis_out_of_order_takes_no_coordinate() {
        assert_eq!(range(900.0, 400.0, 100.0).coordinate(700.0), None);
        assert_eq!(range(100.0, 50.0, 900.0).coordinate(700.0), None);
        assert_eq!(range(100.0, 1000.0, 900.0).coordinate(700.0), None);
        assert_eq!(range(100.0, f32::NAN, 900.0).coordinate(700.0), None);
    }
}
