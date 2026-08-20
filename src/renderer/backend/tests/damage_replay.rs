//! Scissor transitions and the per-damage-rect replay a partial frame
//! drives.

use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::renderer::backend::schedule::{MaskPlan, RenderStep};
use crate::renderer::backend::tests::support::{
    DrawOp, buf_with, buf_with_batches, buf_with_image_anchors, collect, scissor_count, simplify,
    text_batch,
};
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::paint_tier::PaintTier;

/// Pin: under partial damage, a `PreClear` step runs *before* any
/// group draws. Without it, `LoadOp::Load` leaves last frame's pixels
/// in place; new draws with AA fringe alpha < 1 blend over them and
/// drift across frames (manifests as "stays hovered after I move
/// away"). Counter-pin: `None` damage skips `PreClear` entirely.
#[test]
fn preclear_emits_under_partial_damage() {
    let buf = buf_with_batches(
        vec![DrawGroup {
            scissor: None,
            rounded_clips: Span::default(),
            quads: Span::new(0, 1),
        }],
        vec![text_batch(Span::new(0, 1), 0)],
    );
    let damage = Some(URect::new(0, 0, 50, 50));
    assert_eq!(
        simplify(&buf, &collect(&buf, damage, &MaskPlan::default(), false)),
        vec![DrawOp::PreClear, DrawOp::Quads(0), DrawOp::Text(0),],
    );
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &MaskPlan::default(), false)),
        vec![DrawOp::Quads(0), DrawOp::Text(0)],
    );
}

/// Pin the multi-pass invariant `WgpuBackend::submit` relies on: with
/// two damage rects, the schedule is replayed once per rect and each
/// replay emits its own `PreClear` followed by group draws scissored
/// to that rect. Two corner rects + two groups (one inside each rect)
/// → pass A only emits group 0, pass B only emits group 1.
#[test]
fn schedule_replays_per_damage_rect() {
    // Two groups whose own scissors carve the surface into two halves.
    let buf = buf_with(vec![
        DrawGroup {
            scissor: Some(URect::new(0, 0, 50, 100)),
            rounded_clips: Span::default(),
            quads: Span::new(0, 1),
        },
        DrawGroup {
            scissor: Some(URect::new(50, 0, 50, 100)),
            rounded_clips: Span::default(),
            quads: Span::new(1, 1),
        },
    ]);
    // DamageEngine rect A covers only group 0; rect B covers only group 1.
    let pass_a = collect(
        &buf,
        Some(URect::new(0, 0, 50, 100)),
        &MaskPlan::default(),
        false,
    );
    let pass_b = collect(
        &buf,
        Some(URect::new(50, 0, 50, 100)),
        &MaskPlan::default(),
        false,
    );
    let mut combined = pass_a;
    combined.extend(pass_b);
    assert_eq!(
        simplify(&buf, &combined),
        vec![
            // Pass A: PreClear inside rect A, then group 0.
            DrawOp::PreClear,
            DrawOp::Quads(0),
            // Pass B: PreClear inside rect B, then group 1.
            DrawOp::PreClear,
            DrawOp::Quads(1),
        ],
    );
}

/// Pin the scissor-transition contract on the non-stencil path: a walk
/// opens with a mandatory `SetScissor`, and after that one appears only
/// where the requested rect actually changes. Axes:
///
/// - Partial damage + a narrower group → the damage rect, then the
///   group's own.
/// - A group whose effective rect *is* the damage rect → its narrow
///   collapses into the mandatory opener.
/// - Quads then an image batch in one group with no text → the restore
///   before the higher-kind draws costs nothing. This is the case the
///   renderer review flagged: it used to emit `SetScissor(same)`.
/// - Counter-pin for that: give the group a text batch whose scissor
///   differs, and the restore before the image batch MUST emit —
///   dedup may not swallow a transition the drain really made.
/// - Adjacent groups: equal scissors collapse to one, unequal don't.
#[test]
fn scissor_steps_emit_once_per_transition() {
    let narrow = URect::new(10, 10, 50, 50);
    let group = |scissor, q| DrawGroup {
        scissor: Some(scissor),
        rounded_clips: Span::default(),
        quads: Span::new(q, 1),
    };
    let buf = buf_with(vec![group(narrow, 0)]);
    let damage = URect::new(0, 0, 80, 80);
    assert_eq!(
        collect(&buf, Some(damage), &MaskPlan::default(), false),
        vec![
            RenderStep::SetScissor(damage),
            RenderStep::PreClear,
            // (10,10,50,50) ∩ damage = (10,10,50,50).
            RenderStep::SetScissor(narrow),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
        ],
    );
    // Same buffer, damage equal to the group's rect: nothing to narrow.
    assert_eq!(
        collect(&buf, Some(narrow), &MaskPlan::default(), false),
        vec![
            RenderStep::SetScissor(narrow),
            RenderStep::PreClear,
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
        ],
    );

    let buf = buf_with_image_anchors(vec![group(narrow, 0)], &[0]);
    assert_eq!(
        collect(&buf, None, &MaskPlan::default(), false),
        vec![
            RenderStep::SetScissor(narrow),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
            RenderStep::TierBatch {
                tier: PaintTier::Image,
                batch: 0,
            },
        ],
    );

    let mut buf = buf_with_image_anchors(vec![group(narrow, 0)], &[0]);
    buf.text_batches.push(text_batch(Span::new(0, 1), 0));
    assert_eq!(
        collect(&buf, None, &MaskPlan::default(), false),
        vec![
            RenderStep::SetScissor(narrow),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
            // The batch's sentinel scissor is wider than the group's.
            RenderStep::SetScissor(URect::new(0, 0, u32::MAX, u32::MAX)),
            RenderStep::Text { batch: 0 },
            // So the group's restore is a real transition, not a repeat.
            RenderStep::SetScissor(narrow),
            RenderStep::TierBatch {
                tier: PaintTier::Image,
                batch: 0,
            },
        ],
    );

    for (second, expected) in [(narrow, 1), (URect::new(60, 10, 20, 20), 2)] {
        let buf = buf_with(vec![group(narrow, 0), group(second, 1)]);
        let steps = collect(&buf, None, &MaskPlan::default(), false);
        assert_eq!(
            scissor_count(&steps),
            expected,
            "adjacent groups scissored {narrow:?} then {second:?}",
        );
    }
}

/// Pin: a group whose scissor is disjoint from the damage rect emits
/// no steps (no scissor set, no draws). The damage filter is applied
/// at schedule time, not delegated to the GPU scissor.
#[test]
fn group_outside_damage_emits_no_steps() {
    let buf = buf_with(vec![
        // Group 0: in damage
        DrawGroup {
            scissor: Some(URect::new(0, 0, 30, 30)),
            rounded_clips: Span::default(),
            quads: Span::new(0, 1),
        },
        // Group 1: outside damage
        DrawGroup {
            scissor: Some(URect::new(60, 60, 30, 30)),
            rounded_clips: Span::default(),
            quads: Span::new(1, 1),
        },
    ]);
    let damage = URect::new(0, 0, 40, 40);
    assert_eq!(
        simplify(
            &buf,
            &collect(&buf, Some(damage), &MaskPlan::default(), false)
        ),
        vec![DrawOp::PreClear, DrawOp::Quads(0)],
    );
}
