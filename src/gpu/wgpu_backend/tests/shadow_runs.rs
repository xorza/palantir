//! How a group's quads split between the quad and the shadow pipelines.

use crate::common::span::Span;
use crate::gpu::frame::schedule::RenderStep;
use crate::gpu::wgpu_backend::tests::support::{
    DrawOp, buf_with, buf_with_batches, group, plain_steps, simplify, text_batch,
};
use crate::primitives::packed::fill_kind::FillKind;
use crate::renderer::render_buffer::RenderBuffer;

/// `kinds` for the buffer's quads, in order.
fn with_kinds(mut buffer: RenderBuffer, kinds: &[FillKind]) -> RenderBuffer {
    buffer.quads.resize(kinds.len(), buffer.quads[0]);
    for (quad, &kind) in buffer.quads.iter_mut().zip(kinds) {
        quad.fill_kind = kind;
    }
    buffer
}

/// The draws of `buffer`, without the scissor and stencil bookkeeping.
fn draws(buffer: &RenderBuffer) -> Vec<RenderStep> {
    plain_steps(buffer)
        .into_iter()
        .filter(|s| matches!(s, RenderStep::Quads { .. } | RenderStep::Shadows { .. }))
        .collect()
}

#[derive(Debug)]
struct Case<'a> {
    label: &'static str,
    /// The buffer's quads, in order.
    kinds: &'a [FillKind],
    /// Each group's quad span.
    groups: &'a [Span],
    want: &'a [RenderStep],
}

/// A group's range splits into one run per pipeline, in paint order,
/// wherever the shadow kind starts or stops, and both shadow kinds share
/// a run. A range of one kind stays one draw, and runs never cross a
/// group.
#[test]
fn quad_ranges_split_where_the_shadow_kind_changes() {
    const SOLID: FillKind = FillKind::SOLID;
    const DROP: FillKind = FillKind::SHADOW_DROP;
    const INSET: FillKind = FillKind::SHADOW_INSET;
    let quads = |start, len| RenderStep::Quads {
        range: Span::new(start, len),
    };
    let shadows = |start, len| RenderStep::Shadows {
        range: Span::new(start, len),
    };
    let cases = [
        Case {
            label: "no_shadow",
            kinds: &[SOLID, FillKind::TRIANGLE, SOLID],
            groups: &[Span::new(0, 3)],
            want: &[quads(0, 3)],
        },
        Case {
            label: "only_shadows",
            kinds: &[DROP, INSET],
            groups: &[Span::new(0, 2)],
            want: &[shadows(0, 2)],
        },
        Case {
            label: "interleaved",
            kinds: &[SOLID, DROP, INSET, SOLID, DROP],
            groups: &[Span::new(0, 5)],
            want: &[quads(0, 1), shadows(1, 2), quads(3, 1), shadows(4, 1)],
        },
        Case {
            label: "shadow_first",
            kinds: &[DROP, SOLID, SOLID],
            groups: &[Span::new(0, 3)],
            want: &[shadows(0, 1), quads(1, 2)],
        },
        Case {
            label: "runs_stop_at_groups",
            kinds: &[DROP, DROP, SOLID, SOLID],
            groups: &[Span::new(0, 1), Span::new(1, 2), Span::new(3, 1)],
            want: &[shadows(0, 1), shadows(1, 1), quads(2, 1), quads(3, 1)],
        },
    ];
    for case in cases {
        let buffer = with_kinds(
            buf_with(case.groups.iter().copied().map(group).collect()),
            case.kinds,
        );
        assert_eq!(draws(&buffer), case.want, "{}", case.label);
    }
}

/// A group's text still follows all of its quad runs, shadows included,
/// so a label stays above its own chrome's shadow.
#[test]
fn text_follows_every_run_of_its_group() {
    let buffer = with_kinds(
        buf_with_batches(
            vec![group(Span::new(0, 3)), group(Span::new(3, 1))],
            vec![text_batch(Span::new(0, 1), 0)],
        ),
        &[
            FillKind::SHADOW_DROP,
            FillKind::SOLID,
            FillKind::SHADOW_DROP,
            FillKind::SOLID,
        ],
    );
    assert_eq!(
        simplify(&buffer, &plain_steps(&buffer)),
        [
            DrawOp::Shadows(0),
            DrawOp::Quads(0),
            DrawOp::Shadows(0),
            DrawOp::Text(0),
            DrawOp::Quads(1),
        ],
    );
}
