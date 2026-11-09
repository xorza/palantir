//! The chrome of a painted box: fill, border, corner radii, a shadow and a material, as one value
//! a theme hands over whole, for a node's box, a rect shape and a modal's scrim alike.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::math::animatable::Animatable;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::{Gradient, GradientGeometry};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::shadow::Shadow;
use palantir_derive::{Animatable, Lerp};

/// A painted box: the one value of a node's box (`Block`, `Panel`, `Grid`, per-state widget
/// visuals), a rect shape ([`Shape::rect`](crate::widget::Shape::rect)) and a modal's scrim, as
/// Flutter's `BoxDecoration` serves `Container` and a standalone `DecoratedBox`.
/// [`Self::NONE`] is transparent fill, no border, square corners: nothing emitted. Pure data;
/// paint emission goes through `Tree::chrome_table` and the encoder.
///
/// `Animatable` is derived: fill and border interpolate componentwise, `corners` is
/// `#[animate(lerp_only)]`, so a look's fade takes its radii at once and a theme's lerp blends
/// them. "No border" is `Border::NONE`, not an `Option`; paint-time `is_noop` filtering catches
/// authored and animation-decayed no-ops.
///
/// Plain data, so arithmetic may pass through values no widget takes. A widget's
/// `chrome` and `default_chrome` check it where it enters and panic unless
/// every color is a [color](crate::widget::domain::color), gradient angles are
/// [angles](crate::widget::domain::angle), centers and the shadow's offset and spread
/// [offsets](crate::widget::domain::offset), the border width, radial radii, each corner
/// radius and the shadow's blur [lengths](crate::widget::domain::length), and the material as
/// [`Material`] lists. Every value but the border width packs into an f16 lane, so it is a
/// [lane](crate::widget::domain::lane) too.
// Intentionally **not `Copy`**: it is the largest of the three types
// `primitives::math::animatable::Animatable` discusses, and the recording chain takes it
// by reference.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Lerp, Animatable)]
#[must_use]
pub struct Chrome {
    /// Interior paint. [`Brush::TRANSPARENT`] fills nothing.
    pub fill: Brush,
    /// The edge ring, painted inside the rect. Layout adds its width to the
    /// padding, so children sit inside the border. `Border::NONE` (the `Default`)
    /// is omitted from serialized output.
    #[serde(default, skip_serializing_if = "Border::is_noop")]
    pub border: Border,
    /// Zero (or sub-`EPS`) radii, the `Default`, are omitted from serialized
    /// output.
    #[serde(
        default,
        skip_serializing_if = "Corners::is_approx_zero",
        deserialize_with = "crate::primitives::math::domain::file::corners"
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
    /// The backdrop effect the box wears under its fill, which tints what it shows; `None` (the
    /// `Default`) wears none and is omitted from serialized output. A gradient fill and the
    /// border paint over it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<Material>,
}

impl Chrome {
    /// Panics unless its fill, border, corners, shadow and material each pass their own
    /// check. One branch-free test of every part, the checks that name the broken rule only on
    /// the cold path: `Widget::record` runs it on every chromed widget each frame, where the part
    /// checks one by one took 4.4 % of a CPU frame (`.notes/PERF.md`, "The CPU frame").
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        if !self.is_valid() {
            self.reject();
        }
    }

    /// Whether every part holds its kinds: the test [`Self::validate`] makes, without naming the
    /// rule a part breaks.
    const fn is_valid(&self) -> bool {
        self.fill.is_valid()
            & self.border.is_valid()
            & self.corners.is_valid()
            & self.shadow.is_valid()
            & match &self.material {
                Some(material) => material.is_valid(),
                None => true,
            }
    }

    /// [`Self::validate`]'s cold half: panics with the rule a part breaks.
    #[cold]
    #[inline(never)]
    #[track_caller]
    const fn reject(&self) -> ! {
        self.fill.validate();
        self.border.validate();
        self.corners.validate();
        self.shadow.validate();
        if let Some(material) = &self.material {
            material.validate();
        }
        panic!("`Chrome::is_valid` refused a chrome whose parts each pass their check");
    }

    /// Canonical chrome that paints nothing; an explicit override to suppress
    /// theme chrome.
    pub const NONE: Self = Self {
        fill: Brush::TRANSPARENT,
        border: Border::NONE,
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
        material: None,
    };

    /// True when this paints nothing visible. The encoder skips no-op chrome; the
    /// shadow check is required since the chrome branch paints the shadow as its
    /// own draw.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        self.fill.is_noop()
            && self.border.is_noop()
            && self.shadow.is_noop()
            && self.material.is_none()
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
    /// width paints nothing. On the width alone, not [`Border::is_noop`]: a border an invisible
    /// color makes still gets room.
    #[inline]
    pub(crate) const fn border_inset(&self) -> f32 {
        if is_invisible(self.border.width) {
            0.0
        } else {
            self.border.width
        }
    }

    /// A plain fill: no border, corners, shadow or material.
    pub fn fill(brush: impl Into<Brush>) -> Self {
        Self {
            fill: brush.into(),
            border: Border::NONE,
            corners: Corners::ZERO,
            shadow: Shadow::NONE,
            material: None,
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
    pub const fn with_border(mut self, border: Border) -> Self {
        self.border = border;
        self
    }

    /// Set the drop/inset shadow, keeping fill/corners/border.
    pub const fn with_shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = shadow;
        self
    }

    /// Set the material, keeping the rest: the fill tints what it shows.
    pub const fn with_material(mut self, material: Material) -> Self {
        self.material = Some(material);
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
            || self.material.as_ref().is_some_and(Material::has_nan)
    }
}

impl Default for Chrome {
    fn default() -> Self {
        Self::NONE
    }
}

/// A plain fill of `brush`, as [`Chrome::fill`]: sugar for a box that only fills, as
/// `Shape::rect(color)` and `Modal::scrim(color)` take one.
impl From<Brush> for Chrome {
    fn from(brush: Brush) -> Self {
        Self::fill(brush)
    }
}

/// A plain fill of `color`, as [`Chrome::fill`].
impl From<RgbaF32> for Chrome {
    fn from(color: RgbaF32) -> Self {
        Self::fill(color)
    }
}

/// A plain fill of `gradient`, as [`Chrome::fill`].
impl<G: GradientGeometry> From<Gradient<G>> for Chrome
where
    Brush: From<Gradient<G>>,
{
    fn from(gradient: Gradient<G>) -> Self {
        Self::fill(gradient)
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
    use crate::primitives::paint::brush::gradient::conic_geometry::ConicGradient;
    use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::paint::brush::gradient::radial_geometry::RadialGradient;
    use crate::primitives::paint::material::lens::Lens;
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

    /// `validate`'s one branch-free test refuses exactly what the part checks one by one refuse,
    /// and panics with the same rule at the caller: each scalar of each part swept over the
    /// special values of its kinds, around a chrome that holds every part, refusing as many of
    /// them as its kind does.
    #[test]
    fn validate_refuses_what_the_part_checks_refuse() {
        type Set = fn(&mut Chrome, f32);
        /// The fill's scalar `idx` of two, whatever its kind: one of each kind of value it holds.
        fn fill(chrome: &mut Chrome, idx: usize) -> &mut f32 {
            match (&mut chrome.fill, idx) {
                (Brush::Solid(c), 0) => &mut c.r,
                (Brush::Solid(c), _) => &mut c.a,
                (Brush::Linear(g), _) => &mut g.geometry.angle,
                (Brush::Radial(g), 0) => &mut g.geometry.center.x,
                (Brush::Radial(g), _) => &mut g.geometry.radius.y,
                (Brush::Conic(g), 0) => &mut g.geometry.center.y,
                (Brush::Conic(g), _) => &mut g.geometry.start_angle,
            }
        }
        fn material(chrome: &mut Chrome) -> &mut Material {
            chrome.material.as_mut().unwrap()
        }
        fn lens(chrome: &mut Chrome) -> &mut Lens {
            &mut material(chrome).input.lens
        }
        // How many of `values` each kind refuses. A lane (a color channel, an offset): ±65520,
        // f32::MAX, NaN, ±inf. A length that is a lane, or a factor: those and -1. An angle: NaN
        // and ±inf. A length that is no lane (a border's width): -1, -65520, NaN and ±inf. A
        // refractive index that is a lane: a lane's six and the five below 1.
        const LANE: usize = 6;
        const LENGTH_LANE: usize = 7;
        const ANGLE: usize = 3;
        const LENGTH: usize = 5;
        const REFRACTIVE_INDEX: usize = 11;
        let sets: [(Set, usize); 15] = [
            (|c, v| c.border.width = v, LENGTH),
            (|c, v| c.border.color.g = v, LANE),
            (|c, v| c.shadow.color.b = v, LANE),
            (|c, v| c.shadow.offset.x = v, LANE),
            (|c, v| c.shadow.offset.y = v, LANE),
            (|c, v| c.shadow.blur = v, LENGTH_LANE),
            (|c, v| c.shadow.spread = v, LANE),
            (|c, v| material(c).blur = v, LENGTH_LANE),
            (|c, v| material(c).input.saturation = v, LENGTH_LANE),
            (|c, v| material(c).input.noise = v, LENGTH_LANE),
            (|c, v| material(c).max_luminance = Some(v), LENGTH_LANE),
            (|c, v| material(c).min_luminance = Some(v), LENGTH_LANE),
            (|c, v| lens(c).bezel = v, LENGTH_LANE),
            (|c, v| lens(c).thickness = v, LENGTH_LANE),
            (|c, v| lens(c).refractive_index = v, REFRACTIVE_INDEX),
        ];
        let ink = RgbaF32::new(0.2, 0.4, 0.6, 1.0);
        // Each kind of fill, with what its two scalars refuse.
        let fills: [(Brush, [usize; 2]); 4] = [
            (ink.into(), [LANE, LANE]),
            (
                LinearGradient::two_stop(0.5, ink, RgbaF32::WHITE).into(),
                [ANGLE, ANGLE],
            ),
            (
                RadialGradient::two_stop(Vec2::splat(0.5), Vec2::splat(0.5), ink, RgbaF32::WHITE)
                    .into(),
                [LANE, LENGTH_LANE],
            ),
            (
                ConicGradient::two_stop(Vec2::splat(0.5), 0.5, ink, RgbaF32::WHITE).into(),
                [LANE, ANGLE],
            ),
        ];
        // Each kind's edges: a sign, zero, the lane's last value and the first past it, the
        // refractive index's floor, and the non-finite values.
        let values = [
            0.5,
            0.0,
            -0.0,
            -1.0,
            1.0,
            0.999,
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
                .with_shadow(Shadow::drop(RgbaF32::BLACK, Vec2::new(0.0, 4.0), 6.0))
                .with_material(Material::glass(Lens::WINDOW_GLASS));
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
                        chrome.fill.validate();
                        chrome.border.validate();
                        chrome.corners.validate();
                        chrome.shadow.validate();
                        if let Some(material) = &chrome.material {
                            material.validate();
                        }
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
        assert_eq!(rounded, Chrome::fill(ink).with_corners(Corners::all(6.0)));
        assert_eq!(rounded.corners, Corners::all(6.0));

        let bordered = rounded.with_border(Border::new(RgbaF32::BLACK, 2.0));
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
            .with_border(Border::new(RgbaF32::BLACK, 2.0))
            .with_shadow(Shadow::drop(RgbaF32::BLACK, Vec2::new(0.0, 4.0), 6.0));
        let half = |color| RgbaF32::lerp(&RgbaF32::TRANSPARENT, color, 0.5);
        let mid = Chrome {
            fill: half(&ink).into(),
            border: Border::new(half(&RgbaF32::BLACK), 1.0),
            corners: Corners::all(4.0),
            shadow: Shadow::drop(half(&RgbaF32::BLACK), Vec2::new(0.0, 2.0), 3.0),
            material: None,
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
