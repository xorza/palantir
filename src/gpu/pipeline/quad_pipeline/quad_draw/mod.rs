//! Which quad pipeline an instance draws through, worked out once per frame.

use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::ShadowEntry;
use crate::renderer::render_buffer::fill_form::FillForm;
use crate::renderer::render_buffer::quad_draw_row::QuadKey;

/// The pipeline a quad instance draws through, and so what it binds: a fill's form, or a
/// shadow's entry, which also reads the cutout tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuadDraw {
    Fill(FillForm),
    Shadow(ShadowEntry),
}

impl QuadDraw {
    /// Every draw, in [`Self::idx`] order: the fills, then the shadows.
    pub(crate) const ALL: [Self; FillForm::ALL.len() + 3] = {
        let mut all = [Self::Shadow(ShadowEntry::General); FillForm::ALL.len() + 3];
        let mut at = 0;
        while at < FillForm::ALL.len() {
            all[at] = Self::Fill(FillForm::ALL[at]);
            at += 1;
        }
        all[at + 1] = Self::Shadow(ShadowEntry::Tables);
        all[at + 2] = Self::Shadow(ShadowEntry::TablesWide);
        all
    };

    /// This draw's place in [`Self::ALL`], its bit in a [`QuadDraws`].
    pub(crate) const fn idx(self) -> u32 {
        let fills = FillForm::ALL.len() as u32;
        match self {
            Self::Fill(form) => form.idx(),
            Self::Shadow(ShadowEntry::General) => fills,
            Self::Shadow(ShadowEntry::Tables) => fills + 1,
            Self::Shadow(ShadowEntry::TablesWide) => fills + 2,
        }
    }

    /// Whether it reads the cutout tables on the mask pages, whose open slots its pipeline is
    /// built for.
    pub(crate) const fn reads_cutouts(self) -> bool {
        matches!(
            self,
            Self::Shadow(ShadowEntry::Tables | ShadowEntry::TablesWide)
        )
    }

    const fn bit(self) -> u16 {
        1 << self.idx()
    }

    /// The timing bucket a run of this draw lands in.
    pub(crate) const fn batch_kind(self) -> BatchKind {
        match self {
            Self::Fill(_) => BatchKind::Quads,
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

/// How a box, a circle or a triangle, or a shadow, draws: as its template, the regions where a
/// fill's distance has a closed form (`vs_template`) or a shadow's rim around the hole where it
/// paints nothing (`vs_shadow`), or as one quad over its bounds that measures each pixel in full,
/// the reference the template is compared with, which only internals pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShapeCells {
    Template,
    #[cfg(feature = "internals")]
    OneQuad,
}

impl ShapeCells {
    /// The primitives a draw of these cells assembles: the template's indexed triangles, or the
    /// quad's strip of four.
    pub(crate) const fn topology(self) -> wgpu::PrimitiveTopology {
        match self {
            Self::Template => wgpu::PrimitiveTopology::TriangleList,
            #[cfg(feature = "internals")]
            Self::OneQuad => wgpu::PrimitiveTopology::TriangleStrip,
        }
    }
}

/// Write the draw of each quad of `keys` into `draws`: a shadow through its `entries` row, every
/// one through the general entry where the shadows draw [`ShapeCells::OneQuad`], a fill through
/// its form. Returns the draws taken.
pub(crate) fn fill(
    draws: &mut Vec<QuadDraw>,
    keys: &[QuadKey],
    entries: &[ShadowEntry],
    cells: ShapeCells,
) -> QuadDraws {
    draws.clear();
    draws.extend(keys.iter().enumerate().map(|(i, &key)| match key {
        QuadKey::Fill(form) => QuadDraw::Fill(form),
        QuadKey::Shadow => match cells {
            ShapeCells::Template => QuadDraw::Shadow(entries[i]),
            #[cfg(feature = "internals")]
            ShapeCells::OneQuad => QuadDraw::Shadow(ShadowEntry::General),
        },
    }));
    draws.iter().copied().collect()
}

#[cfg(test)]
mod tests;
