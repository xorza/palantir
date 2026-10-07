//! Scissor transitions and the per-damage-rect replay of a partial frame.

use crate::common::span::Span;
use crate::gpu::frame::schedule::RenderStep;
use crate::gpu::wgpu_backend::tests::support::{
    DrawOp, buf_with, buf_with_batches, buf_with_tier_anchors, collect, group, plain_steps,
    scissor_count, simplify, text_batch,
};
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::paint_tier::PaintTier;

/// Under partial damage `PreClear` runs before any group draws, else `LoadOp::Load` keeps last frame's
/// pixels and AA fringes drift. `None` damage skips it.
#[test]
fn preclear_emits_under_partial_damage() {
    let buf = buf_with_batches(
        vec![DrawGroup {
            scissor: None,
            ..group(Span::new(0, 1))
        }],
        vec![text_batch(Span::new(0, 1), 0)],
    );
    let damage = Some(URect::new(0, 0, 50, 50));
    assert_eq!(
        simplify(&buf, &collect(&buf, damage, None)),
        vec![DrawOp::PreClear, DrawOp::Quads(0), DrawOp::Text(0),],
    );
    assert_eq!(
        simplify(&buf, &plain_steps(&buf)),
        vec![DrawOp::Quads(0), DrawOp::Text(0)],
    );
}

/// With two damage rects the schedule replays once per rect (`WgpuBackend::submit` relies on it): pass A
/// emits group 0, pass B group 1, each after its own `PreClear`.
#[test]
fn schedule_replays_per_damage_rect() {
    let buf = buf_with(vec![
        DrawGroup {
            scissor: Some(URect::new(0, 0, 50, 100)),
            ..group(Span::new(0, 1))
        },
        DrawGroup {
            scissor: Some(URect::new(50, 0, 50, 100)),
            ..group(Span::new(1, 1))
        },
    ]);
    let pass_a = collect(&buf, Some(URect::new(0, 0, 50, 100)), None);
    let pass_b = collect(&buf, Some(URect::new(50, 0, 50, 100)), None);
    let mut combined = pass_a;
    combined.extend(pass_b);
    assert_eq!(
        simplify(&buf, &combined),
        vec![
            DrawOp::PreClear,
            DrawOp::Quads(0),
            DrawOp::PreClear,
            DrawOp::Quads(1),
        ],
    );
}

/// A non-stencil walk opens with a mandatory `SetScissor`, then emits one only on a real change:
/// a narrower group after damage, a restore only where a text batch's wider scissor intervened,
/// and adjacent equal scissors collapse.
#[test]
fn scissor_steps_emit_once_per_transition() {
    let narrow = URect::new(10, 10, 50, 50);
    let scissored = |scissor, q| DrawGroup {
        scissor: Some(scissor),
        ..group(Span::new(q, 1))
    };
    let buf = buf_with(vec![scissored(narrow, 0)]);
    let damage = URect::new(0, 0, 80, 80);
    assert_eq!(
        collect(&buf, Some(damage), None),
        vec![
            RenderStep::SetScissor(damage),
            RenderStep::PreClear,
            RenderStep::SetScissor(narrow),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
        ],
    );
    assert_eq!(
        collect(&buf, Some(narrow), None),
        vec![
            RenderStep::SetScissor(narrow),
            RenderStep::PreClear,
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
        ],
    );

    let buf = buf_with_tier_anchors(vec![scissored(narrow, 0)], PaintTier::Image, &[0]);
    assert_eq!(
        plain_steps(&buf),
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

    let mut buf = buf_with_tier_anchors(vec![scissored(narrow, 0)], PaintTier::Image, &[0]);
    buf.text_batches.push(text_batch(Span::new(0, 1), 0));
    assert_eq!(
        plain_steps(&buf),
        vec![
            RenderStep::SetScissor(narrow),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
            RenderStep::SetScissor(URect::new(0, 0, u32::MAX, u32::MAX)),
            RenderStep::Text { batch: 0 },
            RenderStep::SetScissor(narrow),
            RenderStep::TierBatch {
                tier: PaintTier::Image,
                batch: 0,
            },
        ],
    );

    for (second, expected) in [(narrow, 1), (URect::new(60, 10, 20, 20), 2)] {
        let buf = buf_with(vec![scissored(narrow, 0), scissored(second, 1)]);
        let steps = plain_steps(&buf);
        assert_eq!(
            scissor_count(&steps),
            expected,
            "adjacent groups scissored {narrow:?} then {second:?}",
        );
    }
}

/// A group disjoint from the damage rect emits no steps: filtered at schedule time, not by the GPU scissor.
#[test]
fn group_outside_damage_emits_no_steps() {
    let buf = buf_with(vec![
        DrawGroup {
            scissor: Some(URect::new(0, 0, 30, 30)),
            ..group(Span::new(0, 1))
        },
        DrawGroup {
            scissor: Some(URect::new(60, 60, 30, 30)),
            ..group(Span::new(1, 1))
        },
    ]);
    let damage = URect::new(0, 0, 40, 40);
    assert_eq!(
        simplify(&buf, &collect(&buf, Some(damage), None)),
        vec![DrawOp::PreClear, DrawOp::Quads(0)],
    );
}
