use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::brush::gradient::Spread;
use crate::renderer::frontend::composer::quad_runs::QuadRuns;
use crate::renderer::quad::Quad;

/// A 10 × 10 quad of `kind` at `x`, a row apart from every other `x`.
fn quad(x: f32, kind: FillKind) -> Quad {
    Quad {
        fill_kind: kind,
        ..Quad::of_rect(Rect::new(x, 0.0, 10.0, 10.0))
    }
}

/// The order `join` leaves, as the x of each quad.
fn joined(quads: &mut [Quad]) -> Vec<f32> {
    QuadRuns::default().join(quads);
    quads.iter().map(|q| q.rect.min.x).collect()
}

/// Apart quads alternating solid and gradient join into one run of each, the later solid moving
/// back past the gradient it does not overlap: solid 0, gradient 20, solid 40, gradient 60 become
/// solid 0, solid 40, gradient 20, gradient 60.
#[test]
fn apart_quads_join_their_runs() {
    let gradient = FillKind::linear(Spread::Pad);
    let mut quads = [
        quad(0.0, FillKind::SOLID),
        quad(20.0, gradient),
        quad(40.0, FillKind::SOLID),
        quad(60.0, gradient),
    ];
    assert_eq!(joined(&mut quads), [0.0, 40.0, 20.0, 60.0]);
}

/// A quad does not move back past one it overlaps: the solid at 25 overlaps the gradient at 20, so
/// solid 0, gradient 20, solid 25 keep their order.
#[test]
fn an_overlapping_quad_keeps_its_order() {
    let mut quads = [
        quad(0.0, FillKind::SOLID),
        quad(20.0, FillKind::linear(Spread::Pad)),
        quad(25.0, FillKind::SOLID),
    ];
    assert_eq!(joined(&mut quads), [0.0, 20.0, 25.0]);
}

/// A quad looks back [`QuadRuns::LOOKBACK`] quads and no further: a solid behind 10 apart
/// gradients joins it, one behind 11 does not.
#[test]
fn a_quad_looks_back_a_bounded_way() {
    let gradient = FillKind::linear(Spread::Pad);
    for (gradients, joins) in [(QuadRuns::LOOKBACK - 1, true), (QuadRuns::LOOKBACK, false)] {
        let mut quads = vec![quad(0.0, FillKind::SOLID)];
        quads.extend((1..=gradients).map(|i| quad(20.0 * i as f32, gradient)));
        let last = 20.0 * (gradients + 1) as f32;
        quads.push(quad(last, FillKind::SOLID));
        let order = joined(&mut quads);
        assert_eq!(order[1] == last, joins, "{gradients} gradients between");
    }
}

/// A shadow keys a run of its own, apart from every form, and a triangle another.
#[test]
fn a_shadow_and_a_triangle_key_runs_of_their_own() {
    let mut quads = [
        quad(0.0, FillKind::SHADOW_DROP),
        quad(20.0, FillKind::SOLID),
        quad(40.0, FillKind::SHADOW_INSET),
        quad(60.0, FillKind::SOLID.with_triangle()),
        quad(80.0, FillKind::SOLID),
    ];
    assert_eq!(joined(&mut quads), [0.0, 40.0, 20.0, 80.0, 60.0]);
}
