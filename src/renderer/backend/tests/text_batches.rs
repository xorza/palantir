//! Where a text batch spanning several groups actually emits.

use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::renderer::backend::schedule::MaskPlan;
use crate::renderer::backend::tests::support::{
    DrawOp, buf_with_batches, collect, simplify, text_batch,
};
use crate::renderer::render_buffer::draw_group::DrawGroup;

/// Pin: text in group 0 renders *between* group 0's quads and group 1's
/// quads, so a child quad declared after a label can occlude it. The
/// per-group z-order contract — the showcase tab `text z-order`
/// demonstrates the visual outcome.
#[test]
fn text_interleaves_per_group() {
    let buf = buf_with_batches(
        vec![
            // Group 0: 2 quads + 1 text (via the batch below)
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::new(0, 2),
            },
            // Group 1: 1 quad, no text
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::new(2, 1),
            },
        ],
        vec![text_batch(Span::new(0, 1), 0)],
    );
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &MaskPlan::default(), false)),
        vec![DrawOp::Quads(0), DrawOp::Text(0), DrawOp::Quads(1)],
    );
}

/// Edge case: a group with text but no quads (e.g. a Hug parent whose
/// only paint is its label). Schedule must still emit `Text(i)`.
#[test]
fn text_emits_for_quadless_group() {
    let buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::new(0, 1),
            },
            // Group 1: text-only (quad span empty).
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::new(1, 0),
            },
        ],
        vec![text_batch(Span::new(0, 2), 1)],
    );
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &MaskPlan::default(), false)),
        // Group 0 has no text → not part of a batch. Group 1's text
        // is the only batch (idx 0), emitted after group 1's quads
        // (it has none) → immediately.
        vec![DrawOp::Quads(0), DrawOp::Text(0)],
    );
}

/// Pin: two groups sharing one text batch emit `Text` ONCE, after the
/// last group's quads. Without coalescing the schedule would emit two
/// text steps, and the backend two raster passes.
#[test]
fn text_batch_spanning_two_groups_emits_once_at_last_group() {
    let buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::new(0, 1),
            },
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::new(1, 1),
            },
        ],
        vec![text_batch(Span::new(0, 2), 1)],
    );
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &MaskPlan::default(), false)),
        vec![DrawOp::Quads(0), DrawOp::Quads(1), DrawOp::Text(0)],
    );
}

/// Pin: a batch whose `last_group` is followed by a text-less group
/// still emits Text at `last_group`, not pushed forward. Counter-pin
/// against an off-by-one in the cursor advance.
#[test]
fn text_batch_emits_at_last_group_even_with_trailing_quad_group() {
    let buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::new(0, 1),
            },
            // Group 1: trailing quad-only group (different batch state).
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::new(1, 1),
            },
        ],
        vec![text_batch(Span::new(0, 1), 0)],
    );
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &MaskPlan::default(), false)),
        vec![DrawOp::Quads(0), DrawOp::Text(0), DrawOp::Quads(1)],
    );
}

/// Pin: a batch whose `last_group` falls in a damage-skipped group
/// must still render — earlier groups in the same batch may sit
/// inside the damage rect, and dropping the whole batch silently
/// removes their text. The batch scissor (`TextBatch::scissor`,
/// set before the Text step) clips the merged text, so emitting
/// late is paint-safe.
#[test]
fn text_batch_anchored_in_damage_skipped_group_still_emits() {
    // Two groups in distinct scissors. Both contribute text to one
    // batch (last_group = 1). Damage rect covers group 0's scissor
    // only, so group 1 is filtered out by the damage intersect.
    let buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: Some(URect::new(0, 0, 50, 50)),
                rounded_clips: Span::default(),
                quads: Span::new(0, 1),
            },
            DrawGroup {
                scissor: Some(URect::new(60, 0, 40, 50)),
                rounded_clips: Span::default(),
                quads: Span::new(1, 1),
            },
        ],
        vec![text_batch(Span::new(0, 2), 1)],
    );
    // Damage rect: covers only group 0.
    let damage = URect::new(0, 0, 50, 50);
    let steps = simplify(
        &buf,
        &collect(&buf, Some(damage), &MaskPlan::default(), false),
    );
    // Must include Text(0) — group 0's text lives in batch 0, and
    // batch 0 anchored at the skipped group 1 must still emit.
    assert!(
        steps.contains(&DrawOp::Text(0)),
        "batch anchored at damage-skipped group must still render; got {steps:?}",
    );
}

/// Pin: when the batch's `last_group` is the **final** group AND that
/// group is damage-skipped, the trailing drain after the per-group
/// loop must still emit the batch. Without it the in-group drain
/// (which only triggers when reaching a later non-skipped group)
/// never fires, and the text vanishes.
#[test]
fn text_batch_anchored_in_trailing_skipped_group_drains_after_loop() {
    let buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: Some(URect::new(0, 0, 50, 50)),
                rounded_clips: Span::default(),
                quads: Span::new(0, 1),
            },
            DrawGroup {
                // Final group, outside damage.
                scissor: Some(URect::new(60, 0, 40, 50)),
                rounded_clips: Span::default(),
                quads: Span::new(1, 1),
            },
        ],
        vec![text_batch(Span::new(0, 2), 1)],
    );
    let damage = URect::new(0, 0, 50, 50);
    let steps = simplify(
        &buf,
        &collect(&buf, Some(damage), &MaskPlan::default(), false),
    );
    assert!(
        steps.contains(&DrawOp::Text(0)),
        "trailing drain must emit batch when last_group is tail-skipped; got {steps:?}",
    );
}

/// Pin: two distinct batches → two `Text` steps, each at its own
/// `last_group`. The schedule cursor advances correctly through the
/// batch list without skipping or doubling up.
#[test]
fn two_text_batches_emit_at_their_own_last_groups() {
    let buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::new(0, 1),
            },
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::new(1, 1),
            },
        ],
        vec![
            text_batch(Span::new(0, 1), 0),
            text_batch(Span::new(1, 1), 1),
        ],
    );
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &MaskPlan::default(), false)),
        vec![
            DrawOp::Quads(0),
            DrawOp::Text(0),
            DrawOp::Quads(1),
            DrawOp::Text(1),
        ],
    );
}
