use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::renderer::frontend::composer::quad_runs::QuadRuns;
use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
use crate::renderer::render_buffer::quad_draw_row::{QuadInstance, QuadKey};
use crate::renderer::render_buffer::shape_form::ShapeForm;

const SOLID: QuadKey = QuadKey::Fill(FillForm::BOX_SOLID);
const GRADIENT: QuadKey = QuadKey::Fill(FillForm::new(ShapeForm::Box, FillBrush::Gradient));
const TRIANGLE: QuadKey = QuadKey::Fill(FillForm::new(ShapeForm::Triangle, FillBrush::Solid));

/// The x of each quad in the order `join` leaves them, each 10 × 10 at its `x` under no scissor;
/// each key moves with its quad.
fn joined(rows: &[(f32, QuadKey)]) -> Vec<f32> {
    let scissored: Vec<(f32, QuadKey, Option<URect>)> =
        rows.iter().map(|&(x, key)| (x, key, None)).collect();
    joined_under(&scissored)
}

/// [`joined`] with each quad's run scissor, which moves with its quad.
fn joined_under(rows: &[(f32, QuadKey, Option<URect>)]) -> Vec<f32> {
    let mut keys: Vec<QuadKey> = rows.iter().map(|&(_, key, _)| key).collect();
    let mut scissors: Vec<Option<URect>> = rows.iter().map(|&(_, _, scissor)| scissor).collect();
    let mut quads: Vec<QuadInstance> = rows
        .iter()
        .map(|&(x, _, _)| QuadInstance::of_rect(Rect::new(x, 0.0, 10.0, 10.0)))
        .collect();
    QuadRuns::default().join(&mut keys, &mut scissors, &mut quads);
    let order: Vec<f32> = quads.iter().map(|q| q.header.rect.min.x).collect();
    for (at, x) in order.iter().enumerate() {
        let source = rows
            .iter()
            .find(|(from, _, _)| from == x)
            .expect("a quad of the input");
        assert_eq!(
            keys[at], source.1,
            "the key of the quad at {x} moved with it"
        );
        assert_eq!(
            scissors[at], source.2,
            "the scissor of the quad at {x} moved with it"
        );
    }
    order
}

/// Apart quads alternating solid and gradient join into one run of each, the later solid moving
/// back past the gradient it does not overlap: solid 0, gradient 20, solid 40, gradient 60 become
/// solid 0, solid 40, gradient 20, gradient 60.
#[test]
fn apart_quads_join_their_runs() {
    let rows = [
        (0.0, SOLID),
        (20.0, GRADIENT),
        (40.0, SOLID),
        (60.0, GRADIENT),
    ];
    assert_eq!(joined(&rows), [0.0, 40.0, 20.0, 60.0]);
}

/// A quad does not move back past one it overlaps: the solid at 25 overlaps the gradient at 20, so
/// solid 0, gradient 20, solid 25 keep their order.
#[test]
fn an_overlapping_quad_keeps_its_order() {
    let rows = [(0.0, SOLID), (20.0, GRADIENT), (25.0, SOLID)];
    assert_eq!(joined(&rows), [0.0, 20.0, 25.0]);
}

/// A quad looks back [`QuadRuns::LOOKBACK`] quads and no further: a solid behind 9 apart
/// gradients joins it, one behind 10 does not.
#[test]
fn a_quad_looks_back_a_bounded_way() {
    for (gradients, joins) in [(QuadRuns::LOOKBACK - 1, true), (QuadRuns::LOOKBACK, false)] {
        let mut rows = vec![(0.0, SOLID)];
        rows.extend((1..=gradients).map(|i| (20.0 * i as f32, GRADIENT)));
        let last = 20.0 * (gradients + 1) as f32;
        rows.push((last, SOLID));
        let order = joined(&rows);
        assert_eq!(order[1] == last, joins, "{gradients} gradients between");
    }
}

/// A shadow keys a run of its own, apart from every form, and a triangle another.
#[test]
fn a_shadow_and_a_triangle_key_runs_of_their_own() {
    let rows = [
        (0.0, QuadKey::Shadow),
        (20.0, SOLID),
        (40.0, QuadKey::Shadow),
        (60.0, TRIANGLE),
        (80.0, SOLID),
    ];
    assert_eq!(joined(&rows), [0.0, 40.0, 20.0, 80.0, 60.0]);
}

/// A quad joins only a run of its scissor: of three apart solids, the first and last under no
/// scissor join past the middle one, which a scissor cuts, so 0, 20, 40 become 0, 40, 20.
#[test]
fn a_quad_joins_only_a_run_of_its_scissor() {
    let cut = Some(URect::new(15, 0, 10, 10));
    let rows = [(0.0, SOLID, None), (20.0, SOLID, cut), (40.0, SOLID, None)];
    assert_eq!(joined_under(&rows), [0.0, 40.0, 20.0]);
}

/// A quad its clip holds, where that clip cuts no quad of the group, joins a run under no clip:
/// the solid at 20 under (15, 0, 20 × 10), which holds it, joins the solid at 0, leaving 0, 20, 40
/// in order, all under none, whether the group is of one key, which the join passes over, or the
/// last quad is a gradient, which makes it walk the group. Where the clip also cuts the solid at
/// 30, 30..40 past its edge at 35, the solid at 20 stays under it, and the two clipped solids run
/// together.
#[test]
fn a_quad_its_clip_holds_joins_a_run_under_none_unless_the_clip_cuts_another() {
    let holding = Some(URect::new(15, 0, 20, 10));
    for last in [SOLID, GRADIENT] {
        let rows = [
            (0.0, SOLID, None),
            (20.0, SOLID, holding),
            (40.0, last, None),
        ];
        let mut keys: Vec<QuadKey> = rows.iter().map(|&(_, key, _)| key).collect();
        let mut scissors: Vec<Option<URect>> =
            rows.iter().map(|&(_, _, scissor)| scissor).collect();
        let mut quads: Vec<QuadInstance> = rows
            .iter()
            .map(|&(x, _, _)| QuadInstance::of_rect(Rect::new(x, 0.0, 10.0, 10.0)))
            .collect();
        QuadRuns::default().join(&mut keys, &mut scissors, &mut quads);
        assert_eq!(scissors, [None, None, None], "{last:?}");
        assert_eq!(keys, [SOLID, SOLID, last]);
    }

    let rows = [
        (0.0, SOLID, None),
        (20.0, SOLID, holding),
        (30.0, SOLID, holding),
    ];
    assert_eq!(joined_under(&rows), [0.0, 20.0, 30.0]);
}
