//! [`MaterialPaint`]: a box's paint over the material it wears.

use crate::primitives::paint::material::Material;
use crate::primitives::paint::paint::Paint;

/// What covers a box that wears a material: its [`Paint`] over a [`Material`], as CSS pairs
/// `backdrop-filter` with `background-color`, `border` and `box-shadow`. A rect, a circle and a
/// triangle take it ([`Ui::add_shape`](crate::Ui::add_shape)); [`Paint::with_material`] makes
/// one, and a [`Material`] alone paints as one with [`Paint::NONE`].
///
/// Bottom to top: a drop shadow; the material over the whole box, the border band included, as
/// CSS's backdrop filter covers the border box; the fill over the whole box too, as CSS's
/// background covers the border box, which the material shows through by the fill's alpha; an
/// inset shadow inside the border; the border inside the edge, over the fill.
///
/// A node wears a material as a shape of its own: a box that pads or clips draws it in a node
/// around the one that does, as a node's shapes paint inside its padding's clip.
///
/// Plain data, checked where it enters a shape: the paint as [`Paint`] lists, the material as
/// [`Material`] does.
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct MaterialPaint {
    /// The fill, border and shadow, the fill over the material.
    pub paint: Paint,
    /// The backdrop effect under the fill.
    pub material: Material,
}

impl MaterialPaint {
    /// Panics unless the paint and the material each hold their kinds: one branch-free test, the
    /// checks that name the broken rule only on the cold path, as [`Paint`]'s.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        if !(self.paint.is_valid() & self.material.is_valid()) {
            self.reject();
        }
    }

    /// [`Self::validate`]'s cold half: panics with the rule a part breaks.
    #[cold]
    #[inline(never)]
    #[track_caller]
    const fn reject(&self) -> ! {
        self.paint.validate();
        self.material.validate();
        panic!("`MaterialPaint::validate` refused a paint whose parts each pass their check");
    }
}

/// A box of `material` alone: no fill, border or shadow.
impl From<Material> for MaterialPaint {
    fn from(material: Material) -> Self {
        Paint::NONE.with_material(material)
    }
}
