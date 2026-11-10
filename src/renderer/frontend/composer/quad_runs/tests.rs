use crate::primitives::geometry::rect::Rect;
use crate::renderer::frontend::composer::quad_runs::QuadRuns;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
use crate::renderer::render_buffer::quad_row::QuadKey;
use crate::renderer::render_buffer::shape_form::ShapeForm;

const SOLID: QuadKey = QuadKey::Fill(FillForm::BOX_SOLID);
const GRADIENT: QuadKey = QuadKey::Fill(FillForm::new(ShapeForm::Box, FillBrush::Gradient));
const TRIANGLE: QuadKey = QuadKey::Fill(FillForm::new(ShapeForm::Triangle, FillBrush::Solid));

/// The order `join` leaves quads of `keys`, each 10 × 10 at its `x`, a row apart from every other
/// `x`, as the x of each quad; each key moves with its quad.
fn joined(rows: &[(f32, QuadKey)]) -> Vec<f32> {
    let mut keys: Vec<QuadKey> = rows.iter().map(|&(_, key)| key).collect();
    let mut quads: Vec<Quad> = rows
        .iter()
        .map(|&(x, _)| Quad::of_rect(Rect::new(x, 0.0, 10.0, 10.0)))
        .collect();
    QuadRuns::default().join(&mut keys, &mut quads);
    let order: Vec<f32> = quads.iter().map(|q| q.header.rect.min.x).collect();
    for (x, key) in order.iter().zip(&keys) {
        let source = rows
            .iter()
            .find(|(at, _)| at == x)
            .expect("a quad of the input");
        assert_eq!(*key, source.1, "the key of the quad at {x} moved with it");
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

/// A quad looks back [`QuadRuns::LOOKBACK`] quads and no further: a solid behind 10 apart
/// gradients joins it, one behind 11 does not.
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
