//! Mesh and image batches: ordering within a group, and dropping with it.

use crate::common::span::Span;
use crate::gpu::frame::schedule::MaskPlan;
use crate::gpu::wgpu_backend::tests::support::{
    DrawOp, buf_with, buf_with_tier_anchors, collect, group, plain_steps, simplify,
};
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::group_batch::GroupBatch;
use crate::renderer::render_buffer::paint_tier::PaintTier;

/// Pin: each mesh-emitting group contributes its own mesh-tier batch,
/// drained at the group iteration anchored by `last_group`. Two
/// adjacent mesh groups → two emit steps, in order.
#[test]
fn mesh_batches_emit_per_group_in_order() {
    let buf = buf_with_tier_anchors(
        vec![
            DrawGroup {
                scissor: None,
                ..group(Span::default())
            },
            DrawGroup {
                scissor: None,
                ..group(Span::default())
            },
        ],
        PaintTier::Mesh,
        &[0, 1],
    );
    assert_eq!(
        simplify(&buf, &plain_steps(&buf)),
        vec![DrawOp::Meshes(0), DrawOp::Meshes(1)],
    );
}

/// Pin: a mesh batch anchored in a damage-skipped group is silently
/// dropped — the stale-cursor advance at the top of each schedule
/// iteration moves past it, so no mesh-tier step is emitted for
/// invisible meshes. Counter-pin: the visible group still drains
/// its own batch.
#[test]
fn mesh_batch_in_damage_skipped_group_drops_silently() {
    let buf = buf_with_tier_anchors(
        vec![
            DrawGroup {
                scissor: Some(URect::new(0, 0, 50, 100)),
                ..group(Span::default())
            },
            DrawGroup {
                scissor: Some(URect::new(50, 0, 50, 100)),
                ..group(Span::default())
            },
        ],
        PaintTier::Mesh,
        &[0, 1],
    );
    let damage = Some(URect::new(50, 0, 50, 100));
    assert_eq!(
        simplify(&buf, &collect(&buf, damage, &MaskPlan::default(), false)),
        vec![DrawOp::PreClear, DrawOp::Meshes(1)],
    );
}

/// Pin: an image batch anchored at group `j` replays after
/// that group's quads and meshes (image sits at mesh tier in the
/// kind order). Counter-pin to ensure the new `next_image_batch`
/// cursor wires through both stencil and non-stencil paths.
#[test]
fn image_batch_emits_after_group_quads_in_non_stencil_path() {
    let buf = buf_with_tier_anchors(
        vec![
            DrawGroup {
                scissor: None,
                ..group(Span::default())
            },
            DrawGroup {
                scissor: None,
                ..group(Span::default())
            },
        ],
        PaintTier::Image,
        &[0, 1],
    );
    assert_eq!(
        simplify(&buf, &plain_steps(&buf)),
        vec![DrawOp::Images(0), DrawOp::Images(1)],
    );
}

/// Pin: image batch in a damage-skipped group is silently dropped.
#[test]
fn image_batch_in_damage_skipped_group_drops_silently() {
    let buf = buf_with_tier_anchors(
        vec![
            DrawGroup {
                scissor: Some(URect::new(0, 0, 50, 100)),
                ..group(Span::default())
            },
            DrawGroup {
                scissor: Some(URect::new(50, 0, 50, 100)),
                ..group(Span::default())
            },
        ],
        PaintTier::Image,
        &[0, 1],
    );
    let damage = Some(URect::new(50, 0, 50, 100));
    assert_eq!(
        simplify(&buf, &collect(&buf, damage, &MaskPlan::default(), false)),
        vec![DrawOp::PreClear, DrawOp::Images(1)],
    );
}

/// Pin: the backend replays higher-kind batches in `PaintTier` order.
///
/// The composer's flush arbitration reads that ordering directly —
/// `HigherKindRects::conflicts` flushes only when the incoming tier
/// sorts *below* one already recorded, which is correct exactly when the
/// backend paints them in the same sequence. Until this test the only
/// check was `higher_kind`'s own, comparing `conflicts` against
/// `PaintTier`'s derived `Ord` — both sides of one file, so reordering
/// the enum kept it green while silently changing which tier paints on
/// top.
///
/// Written as "the emitted order equals the tiers sorted by `Ord`" so
/// the assertion follows the enum rather than restating a fourth
/// hand-written copy of Mesh → Image → Icon → Curve.
#[test]
fn higher_kind_replay_follows_paint_tier_order() {
    let mut buf = buf_with(vec![DrawGroup {
        scissor: None,
        ..group(Span::default())
    }]);
    // One batch of every tier anchored in the single group, so the emit
    // sequence is entirely the drain order.
    let anchored = GroupBatch {
        items: Span::new(0, 1),
        last_group: 0,
    };
    for tier in PaintTier::ALL {
        buf.batches_mut(tier).push(anchored);
    }

    let emitted: Vec<PaintTier> = simplify(&buf, &plain_steps(&buf))
        .into_iter()
        .filter_map(|op| match op {
            DrawOp::Meshes(_) => Some(PaintTier::Mesh),
            DrawOp::Images(_) => Some(PaintTier::Image),
            DrawOp::Icons(_) => Some(PaintTier::Icon),
            DrawOp::Curves(_) => Some(PaintTier::Curve),
            _ => None,
        })
        .collect();

    let mut expected = PaintTier::ALL.to_vec();
    expected.sort();
    assert_eq!(
        emitted.len(),
        expected.len(),
        "every tier must contribute exactly one step",
    );
    assert_eq!(
        emitted, expected,
        "backend replay order must match PaintTier's Ord — the composer's \
         flush arbitration is only sound while the two agree",
    );
}
