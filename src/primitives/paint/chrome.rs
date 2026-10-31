//! The chrome a container paints behind its children: fill, border, corner radii,
//! a shadow and a backdrop filter, as one value a theme hands over whole.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use palantir_anim_derive::Animatable;

/// Paint data shared by container widgets (`Block`, `Panel`, `Grid`) and per-state
/// widget visuals. [`Self::NONE`] is transparent fill, no border, square corners:
/// nothing emitted. Pure data; paint emission goes through `Tree::chrome_table` and
/// the encoder.
///
/// `Animatable` is derived: fill and border interpolate componentwise, `corners` is
/// `#[animate(snap)]`. "No border" is `Stroke::NONE`, not an `Option`; paint-time
/// `is_noop` filtering catches authored and animation-decayed no-ops.
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
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Animatable)]
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
    #[animate(snap)]
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

    /// How far the border reaches in from the edge: its width, or zero when the
    /// width paints nothing. **The one definition of the fold** `Tree::open_node`
    /// applies to a chrome's padding, for widgets that need the inner rect before
    /// the tree has it (`TextEdit`, `TextEditTheme::corner_centering`). On the
    /// width alone, not [`Stroke::is_noop`]: a border an invisible color makes
    /// still gets room.
    #[inline]
    pub(crate) const fn border_inset(&self) -> f32 {
        if is_invisible(self.border.width) {
            0.0
        } else {
            self.border.width
        }
    }

    /// A plain fill: no border, corners or shadow.
    pub fn fill<I: Into<Brush>>(brush: I) -> Self {
        Self {
            fill: brush.into(),
            border: Stroke::NONE,
            corners: Corners::ZERO,
            shadow: Shadow::NONE,
            backdrop_filter: BackdropFilter::NONE,
        }
    }

    /// Solid fill with rounded corners; no border or shadow.
    pub fn rounded<I: Into<Brush>>(brush: I, corners: Corners) -> Self {
        Self {
            fill: brush.into(),
            border: Stroke::NONE,
            corners,
            shadow: Shadow::NONE,
            backdrop_filter: BackdropFilter::NONE,
        }
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

    /// This chrome over frosted glass: its own solid fill as the tint, at `frost.tint_alpha`,
    /// over a backdrop blurred by `frost.blur`, as Fluent puts acrylic under flyouts and menus
    /// (<https://learn.microsoft.com/windows/apps/design/style/acrylic>). The glass shows at most
    /// the luminance that keeps `frost.ink`, the dimmest ink the surface paints, at WCAG's 4.5:1
    /// over any backdrop, as an 8-bit target stores the composite.
    ///
    /// The brightest ground `ink` holds 4.5:1 on is `g = (Y(ink) + 0.05) / 4.5 − 0.05`; a step of
    /// the 8-bit target lowers it to `g'`, what the composite may reach before the target stores
    /// it higher. The ground over a backdrop at the ceiling `c` is `(1 − α)·c + α·Y(tint)`, both
    /// in the linear light the composite runs in; so `c = (g' − α·Y(tint)) / (1 − α)`.
    ///
    /// # Panics
    ///
    /// Unless the fill is solid, `tint_alpha` is a fraction under 1, and the ink holds its
    /// contrast on the opaque tint.
    #[track_caller]
    pub fn with_frost(mut self, frost: Frost) -> Self {
        let Frost {
            blur,
            tint_alpha,
            ink,
        } = frost;
        let tint = self
            .fill
            .as_solid()
            .expect("a frosted surface has a solid fill");
        assert!(
            (0.0..1.0).contains(&tint_alpha),
            "a frost's tint alpha is a fraction under 1: {tint_alpha}"
        );
        debug_assert!(
            tint_alpha * tint.r.min(tint.g).min(tint.b) > SRGB_TOE,
            "every channel of a ground clears the sRGB curve's linear toe, as `before_storing` needs"
        );
        let ground = before_storing((ink.luminance() + 0.05) / TEXT_CONTRAST - 0.05);
        let tint_luminance = tint.luminance();
        assert!(
            tint_luminance <= ground,
            "the ink holds its contrast on the opaque surface"
        );
        let ceiling = (ground - tint_alpha * tint_luminance) / (1.0 - tint_alpha);
        self.fill = tint.with_alpha(tint_alpha).into();
        self.backdrop_filter = BackdropFilter::frosted(blur).with_max_luminance(ceiling);
        self
    }
}

/// What [`Chrome::with_frost`] lays under a chrome.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frost {
    /// The backdrop's blur, σ in logical px.
    pub blur: f32,
    /// The tint's alpha over the glass, a fraction under 1.
    pub tint_alpha: f32,
    /// The dimmest ink the surface paints, which holds its contrast over any backdrop.
    pub ink: RgbaF32,
}

/// The contrast WCAG 1.4.3 asks of text against its ground
/// (<https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum>).
const TEXT_CONTRAST: f32 = 4.5;

/// The linear level where the sRGB curve's linear toe ends and its power segment starts.
const SRGB_TOE: f32 = 0.003_130_8;

/// The brightest linear level a composite may reach and still store at most `g`: an 8-bit target
/// stores a channel within one encoded step of it, half a step of the dither the composite adds
/// and half of the rounding to a byte. So the level is the one a step under `B`, the highest byte
/// that decodes to at most `g`. A colored pixel stores no brighter than a neutral one of its
/// luminance: on the curve's power segment, where every channel of a ground lies as it keeps `α`
/// of the tint's, the stored level `decode(encode(c) + step)` is concave in `c`, so by Jensen the
/// weighted channels gain at most what their mean does.
fn before_storing(g: f32) -> f32 {
    let level = |byte: u8| RgbaF32::from_srgba(SrgbaU8::rgb(byte, byte, byte)).r;
    let mut top = RgbaF32::new(g, g, g, 1.0).to_srgba().r;
    if level(top) > g {
        top -= 1;
    }
    level(
        top.checked_sub(1)
            .expect("the ink holds its contrast a step above black"),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The glass's ceiling holds the muted ink at WCAG's 4.5:1 over the brightest backdrop as an
    /// 8-bit target stores it. `#a4a4a4` is `Y = 0.371238`, so the brightest ground is
    /// `(0.371238 + 0.05) / 4.5 − 0.05 = 0.043608`, which encodes to byte 58.91, so byte 58 is
    /// the highest at or under it, and the composite may reach byte 57, `0.04091520`. On a
    /// `#383838` tint, `Y = 0.039546`, the ceiling is
    /// `(0.04091520 − 0.72 · 0.039546) / 0.28 = 0.04443538`, and on `#1f1f1f`, `Y = 0.013702`,
    /// `(0.04091520 − 0.72 · 0.013702) / 0.28 = 0.11089178`. A ground at either ceiling stores
    /// byte 57.
    #[test]
    fn a_frost_holds_its_ink_at_the_text_contrast() {
        let ink = RgbaF32::hex(0xa4a4a4);
        let level = |byte: u8| RgbaF32::from_srgba(SrgbaU8::rgb(byte, byte, byte));
        assert!(ink.contrast_ratio(level(58)) >= 4.5);
        assert!(
            ink.contrast_ratio(level(59)) < 4.5,
            "byte 58 is the highest"
        );
        for (tint, want) in [(0x383838, 0.04443538), (0x1f1f1f, 0.11089178)] {
            let tint = RgbaF32::hex(tint);
            let frost = Frost {
                blur: 20.0,
                tint_alpha: 0.72,
                ink,
            };
            let glass = Chrome::rounded(tint, Corners::all(4.0)).with_frost(frost);
            let ceiling = glass.backdrop_filter.max_luminance.expect("a ceiling");
            assert_eq!(
                glass.backdrop_filter,
                BackdropFilter::frosted(20.0).with_max_luminance(ceiling)
            );
            // The hand value's eighth digit, and the derivation's f32 rounding, a few ulps of 1e-8.
            assert!((ceiling - want).abs() < 1e-7, "{ceiling}");
            assert_eq!(glass.fill, tint.with_alpha(0.72).into());
            let ground = 0.28 * ceiling + 0.72 * tint.luminance();
            assert!((ground - level(57).r).abs() < 2e-8, "{ground}");
        }
    }

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
}
