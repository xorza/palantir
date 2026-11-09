use crate::common::span::Span;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::ShadowEntry;
use crate::gpu::pipeline::quad_pipeline::quad_draw::{self, QuadDraw, ShadowCells};
use crate::gpu::pipeline::runs::{Run, Runs};
use crate::primitives::packed::fill_kind::FillKind;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::quad_form::QuadForm;

/// One draw per quad: a shadow takes its entry, or the general one drawn whole, any
/// other quad its form. A group's span splits into one run per draw, so a drop and an inset
/// shadow of one entry share a run, and two entries do not; `Solid Shadow Solid` stays three.
#[test]
fn each_quad_draws_through_its_form_or_its_shadow_entry() {
    let kinds = [
        FillKind::SOLID,
        FillKind::SHADOW_DROP,
        FillKind::SHADOW_INSET,
        FillKind::SHADOW_DROP,
        FillKind::SOLID.with_triangle(),
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
    let solid = QuadDraw::Form(QuadForm::BoxSolid);
    let mut draws = Vec::new();
    let taken = quad_draw::fill(&mut draws, &quads, &entries, ShadowCells::Grid);
    assert_eq!(taken.iter().len(), 4);
    assert_eq!(
        taken.iter().collect::<Vec<_>>(),
        [
            solid,
            QuadDraw::Form(QuadForm::TriangleSolid),
            QuadDraw::Shadow(ShadowEntry::Tables),
            QuadDraw::Shadow(ShadowEntry::TablesWide),
        ],
        "each draw taken once, in `QuadDraw::ALL` order"
    );
    assert_eq!(
        draws,
        [
            solid,
            QuadDraw::Shadow(ShadowEntry::Tables),
            QuadDraw::Shadow(ShadowEntry::Tables),
            QuadDraw::Shadow(ShadowEntry::TablesWide),
            QuadDraw::Form(QuadForm::TriangleSolid),
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
            run(QuadDraw::Form(QuadForm::TriangleSolid), 4, 1),
            run(solid, 5, 1),
        ]
    );

    let taken = quad_draw::fill(&mut draws, &quads, &entries, ShadowCells::Whole);
    let general = QuadDraw::Shadow(ShadowEntry::General);
    assert_eq!(
        taken.iter().collect::<Vec<_>>(),
        [solid, QuadDraw::Form(QuadForm::TriangleSolid), general]
    );
    assert_eq!(
        draws[1..4],
        [general; 3],
        "one cell: every shadow the general entry"
    );
    assert_eq!(draws[0], solid, "a form does not move with the grid");
}
