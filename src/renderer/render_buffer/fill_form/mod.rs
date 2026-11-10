//! [`FillForm`]: the pipeline key of a fill, its shape's form and its brush's.

use crate::primitives::packed::fill_kind::FillKind;
use crate::renderer::render_buffer::shape_form::ShapeForm;

/// The brush half of a fill's pipeline key, pinned as `FILL_BRUSH`, so a solid fill's pipeline
/// skips a gradient's register cost (VideoCore: 54 temporaries and two threads for every brush,
/// 22 and four for solid).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FillBrush {
    Solid = 0,
    Gradient = 1,
}

impl FillBrush {
    /// The brush a fill of `kind` paints with.
    ///
    /// # Panics
    ///
    /// Panics on the curves' ramp, which no fill paints.
    pub(crate) const fn of(kind: FillKind) -> Self {
        match kind.tag() {
            FillKind::TAG_SOLID => Self::Solid,
            FillKind::TAG_LINEAR | FillKind::TAG_RADIAL | FillKind::TAG_CONIC => Self::Gradient,
            _ => panic!("a fill paints the tag"),
        }
    }
}

/// The pipeline a fill draws through: the template form of its shape, and its brush. Built only
/// by [`Self::new`], so no fill names the rect form, which only an image draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FillForm {
    shape: ShapeForm,
    brush: FillBrush,
}

impl FillForm {
    /// Every fill form, in [`Self::idx`] order.
    pub(crate) const ALL: [Self; 6] = [
        Self::new(ShapeForm::Box, FillBrush::Solid),
        Self::new(ShapeForm::Box, FillBrush::Gradient),
        Self::new(ShapeForm::BoxOutside, FillBrush::Solid),
        Self::new(ShapeForm::BoxOutside, FillBrush::Gradient),
        Self::new(ShapeForm::Triangle, FillBrush::Solid),
        Self::new(ShapeForm::Triangle, FillBrush::Gradient),
    ];

    /// What nearly every fill draws through: a box's template under a solid brush.
    pub(crate) const BOX_SOLID: Self = Self::new(ShapeForm::Box, FillBrush::Solid);

    /// The fill of a `shape` under `brush`.
    ///
    /// # Panics
    ///
    /// Panics on [`ShapeForm::Rect`]: a fill draws its template.
    pub(crate) const fn new(shape: ShapeForm, brush: FillBrush) -> Self {
        assert!(
            !matches!(shape, ShapeForm::Rect),
            "a fill draws its shape's template"
        );
        Self { shape, brush }
    }

    pub(crate) const fn shape(self) -> ShapeForm {
        self.shape
    }

    pub(crate) const fn brush(self) -> FillBrush {
        self.brush
    }

    /// This form's place in [`Self::ALL`]: two brushes to each template form.
    pub(crate) const fn idx(self) -> u32 {
        (self.shape as u32 - ShapeForm::Box as u32) * 2 + self.brush as u32
    }
}

#[cfg(test)]
mod tests;
