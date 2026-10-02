//! Where a text batch spanning several groups actually emits.

use crate::gpu::schedule::MaskPlan;
use crate::gpu::tests::support::{DrawOp, buf_with_batches, collect, group, simplify, text_batch};
use crate::primitives::span::Span;
use crate::primitives::urect::URect;
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

/// A text batch emits once, right after its `last_group`'s quads, and
/// nowhere else.
///
/// - `interleaves_per_group`: text in group 0 renders *between* group 0's
///   quads and group 1's, so a child quad declared after a label can
///   occlude it — the per-group z-order the showcase's `text z-order` tab
///   shows.
/// - `quadless_group`: a group with text and no quads (a Hug parent whose
///   only paint is its label) still emits its `Text`.
/// - `spans_two_groups`: two groups sharing one batch emit `Text` once,
///   after the last group's quads — not one raster pass each.
/// - `trailing_quad_group`: a batch followed by a text-less group still
///   emits at its `last_group`, not pushed forward.
/// - `anchored_in_trailing_skipped_group`: the batch's `last_group` falls
///   outside the damage, but group 0 inside it contributed text, so the
///   batch still renders — out of the trailing drain, since no later
///   unskipped group exists to drain it. The batch scissor clips the
///   merged text, so emitting late is paint-safe.
/// - `two_batches`: each batch emits at its own `last_group`, the cursor
///   neither skipping nor doubling one.
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
        let steps = collect(&buf, case.damage, &MaskPlan::default(), false);
        assert_eq!(simplify(&buf, &steps), case.want, "{}", case.label);
    }
}
