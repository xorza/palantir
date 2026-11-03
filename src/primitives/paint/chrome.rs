//! The chrome a container paints behind its children: fill, border, corner radii,
//! a shadow and a backdrop filter, as one value a theme hands over whole.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::math::animatable::Animatable;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use palantir_anim_derive::{Animatable, Lerp};

/// Paint data shared by container widgets (`Block`, `Panel`, `Grid`) and per-state
/// widget visuals. [`Self::NONE`] is transparent fill, no border, square corners:
/// nothing emitted. Pure data; paint emission goes through `Tree::chrome_table` and
/// the encoder.
///
/// `Animatable` is derived: fill and border interpolate componentwise, `corners` is
/// `#[animate(lerp_only)]`, so a look's fade takes its radii at once and a theme's lerp blends
/// them. "No border" is `Stroke::NONE`, not an `Option`; paint-time `is_noop` filtering catches
/// authored and animation-decayed no-ops.
///
/// Plain data, so arithmetic may pass through values no widget takes. A widget's
/// `chrome` and `default_chrome` check it where it enters and panic unless
/// every color is a [color](crate::widget::domain::color), gradient angles are
/// [angles](crate::widget::domain::angle), centers and the shadow's offset and spread
/// [offsets](crate::widget::domain::offset), the border width, radial radii, each corner
/// radius and the shadow's blur [lengths](crate::widget::domain::length), and the backdrop
/// filter as [`BackdropFilter`] lists. Every value but the border width packs into an f16
/// lane, so it is a [lane](crate::widget::domain::lane) too.
// Intentionally **not `Copy`**: it is the largest of the three types
// `primitives::math::animatable::Animatable` discusses, and the recording chain takes it
// by reference.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Lerp, Animatable)]
#[must_use]
pub struct Chrome {
    /// Interior paint. [`Brush::TRANSPARENT`] fills nothing.
    pub fill: Brush,
    /// The edge ring, painted inside the rect. Layout adds its width to the
    /// padding, so children sit inside the border. `Stroke::NONE` (the `Default`)
    /// is omitted from serialized output.
    #[serde(default, skip_serializing_if = "Stroke::is_noop")]
    pub border: Stroke,
    /// Zero (or sub-`EPS`) radii, the `Default`, are omitted from serialized
    /// output.
    #[serde(
        default,
        skip_serializing_if = "Corners::is_approx_zero",
        deserialize_with = "crate::primitives::packed::serde::checked::corners"
    )]
    #[animate(lerp_only)]
    pub corners: Corners,
    /// Single drop / inset shadow; `Shadow::NONE` (the `Default`) means none and is
    /// omitted from serialized output. It animates componentwise (alpha lerps for
    /// hover-elevation). For multi-shadow stacks push shadow shapes through
    /// `Ui::add_shape`. As in CSS `box-shadow`, a drop shadow paints under the fill
    /// and an inset one over it, inside the border.
    #[serde(default, skip_serializing_if = "Shadow::is_noop")]
    pub shadow: Shadow,
    /// What the chrome does to the pixels painted before it, under its fill, which tints
    /// them: [`BackdropFilter::NONE`] (the `Default`) does nothing and is omitted from
    /// serialized output. A gradient fill and the border paint over the pane.
    #[serde(default, skip_serializing_if = "BackdropFilter::is_noop")]
    pub backdrop_filter: BackdropFilter,
}

impl Chrome {
    /// Panics unless its fill, border, corners and shadow each pass their own
    /// check.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        self.fill.validate();
        self.border.validate();
        self.corners.validate();
        self.shadow.validate();
        self.backdrop_filter.validate();
    }

    /// Canonical chrome that paints nothing; an explicit override to suppress
    /// theme chrome.
    pub const NONE: Self = Self {
        fill: Brush::TRANSPARENT,
        border: Stroke::NONE,
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
        backdrop_filter: BackdropFilter::NONE,
    };

    /// True when this paints nothing visible. The encoder skips no-op chrome; the
    /// shadow check is required since the chrome branch paints the shadow as its
    /// own draw.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        self.fill.is_noop()
            && self.border.is_noop()
            && self.shadow.is_noop()
            && self.backdrop_filter.is_noop()
    }

    /// `padding` grown by the border on every edge, as a node wearing this chrome lays out: **the
    /// one definition of the fold** `Tree::open_node` applies, for widgets that need the inner
    /// rect before the tree has it (`TextEdit`, `TextEditTheme::corner_centering`, the switch's
    /// knob). Through the f16 lanes [`Spacing`] stores, so a fractional border, as one a look
    /// animates, rounds here as it does in the layout.
    #[inline]
    pub(crate) fn fold_padding(&self, padding: Spacing) -> Spacing {
        let ring = self.border_inset();
        if ring == 0.0 {
            return padding;
        }
        Spacing::saturating(padding.to_array().map(|lane| lane + ring))
    }

    /// How far the border reaches in from the edge: its width, or zero when the
    /// width paints nothing. On the width alone, not [`Stroke::is_noop`]: a border an invisible
    /// color makes still gets room.
    #[inline]
    pub(crate) const fn border_inset(&self) -> f32 {
        if is_invisible(self.border.width) {
            0.0
        } else {
            self.border.width
        }
    }

    /// A plain fill: no border, corners or shadow.
    pub fn fill(brush: impl Into<Brush>) -> Self {
        Self {
            fill: brush.into(),
            border: Stroke::NONE,
            corners: Corners::ZERO,
            shadow: Shadow::NONE,
            backdrop_filter: BackdropFilter::NONE,
        }
    }

    /// A fill with rounded corners: no border or shadow.
    pub fn rounded(brush: impl Into<Brush>, corners: impl Into<Corners>) -> Self {
        Self::fill(brush).with_corners(corners)
    }

    /// Set the fill, keeping the rest.
    pub fn with_fill(mut self, brush: impl Into<Brush>) -> Self {
        self.fill = brush.into();
        self
    }

    /// Set the corner radii, keeping the rest.
    pub fn with_corners(mut self, corners: impl Into<Corners>) -> Self {
        self.corners = corners.into();
        self
    }

    /// Set the border, keeping fill/corners/shadow.
    pub const fn with_border(mut self, border: Stroke) -> Self {
        self.border = border;
        self
    }

    /// Set the drop/inset shadow, keeping fill/corners/border.
    pub const fn with_shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = shadow;
        self
    }

    /// Set the backdrop filter, keeping the rest: the fill tints the filtered pixels.
    pub const fn with_backdrop_filter(mut self, filter: BackdropFilter) -> Self {
        self.backdrop_filter = filter;
        self
    }
}

/// Every field can carry a NaN (the fill through a gradient's geometry, the rest
/// through scalars); `lower::chrome` screens it, as a `Chrome` never passes
/// through `Shapes::add`.
impl NanCheck for Chrome {
    fn has_nan(&self) -> bool {
        self.fill.has_nan()
            || self.border.has_nan()
            || self.corners.has_nan()
            || self.shadow.has_nan()
            || self.backdrop_filter.has_nan()
    }
}

impl Default for Chrome {
    fn default() -> Self {
        Self::NONE
    }
}

/// An absent chrome paints as [`Chrome::NONE`], so a lerp between `None` and a chrome blends
/// through it, as Flutter's `BoxDecoration.lerp` scales a decoration up from nothing
/// (<https://api.flutter.dev/flutter/painting/BoxDecoration/lerp.html>): a panel chrome that
/// appears fades in, its radii and shadow growing from zero. Motion reads a `None` value as
/// `NONE` too, a fixed point of [`Animatable::to_motion`], and a `None` offset or velocity as
/// zero.
impl Lerp for Option<Chrome> {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        if t == 0.0 {
            return a;
        }
        if t == 1.0 {
            return b;
        }
        match (a, b) {
            (None, None) => None,
            (a, b) => Some(Chrome::lerp(
                a.unwrap_or(Chrome::NONE),
                b.unwrap_or(Chrome::NONE),
                t,
            )),
        }
    }
}

impl Animatable for Option<Chrome> {
    #[inline]
    fn sub(self, other: Self) -> Self {
        match (self, other) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(Chrome::NONE).sub(b.unwrap_or(Chrome::NONE))),
        }
    }
    #[inline]
    fn add(self, other: Self) -> Self {
        match other {
            None => self,
            Some(offset) => Some(self.unwrap_or(Chrome::NONE).add(offset)),
        }
    }
    #[inline]
    fn scale(self, k: f32) -> Self {
        self.map(|chrome| chrome.scale(k))
    }
    #[inline]
    fn magnitude_squared(self) -> f32 {
        self.map_or(0.0, Chrome::magnitude_squared)
    }
    #[inline]
    fn settle_distance_squared(self) -> f32 {
        self.map_or(0.0, Chrome::settle_distance_squared)
    }
    #[inline]
    fn zero() -> Self {
        None
    }
    #[inline]
    fn to_motion(self) -> Self {
        self.map(Chrome::to_motion)
    }
    #[inline]
    fn from_motion(motion: Self) -> Self {
        motion.map(Chrome::from_motion)
    }
    #[inline]
    fn normalize_for_spring(&mut self, target: &Self, velocity: &mut Self) {
        if let (Some(value), Some(target)) = (self.as_mut(), target) {
            value.normalize_for_spring(target, velocity.get_or_insert_with(Chrome::zero));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::paint::color::RgbaF32;

    use super::*;

    // `with_border`/`with_shadow` chained in a const context fail to compile if
    // they stop being const.
    const _CONST_BUILDER: Chrome = Chrome::NONE
        .with_border(Stroke::NONE)
        .with_shadow(Shadow::NONE);

    #[test]
    fn with_border_and_with_shadow_set_the_named_field_only() {
        let base = Chrome::rounded(RgbaF32::WHITE, Corners::all(4.0));
        let border = Stroke::new(RgbaF32::BLACK, 2.0);
        let shadow = Shadow::drop(RgbaF32::BLACK, glam::Vec2::ZERO, 4.0);

        let with_border = base.clone().with_border(border);
        assert_eq!(with_border.border, border);
        assert_eq!(with_border.fill, base.fill);
        assert_eq!(with_border.corners, base.corners);
        assert_eq!(with_border.shadow, base.shadow);

        let with_shadow = base.clone().with_shadow(shadow);
        assert_eq!(with_shadow.shadow, shadow);
        assert_eq!(with_shadow.fill, base.fill);
        assert_eq!(with_shadow.border, base.border);
        assert_eq!(with_shadow.corners, base.corners);
    }

    #[test]
    fn none_is_the_default_noop_chrome() {
        assert_eq!(Chrome::default(), Chrome::NONE);
        assert!(Chrome::NONE.is_noop());
    }

    /// `rounded` is a fill with corners, a radius converting as a shape's does; `with_fill` and
    /// `with_corners` replace their field alone, whatever brush the fill holds.
    #[test]
    fn rounded_and_the_with_setters_set_their_field_alone() {
        let ink = RgbaF32::new(0.2, 0.4, 0.6, 1.0);
        let rounded = Chrome::rounded(ink, 6.0);
        assert_eq!(rounded, Chrome::fill(ink).with_corners(Corners::all(6.0)));
        assert_eq!(rounded.corners, Corners::all(6.0));

        let bordered = rounded.with_border(Stroke::new(RgbaF32::BLACK, 2.0));
        let gradient = LinearGradient::two_stop(0.0, RgbaF32::BLACK, RgbaF32::WHITE);
        let refilled = bordered.clone().with_fill(gradient.clone());
        assert_eq!(refilled.fill, Brush::Linear(gradient));
        assert_eq!(
            Chrome {
                fill: bordered.fill.clone(),
                ..refilled
            },
            bordered
        );
        let recornered = bordered.clone().with_corners(Corners::top(3.0));
        assert_eq!(recornered.corners, Corners::top(3.0));
        assert_eq!(
            Chrome {
                corners: bordered.corners,
                ..recornered
            },
            bordered
        );
    }

    /// A chrome that appears blends from `NONE`: halfway its fill, border and shadow are at half
    /// their alpha, and its radii, border width and shadow geometry at half their size. Leaving
    /// reads the same backwards, the ends are exact, and in motion an absent value is `NONE`
    /// while an absent offset adds nothing.
    #[test]
    fn an_absent_chrome_blends_through_none() {
        let ink = RgbaF32::new(0.2, 0.4, 0.6, 1.0);
        let chrome = Chrome::rounded(ink, Corners::all(8.0))
            .with_border(Stroke::new(RgbaF32::BLACK, 2.0))
            .with_shadow(Shadow::drop(RgbaF32::BLACK, glam::Vec2::new(0.0, 4.0), 6.0));
        let half = |color| RgbaF32::lerp(RgbaF32::TRANSPARENT, color, 0.5);
        let mid = Chrome {
            fill: half(ink).into(),
            border: Stroke::new(half(RgbaF32::BLACK), 1.0),
            corners: Corners::all(4.0),
            shadow: Shadow::drop(half(RgbaF32::BLACK), glam::Vec2::new(0.0, 2.0), 3.0),
            backdrop_filter: BackdropFilter::NONE,
        };
        assert_eq!(half(ink).a, 0.5);
        let some = Some(chrome.clone());
        assert_eq!(Option::lerp(None, some.clone(), 0.5), Some(mid.clone()));
        assert_eq!(Option::lerp(some.clone(), None, 0.5), Some(mid));
        assert_eq!(Option::lerp(None, some.clone(), 0.0), None);
        assert_eq!(Option::lerp(None, some.clone(), 1.0), some);
        assert_eq!(Option::<Chrome>::lerp(None, None, 0.5), None);

        let offset = None.sub(some.clone());
        assert_eq!(offset, Some(Chrome::NONE.sub(chrome)));
        assert_eq!(some.clone().add(None), some);
        assert_eq!(Option::<Chrome>::zero(), None);
        assert_eq!(Option::<Chrome>::None.magnitude_squared(), 0.0);
    }
}
