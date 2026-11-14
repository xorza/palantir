//! [`Chrome`]: a node's box, its corners and its paint, as one value a theme hands over whole.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::math::animatable::Animatable;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::Gradient;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::shadow::Shadow;
use palantir_derive::{Animatable, Lerp};

/// A node's box: its corner radii and its [`Paint`], the one value a node's box (`Block`,
/// `Panel`, `Grid`, per-state widget visuals) and a modal's scrim take, as Compose's
/// `Modifier.background(brush, shape)` takes the shape beside the brush. The node's arranged rect
/// is the rest of its geometry. [`Self::NONE`] paints nothing. Pure data; paint emission goes
/// through `Tree::chrome_table` and the encoder.
///
/// `Animatable` is derived: the paint interpolates as [`Paint`] does, and `corners` is
/// `#[animate(lerp_only)]`, so a look's fade takes its radii at once and a theme's lerp blends
/// them.
///
/// Plain data, checked where it enters a widget: each corner radius a
/// [length](crate::widget::domain::length) and a [lane](crate::widget::domain::lane), and the
/// paint as [`Paint`] lists.
// Intentionally **not `Copy`**: it is the largest of the three types
// `primitives::math::animatable::Animatable` discusses, and the recording chain takes it
// by reference.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Lerp, Animatable)]
#[serde(deny_unknown_fields)]
#[must_use]
pub struct Chrome {
    /// Zero (or sub-`EPS`) radii, the `Default`, are omitted from serialized output.
    #[serde(
        default,
        skip_serializing_if = "Corners::is_approx_zero",
        deserialize_with = "crate::primitives::math::domain::file::corners"
    )]
    #[animate(lerp_only)]
    pub corners: Corners,
    /// What covers the box.
    pub paint: Paint,
}

impl Chrome {
    /// Panics unless its corners and its paint each pass their own check. One branch-free test of
    /// every part, the checks that name the broken rule only on the cold path: `Widget::record`
    /// runs it on every chromed widget each frame, where the part checks one by one took 4.4 % of
    /// a CPU frame (`.notes/PERF.md`, "The CPU frame").
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        if !(self.corners.is_valid() & self.paint.is_valid()) {
            self.reject();
        }
    }

    /// [`Self::validate`]'s cold half: panics with the rule a part breaks.
    #[cold]
    #[inline(never)]
    #[track_caller]
    const fn reject(&self) -> ! {
        self.corners.validate();
        self.paint.reject();
    }

    /// Canonical chrome that paints nothing; an explicit override to suppress theme chrome.
    pub const NONE: Self = Self {
        corners: Corners::ZERO,
        paint: Paint::NONE,
    };

    /// True when this paints nothing visible. The encoder skips no-op chrome.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        self.paint.is_noop()
    }

    /// `padding` grown by the border on every edge, as a node wearing this chrome lays out: **the
    /// one definition of the fold** `Tree::open_node` applies, for widgets that need the inner
    /// rect before the tree has it (`TextEdit`, `TextEditTheme::corner_centering`, the switch's
    /// knob). Through the f16 lanes [`Spacing`] stores, so a fractional border, as one a look
    /// animates, rounds here as it does in the layout.
    #[inline]
    pub(crate) fn fold_padding(&self, padding: Spacing) -> Spacing {
        let ring = self.paint.border_inset();
        if ring == 0.0 {
            return padding;
        }
        Spacing::saturating(padding.to_array().map(|lane| lane + ring))
    }

    /// A box of `paint` with square corners.
    pub fn new(paint: impl Into<Paint>) -> Self {
        Self {
            corners: Corners::ZERO,
            paint: paint.into(),
        }
    }

    /// A box of `paint` with rounded `corners`.
    pub fn rounded(paint: impl Into<Paint>, corners: impl Into<Corners>) -> Self {
        Self::new(paint).with_corners(corners)
    }

    /// This box with its corner radii `corners`.
    pub fn with_corners(mut self, corners: impl Into<Corners>) -> Self {
        self.corners = corners.into();
        self
    }

    /// This box with its paint `paint`.
    pub fn with_paint(mut self, paint: impl Into<Paint>) -> Self {
        self.paint = paint.into();
        self
    }

    /// Sugar for [`Paint::with_fill`] on this box's paint.
    pub fn with_fill(mut self, brush: impl Into<Brush>) -> Self {
        self.paint = self.paint.with_fill(brush);
        self
    }

    /// Sugar for [`Paint::with_border`] on this box's paint.
    pub const fn with_border(mut self, border: Border) -> Self {
        self.paint.border = border;
        self
    }

    /// Sugar for [`Paint::with_shadow`] on this box's paint.
    pub const fn with_shadow(mut self, shadow: Shadow) -> Self {
        self.paint.shadow = shadow;
        self
    }
}

/// Either part can carry a NaN; `lower::chrome` screens it, as a `Chrome` never passes through
/// `Shapes::push`.
impl NanCheck for Chrome {
    fn has_nan(&self) -> bool {
        self.corners.has_nan() || self.paint.has_nan()
    }
}

impl Default for Chrome {
    fn default() -> Self {
        Self::NONE
    }
}

/// A square box of `brush`, as [`Chrome::new`]: sugar for a box that only fills, as
/// `Modal::scrim(color)` takes one.
impl From<Brush> for Chrome {
    fn from(brush: Brush) -> Self {
        Self::new(brush)
    }
}

/// A square box of `color`, as [`Chrome::new`].
impl From<RgbaF32> for Chrome {
    fn from(color: RgbaF32) -> Self {
        Self::new(color)
    }
}

/// A square box of `gradient`, as [`Chrome::new`].
impl From<Gradient> for Chrome {
    fn from(gradient: Gradient) -> Self {
        Self::new(gradient)
    }
}

/// A square box of `paint`, as [`Chrome::new`].
impl From<Paint> for Chrome {
    fn from(paint: Paint) -> Self {
        Self::new(paint)
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
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        if t == 0.0 {
            return a.clone();
        }
        if t == 1.0 {
            return b.clone();
        }
        match (a, b) {
            (None, None) => None,
            (a, b) => Some(Chrome::lerp(
                a.as_ref().unwrap_or(&Chrome::NONE),
                b.as_ref().unwrap_or(&Chrome::NONE),
                t,
            )),
        }
    }
}

impl Animatable for Option<Chrome> {
    #[inline]
    fn sub(&self, other: &Self) -> Self {
        match (self, other) {
            (None, None) => None,
            (a, b) => Some(
                a.as_ref()
                    .unwrap_or(&Chrome::NONE)
                    .sub(b.as_ref().unwrap_or(&Chrome::NONE)),
            ),
        }
    }
    #[inline]
    fn add(&self, other: &Self) -> Self {
        match other {
            None => self.clone(),
            Some(offset) => Some(self.as_ref().unwrap_or(&Chrome::NONE).add(offset)),
        }
    }
    #[inline]
    fn scale(&self, k: f32) -> Self {
        self.as_ref().map(|chrome| chrome.scale(k))
    }
    #[inline]
    fn magnitude_squared(&self) -> f32 {
        self.as_ref().map_or(0.0, Chrome::magnitude_squared)
    }
    #[inline]
    fn settle_distance_squared(&self) -> f32 {
        self.as_ref().map_or(0.0, Chrome::settle_distance_squared)
    }
    #[inline]
    fn zero() -> Self {
        None
    }
    #[inline]
    fn to_motion(&self) -> Self {
        self.as_ref().map(Chrome::to_motion)
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
    use crate::internals::panic_probe;
    use crate::primitives::paint::brush::gradient::gradient_geometry::GradientGeometry;
    use glam::Vec2;

    use super::*;

    // `with_border`/`with_shadow` chained in a const context fail to compile if
    // they stop being const.
    const _CONST_BUILDER: Chrome = Chrome::NONE
        .with_border(Border::NONE)
        .with_shadow(Shadow::NONE);

    #[test]
    fn with_border_and_with_shadow_set_the_named_field_only() {
        let base = Chrome::rounded(RgbaF32::WHITE, Corners::all(4.0));
        let border = Border::new(RgbaF32::BLACK, 2.0);
        let shadow = Shadow::drop(RgbaF32::BLACK, Vec2::ZERO, 4.0);

        let with_border = base.clone().with_border(border);
        assert_eq!(with_border.paint.border, border);
        assert_eq!(with_border.paint.fill, base.paint.fill);
        assert_eq!(with_border.corners, base.corners);
        assert_eq!(with_border.paint.shadow, base.paint.shadow);

        let with_shadow = base.clone().with_shadow(shadow);
        assert_eq!(with_shadow.paint.shadow, shadow);
        assert_eq!(with_shadow.paint.fill, base.paint.fill);
        assert_eq!(with_shadow.paint.border, base.paint.border);
        assert_eq!(with_shadow.corners, base.corners);
    }

    /// `validate`'s one branch-free test refuses exactly what the part checks one by one refuse,
    /// and panics with the same rule at the caller: each scalar of each part swept over the
    /// special values of its kinds, around a chrome that holds every part, refusing as many of
    /// them as its kind does.
    #[test]
    fn validate_refuses_what_the_part_checks_refuse() {
        type Set = fn(&mut Chrome, f32);
        /// The fill's scalar `idx` of two, whatever its kind: one of each kind of value it holds.
        fn fill(chrome: &mut Chrome, idx: usize) -> &mut f32 {
            match (&mut chrome.paint.fill, idx) {
                (Brush::Solid(c), 0) => &mut c.r,
                (Brush::Solid(c), _) => &mut c.a,
                (Brush::Gradient(g), idx) => match (&mut g.geometry, idx) {
                    (GradientGeometry::Linear(g), _) => &mut g.angle,
                    (GradientGeometry::Radial(g), 0) => &mut g.center.x,
                    (GradientGeometry::Radial(g), _) => &mut g.radius.y,
                    (GradientGeometry::Conic(g), 0) => &mut g.center.y,
                    (GradientGeometry::Conic(g), _) => &mut g.start_angle,
                },
            }
        }
        // How many of `values` each kind refuses. A lane (a color channel, an offset): ±65520,
        // f32::MAX, NaN, ±inf. A length that is a lane: those and -1. An angle: NaN and ±inf. A
        // length that is no lane (a border's width): -1, -65520, NaN and ±inf.
        const LANE: usize = 6;
        const LENGTH_LANE: usize = 7;
        const ANGLE: usize = 3;
        const LENGTH: usize = 5;
        let sets: [(Set, usize); 7] = [
            (|c, v| c.paint.border.width = v, LENGTH),
            (|c, v| c.paint.border.color.g = v, LANE),
            (|c, v| c.paint.shadow.color.b = v, LANE),
            (|c, v| c.paint.shadow.offset.x = v, LANE),
            (|c, v| c.paint.shadow.offset.y = v, LANE),
            (|c, v| c.paint.shadow.blur = v, LENGTH_LANE),
            (|c, v| c.paint.shadow.spread = v, LANE),
        ];
        let ink = RgbaF32::new(0.2, 0.4, 0.6, 1.0);
        // Each kind of fill, with what its two scalars refuse.
        let fills: [(Brush, [usize; 2]); 4] = [
            (ink.into(), [LANE, LANE]),
            (
                Gradient::linear(0.5, [ink, RgbaF32::WHITE]).into(),
                [ANGLE, ANGLE],
            ),
            (
                Gradient::centered_radial([ink, RgbaF32::WHITE]).into(),
                [LANE, LENGTH_LANE],
            ),
            (
                Gradient::conic(Vec2::splat(0.5), 0.5, [ink, RgbaF32::WHITE]).into(),
                [LANE, ANGLE],
            ),
        ];
        // Each kind's edges: a sign, zero, the lane's last value and the first past it, and the
        // non-finite values.
        let values = [
            0.5,
            0.0,
            -0.0,
            -1.0,
            1.0,
            65504.0,
            65520.0,
            -65520.0,
            f32::MAX,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        for (brush, fill_refusals) in fills {
            let held = Chrome::rounded(ink, Corners::all(4.0))
                .with_fill(brush)
                .with_border(Border::new(RgbaF32::BLACK, 2.0))
                .with_shadow(Shadow::drop(RgbaF32::BLACK, Vec2::new(0.0, 4.0), 6.0));
            assert_eq!(panic_probe::caught(|| held.validate()), Ok(()));
            let fill_sets: [(Set, usize); 2] = [
                (|c, v| *fill(c, 0) = v, fill_refusals[0]),
                (|c, v| *fill(c, 1) = v, fill_refusals[1]),
            ];
            for (set, refusals) in fill_sets.into_iter().chain(sets) {
                let mut refused = 0;
                for v in values {
                    let mut chrome = held.clone();
                    set(&mut chrome, v);
                    let parts = panic_probe::caught(|| {
                        chrome.paint.fill.validate();
                        chrome.paint.border.validate();
                        chrome.corners.validate();
                        chrome.paint.shadow.validate();
                    });
                    let whole = panic_probe::caught(|| chrome.validate());
                    assert_eq!(whole, parts, "{chrome:?}");
                    if let Err(rule) = parts {
                        refused += 1;
                        let line = line!() + 1;
                        let site = panic_probe::panic_site(&rule, || chrome.validate());
                        assert_eq!(site, format!("{}:{line}", file!()), "{rule}");
                    }
                }
                assert_eq!(refused, refusals, "{held:?}");
            }
        }
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
        assert_eq!(rounded, Chrome::new(ink).with_corners(Corners::all(6.0)));
        assert_eq!(rounded.corners, Corners::all(6.0));

        let bordered = rounded.with_border(Border::new(RgbaF32::BLACK, 2.0));
        let gradient = Gradient::horizontal([RgbaF32::BLACK, RgbaF32::WHITE]);
        let refilled = bordered.clone().with_fill(gradient.clone());
        assert_eq!(refilled.paint.fill, Brush::Gradient(gradient));
        assert_eq!(refilled.with_fill(bordered.paint.fill.clone()), bordered);
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
            .with_border(Border::new(RgbaF32::BLACK, 2.0))
            .with_shadow(Shadow::drop(RgbaF32::BLACK, Vec2::new(0.0, 4.0), 6.0));
        let half = |color| RgbaF32::lerp(&RgbaF32::TRANSPARENT, color, 0.5);
        let mid = Chrome {
            corners: Corners::all(4.0),
            paint: Paint {
                fill: half(&ink).into(),
                border: Border::new(half(&RgbaF32::BLACK), 1.0),
                shadow: Shadow::drop(half(&RgbaF32::BLACK), Vec2::new(0.0, 2.0), 3.0),
            },
        };
        assert_eq!(half(&ink).a, 0.5);
        let some = Some(chrome.clone());
        assert_eq!(Option::lerp(&None, &some, 0.5), Some(mid.clone()));
        assert_eq!(Option::lerp(&some, &None, 0.5), Some(mid));
        assert_eq!(Option::lerp(&None, &some, 0.0), None);
        assert_eq!(Option::lerp(&None, &some.clone(), 1.0), some);
        assert_eq!(Option::<Chrome>::lerp(&None, &None, 0.5), None);

        let offset = None.sub(&some);
        assert_eq!(offset, Some(Chrome::NONE.sub(&chrome)));
        assert_eq!(some.clone().add(&None), some);
        assert_eq!(Option::<Chrome>::zero(), None);
        assert_eq!(Option::<Chrome>::None.magnitude_squared(), 0.0);
    }
}
