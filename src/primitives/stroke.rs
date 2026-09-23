//! A line's colour and width, as a border or as a path's stroke.

use crate::primitives::approx::paints_nothing;
use crate::primitives::color::RgbaF32;
use crate::primitives::nan::NanCheck;
use palantir_anim_derive::Animatable;

/// One colour and one width, and nothing about where the width lies:
/// that is the shape's rule. An area shape — a [`Background`], a rect, a
/// triangle — paints it as a border inside its edge. A path shape — a
/// line, a curve, an arc, a polyline — centres it on the path.
///
/// [`Background`]: crate::Background
#[derive(
    Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, Animatable,
)]
pub struct Stroke {
    /// Ink colour.
    pub color: RgbaF32,
    /// Width in logical pixels.
    pub width: f32,
}

impl Stroke {
    /// Canonical "no stroke" — width 0, transparent color. Equivalent
    /// to `Stroke::default()` but `const`, so callers can use it in
    /// const contexts and read it as the sentinel "this background
    /// has no border" without needing `Option<Stroke>` in the type.
    pub const ZERO: Self = Self {
        color: RgbaF32::TRANSPARENT,
        width: 0.0,
    };

    /// True when this stroke would paint nothing visible — width is
    /// sub-UI-tolerance (including negative, treated as zero), or
    /// the color is fully transparent. The animation pipeline lerps
    /// `Stroke` directly through `Stroke::ZERO`, so a "bordered →
    /// borderless" transition settles at `is_noop()` and the encoder
    /// filters it out without any `Option` collapse step.
    /// `&self` where the crate's other `Copy` paint predicates take
    /// `self`: `Background`'s `skip_serializing_if` names this, and
    /// serde requires an `fn(&T) -> bool` there. `Corners::approx_zero`
    /// takes `&self` for the same reason.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        paints_nothing(self.width) || self.color.is_noop()
    }

    /// Construct a stroke with `color` and `width`.
    #[inline]
    pub const fn new(color: RgbaF32, width: f32) -> Self {
        Self { color, width }
    }
}

impl NanCheck for Stroke {
    #[inline]
    fn has_nan(&self) -> bool {
        self.color.has_nan() || self.width.is_nan()
    }
}
