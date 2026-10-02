//! A lowered shape's stroke.

use crate::primitives::approx::paints_nothing;
use crate::primitives::color::rgba_f16::RgbaF16;
use crate::primitives::nan::NanCheck;
use crate::primitives::stroke::Stroke;

/// Lowered stroke: a straight-alpha colour and a logical-px width.
///
/// `width` leads so the `#[repr(C)]` layout has no interior padding —
/// which is what keeps the type `Pod`, and so hashable in one
/// `Hasher::pod` call by both `compute_record_hash` and
/// `lower::background`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ShapeStroke {
    pub(crate) width: f32,
    pub(crate) color: RgbaF16,
}

impl ShapeStroke {
    /// This stroke with its colour's alpha scaled by `by`.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        Self {
            color: self.color.faded(by),
            ..self
        }
    }

    /// The stroke a no-op normalizes to, and what a payload's
    /// "no stroke here" reads as.
    pub(crate) const NONE: Self = Self {
        width: 0.0,
        color: RgbaF16::TRANSPARENT,
    };

    #[inline]
    pub(crate) const fn is_noop(self) -> bool {
        paints_nothing(self.width) || self.color.is_noop()
    }

    /// Collapse a no-op stroke to [`Self::NONE`]; pass anything else
    /// through verbatim.
    ///
    /// Every quad-tier draw normalizes here, and that is what lets
    /// `DrawQuadPayload::is_noop` test the stroke by colour alone: a
    /// transparent stroke colour in a payload means exactly "the stroke
    /// was a no-op".
    ///
    /// A NaN width normalizes away like any other non-painting width —
    /// `paints_nothing` classifies it as invisible. Catching a NaN *loudly* is
    /// `Shapes::add`'s job, at the authoring boundary
    /// where the value still has a call site; by the time it reaches
    /// here the useful thing to do is fail safe.
    #[inline]
    pub(crate) fn normalized(self) -> Self {
        if self.is_noop() { Self::NONE } else { self }
    }
}

impl From<&Stroke> for ShapeStroke {
    /// Normalized on the way in, so a lowered stroke is canonical
    /// wherever it lands afterwards.
    ///
    /// Here rather than at each consumer, because a record's raw bytes
    /// are a hash key: `-0.0`, a sub-`EPS` hair, and a wide-but-invisible
    /// ink all paint nothing, and every one of them reaching the hash as
    /// its own bit pattern would split the damage and measure keys for a
    /// difference nothing can see.
    #[inline]
    fn from(stroke: &Stroke) -> Self {
        Self {
            width: stroke.width,
            color: RgbaF16::from(stroke.color),
        }
        .normalized()
    }
}

impl From<Stroke> for ShapeStroke {
    #[inline]
    fn from(stroke: Stroke) -> Self {
        Self::from(&stroke)
    }
}

impl NanCheck for ShapeStroke {
    #[inline]
    fn has_nan(&self) -> bool {
        self.width.is_nan() || self.color.has_nan()
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::color::RgbaF32;
    use crate::primitives::stroke::Stroke;
    use crate::scene::shapes::paint::shape_stroke::ShapeStroke;

    /// Every stroke that paints nothing lowers to one value, so the
    /// record bytes a hash reads cannot split on an invisible
    /// difference. A `-0.0` width, a hair below `EPS`, and a wide stroke
    /// in transparent ink are three spellings of the same nothing.
    #[test]
    fn every_invisible_stroke_lowers_to_one_value() {
        let cases = [
            Stroke::new(RgbaF32::WHITE, -0.0),
            Stroke::new(RgbaF32::WHITE, 0.0),
            Stroke::new(RgbaF32::WHITE, 1e-9),
            Stroke::new(RgbaF32::TRANSPARENT, 4.0),
        ];
        for stroke in cases {
            assert_eq!(
                ShapeStroke::from(&stroke),
                ShapeStroke::NONE,
                "{stroke:?} paints nothing",
            );
        }
        // …and a stroke that does paint crosses verbatim.
        let visible = Stroke::new(RgbaF32::WHITE, 2.0);
        assert_eq!(
            ShapeStroke::from(&visible),
            ShapeStroke {
                width: 2.0,
                color: RgbaF32::WHITE.into(),
            },
        );
    }
}
