//! How a stroke ends, turns and breaks: [`LineCap`], [`LineJoin`] and [`Dash`].

use crate::primitives::math::domain;
use crate::primitives::math::nan::NanCheck;

/// How a [`Stroke`](crate::Stroke) ends: a line, a Bézier, an arc or a polyline.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub enum LineCap {
    /// Ends exactly at the endpoint. The default.
    #[default]
    Butt = 0,
    /// Extends by half the stroke width along the tangent.
    Square = 1,
    /// Adds a half-disc past the endpoint.
    Round = 2,
}

/// How a [`Stroke`](crate::Stroke) turns at a polyline's interior points.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub enum LineJoin {
    /// Extends both edges to their crossing point. The default; becomes [`Self::Bevel`] past the miter limit.
    #[default]
    Miter = 0,
    /// Cuts the corner off square.
    Bevel = 1,
    /// Rounds the corner by the stroke radius.
    Round = 2,
}

/// A [`Stroke`](crate::Stroke)'s dash pattern: `on` of line, then `off` of gap, repeated from the
/// path's start, each a length along the path in logical px, as SVG's `stroke-dasharray` of two
/// lengths and Skia's dash path effect. Each dash is a stroke of its own: it ends in the
/// stroke's cap, which a round or square one extends past `on` by half the width at both ends,
/// as SVG draws them.
///
/// A line, a Bézier, an arc and a circle draw it; a polyline draws none, as it draws no ramp.
/// A pattern whose `on` and `off` are together invisible draws the line whole.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[must_use]
pub struct Dash {
    /// The length of each dash, logical px.
    #[serde(deserialize_with = "crate::primitives::file::length")]
    pub on: f32,
    /// The length of each gap, logical px.
    #[serde(deserialize_with = "crate::primitives::file::length")]
    pub off: f32,
}

impl Dash {
    /// A pattern of `on` px of line and `off` px of gap.
    pub const fn new(on: f32, off: f32) -> Self {
        Self { on, off }
    }

    /// The length of one dash and one gap.
    pub(crate) const fn period(self) -> f32 {
        self.on + self.off
    }

    /// True when the pattern breaks nothing: no gap, or a dash and a gap too short to see.
    pub(crate) const fn is_solid(self) -> bool {
        domain::is_invisible(self.off) || domain::is_invisible(self.period())
    }

    /// Panics unless `on` and `off` are [lengths](domain::length).
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(self) {
        domain::length(self.on);
        domain::length(self.off);
    }
}

impl NanCheck for Dash {
    fn has_nan(&self) -> bool {
        self.on.is_nan() || self.off.is_nan()
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::line_style::Dash;
    use crate::primitives::paint::stroke::Stroke;

    /// A pattern breaks a line only where it has a gap to see: no gap, and a dash and a gap too
    /// short to see, are the whole line. A dash of no length with a gap still breaks it, as a
    /// round cap draws each as a dot.
    #[test]
    fn a_pattern_with_no_gap_to_see_is_solid() {
        assert!(!Dash::new(6.0, 4.0).is_solid());
        assert!(!Dash::new(0.0, 4.0).is_solid(), "dots");
        assert!(Dash::new(6.0, 0.0).is_solid(), "no gap");
        assert!(Dash::new(0.0, 0.0).is_solid(), "no period");
        assert_eq!(Dash::new(6.0, 4.0).period(), 10.0);
    }

    /// A stroke holds its dash as given, and a file that names none reads none.
    #[test]
    fn a_stroke_holds_its_dash() {
        let plain = Stroke::new(RgbaF32::WHITE, 2.0);
        assert_eq!(plain.dash, None);
        let dashed = plain.with_dash(Dash::new(6.0, 4.0));
        assert_eq!(dashed.dash, Some(Dash { on: 6.0, off: 4.0 }));
        dashed.validate();
        let text = ron::to_string(&dashed).unwrap();
        assert_eq!(ron::from_str::<Stroke>(&text).unwrap(), dashed);
        assert!(!ron::to_string(&plain).unwrap().contains("dash"));
        assert_eq!(
            ron::from_str::<Stroke>(&ron::to_string(&plain).unwrap()).unwrap(),
            plain
        );
    }

    #[test]
    #[should_panic(expected = "a length must be finite and not negative")]
    fn a_dash_of_negative_length_is_refused_where_it_enters_a_shape() {
        Stroke::new(RgbaF32::WHITE, 2.0)
            .with_dash(Dash::new(-1.0, 4.0))
            .validate();
    }
}
