//! Rounded-clip mask stamping: when a chain writes, dedups, restamps, or clears.

use crate::common::span::Span;
use crate::gpu::frame::schedule::MaskPlan;
use crate::gpu::frame::schedule::RenderStep;
use crate::gpu::wgpu_backend::tests::support::{
    DrawOp, buf_with, buf_with_batches, collect, group, scissor_count, simplify, text_batch,
};
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::text_batch::TextBatch;
use crate::renderer::render_buffer::{RenderBuffer, RoundedClip};
use glam::Vec2;

/// A stencil-clipped group stamps its mask before its draws, and the walk ends with a tail `MaskClear` (the pass clears stencil once and padded damage scissors can overlap). Raw steps pin the depth-1 grammar: stamp at ref 0, content at ref 1, one shared `SetScissor`.
#[test]
fn stencil_group_brackets_draws_with_mask_write() {
    let mut buf = buf_with_batches(
        vec![DrawGroup {
            rounded_clips: Span::new(0, 1),
            ..group(Span::new(0, 2))
        }],
        vec![TextBatch {
            texts: Span::new(0, 1),
            last_group: 0,
            scissor: URect::new(0, 0, 100, 100),
            rounded_clips: Span::new(0, 1),
        }],
    );
    buf.rounded_clips = vec![rounded(100.0, 100.0, 8.0)];
    let mi = mask_ix(&buf);
    assert_eq!(mi.groups, vec![Span::new(0, 1)]);
    assert_eq!(mi.batches, vec![Span::new(0, 1)]);
    assert_eq!(mi.quads().len(), 1);
    let steps = collect(&buf, None, Some(&mi));
    assert_eq!(
        simplify(&buf, &steps),
        vec![
            DrawOp::MaskWrite(0),
            DrawOp::Quads(0),
            DrawOp::Text(0),
            DrawOp::MaskClear(0),
        ],
    );
    let s = URect::new(0, 0, 100, 100);
    assert_eq!(
        steps,
        vec![
            RenderStep::SetScissor(s),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::Quads {
                range: Span::new(0, 2),
            },
            // Batch drain: same chain and scissor, so no transition; text at ref 1 under the stamp.
            RenderStep::Text { batch: 0 },
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
        ],
    );
    assert_eq!(scissor_count(&steps), 1);
    assert_eq!(
        mask_scissors(&steps),
        vec![
            MaskUnderScissor {
                step: RenderStep::MaskStamp(0),
                scissor: s,
            },
            MaskUnderScissor {
                step: RenderStep::MaskClear(0),
                scissor: s,
            },
        ],
    );
}

/// A non-rounded group in a stencil pass runs at `stencil_ref = 0` with no mask quads; beside a rounded sibling, each keeps its own bracket.
#[test]
fn stencil_mixed_rounded_and_plain_groups_keep_brackets_local() {
    let mut buf = buf_with_batches(
        vec![
            DrawGroup {
                rounded_clips: Span::new(0, 1),
                ..group(Span::new(0, 1))
            },
            group(Span::new(1, 1)),
        ],
        vec![text_batch(Span::new(0, 1), 1)],
    );
    buf.rounded_clips = vec![rounded(100.0, 100.0, 8.0)];
    let mi = mask_ix(&buf);
    assert_eq!(mi.groups, vec![Span::new(0, 1), Span::default()]);
    assert_eq!(
        simplify(&buf, &collect(&buf, None, Some(&mi))),
        vec![
            DrawOp::MaskWrite(0),
            DrawOp::Quads(0),
            DrawOp::MaskClear(0),
            DrawOp::Quads(1),
            DrawOp::Text(0),
        ],
    );
}

/// `MaskPlan::build` dedups value-equal chains onto one mask-quad run and the schedule elides the clear and re-stamp between sharing groups; a differing clip does the full transition, and the walk tail-clears the last mask.
#[test]
fn stencil_consecutive_same_mask_groups_dedup_writes() {
    let mut buf = buf_with(vec![
        DrawGroup {
            rounded_clips: Span::new(0, 1),
            ..group(Span::new(0, 1))
        },
        DrawGroup {
            rounded_clips: Span::new(0, 1),
            ..group(Span::new(1, 1))
        },
        DrawGroup {
            rounded_clips: Span::new(1, 1),
            ..group(Span::new(2, 1))
        },
    ]);
    buf.rounded_clips = vec![rounded(100.0, 100.0, 8.0), rounded(50.0, 50.0, 4.0)];
    let mi = mask_ix(&buf);
    assert_eq!(
        mi.groups,
        vec![Span::new(0, 1), Span::new(0, 1), Span::new(1, 1)]
    );
    assert_eq!(mi.quads().len(), 2);

    let steps = collect(&buf, None, Some(&mi));
    assert_eq!(
        simplify(&buf, &steps),
        vec![
            DrawOp::MaskWrite(0),
            DrawOp::Quads(0),
            DrawOp::Quads(1),
            DrawOp::MaskClear(0),
            DrawOp::MaskWrite(1),
            DrawOp::Quads(2),
            DrawOp::MaskClear(1),
        ],
    );
    // The sharing groups share a scissor, so nothing separates their quads.
    let q0 = steps
        .iter()
        .position(|s| matches!(s, RenderStep::Quads { range } if *range == Span::new(0, 1)))
        .unwrap();
    let q1 = steps
        .iter()
        .position(|s| matches!(s, RenderStep::Quads { range } if *range == Span::new(1, 1)))
        .unwrap();
    assert!(
        steps[q0 + 1..q1].is_empty(),
        "same-mask groups sharing a scissor need no steps between their quads; got {:?}",
        &steps[q0 + 1..q1],
    );
    assert_eq!(scissor_count(&steps), 1);
}

/// Sharing a mask index is only safe while each group's scissor stays inside the stamp's: a wider scissor exposes stencil 0 that would fail `Equal(1)`, so the schedule clears and re-stamps.
#[test]
fn stencil_same_mask_wider_scissor_restamps() {
    let mut buf = buf_with(vec![
        DrawGroup {
            scissor: Some(URect::new(0, 0, 50, 100)),
            rounded_clips: Span::new(0, 1),
            ..group(Span::new(0, 1))
        },
        DrawGroup {
            rounded_clips: Span::new(0, 1),
            ..group(Span::new(1, 1))
        },
    ]);
    buf.rounded_clips = vec![rounded(100.0, 100.0, 8.0)];
    let mi = mask_ix(&buf);
    assert_eq!(mi.groups, vec![Span::new(0, 1), Span::new(0, 1)]);
    assert_eq!(mi.quads().len(), 1);
    // but the schedule re-brackets: clear under (0,0,50,100), re-stamp under (0,0,100,100), tail clear.
    assert_eq!(
        simplify(&buf, &collect(&buf, None, Some(&mi))),
        vec![
            DrawOp::MaskWrite(0),
            DrawOp::Quads(0),
            DrawOp::MaskClear(0),
            DrawOp::MaskWrite(0),
            DrawOp::Quads(1),
            DrawOp::MaskClear(0),
        ],
    );
}

/// A stencil-pass group with text but no quads still emits the mask write, else the text leaks past the clip.
#[test]
fn stencil_text_only_group_still_writes_mask() {
    let mut buf = buf_with_batches(
        vec![DrawGroup {
            rounded_clips: Span::new(0, 1),
            ..group(Span::new(0, 0))
        }],
        vec![TextBatch {
            texts: Span::new(0, 1),
            last_group: 0,
            scissor: URect::new(0, 0, 100, 100),
            rounded_clips: Span::new(0, 1),
        }],
    );
    buf.rounded_clips = vec![rounded(100.0, 100.0, 8.0)];
    let mi = mask_ix(&buf);
    assert_eq!(
        simplify(&buf, &collect(&buf, None, Some(&mi))),
        vec![DrawOp::MaskWrite(0), DrawOp::Text(0), DrawOp::MaskClear(0)],
    );
}

/// Group A stamps inside SA, then B has a disjoint SB. The clear must replay under SA before B's `SetScissor`, else it runs where the stamp never wrote and leaves residue; a masked last group tail-clears.
#[test]
fn stencil_stale_mask_clears_under_stamp_scissor_then_tail_clears() {
    let sa = URect::new(0, 0, 40, 40);
    let sb = URect::new(50, 0, 40, 40);
    let sc = URect::new(0, 50, 100, 50);
    let clipped = |scissor, chain, q| DrawGroup {
        scissor: Some(scissor),
        rounded_clips: chain,
        ..group(Span::new(q, 1))
    };
    let clips = vec![rounded(40.0, 40.0, 8.0), rounded(40.0, 40.0, 4.0)];
    let mut buf = buf_with(vec![
        clipped(sa, Span::new(0, 1), 0),
        clipped(sb, Span::new(1, 1), 1),
        clipped(sc, Span::default(), 2),
    ]);
    buf.rounded_clips = clips.clone();
    let mi = mask_ix(&buf);
    assert_eq!(
        mi.groups,
        vec![Span::new(0, 1), Span::new(1, 1), Span::default()]
    );
    let steps = collect(&buf, None, Some(&mi));
    assert_eq!(
        steps,
        vec![
            RenderStep::SetScissor(sa),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
            // A to B: clear under SA (still held) before SetScissor(SB); SA and SB are disjoint.
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            RenderStep::SetScissor(sb),
            RenderStep::MaskStamp(1),
            RenderStep::SetStencilRef(1),
            RenderStep::Quads {
                range: Span::new(1, 1),
            },
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(1),
            RenderStep::SetScissor(sc),
            RenderStep::Quads {
                range: Span::new(2, 1),
            },
        ],
    );
    assert_eq!(
        mask_scissors(&steps),
        vec![
            MaskUnderScissor {
                step: RenderStep::MaskStamp(0),
                scissor: sa,
            },
            MaskUnderScissor {
                step: RenderStep::MaskClear(0),
                scissor: sa,
            },
            MaskUnderScissor {
                step: RenderStep::MaskStamp(1),
                scissor: sb,
            },
            MaskUnderScissor {
                step: RenderStep::MaskClear(1),
                scissor: sb,
            },
        ],
    );
    assert_eq!(scissor_count(&steps), 3);

    // Without C the walk ends with mask 1 stamped, so a tail clear under SB closes it.
    let mut buf = buf_with(vec![
        clipped(sa, Span::new(0, 1), 0),
        clipped(sb, Span::new(1, 1), 1),
    ]);
    buf.rounded_clips = clips;
    let mi = mask_ix(&buf);
    let steps = collect(&buf, None, Some(&mi));
    assert_eq!(
        &steps[steps.len() - 2..],
        &[RenderStep::SetStencilRef(0), RenderStep::MaskClear(1)],
    );
    assert_eq!(
        mask_scissors(&steps).last(),
        Some(&MaskUnderScissor {
            step: RenderStep::MaskClear(1),
            scissor: sb,
        }),
    );
}

/// Depth-2 chain grammar. Group 0 nests two rounded clips: outer at ref 0 gives stencil 1, inner at ref 1 gives stencil 2, content at ref 2. Group 1 has a value-equal chain in another span and elides. Group 2 is unmasked: one clear of the outermost mask resets the chain. A second walk (groups 0+1) pins the tail clear.
#[test]
fn stencil_nested_chain_stamps_ladder_elides_and_single_clears() {
    let e = URect::new(0, 0, 100, 100);
    let outer = rounded(100.0, 100.0, 8.0);
    let inner = rounded(80.0, 80.0, 4.0);
    let clipped = |chain, q| DrawGroup {
        scissor: Some(e),
        rounded_clips: chain,
        ..group(Span::new(q, 1))
    };
    let mut buf = buf_with(vec![
        clipped(Span::new(0, 2), 0),
        clipped(Span::new(2, 2), 1),
        clipped(Span::default(), 2),
    ]);
    buf.rounded_clips = vec![outer, inner, outer, inner];
    let mi = mask_ix(&buf);
    assert_eq!(
        mi.groups,
        vec![Span::new(0, 2), Span::new(0, 2), Span::default()]
    );
    assert_eq!(mi.quads().len(), 2);
    let steps = collect(&buf, None, Some(&mi));
    assert_eq!(
        steps,
        vec![
            RenderStep::SetScissor(e),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::MaskStamp(1),
            RenderStep::SetStencilRef(2),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
            RenderStep::Quads {
                range: Span::new(1, 1),
            },
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            RenderStep::Quads {
                range: Span::new(2, 1),
            },
        ],
    );
    assert_eq!(scissor_count(&steps), 1);

    let mut buf = buf_with(vec![
        clipped(Span::new(0, 2), 0),
        clipped(Span::new(2, 2), 1),
    ]);
    buf.rounded_clips = vec![outer, inner, outer, inner];
    let mi = mask_ix(&buf);
    let steps = collect(&buf, None, Some(&mi));
    assert_eq!(
        &steps[steps.len() - 2..],
        &[RenderStep::SetStencilRef(0), RenderStep::MaskClear(0)],
    );
    assert_eq!(
        mask_scissors(&steps).last(),
        Some(&MaskUnderScissor {
            step: RenderStep::MaskClear(0),
            scissor: e,
        }),
    );
}

/// A rounded batch drained while no group painted (its groups sit outside the damage but the batch's bounds union pokes in) must stamp its own mask before its `Text` step, then tail-clear it.
///
/// Groups at (0,0,40,40) and (50,50,40,40) share a chain; the batch scissor is (0,0,90,90); damage (60,0,30,40) misses both groups but hits the union.
#[test]
fn stencil_drained_batch_stamps_own_mask_before_text() {
    let chain = Span::new(0, 1);
    let mut buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: Some(URect::new(0, 0, 40, 40)),
                rounded_clips: chain,
                ..group(Span::new(0, 1))
            },
            DrawGroup {
                scissor: Some(URect::new(50, 50, 40, 40)),
                rounded_clips: chain,
                ..group(Span::new(1, 1))
            },
        ],
        vec![TextBatch {
            texts: Span::new(0, 2),
            last_group: 1,
            scissor: URect::new(0, 0, 90, 90),
            rounded_clips: chain,
        }],
    );
    buf.rounded_clips = vec![rounded(40.0, 40.0, 8.0)];
    let mi = mask_ix(&buf);
    assert_eq!(mi.batches, vec![Span::new(0, 1)]);
    let damage = URect::new(60, 0, 30, 40);
    // Batch scissor and damage intersect at the damage rect.
    let s = URect::new(60, 0, 30, 40);
    let steps = collect(&buf, Some(damage), Some(&mi));
    assert_eq!(
        steps,
        vec![
            RenderStep::SetScissor(damage),
            RenderStep::PreClear,
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::Text { batch: 0 },
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
        ],
    );
    assert_eq!(
        mask_scissors(&steps),
        vec![
            MaskUnderScissor {
                step: RenderStep::MaskStamp(0),
                scissor: s,
            },
            MaskUnderScissor {
                step: RenderStep::MaskClear(0),
                scissor: s,
            },
        ],
    );
}

/// A batch anchored in a damage-skipped group whose chain is still stamped elides: text at ref 1 under the live mask, and the unmasked group after it restores with the usual clear.
#[test]
fn stencil_drained_batch_elides_when_own_chain_still_stamped() {
    let chain = Span::new(0, 1);
    let sa = URect::new(0, 0, 40, 40);
    let mut buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: Some(sa),
                rounded_clips: chain,
                ..group(Span::new(0, 1))
            },
            DrawGroup {
                scissor: Some(URect::new(0, 50, 40, 40)),
                rounded_clips: chain,
                ..group(Span::new(1, 1))
            },
            DrawGroup {
                scissor: Some(URect::new(45, 0, 50, 40)),
                ..group(Span::new(2, 1))
            },
        ],
        vec![TextBatch {
            texts: Span::new(0, 2),
            last_group: 1,
            scissor: URect::new(0, 0, 40, 90),
            rounded_clips: chain,
        }],
    );
    buf.rounded_clips = vec![rounded(40.0, 40.0, 8.0)];
    let mi = mask_ix(&buf);
    let damage = URect::new(0, 0, 100, 40);
    // Batch scissor and damage intersect at (0,0,40,40), group 0's stamp scissor.
    assert_eq!(
        collect(&buf, Some(damage), Some(&mi)),
        vec![
            RenderStep::SetScissor(damage),
            RenderStep::PreClear,
            RenderStep::SetScissor(sa),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
            RenderStep::Text { batch: 0 },
            // Group 2 (unmasked): clear under the stamp-time scissor, then its own scissor and quads at ref 0.
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            RenderStep::SetScissor(URect::new(45, 0, 50, 40)),
            RenderStep::Quads {
                range: Span::new(2, 1),
            },
        ],
    );
}

/// An unmasked batch drained while a mask is active must clear it before its `Text` step, else glyphs fail `Equal(ref)` against the foreign stamp.
#[test]
fn stencil_unmasked_batch_drained_under_active_mask_clears_first() {
    let sa = URect::new(0, 0, 40, 40);
    let mut buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: Some(sa),
                rounded_clips: Span::new(0, 1),
                ..group(Span::new(0, 1))
            },
            DrawGroup {
                scissor: Some(URect::new(50, 0, 40, 40)),
                ..group(Span::new(1, 1))
            },
        ],
        vec![TextBatch {
            texts: Span::new(0, 1),
            last_group: 1,
            scissor: URect::new(0, 0, 90, 40),
            rounded_clips: Span::default(),
        }],
    );
    buf.rounded_clips = vec![rounded(40.0, 40.0, 8.0)];
    let mi = mask_ix(&buf);
    let damage = URect::new(0, 0, 45, 45);
    assert_eq!(
        collect(&buf, Some(damage), Some(&mi)),
        vec![
            RenderStep::SetScissor(damage),
            RenderStep::PreClear,
            RenderStep::SetScissor(sa),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
            // Trailing drain: the unmasked batch clears group 0's stamp under the stamp-time scissor, then draws at ref 0.
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            RenderStep::SetScissor(URect::new(0, 0, 45, 40)),
            RenderStep::Text { batch: 0 },
        ],
    );
}

/// Staging dedups against every chain seen this frame, not just the previous group: groups 0 and 2 share a run across a foreign chain, so three groups stage two mask quads.
#[test]
fn stencil_dedups_a_chain_seen_before_the_previous_group() {
    let e = URect::new(0, 0, 100, 100);
    let outer = rounded(100.0, 100.0, 8.0);
    let inner = rounded(50.0, 50.0, 4.0);
    let clipped = |chain, q| DrawGroup {
        scissor: Some(e),
        rounded_clips: chain,
        ..group(Span::new(q, 1))
    };
    let mut buf = buf_with(vec![
        clipped(Span::new(0, 1), 0),
        clipped(Span::new(1, 1), 1),
        clipped(Span::new(2, 1), 2),
    ]);
    buf.rounded_clips = vec![outer, inner, outer];
    let mi = mask_ix(&buf);
    assert_eq!(
        mi.groups,
        vec![Span::new(0, 1), Span::new(1, 1), Span::new(0, 1)]
    );
    assert_eq!(
        mi.quads().len(),
        2,
        "the repeated chain staged a second copy"
    );

    let steps = collect(&buf, None, Some(&mi));
    assert_eq!(
        simplify(&buf, &steps),
        vec![
            DrawOp::MaskWrite(0),
            DrawOp::Quads(0),
            DrawOp::MaskClear(0),
            DrawOp::MaskWrite(1),
            DrawOp::Quads(1),
            DrawOp::MaskClear(1),
            DrawOp::MaskWrite(0),
            DrawOp::Quads(2),
            DrawOp::MaskClear(0),
        ],
    );
}

/// A skipped group costs the next nothing: group 1 misses the damage, so group 0's chain is still stamped when group 2 arrives with the same chain in another span. One `MaskWrite`; separate staging would clear a correct mask.
#[test]
fn stencil_keeps_a_chain_stamped_across_a_skipped_group() {
    let e = URect::new(0, 0, 100, 100);
    let outer = rounded(100.0, 100.0, 8.0);
    let inner = rounded(50.0, 50.0, 4.0);
    let mut buf = buf_with(vec![
        DrawGroup {
            scissor: Some(e),
            rounded_clips: Span::new(0, 1),
            ..group(Span::new(0, 1))
        },
        // Outside the damage rect, so the walk skips it.
        DrawGroup {
            scissor: Some(URect::new(200, 200, 10, 10)),
            rounded_clips: Span::new(1, 1),
            ..group(Span::new(1, 1))
        },
        DrawGroup {
            scissor: Some(e),
            rounded_clips: Span::new(2, 1),
            ..group(Span::new(2, 1))
        },
    ]);
    buf.rounded_clips = vec![outer, inner, outer];
    let mi = mask_ix(&buf);
    assert_eq!(
        mi.groups,
        vec![Span::new(0, 1), Span::new(1, 1), Span::new(0, 1)]
    );

    let steps = collect(&buf, Some(e), Some(&mi));
    assert_eq!(
        simplify(&buf, &steps),
        vec![
            DrawOp::PreClear,
            DrawOp::MaskWrite(0),
            DrawOp::Quads(0),
            DrawOp::Quads(2),
            DrawOp::MaskClear(0),
        ],
    );
}

/// Runs the real mask staging (CPU half) over `buf`.
fn mask_ix(buf: &RenderBuffer) -> MaskPlan {
    let mut mi = MaskPlan::default();
    mi.build(buf);
    mi
}

/// A mask draw plus the scissor the pass held when it ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MaskUnderScissor {
    step: RenderStep,
    scissor: URect,
}

/// Replays `steps`, pairing each mask draw with the scissor in force (`SetScissor` is deduplicated, so not necessarily the preceding step).
fn mask_scissors(steps: &[RenderStep]) -> Vec<MaskUnderScissor> {
    let mut scissor = None;
    let mut out = Vec::new();
    for &step in steps {
        match step {
            RenderStep::SetScissor(r) => scissor = Some(r),
            RenderStep::MaskStamp(_) | RenderStep::MaskClear(_) => out.push(MaskUnderScissor {
                step,
                scissor: scissor.expect("mask draw before any SetScissor"),
            }),
            _ => {}
        }
    }
    out
}

fn rounded(w: f32, h: f32, radius: f32) -> RoundedClip {
    RoundedClip {
        mask_rect: Rect {
            min: Vec2::ZERO,
            size: Size::new(w, h),
        },
        corners: Corners::all(radius),
    }
}
