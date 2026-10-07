//! The chrome a container paints behind its children: fill, border, corner radii
//! and a shadow, as one value a theme hands over whole.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use palantir_anim_derive::Animatable;

/// Paint data shared by container widgets (`Block`, `Panel`, `Grid`) and per-state
/// widget visuals. [`Self::NONE`] is transparent fill, no border, zero radius:
/// nothing emitted. Pure data; paint emission goes through `Tree::chrome_table` and
/// the encoder.
///
/// `Animatable` is derived: fill and border interpolate componentwise, `radius` is
/// `#[animate(snap)]`. "No border" is `Stroke::NONE`, not an `Option`; paint-time
/// `is_noop` filtering catches authored and animation-decayed no-ops.
///
/// Plain data, so arithmetic may pass through values no widget takes. A widget's
/// `background` and `default_background` check it where it enters and panic unless
/// every colour is a [colour](crate::widget::domain::color), gradient angles are
/// [angles](crate::widget::domain::angle), centres
/// [offsets](crate::widget::domain::offset), radial radii and the border width
/// [lengths](crate::widget::domain::length), each corner radius a length of at most
/// 65504 (one f16 lane), and the shadow's offset and spread offsets and its blur a
/// length.
// Intentionally **not `Copy`**: it is the largest of the three types
// `animation::animatable::Animatable` discusses, and the recording chain takes it
// by reference.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Animatable)]
#[must_use]
pub struct Background {
    /// Interior paint. [`Brush::TRANSPARENT`] fills nothing.
    pub fill: Brush,
    /// The edge ring, painted inside the rect. Layout adds its width to the
    /// padding, so children sit inside the border. `Stroke::NONE` (the `Default`)
    /// is omitted from serialized output.
    #[serde(default, skip_serializing_if = "Stroke::is_noop")]
    pub border: Stroke,
    /// Zero (or sub-`EPS`) radii, the `Default`, are omitted from serialized
    /// output.
    #[serde(default, skip_serializing_if = "Corners::is_approx_zero")]
    #[animate(snap)]
    pub corners: Corners,
    /// Single drop / inset shadow; `Shadow::NONE` (the `Default`) means none and is
    /// omitted from serialized output. It animates componentwise (alpha lerps for
    /// hover-elevation). For multi-shadow stacks push shadow shapes through
    /// `Ui::add_shape`. As in CSS `box-shadow`, a drop shadow paints under the fill
    /// and an inset one over it, inside the border.
    #[serde(default, skip_serializing_if = "Shadow::is_noop")]
    pub shadow: Shadow,
}

impl Background {
    /// Panics unless its fill, border, corners and shadow each pass their own
    /// check.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        self.fill.validate();
        self.border.validate();
        self.corners.validate();
        self.shadow.validate();
    }

    /// Canonical background that paints nothing; an explicit override to suppress
    /// theme chrome.
    pub const NONE: Self = Self {
        fill: Brush::TRANSPARENT,
        border: Stroke::NONE,
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };

    /// True when this paints nothing visible. The encoder skips no-op chrome; the
    /// shadow check is required since the chrome branch paints the shadow as its
    /// own draw.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        self.fill.is_noop() && self.border.is_noop() && self.shadow.is_noop()
    }

    /// How far the border reaches in from the edge: its width, or zero when the
    /// width paints nothing. **The one definition of the fold** `Tree::open_node`
    /// applies to a chrome's padding, for widgets that need the inner rect before
    /// the tree has it (`TextEdit`, `TextEditTheme::corner_centering`). On the
    /// width alone, not [`Stroke::is_noop`]: a border an invisible colour makes
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
        }
    }

    /// Solid fill with rounded corners; no border or shadow.
    pub fn rounded<I: Into<Brush>>(brush: I, corners: Corners) -> Self {
        Self {
            fill: brush.into(),
            border: Stroke::NONE,
            corners,
            shadow: Shadow::NONE,
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
}

/// Every field can carry a NaN (the fill through a gradient's geometry, the rest
/// through scalars); `lower::background` screens it, as a `Background` never passes
/// through `Shapes::add`.
impl NanCheck for Background {
    fn has_nan(&self) -> bool {
        self.fill.has_nan()
            || self.border.has_nan()
            || self.corners.has_nan()
            || self.shadow.has_nan()
    }
}

impl Default for Background {
    fn default() -> Self {
        Self::NONE
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::color::RgbaF32;

    use super::*;

    // `with_border`/`with_shadow` chained in a const context fail to compile if
    // they stop being const.
    const _CONST_BUILDER: Background = Background::NONE
        .with_border(Stroke::NONE)
        .with_shadow(Shadow::NONE);

    #[test]
    fn with_border_and_with_shadow_set_the_named_field_only() {
        let base = Background::rounded(RgbaF32::WHITE, Corners::all(4.0));
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
    fn none_is_the_default_noop_background() {
        assert_eq!(Background::default(), Background::NONE);
        assert!(Background::NONE.is_noop());
    }
}
