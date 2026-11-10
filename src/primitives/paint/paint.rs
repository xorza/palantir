//! [`Paint`]: what covers a box, a rect shape's, a circle's, a triangle's or a node's.

use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::{Gradient, GradientGeometry};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::shadow::Shadow;
use palantir_derive::{Animatable, Lerp};

/// What covers a box: its fill, its border, its shadow, and the material under its fill, as CSS
/// pairs `background-color` with `border`, `box-shadow` and `backdrop-filter`. A rect, a circle
/// and a triangle take it ([`Ui::add_shape`](crate::Ui::add_shape)), and a node's
/// [`Chrome`](crate::Chrome) holds one beside its corners. [`Self::NONE`] paints nothing.
///
/// Bottom to top: a drop shadow; the material over the whole box, the border band included, as
/// CSS's backdrop filter covers the border box; the fill over the whole box too, as CSS's
/// background covers the border box, which a material shows through by the fill's alpha; an
/// inset shadow inside the border; the border inside the edge, over the fill.
///
/// `Animatable` is derived: the fill, the border and the shadow interpolate componentwise, the
/// material as [`Material`] blends. "No border" is `Border::NONE`, not an `Option`.
///
/// Plain data, so arithmetic may pass through values no widget takes. A widget's `chrome` and a
/// shape check it where it enters and panic unless every color is a
/// [color](crate::widget::domain::color), gradient angles are
/// [angles](crate::widget::domain::angle), centers and the shadow's offset and spread
/// [offsets](crate::widget::domain::offset), the border width, radial radii and the shadow's blur
/// [lengths](crate::widget::domain::length), and the material as [`Material`] lists. Every value
/// but the border width packs into an f16 lane, so it is a [lane](crate::widget::domain::lane) too.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Lerp, Animatable)]
#[must_use]
pub struct Paint {
    /// The interior's brush, over the material where there is one. [`Brush::TRANSPARENT`] fills
    /// nothing.
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
    /// The backdrop effect the box wears under its fill; `None` (the `Default`) wears none and is
    /// omitted from serialized output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<Material>,
}

impl Paint {
    /// Paint that covers nothing.
    pub const NONE: Self = Self {
        fill: Brush::TRANSPARENT,
        border: Border::NONE,
        shadow: Shadow::NONE,
        material: None,
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

    /// This paint over `material`, which its fill covers.
    pub const fn with_material(mut self, material: Material) -> Self {
        self.material = Some(material);
        self
    }

    /// True when this paints nothing visible.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        self.fill.is_noop()
            && self.border.is_noop()
            && self.shadow.is_noop()
            && self.material.is_none()
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
        self.fill.is_valid()
            & self.border.is_valid()
            & self.shadow.is_valid()
            & match &self.material {
                Some(material) => material.is_valid(),
                None => true,
            }
    }

    /// Panics unless its fill, border, shadow and material each pass their own check: one
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
        if let Some(material) = &self.material {
            material.validate();
        }
        panic!("`Paint::is_valid` refused a paint whose parts each pass their check");
    }
}

/// Every field can carry a NaN, the fill through a gradient's geometry, the rest through scalars.
impl NanCheck for Paint {
    fn has_nan(&self) -> bool {
        self.fill.has_nan()
            || self.border.has_nan()
            || self.shadow.has_nan()
            || self.material.as_ref().is_some_and(Material::has_nan)
    }
}

impl Default for Paint {
    fn default() -> Self {
        Self::NONE
    }
}

/// A plain fill of `brush`: no border, shadow or material.
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

/// A plain fill of `gradient`.
impl<G: GradientGeometry> From<Gradient<G>> for Paint
where
    Brush: From<Gradient<G>>,
{
    fn from(gradient: Gradient<G>) -> Self {
        Self::from(Brush::from(gradient))
    }
}
