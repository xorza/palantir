//! Which quad pipeline an instance draws through, worked out once per frame.

use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::ShadowEntry;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::quad_form::QuadForm;

/// The pipeline a quad instance draws through, and so what it binds: a form of `fs`, or a
/// shadow's entry, which also reads the cutout tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuadDraw {
    Form(QuadForm),
    Shadow(ShadowEntry),
}

impl QuadDraw {
    /// Every draw, in [`Self::idx`] order.
    pub(crate) const ALL: [Self; 8] = [
        Self::Form(QuadForm::BoxSolid),
        Self::Form(QuadForm::BoxGradient),
        Self::Form(QuadForm::BoxOutside),
        Self::Form(QuadForm::TriangleSolid),
        Self::Form(QuadForm::TriangleGradient),
        Self::Shadow(ShadowEntry::General),
        Self::Shadow(ShadowEntry::Tables),
        Self::Shadow(ShadowEntry::TablesWide),
    ];

    /// This draw's place in [`Self::ALL`], its bit in a [`QuadDraws`].
    pub(crate) const fn idx(self) -> u32 {
        match self {
            Self::Form(form) => form as u32,
            Self::Shadow(ShadowEntry::General) => 5,
            Self::Shadow(ShadowEntry::Tables) => 6,
            Self::Shadow(ShadowEntry::TablesWide) => 7,
        }
    }

    const fn bit(self) -> u16 {
        1 << self.idx()
    }

    /// The timing bucket a run of this draw lands in.
    pub(crate) const fn batch_kind(self) -> BatchKind {
        match self {
            Self::Form(_) => BatchKind::Quads,
            Self::Shadow(_) => BatchKind::Shadows,
        }
    }
}

/// A set of [`QuadDraw`]s: the ones a frame's quads take, which its pipelines are built for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct QuadDraws(u16);

impl QuadDraws {
    /// This set and `draw`.
    #[must_use]
    pub(crate) const fn with(self, draw: QuadDraw) -> Self {
        Self(self.0 | draw.bit())
    }

    /// The draws in the set, in [`QuadDraw::ALL`] order.
    pub(crate) const fn iter(self) -> QuadDrawsIter {
        QuadDrawsIter { left: self.0 }
    }
}

impl FromIterator<QuadDraw> for QuadDraws {
    fn from_iter<T: IntoIterator<Item = QuadDraw>>(draws: T) -> Self {
        draws.into_iter().fold(Self::default(), Self::with)
    }
}

impl<const N: usize> From<[QuadDraw; N]> for QuadDraws {
    fn from(draws: [QuadDraw; N]) -> Self {
        draws.into_iter().collect()
    }
}

impl IntoIterator for &QuadDraws {
    type Item = QuadDraw;
    type IntoIter = QuadDrawsIter;

    fn into_iter(self) -> QuadDrawsIter {
        self.iter()
    }
}

/// The draws of a [`QuadDraws`], lowest bit first.
#[derive(Clone, Debug)]
pub(crate) struct QuadDrawsIter {
    left: u16,
}

impl Iterator for QuadDrawsIter {
    type Item = QuadDraw;

    fn next(&mut self) -> Option<QuadDraw> {
        if self.left == 0 {
            return None;
        }
        let at = self.left.trailing_zeros();
        self.left &= self.left - 1;
        Some(QuadDraw::ALL[at as usize])
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.left.count_ones() as usize;
        (left, Some(left))
    }
}

impl ExactSizeIterator for QuadDrawsIter {}

/// How a shadow draws: the grid of cells around its hole, or one cell of the full form, the
/// reference the grid is compared with, which only internals pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShadowCells {
    Grid,
    #[cfg(feature = "internals")]
    Whole,
}

/// How a box, a circle or a triangle draws: as its template, the regions where its distance has
/// a closed form (`vs_template`), or as one quad over its bounds that measures the distance at
/// each pixel, the reference the template is compared with, which only internals pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShapeCells {
    Template,
    #[cfg(feature = "internals")]
    OneQuad,
}

/// Write each of `quads`' draws into `draws`: a shadow through its `entries` row, every one
/// through the general entry where the shadows draw [`ShadowCells::Whole`], any other quad
/// through its form. Returns the draws taken.
pub(crate) fn fill(
    draws: &mut Vec<QuadDraw>,
    quads: &[Quad],
    entries: &[ShadowEntry],
    cells: ShadowCells,
) -> QuadDraws {
    draws.clear();
    draws.extend(quads.iter().enumerate().map(|(i, quad)| {
        if quad.fill_kind.is_shadow() {
            match cells {
                ShadowCells::Grid => QuadDraw::Shadow(entries[i]),
                #[cfg(feature = "internals")]
                ShadowCells::Whole => QuadDraw::Shadow(ShadowEntry::General),
            }
        } else {
            QuadDraw::Form(QuadForm::of(quad.fill_kind))
        }
    }));
    draws.iter().copied().collect()
}

#[cfg(test)]
mod tests;
