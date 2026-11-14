//! [`Paint`]: what covers a box, a rect shape's, a circle's, a triangle's or a node's.

use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::Gradient;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::material_paint::MaterialPaint;
use crate::primitives::paint::shadow::Shadow;
use palantir_derive::{Animatable, Lerp};

/// What covers a box: its fill, its border and its shadow, as CSS pairs `background-color` with
/// `border` and `box-shadow`. A rect, a circle and a triangle take it
/// ([`Ui::add_shape`](crate::Ui::add_shape)), and a node's [`Chrome`](crate::Chrome) holds one
/// beside its corners. [`Self::NONE`] paints nothing. A shape wears a material under it through
/// [`Self::with_material`], and nothing else does (`src/shape/AGENTS.md`, "A material is a
/// shape's paint").
///
/// Bottom to top: a drop shadow; the fill over the whole box, as CSS's background covers the
/// border box; an inset shadow inside the border; the border inside the edge, over the fill.
///
/// Plain data, so arithmetic may pass through values no widget takes. A widget's `chrome` and a
/// shape check it where it enters and panic unless every color is a
/// [color](crate::widget::domain::color), gradient angles are
/// [angles](crate::widget::domain::angle), centers and the shadow's offset and spread
/// [offsets](crate::widget::domain::offset), the border width, radial radii and the shadow's blur
/// [lengths](crate::widget::domain::length). Every value but the border width packs into an f16 lane, so it is a [lane](crate::widget::domain::lane) too.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Lerp, Animatable)]
#[serde(deny_unknown_fields)]
#[must_use]
pub struct Paint {
    /// The interior's brush. [`Brush::TRANSPARENT`] fills nothing.
    pub fill: Brush,
    /// The edge band, painted inside the box. Layout adds a node's border width to its padding,
    /// so children sit inside the border. `Border::NONE` (the `Default`) is omitted from
    /// serialized output.
    #[serde(default, skip_serializing_if = "Border::is_noop")]
    pub border: Border,
    /// One drop or inset shadow; `Shadow::NONE` (the `Default`) means none and is omitted from
    /// serialized output. As in CSS `box-shadow`, a drop shadow paints under the fill and an
    /// inset one over it, inside the border; a stack of shadows is one shape added again with a
    /// paint of a shadow alone.
    #[serde(default, skip_serializing_if = "Shadow::is_noop")]
    pub shadow: Shadow,
}

impl Paint {
    /// Paint that covers nothing.
    pub const NONE: Self = Self {
        fill: Brush::TRANSPARENT,
        border: Border::NONE,
        shadow: Shadow::NONE,
    };

    /// This paint with its fill `brush`.
    pub fn with_fill(mut self, brush: impl Into<Brush>) -> Self {
        self.fill = brush.into();
        self
    }

    /// This paint with its border `border`.
    pub const fn with_border(mut self, border: Border) -> Self {
        self.border = border;
        self
    }

    /// This paint with its drop or inset shadow `shadow`.
    pub const fn with_shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = shadow;
        self
    }

    /// This paint over `material`, which its fill covers: a shape's paint that wears a material.
    pub const fn with_material(self, material: Material) -> MaterialPaint {
        MaterialPaint {
            paint: self,
            material,
        }
    }

    /// True when this paints nothing visible.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        self.fill.is_noop() && self.border.is_noop() && self.shadow.is_noop()
    }

    /// How far the border reaches in from the edge: its width, or zero when the width paints
    /// nothing. On the width alone, not [`Border::is_noop`]: a border an invisible color makes
    /// still gets room.
    #[inline]
    pub(crate) const fn border_inset(&self) -> f32 {
        if is_invisible(self.border.width) {
            0.0
        } else {
            self.border.width
        }
    }

    /// Whether every part holds its kinds: the test [`Self::validate`] makes, without naming the
    /// rule a part breaks.
    pub(crate) const fn is_valid(&self) -> bool {
        self.fill.is_valid() & self.border.is_valid() & self.shadow.is_valid()
    }

    /// Panics unless its fill, border and shadow each pass their own check: one
    /// branch-free test, the checks that name the broken rule only on the cold path.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        if !self.is_valid() {
            self.reject();
        }
    }

    /// [`Self::validate`]'s cold half: panics with the rule a part breaks.
    #[cold]
    #[inline(never)]
    #[track_caller]
    pub(crate) const fn reject(&self) -> ! {
        self.fill.validate();
        self.border.validate();
        self.shadow.validate();
        panic!("`Paint::is_valid` refused a paint whose parts each pass their check");
    }
}

/// Every field can carry a NaN, the fill through a gradient's geometry, the rest through scalars.
impl NanCheck for Paint {
    fn has_nan(&self) -> bool {
        self.fill.has_nan() || self.border.has_nan() || self.shadow.has_nan()
    }
}

impl Default for Paint {
    fn default() -> Self {
        Self::NONE
    }
}

/// A plain fill of `brush`: no border or shadow.
impl From<Brush> for Paint {
    fn from(brush: Brush) -> Self {
        Self {
            fill: brush,
            ..Self::NONE
        }
    }
}

/// A plain fill of `color`.
impl From<RgbaF32> for Paint {
    fn from(color: RgbaF32) -> Self {
        Self::from(Brush::from(color))
    }
}

/// A plain fill of `color`, decoded from sRGB bytes.
impl From<SrgbaU8> for Paint {
    fn from(color: SrgbaU8) -> Self {
        Self::from(Brush::from(color))
    }
}

/// A plain fill of `gradient`.
impl From<Gradient> for Paint {
    fn from(gradient: Gradient) -> Self {
        Self::from(Brush::from(gradient))
    }
}

/// A box of `border` alone: no fill or shadow.
impl From<Border> for Paint {
    fn from(border: Border) -> Self {
        Self::NONE.with_border(border)
    }
}

/// A box of `shadow` alone: no fill or border.
impl From<Shadow> for Paint {
    fn from(shadow: Shadow) -> Self {
        Self::NONE.with_shadow(shadow)
    }
}
