//! Which quad pipeline an instance draws through, worked out once per frame.

use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::ShadowEntry;
use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
use crate::renderer::quad::Quad;

/// The pipeline a quad instance draws through, and so what it binds: a form of `fs`, or a
/// shadow's entry, which also reads the cutout tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuadDraw {
    Form(QuadForm),
    Shadow(ShadowEntry),
}

impl QuadDraw {
    /// The timing bucket a run of this draw lands in.
    pub(crate) const fn batch_kind(self) -> BatchKind {
        match self {
            Self::Form(_) => BatchKind::Quads,
            Self::Shadow(_) => BatchKind::Shadows,
        }
    }
}

/// Write each of `quads`' draws into `draws`: a shadow through its `entries` row, every one
/// through the general entry where the shadows draw as one cell (`grid` off), any other quad
/// through its form.
pub(crate) fn fill(draws: &mut Vec<QuadDraw>, quads: &[Quad], entries: &[ShadowEntry], grid: bool) {
    draws.clear();
    draws.extend(quads.iter().enumerate().map(|(i, quad)| {
        if !quad.fill_kind.is_shadow() {
            QuadDraw::Form(QuadForm::of(quad.fill_kind))
        } else if grid {
            QuadDraw::Shadow(entries[i])
        } else {
            QuadDraw::Shadow(ShadowEntry::General)
        }
    }));
}

#[cfg(test)]
mod tests;
