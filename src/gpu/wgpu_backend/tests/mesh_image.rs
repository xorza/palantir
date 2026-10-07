//! Mesh and image batches: ordering within a group, and dropping with it.

use crate::common::span::Span;
use crate::gpu::wgpu_backend::tests::support::{
    DrawOp, buf_with, buf_with_tier_anchors, collect, group, plain_steps, simplify,
};
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::group_batch::GroupBatch;
use crate::renderer::render_buffer::paint_tier::PaintTier;

/// Each mesh-emitting group contributes its own mesh-tier batch, drained at its group; two adjacent groups give two ordered emit steps.
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

/// A mesh batch in a damage-skipped group is dropped (the stale-cursor advance skips it); the visible group still drains its own.
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
        simplify(&buf, &collect(&buf, damage, None)),
        vec![DrawOp::PreClear, DrawOp::Meshes(1)],
    );
}

/// An image batch at group `j` replays after that group's quads and meshes, through both stencil and non-stencil paths.
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

/// An image batch in a damage-skipped group is dropped.
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
        simplify(&buf, &collect(&buf, damage, None)),
        vec![DrawOp::PreClear, DrawOp::Images(1)],
    );
}

/// The backend replays higher-kind batches in `PaintTier` order, which the composer's flush arbitration (`HigherKindRects::conflicts`) assumes. Asserted as "emitted order equals tiers sorted by `Ord`" so it follows the enum.
#[test]
fn higher_kind_replay_follows_paint_tier_order() {
    let mut buf = buf_with(vec![DrawGroup {
        scissor: None,
        ..group(Span::default())
    }]);
    // One batch of every tier in a single group, so emit order is the drain order.
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
