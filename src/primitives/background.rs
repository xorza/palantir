//! The chrome a container paints behind its children: fill, stroke,
//! corner radii and a shadow, as one value a theme hands over whole.

use crate::primitives::brush::Brush;
use crate::primitives::corners::Corners;
use crate::primitives::nan::NanCheck;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use palantir_anim_derive::Animatable;

/// Paint data shared by container widgets (`Frame`, `Panel`, `Grid`)
/// and per-state widget visuals: fill colour, optional stroke, and
/// corner radii. [`Self::NONE`] is transparent fill / no stroke / zero radius
/// — emitting nothing.
///
/// Pure data, no methods that need a `Ui` — paint emission goes
/// through `Tree::chrome_table` and the encoder, not through
/// shape-list registration.
///
/// `Animatable` derived: fill and stroke interpolate componentwise;
/// `radius` is `#[animate(snap)]` (corner-radius morphing across
/// states is rarely-wanted polish and would require `Corners:
/// Animatable`). "No stroke" is `Stroke::ZERO` (width 0, transparent)
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
    pub fill: Brush,
    /// `Stroke::ZERO` (the `Default`) omitted from serialized output —
    /// the common "fill-only, no border" case stays compact.
    #[serde(default, skip_serializing_if = "Stroke::is_noop")]
    pub stroke: Stroke,
    /// Zero (or sub-`EPS`) radii — the `Default` — omitted from
    /// serialized output.
    #[serde(default, skip_serializing_if = "Corners::approx_zero")]
    #[animate(snap)]
    pub corners: Corners,
    /// Single drop / inset shadow. `Shadow::NONE` (the `Default`) is
    /// the "no shadow" sentinel — matches the `Stroke::ZERO`
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
        stroke: Stroke::ZERO,
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };

    /// True when this Background paints nothing visible — transparent
    /// fill + transparent/zero-width stroke + no-op shadow. The
    /// encoder skips emitting a rect quad for no-op chrome so
    /// transparent `Surface::scissor()` defaults don't leak draw
    /// commands. The shadow check is required: the encoder's chrome
    /// branch paints shadow before the rect, so dropping chrome
    /// without considering shadow would silently kill a shadow-only
    /// background.
    #[inline]
    pub fn is_noop(&self) -> bool {
        self.fill.is_noop() && self.stroke.is_noop() && self.shadow.is_noop()
    }

    pub fn fill<I: Into<Brush>>(brush: I) -> Self {
        Self {
            fill: brush.into(),
            stroke: Stroke::ZERO,
            corners: Corners::ZERO,
            shadow: Shadow::NONE,
        }
    }

    /// Solid fill with rounded corners — no stroke, no shadow. The
    /// corner sibling of [`Self::fill`].
    pub fn rounded<I: Into<Brush>>(brush: I, corners: Corners) -> Self {
        Self {
            fill: brush.into(),
            stroke: Stroke::ZERO,
            corners,
            shadow: Shadow::NONE,
        }
    }

    /// Set the border stroke, keeping fill/corners/shadow — the chaining
    /// sibling of [`Self::fill`] / [`Self::rounded`], which start strokeless.
    pub const fn with_stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = stroke;
        self
    }

    /// Set the drop/inset shadow, keeping fill/corners/stroke. Chains after
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
            || self.stroke.has_nan()
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
    use crate::primitives::color::Color;

    use super::*;

    // `with_stroke`/`with_shadow` chained in a const context. If either
    // regresses to non-const, this fails to compile.
    const _CONST_BUILDER: Background = Background::NONE
        .with_stroke(Stroke::ZERO)
        .with_shadow(Shadow::NONE);

    #[test]
    fn with_stroke_and_with_shadow_set_the_named_field_only() {
        let base = Background::rounded(Color::WHITE, Corners::all(4.0));
        let stroke = Stroke::solid(Color::BLACK, 2.0);
        let shadow = Shadow::drop(Color::BLACK, glam::Vec2::ZERO, 4.0);

        let with_stroke = base.clone().with_stroke(stroke);
        assert_eq!(with_stroke.stroke, stroke);
        assert_eq!(with_stroke.fill, base.fill);
        assert_eq!(with_stroke.corners, base.corners);
        assert_eq!(with_stroke.shadow, base.shadow);

        let with_shadow = base.clone().with_shadow(shadow);
        assert_eq!(with_shadow.shadow.blur, 4.0);
        assert_eq!(with_shadow.fill, base.fill);
        assert_eq!(with_shadow.stroke, base.stroke);
        assert_eq!(with_shadow.corners, base.corners);
    }

    #[test]
    fn none_is_the_default_noop_background() {
        assert_eq!(Background::default(), Background::NONE);
        assert!(Background::NONE.is_noop());
    }
}
