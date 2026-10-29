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
mod tests {
    use crate::common::span::Span;
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::ShadowEntry;
    use crate::gpu::pipeline::quad_pipeline::quad_draw::{self, QuadDraw};
    use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
    use crate::gpu::pipeline::runs::{Run, Runs};
    use crate::primitives::packed::fill_kind::FillKind;
    use crate::renderer::quad::Quad;

    /// One draw per quad: a shadow takes its entry, or the general one with the grid off, any
    /// other quad its form. A group's span splits into one run per draw, so a drop and an inset
    /// shadow of one entry share a run, and two entries do not; `Solid Shadow Solid` stays three.
    #[test]
    fn each_quad_draws_through_its_form_or_its_shadow_entry() {
        let kinds = [
            FillKind::SOLID,
            FillKind::SHADOW_DROP,
            FillKind::SHADOW_INSET,
            FillKind::SHADOW_DROP,
            FillKind::TRIANGLE,
            FillKind::SOLID,
        ];
        let quads: Vec<Quad> = kinds
            .iter()
            .map(|&fill_kind| Quad {
                fill_kind,
                ..Quad::default()
            })
            .collect();
        let entries = [
            ShadowEntry::General,
            ShadowEntry::Tables,
            ShadowEntry::Tables,
            ShadowEntry::TablesWide,
            ShadowEntry::General,
            ShadowEntry::General,
        ];
        let solid = QuadDraw::Form(QuadForm::Solid);
        let mut draws = Vec::new();
        quad_draw::fill(&mut draws, &quads, &entries, true);
        assert_eq!(
            draws,
            [
                solid,
                QuadDraw::Shadow(ShadowEntry::Tables),
                QuadDraw::Shadow(ShadowEntry::Tables),
                QuadDraw::Shadow(ShadowEntry::TablesWide),
                QuadDraw::Form(QuadForm::Triangle),
                solid,
            ]
        );
        let run = |key, start, len| Run {
            key,
            instances: Span::new(start, len),
        };
        let runs: Vec<_> = Runs::new(Span::new(0, 6), |i| draws[i as usize]).collect();
        assert_eq!(
            runs,
            [
                run(solid, 0, 1),
                run(QuadDraw::Shadow(ShadowEntry::Tables), 1, 2),
                run(QuadDraw::Shadow(ShadowEntry::TablesWide), 3, 1),
                run(QuadDraw::Form(QuadForm::Triangle), 4, 1),
                run(solid, 5, 1),
            ]
        );

        quad_draw::fill(&mut draws, &quads, &entries, false);
        let general = QuadDraw::Shadow(ShadowEntry::General);
        assert_eq!(
            draws[1..4],
            [general; 3],
            "one cell: every shadow the general entry"
        );
        assert_eq!(draws[0], solid, "a form does not move with the grid");
    }
}
