//! How a group's quads reach the backend: one step, shadows among them.

use crate::common::span::Span;
use crate::gpu::frame::schedule::RenderStep;
use crate::gpu::frame::schedule::tests::support::{
    DrawOp, buf_with, buf_with_batches, group, plain_steps, simplify, text_batch,
};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
use crate::renderer::render_buffer::quad_draw_row::{QuadDrawRow, QuadKey};
use crate::renderer::render_buffer::shape_form::ShapeForm;

const SOLID: QuadKey = QuadKey::Fill(FillForm::BOX_SOLID);
const TRIANGLE: QuadKey = QuadKey::Fill(FillForm::new(ShapeForm::Triangle, FillBrush::Solid));
const SHADOW: QuadKey = QuadKey::Shadow;

fn with_kinds(mut buffer: RenderBuffer, keys: &[QuadKey]) -> RenderBuffer {
    let instance = buffer.quads.instance()[0];
    buffer.quads.clear();
    for &key in keys {
        buffer.quads.push(QuadDrawRow { key, instance });
    }
    buffer
}

/// Each group's quads go out as one step whatever pipelines they mix, as the backend splits a
/// range where its pipeline changes (`quad_draw`); a step never crosses a group, and an empty
/// group sends none.
#[test]
fn each_group_draws_its_quads_in_one_step() {
    let buffer = with_kinds(
        buf_with(vec![
            group(Span::new(0, 3)),
            group(Span::new(3, 0)),
            group(Span::new(3, 2)),
        ]),
        &[SOLID, SHADOW, SHADOW, SHADOW, TRIANGLE],
    );
    let quads: Vec<RenderStep> = plain_steps(&buffer)
        .into_iter()
        .filter(|s| matches!(s, RenderStep::Quads { .. }))
        .collect();
    assert_eq!(
        quads,
        [
            RenderStep::Quads {
                range: Span::new(0, 3)
            },
            RenderStep::Quads {
                range: Span::new(3, 2)
            },
        ]
    );
}

/// A group's text follows its quads, its shadows among them, so a label stays above its
/// chrome's shadow.
#[test]
fn text_follows_its_groups_quads() {
    let buffer = with_kinds(
        buf_with_batches(
            vec![group(Span::new(0, 3)), group(Span::new(3, 1))],
            vec![text_batch(Span::new(0, 1), 0)],
        ),
        &[SHADOW, SOLID, SHADOW, SOLID],
    );
    assert_eq!(
        simplify(&buffer, &plain_steps(&buffer)),
        [DrawOp::Quads(0), DrawOp::Text(0), DrawOp::Quads(1)],
    );
}
