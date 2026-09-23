//! The chrome a container paints behind its children: fill, border,
//! corner radii and a shadow, as one value a theme hands over whole.

use crate::primitives::approx::paints_nothing;
use crate::primitives::brush::Brush;
use crate::primitives::corners::Corners;
use crate::primitives::nan::NanCheck;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use palantir_anim_derive::Animatable;

/// Paint data shared by container widgets (`Block`, `Panel`, `Grid`)
/// and per-state widget visuals: fill colour, optional border, and
/// corner radii. [`Self::NONE`] is transparent fill / no border / zero radius
/// — emitting nothing.
///
/// Pure data, no methods that need a `Ui` — paint emission goes
/// through `Tree::chrome_table` and the encoder, not through
/// shape-list registration.
///
/// `Animatable` derived: fill and border interpolate componentwise;
/// `radius` is `#[animate(snap)]` (corner-radius morphing across
/// states is rarely-wanted polish and would require `Corners:
/// Animatable`). "No border" is `Stroke::ZERO` (width 0, transparent)
/// — there is no `Option<Stroke>` here. The animation pipeline lerps
/// `Stroke` directly through `Stroke::ZERO`; paint-time `is_noop`
/// filtering catches both authored and animation-decayed no-ops.
// `Background` is intentionally **not `Copy`**: the recording chain
// (`Widget::record` → `Forest::open_node` → `Tree::open_node` →
// `shapes::lower::background`) takes it by reference. It is the largest
// of the three types `animation::animatable::Animatable` states the
// whole argument and the measurement for.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Animatable)]
pub struct Background {
    /// Interior paint. [`Brush::TRANSPARENT`] fills nothing.
    pub fill: Brush,
    /// The edge ring, painted inside the rect. Layout adds its width to
    /// the padding, so children sit inside the border without the caller
    /// subtracting it.
    ///
    /// `Stroke::ZERO` (the `Default`) omitted from serialized output —
    /// the common "fill-only, no border" case stays compact.
    #[serde(default, skip_serializing_if = "Stroke::is_noop")]
    pub border: Stroke,
    /// Zero (or sub-`EPS`) radii — the `Default` — omitted from
    /// serialized output.
    #[serde(default, skip_serializing_if = "Corners::approx_zero")]
    #[animate(snap)]
    pub corners: Corners,
    /// Single drop / inset shadow. `Shadow::NONE` (the `Default`) is
    /// the "no shadow" sentinel — matches the `Stroke::ZERO` border
    /// convention so the field stays plain `Shadow` and animates
    /// componentwise (alpha lerps in/out for hover-elevation), with
    /// the paint-time `is_noop` filter catching authored or
    /// animation-decayed no-ops. Multi-shadow stacks: push
    /// `Shape::Shadow` records directly via `Ui::add_shape`.
    /// `Shadow::NONE` (the `Default`) omitted from serialized output —
    /// a noop shadow shouldn't bloat exported themes.
    #[serde(default, skip_serializing_if = "Shadow::is_noop")]
    pub shadow: Shadow,
}

impl Background {
    /// Canonical background that paints nothing. Use this as an explicit
    /// builder override when a theme supplies chrome that this widget should
    /// suppress.
    pub const NONE: Self = Self {
        fill: Brush::TRANSPARENT,
        border: Stroke::ZERO,
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };

    /// True when this Background paints nothing visible — transparent
    /// fill + transparent/zero-width border + no-op shadow. The
    /// encoder skips emitting a rect quad for no-op chrome so
    /// transparent `Surface::scissor()` defaults don't leak draw
    /// commands. The shadow check is required: the encoder's chrome
    /// branch paints shadow before the rect, so dropping chrome
    /// without considering shadow would silently kill a shadow-only
    /// background.
    #[inline]
    pub fn is_noop(&self) -> bool {
        self.fill.is_noop() && self.border.is_noop() && self.shadow.is_noop()
    }

    /// How far the border reaches in from the edge: its width, or zero
    /// when the width paints nothing.
    ///
    /// **The one definition of the fold** `Tree::open_node` applies to a
    /// chrome's padding, so children sit inside the border without the
    /// caller adding it by hand. The widgets that need the same inner
    /// rect before the tree has it — `TextEdit` for its glyph and caret
    /// coordinates, `TextEditTheme::corner_centring` for a `DragValue`'s
    /// in-place edit — read it here rather than each writing the gate
    /// out.
    ///
    /// On the width alone, and not [`Stroke::is_noop`]: a border the
    /// colour makes invisible is still a border the fold makes room for.
    #[inline]
    pub(crate) const fn border_inset(&self) -> f32 {
        if paints_nothing(self.border.width) {
            0.0
        } else {
            self.border.width
        }
    }

    /// A plain fill — no border, no corners, no shadow.
    pub fn fill<I: Into<Brush>>(brush: I) -> Self {
        Self {
            fill: brush.into(),
            border: Stroke::ZERO,
            corners: Corners::ZERO,
            shadow: Shadow::NONE,
        }
    }

    /// Solid fill with rounded corners — no border, no shadow. The
    /// corner sibling of [`Self::fill`].
    pub fn rounded<I: Into<Brush>>(brush: I, corners: Corners) -> Self {
        Self {
            fill: brush.into(),
            border: Stroke::ZERO,
            corners,
            shadow: Shadow::NONE,
        }
    }

    /// Set the border, keeping fill/corners/shadow — the chaining
    /// sibling of [`Self::fill`] / [`Self::rounded`], which start
    /// borderless.
    pub const fn with_border(mut self, border: Stroke) -> Self {
        self.border = border;
        self
    }

    /// Set the drop/inset shadow, keeping fill/corners/border. Chains after
    /// [`Self::fill`] / [`Self::rounded`], which start shadowless.
    pub const fn with_shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = shadow;
        self
    }
}

/// Every field can carry one: the fill through a gradient's geometry,
/// the other three through their own scalars. `lower::background` is
/// what screens it — the chrome path has no record-level gate behind it,
/// because a `Background` never passes through `Shapes::add`.
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
    use crate::primitives::color::RgbaF32;

    use super::*;

    // `with_border`/`with_shadow` chained in a const context. If either
    // regresses to non-const, this fails to compile.
    const _CONST_BUILDER: Background = Background::NONE
        .with_border(Stroke::ZERO)
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
        assert_eq!(with_shadow.shadow.blur, 4.0);
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
