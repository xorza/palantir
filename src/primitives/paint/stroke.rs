//! [`Stroke`]: what a line, a curve, an arc or a polyline is drawn with.

use crate::primitives::math::domain;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::line_style::{LineCap, LineJoin};

/// A line's paint: its color, multiplied along the path by a ramp where it has one; its width,
/// centered on the path; how it ends; and how a polyline turns, as SVG's `stroke` and Skia's
/// stroke paint hold them. A line, a curve, an arc, a circle and a polyline take it
/// ([`Ui::add_shape`](crate::Ui::add_shape)); a box's edge is a [`Border`](crate::Border).
///
/// Plain data, checked where it enters a shape: the width a
/// [length](crate::widget::domain::length) and the color a [color](crate::widget::domain::color).
/// A polyline's colors per point or per segment are its vertices', set on the geometry, and it
/// draws no ramp.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct Stroke {
    /// The color.
    pub color: RgbaF32,
    /// The width in logical px, centered on the path.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub width: f32,
    /// A ramp along the path, `t` from its start to its end, which multiplies the color; `None`
    /// for none. A polyline draws none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp: Option<ColorRamp>,
    /// How each end looks.
    #[serde(default)]
    pub cap: LineCap,
    /// How a polyline turns at each interior point; a curve has no joint.
    #[serde(default)]
    pub join: LineJoin,
}

impl Stroke {
    /// A stroke of `color` and `width`, butt-capped and mitered, with no ramp.
    pub const fn new(color: RgbaF32, width: f32) -> Self {
        Self {
            color,
            width,
            ramp: None,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        }
    }

    /// This stroke with its color multiplied along the path by `ramp`.
    pub const fn with_ramp(mut self, ramp: ColorRamp) -> Self {
        self.ramp = Some(ramp);
        self
    }

    /// This stroke with its ends `cap`.
    pub const fn with_cap(mut self, cap: LineCap) -> Self {
        self.cap = cap;
        self
    }

    /// This stroke with its turns `join`.
    pub const fn with_join(mut self, join: LineJoin) -> Self {
        self.join = join;
        self
    }

    /// True when this stroke paints nothing: an invisible width or color, or a ramp of nothing.
    pub fn is_noop(&self) -> bool {
        domain::is_invisible(self.width)
            || self.color.is_noop()
            || self.ramp.is_some_and(|ramp| ramp.is_noop())
    }

    /// Panics unless the width is a [length](domain::length) and the color a
    /// [color](domain::color).
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        domain::length(self.width);
        let _ = domain::color(self.color);
    }
}

impl NanCheck for Stroke {
    fn has_nan(&self) -> bool {
        self.width.is_nan() || self.color.has_nan()
    }
}
