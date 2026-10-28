//! Where a text batch spanning several groups actually emits.

use crate::common::span::Span;
use crate::gpu::frame::schedule::tests::support::{
    DrawOp, buf_with_batches, collect, collect_between, group, simplify, text_batch,
};
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::text_batch::TextBatch;

#[derive(Debug)]
struct Case<'a> {
    label: &'static str,
    /// Each group's scissor and quad span.
    groups: &'a [(Option<URect>, Span)],
    /// Each batch's text span and `last_group`.
    batches: &'a [(Span, u32)],
    damage: Option<URect>,
    want: &'a [DrawOp],
}

/// A text batch emits once, right after its `last_group`'s quads, nowhere else.
///
/// - `interleaves_per_group`: group 0's text renders between group 0's and group 1's quads (per-group z-order).
/// - `quadless_group`: a group with text and no quads still emits its `Text`.
/// - `spans_two_groups`: two groups sharing a batch emit `Text` once, after the last group's quads.
/// - `trailing_quad_group`: a batch followed by a text-less group emits at its `last_group`.
/// - `anchored_in_trailing_skipped_group`: `last_group` is outside the damage but group 0 contributed text, so the batch renders from the trailing drain; the batch scissor makes late emission paint-safe.
/// - `two_batches`: each emits at its own `last_group`, never skipped or doubled.
#[test]
fn text_batches_emit_once_after_their_last_group() {
    let left = Some(URect::new(0, 0, 50, 50));
    let right = Some(URect::new(60, 0, 40, 50));
    let cases = [
        Case {
            label: "interleaves_per_group",
            groups: &[(None, Span::new(0, 2)), (None, Span::new(2, 1))],
            batches: &[(Span::new(0, 1), 0)],
            damage: None,
            want: &[DrawOp::Quads(0), DrawOp::Text(0), DrawOp::Quads(1)],
        },
        Case {
            label: "quadless_group",
            groups: &[(None, Span::new(0, 1)), (None, Span::new(1, 0))],
            batches: &[(Span::new(0, 2), 1)],
            damage: None,
            want: &[DrawOp::Quads(0), DrawOp::Text(0)],
        },
        Case {
            label: "spans_two_groups",
            groups: &[(None, Span::new(0, 1)), (None, Span::new(1, 1))],
            batches: &[(Span::new(0, 2), 1)],
            damage: None,
            want: &[DrawOp::Quads(0), DrawOp::Quads(1), DrawOp::Text(0)],
        },
        Case {
            label: "trailing_quad_group",
            groups: &[(None, Span::new(0, 1)), (None, Span::new(1, 1))],
            batches: &[(Span::new(0, 1), 0)],
            damage: None,
            want: &[DrawOp::Quads(0), DrawOp::Text(0), DrawOp::Quads(1)],
        },
        Case {
            label: "anchored_in_trailing_skipped_group",
            groups: &[(left, Span::new(0, 1)), (right, Span::new(1, 1))],
            batches: &[(Span::new(0, 2), 1)],
            damage: left,
            want: &[DrawOp::PreClear, DrawOp::Quads(0), DrawOp::Text(0)],
        },
        Case {
            label: "two_batches",
            groups: &[(None, Span::new(0, 1)), (None, Span::new(1, 1))],
            batches: &[(Span::new(0, 1), 0), (Span::new(1, 1), 1)],
            damage: None,
            want: &[
                DrawOp::Quads(0),
                DrawOp::Text(0),
                DrawOp::Quads(1),
                DrawOp::Text(1),
            ],
        },
    ];
    for case in cases {
        let groups = case
            .groups
            .iter()
            .map(|&(scissor, quads)| DrawGroup {
                scissor,
                ..group(quads)
            })
            .collect();
        let batches: Vec<TextBatch> = case
            .batches
            .iter()
            .map(|&(texts, last)| text_batch(texts, last))
            .collect();
        let buf = buf_with_batches(groups, batches);
        let steps = collect(&buf, case.damage, None);
        assert_eq!(simplify(&buf, &steps), case.want, "{}", case.label);
    }
}

/// A main pass broken at a pane's barrier walks the groups before it and their text batches,
/// which the composer closed there, then resumes past them: the two walks draw what the whole
/// walk draws, in its order, and only a walk from the first group clears. A barrier at 0 leaves the
/// first walk only the clear, and the second, from the first group too, is the whole walk; one
/// past the last group leaves the second nothing.
#[test]
fn a_walk_broken_at_a_barrier_draws_the_whole_walk_once() {
    let groups = vec![
        group(Span::new(0, 1)),
        group(Span::new(1, 1)),
        group(Span::new(2, 2)),
    ];
    let batches = vec![
        text_batch(Span::new(0, 1), 0),
        text_batch(Span::new(1, 1), 2),
    ];
    let buf = buf_with_batches(groups, batches);
    let damage = Some(URect::new(0, 0, 100, 100));
    let whole = [
        DrawOp::PreClear,
        DrawOp::Quads(0),
        DrawOp::Text(0),
        DrawOp::Quads(1),
        DrawOp::Quads(2),
        DrawOp::Text(1),
    ];
    assert_eq!(simplify(&buf, &collect(&buf, damage, None)), whole);
    // The ops before each barrier: the clear, then group 0 with its text, group 1, group 2.
    for (barrier, split) in [(0, 1), (1, 3), (2, 4), (3, 6)] {
        let before = collect_between(&buf, damage, None, (0, Some(barrier)));
        let after = collect_between(&buf, damage, None, (barrier, None));
        assert_eq!(simplify(&buf, &before), whole[..split], "before {barrier}");
        let resumed = if barrier == 0 { 0 } else { split };
        assert_eq!(simplify(&buf, &after), whole[resumed..], "after {barrier}");
    }
}
