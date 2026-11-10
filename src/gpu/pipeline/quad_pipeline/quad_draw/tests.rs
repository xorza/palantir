use crate::common::span::Span;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::ShadowEntry;
use crate::gpu::pipeline::quad_pipeline::quad_draw::{self, QuadDraw, ShadowCells};
use crate::gpu::pipeline::runs::{Run, Runs};
use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
use crate::renderer::render_buffer::quad_row::QuadKey;
use crate::renderer::render_buffer::shape_form::ShapeForm;

/// One draw per quad: a shadow takes its entry, or the general one drawn whole, a fill its
/// form. A group's span splits into one run per draw, so two shadows of one entry share a run,
/// and two entries do not; `Solid Shadow Solid` stays three.
#[test]
fn each_quad_draws_through_its_form_or_its_shadow_entry() {
    let fill = QuadKey::Fill(FillForm::BOX_SOLID);
    let triangle_form = FillForm::new(ShapeForm::Triangle, FillBrush::Solid);
    let keys = [
        fill,
        QuadKey::Shadow,
        QuadKey::Shadow,
        QuadKey::Shadow,
        QuadKey::Fill(triangle_form),
        fill,
    ];
    let entries = [
        ShadowEntry::General,
        ShadowEntry::Tables,
        ShadowEntry::Tables,
        ShadowEntry::TablesWide,
        ShadowEntry::General,
        ShadowEntry::General,
    ];
    let solid = QuadDraw::Fill(FillForm::BOX_SOLID);
    let mut draws = Vec::new();
    let taken = quad_draw::fill(&mut draws, &keys, &entries, ShadowCells::Grid);
    assert_eq!(taken.iter().len(), 4);
    assert_eq!(
        taken.iter().collect::<Vec<_>>(),
        [
            solid,
            QuadDraw::Fill(triangle_form),
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
            QuadDraw::Fill(triangle_form),
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
            run(QuadDraw::Fill(triangle_form), 4, 1),
            run(solid, 5, 1),
        ]
    );

    let taken = quad_draw::fill(&mut draws, &keys, &entries, ShadowCells::Whole);
    let general = QuadDraw::Shadow(ShadowEntry::General);
    assert_eq!(
        taken.iter().collect::<Vec<_>>(),
        [solid, QuadDraw::Fill(triangle_form), general]
    );
    assert_eq!(
        draws[1..4],
        [general; 3],
        "one cell: every shadow the general entry"
    );
    assert_eq!(draws[0], solid, "a form does not move with the grid");
}

/// Each draw's index is its place in `ALL`: the six fill forms, then the three shadow entries, so
/// each pipeline slot and each bit of a `QuadDraws` is one draw's.
#[test]
fn each_draw_indexes_its_place() {
    for (at, draw) in QuadDraw::ALL.into_iter().enumerate() {
        assert_eq!(draw.idx() as usize, at, "{draw:?}");
    }
    assert_eq!(
        QuadDraw::ALL[6..],
        [
            QuadDraw::Shadow(ShadowEntry::General),
            QuadDraw::Shadow(ShadowEntry::Tables),
            QuadDraw::Shadow(ShadowEntry::TablesWide),
        ]
    );
}
