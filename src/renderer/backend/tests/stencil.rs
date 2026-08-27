//! Rounded-clip mask stamping: when a chain writes, dedups, restamps, or
//! clears.

use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::renderer::backend::schedule::RenderStep;
use crate::renderer::backend::schedule::{MaskPlan, build_mask_plan};
use crate::renderer::backend::tests::support::{
    DrawOp, buf_with, buf_with_batches, collect, scissor_count, simplify, text_batch,
};
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::text_batch::TextBatch;
use crate::renderer::render_buffer::{RenderBuffer, RoundedClip};
use glam::Vec2;

/// Pin: a stencil-clipped group stamps its mask before its draws so
/// fragments inside the rounded SDF pass `Equal(1)`, and the walk ends
/// with a tail `MaskClear` — the pass clears the stencil once (not per
/// damage rect) and padded damage scissors can overlap, so a stamped
/// mask must never survive a walk. Raw steps additionally pin the
/// depth-1 grammar: the stamp draws at ref 0 (no `SetStencilRef`
/// before it — the pass opens at 0), content follows at ref 1, and the
/// group, its text batch, and the tail clear — all wanting the same
/// rect — share a single `SetScissor`.
#[test]
fn stencil_group_brackets_draws_with_mask_write() {
    let mut buf = buf_with_batches(
        vec![DrawGroup {
            scissor: Some(URect::new(0, 0, 100, 100)),
            rounded_clips: Span::new(0, 1),
            quads: Span::new(0, 2),
        }],
        vec![TextBatch {
            texts: Span::new(0, 1),
            last_group: 0,
            scissor: URect::new(0, 0, 100, 100),
            rounded_clips: Span::new(0, 1),
        }],
    );
    buf.rounded_clips = vec![rounded(100.0, 100.0, 8.0)];
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    assert_eq!(mi.groups, vec![Span::new(0, 1)]);
    assert_eq!(mi.batches, vec![Span::new(0, 1)]);
    assert_eq!(masks.len(), 1);
    let steps = collect(&buf, None, &mi, true);
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
            // Batch drain: same chain, batch scissor inside the stamp's
            // — elided, text draws under the still-stamped mask at ref 1.
            // Its scissor request equals the group's, so no transition.
            RenderStep::Text { batch: 0 },
            // Tail clear, still under the stamp-time scissor: the walk
            // never left it, so only the ref transition is emitted.
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
        ],
    );
    // Group, batch drain, and tail clear all want the same rect — one
    // transition covers the whole walk.
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

/// Pin: in a stencil-attached pass, a *non-rounded* group still runs
/// at `stencil_ref = 0` (matches the cleared stencil so `Equal(0)`
/// passes everywhere) but emits no mask quads. Mixed in with a
/// rounded sibling, each retains its own bracket — the rounded
/// group's mask write/clear must not bleed into the non-rounded
/// neighbor.
#[test]
fn stencil_mixed_rounded_and_plain_groups_keep_brackets_local() {
    let mut buf = buf_with_batches(
        vec![
            // Group 0: rounded clip
            DrawGroup {
                scissor: Some(URect::new(0, 0, 100, 100)),
                rounded_clips: Span::new(0, 1),
                quads: Span::new(0, 1),
            },
            // Group 1: plain (no rounded clip), with text
            DrawGroup {
                scissor: Some(URect::new(0, 0, 100, 100)),
                rounded_clips: Span::default(),
                quads: Span::new(1, 1),
            },
        ],
        vec![text_batch(Span::new(0, 1), 1)],
    );
    buf.rounded_clips = vec![rounded(100.0, 100.0, 8.0)];
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    assert_eq!(mi.groups, vec![Span::new(0, 1), Span::default()]);
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &mi, true)),
        vec![
            // Rounded bracket
            DrawOp::MaskWrite(0),
            DrawOp::Quads(0),
            DrawOp::MaskClear(0),
            // Plain group: no mask write/clear, just draw
            DrawOp::Quads(1),
            // Only group 1 has text → single batch idx 0.
            DrawOp::Text(0),
        ],
    );
}

/// End-to-end pin of the same-mask elision: `build_mask_plan` (the
/// CPU half of `stage_masks`) dedups consecutive value-equal chains
/// onto one shared mask-quad run (common: a rect clip nested in a
/// rounded ancestor inherits the ancestor's chain verbatim, and
/// quad-budget flushes split groups without changing clip), and the
/// schedule then elides the clear + re-stamp between the sharing
/// groups — the mask stays stamped, both draw under ref=1. A third
/// group with a different clip still triggers the full
/// clear-then-write transition, and the walk tail-clears the last
/// stamped mask.
#[test]
fn stencil_consecutive_same_mask_groups_dedup_writes() {
    let mut buf = buf_with(vec![
        // Groups 0 and 1: identical chain values (same span, as the
        // composer emits while the chain is unchanged).
        DrawGroup {
            scissor: Some(URect::new(0, 0, 100, 100)),
            rounded_clips: Span::new(0, 1),
            quads: Span::new(0, 1),
        },
        DrawGroup {
            scissor: Some(URect::new(0, 0, 100, 100)),
            rounded_clips: Span::new(0, 1),
            quads: Span::new(1, 1),
        },
        // Group 2: different clip — full transition required.
        DrawGroup {
            scissor: Some(URect::new(0, 0, 100, 100)),
            rounded_clips: Span::new(1, 1),
            quads: Span::new(2, 1),
        },
    ]);
    buf.rounded_clips = vec![rounded(100.0, 100.0, 8.0), rounded(50.0, 50.0, 4.0)];
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    // Groups 0+1 dedup onto mask 0 (one uploaded instance); group 2
    // gets its own.
    assert_eq!(
        mi.groups,
        vec![Span::new(0, 1), Span::new(0, 1), Span::new(1, 1)]
    );
    assert_eq!(masks.len(), 2);

    let steps = collect(&buf, None, &mi, true);
    assert_eq!(
        simplify(&buf, &steps),
        vec![
            // Group 0: stamp mask 0.
            DrawOp::MaskWrite(0),
            DrawOp::Quads(0),
            // Group 1: same mask — no bracket, just draw.
            DrawOp::Quads(1),
            // Group 2: clear 0 (under its stamp scissor), stamp 1.
            DrawOp::MaskClear(0),
            DrawOp::MaskWrite(1),
            DrawOp::Quads(2),
            // Walk end: mask 1 still stamped — tail clear.
            DrawOp::MaskClear(1),
        ],
    );
    // Elision at raw-step level: the sharing groups also share a
    // scissor, so *nothing at all* separates their quads — no
    // SetStencilRef, no mask quad, and no repeated SetScissor.
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
    // All three groups carry the same scissor: one transition total.
    assert_eq!(scissor_count(&steps), 1);
}

/// Counter-pin on the same-mask elision: sharing a mask index is only
/// safe while each group's scissor stays inside the stamp's. Group 0
/// stamps mask 0 inside a half-width scissor; group 1 carries the
/// same clip but a wider scissor, so pixels in the exposed half still
/// hold stencil 0 and would wrongly fail `Equal(1)` — the schedule
/// must clear and re-stamp (same mask index) under the wider scissor
/// instead of eliding.
#[test]
fn stencil_same_mask_wider_scissor_restamps() {
    let mut buf = buf_with(vec![
        DrawGroup {
            scissor: Some(URect::new(0, 0, 50, 100)),
            rounded_clips: Span::new(0, 1),
            quads: Span::new(0, 1),
        },
        DrawGroup {
            scissor: Some(URect::new(0, 0, 100, 100)),
            rounded_clips: Span::new(0, 1),
            quads: Span::new(1, 1),
        },
    ]);
    buf.rounded_clips = vec![rounded(100.0, 100.0, 8.0)];
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    // Identical clips still dedup to one uploaded mask instance...
    assert_eq!(mi.groups, vec![Span::new(0, 1), Span::new(0, 1)]);
    assert_eq!(masks.len(), 1);
    // ...but the schedule re-brackets: clear under the stamp's
    // (0,0,50,100), re-stamp mask 0 under (0,0,100,100), tail clear.
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &mi, true)),
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

/// Pin: a stencil-pass group with text but no quads still emits the
/// mask write. Without it, the text would render unstenciled —
/// rounded clip would silently leak past the mask boundary. The walk
/// then tail-clears the stamped mask.
#[test]
fn stencil_text_only_group_still_writes_mask() {
    let mut buf = buf_with_batches(
        vec![DrawGroup {
            scissor: Some(URect::new(0, 0, 100, 100)),
            rounded_clips: Span::new(0, 1),
            quads: Span::new(0, 0),
        }],
        vec![TextBatch {
            texts: Span::new(0, 1),
            last_group: 0,
            scissor: URect::new(0, 0, 100, 100),
            rounded_clips: Span::new(0, 1),
        }],
    );
    buf.rounded_clips = vec![rounded(100.0, 100.0, 8.0)];
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &mi, true)),
        vec![DrawOp::MaskWrite(0), DrawOp::Text(0), DrawOp::MaskClear(0)],
    );
}

/// Reproduce the old stale-residue bug shape: rounded group A stamps
/// its mask inside scissor SA, then group B has a *disjoint* scissor
/// SB. The old order emitted `SetScissor(SB)` first and then cleared
/// A's mask — inside SB, where the stamp never wrote — leaving
/// stencil-1 residue across SA ∩ SDF for the rest of the pass. Pin:
/// the clear replays under SA *before* B's `SetScissor`, and a walk
/// whose last group is masked tail-clears so nothing leaks into the
/// next damage rect's walk (padded rect scissors can overlap).
#[test]
fn stencil_stale_mask_clears_under_stamp_scissor_then_tail_clears() {
    let sa = URect::new(0, 0, 40, 40);
    let sb = URect::new(50, 0, 40, 40);
    let sc = URect::new(0, 50, 100, 50);
    let group = |scissor, chain, q| DrawGroup {
        scissor: Some(scissor),
        rounded_clips: chain,
        quads: Span::new(q, 1),
    };
    let clips = vec![rounded(40.0, 40.0, 8.0), rounded(40.0, 40.0, 4.0)];
    let mut buf = buf_with(vec![
        group(sa, Span::new(0, 1), 0),
        group(sb, Span::new(1, 1), 1),
        group(sc, Span::default(), 2),
    ]);
    buf.rounded_clips = clips.clone();
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    assert_eq!(
        mi.groups,
        vec![Span::new(0, 1), Span::new(1, 1), Span::default()]
    );
    let steps = collect(&buf, None, &mi, true);
    assert_eq!(
        steps,
        vec![
            // Group A: narrow to SA, stamp mask 0 at ref 0 (pass opens
            // at 0), content at ref 1.
            RenderStep::SetScissor(sa),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
            // A→B transition: clear A's mask under SA — the scissor the
            // stamp ran under, which the walk still holds — BEFORE any
            // SetScissor(SB). SA ∩ SB is empty, so a clear inside SB
            // (the old order) would touch none of the stamped pixels.
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            // Group B: narrow to SB, stamp mask 1 (ref still 0 after
            // the clear), draw at ref 1.
            RenderStep::SetScissor(sb),
            RenderStep::MaskStamp(1),
            RenderStep::SetStencilRef(1),
            RenderStep::Quads {
                range: Span::new(1, 1),
            },
            // B→C transition: clear B's mask under SB (still held); the
            // clear left ref at 0, which is what unmasked group C needs.
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(1),
            RenderStep::SetScissor(sc),
            RenderStep::Quads {
                range: Span::new(2, 1),
            },
            // C is unmasked: stencil already clean, no tail clear.
        ],
    );
    // The load-bearing invariant, read off the running scissor rather
    // than step adjacency: each mask draw ran under its own group's rect
    // and every clear matched its stamp.
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
    // Three distinct group scissors, three transitions — the clears add
    // none of their own.
    assert_eq!(scissor_count(&steps), 3);

    // Same walk minus C: it now ends with mask 1 stamped, so a tail
    // clear (again under SB, the stamp scissor) must close the walk.
    let mut buf = buf_with(vec![
        group(sa, Span::new(0, 1), 0),
        group(sb, Span::new(1, 1), 1),
    ]);
    buf.rounded_clips = clips;
    let mi = mask_ix(&buf, &mut masks);
    let steps = collect(&buf, None, &mi, true);
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

/// Depth-2 chain grammar, hand-derived end to end. Group 0 nests two
/// rounded clips (outer mask 0, inner mask 1): the stamp ladder runs
/// outer at ref 0 → stencil 1, inner at ref 1 → stencil 2 (only
/// inside the outer), content at ref 2. Group 1 carries a value-equal
/// chain in a *different* span (pop/re-push of identical clips) —
/// `build_mask_plan` dedups by value, so the schedule elides and —
/// since both groups also share a scissor — nothing at all separates
/// the two groups' quads. Group 2
/// is unmasked: ONE clear of the outermost mask resets the whole
/// chain (inner stamps only incremented inside the outer's SDF).
/// Second walk (groups 0+1 only) pins the depth-2 tail clear.
#[test]
fn stencil_nested_chain_stamps_ladder_elides_and_single_clears() {
    let e = URect::new(0, 0, 100, 100);
    let outer = rounded(100.0, 100.0, 8.0);
    let inner = rounded(80.0, 80.0, 4.0);
    let group = |chain, q| DrawGroup {
        scissor: Some(e),
        rounded_clips: chain,
        quads: Span::new(q, 1),
    };
    let mut buf = buf_with(vec![
        group(Span::new(0, 2), 0),
        group(Span::new(2, 2), 1),
        group(Span::default(), 2),
    ]);
    buf.rounded_clips = vec![outer, inner, outer, inner];
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    // Value-equal chains share one mask-quad run: two quads total.
    assert_eq!(
        mi.groups,
        vec![Span::new(0, 2), Span::new(0, 2), Span::default()]
    );
    assert_eq!(masks.len(), 2);
    let steps = collect(&buf, None, &mi, true);
    assert_eq!(
        steps,
        vec![
            // Group 0: ladder — outer at ref 0, inner at ref 1,
            // content at ref 2.
            RenderStep::SetScissor(e),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::MaskStamp(1),
            RenderStep::SetStencilRef(2),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
            // Group 1: identical chain, same scissor — elided down to
            // nothing but its own draw.
            RenderStep::Quads {
                range: Span::new(1, 1),
            },
            // Group 2: one clear of the OUTERMOST quad resets both
            // levels; content at ref 0.
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            RenderStep::Quads {
                range: Span::new(2, 1),
            },
        ],
    );
    // Every group shares the viewport rect: one transition for the walk.
    assert_eq!(scissor_count(&steps), 1);

    // Walk ending at depth 2: tail clear is still the single
    // outermost-quad draw under the stamp-time scissor.
    let mut buf = buf_with(vec![group(Span::new(0, 2), 0), group(Span::new(2, 2), 1)]);
    buf.rounded_clips = vec![outer, inner, outer, inner];
    let mi = mask_ix(&buf, &mut masks);
    let steps = collect(&buf, None, &mi, true);
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

/// Fix-2 pin: a rounded batch drained while NO group painted this
/// walk (both its groups sit outside the damage rect, but the batch's
/// bounds-union rect pokes into it) must stamp ITS OWN mask before
/// its `Text` step — previously it drew under whatever stencil state
/// was active at the drain point (here: none, so `Equal(0)` would
/// have let its glyphs paint square outside the rounded corners).
/// The walk then tail-clears the batch's stamp.
///
/// Geometry: groups at (0,0,40,40) and (50,50,40,40) share one chain;
/// the batch's scissor is the bounds union (0,0,90,90). Damage
/// (60,0,30,40) intersects neither group's scissor but does intersect
/// the union.
#[test]
fn stencil_drained_batch_stamps_own_mask_before_text() {
    let chain = Span::new(0, 1);
    let mut buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: Some(URect::new(0, 0, 40, 40)),
                rounded_clips: chain,
                quads: Span::new(0, 1),
            },
            DrawGroup {
                scissor: Some(URect::new(50, 50, 40, 40)),
                rounded_clips: chain,
                quads: Span::new(1, 1),
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
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    assert_eq!(mi.batches, vec![Span::new(0, 1)]);
    let damage = URect::new(60, 0, 30, 40);
    // Batch scissor ∩ damage = (60,0,30,40) — the damage rect itself,
    // so the batch's scissor request is already satisfied.
    let s = URect::new(60, 0, 30, 40);
    let steps = collect(&buf, Some(damage), &mi, true);
    assert_eq!(
        steps,
        vec![
            RenderStep::SetScissor(damage),
            RenderStep::PreClear,
            // Trailing drain: the batch establishes its own chain
            // (stamp at ref 0, text at ref 1) under its own scissor.
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::Text { batch: 0 },
            // Tail clear of the batch's stamp.
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

/// Fix-2 pin, drain at a later group: a batch anchored in a
/// damage-skipped group whose chain is STILL STAMPED (group 0 shares
/// it, scissor contains the batch's) elides — its text draws under
/// the live mask at ref 1 — and the unmasked group that follows
/// restores its own state with the usual clear.
#[test]
fn stencil_drained_batch_elides_when_own_chain_still_stamped() {
    let chain = Span::new(0, 1);
    let sa = URect::new(0, 0, 40, 40);
    let mut buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: Some(sa),
                rounded_clips: chain,
                quads: Span::new(0, 1),
            },
            // Anchor group: same chain, below the damage rect.
            DrawGroup {
                scissor: Some(URect::new(0, 50, 40, 40)),
                rounded_clips: chain,
                quads: Span::new(1, 1),
            },
            // Plain group after the skipped anchor — the drain point.
            DrawGroup {
                scissor: Some(URect::new(45, 0, 50, 40)),
                rounded_clips: Span::default(),
                quads: Span::new(2, 1),
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
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    let damage = URect::new(0, 0, 100, 40);
    // Batch scissor ∩ damage = (0,0,40,40) = group 0's stamp scissor.
    assert_eq!(
        collect(&buf, Some(damage), &mi, true),
        vec![
            RenderStep::SetScissor(damage),
            RenderStep::PreClear,
            // Group 0 stamps the shared chain.
            RenderStep::SetScissor(sa),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
            // Group 1 skipped; its batch drains before group 2: same
            // chain, and its scissor ∩ damage is exactly group 0's —
            // elided whole, text at ref 1 under the still-stamped mask.
            RenderStep::Text { batch: 0 },
            // Group 2 (unmasked) restores: clear under the stamp-time
            // scissor (still held), then its own scissor + quads at ref 0.
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            RenderStep::SetScissor(URect::new(45, 0, 50, 40)),
            RenderStep::Quads {
                range: Span::new(2, 1),
            },
        ],
    );
}

/// Fix-2 counter-pin: an UNMASKED batch drained while a mask is
/// active must clear that mask before its `Text` step — otherwise the
/// glyphs would stencil-test `Equal(ref)` against the foreign stamp
/// and vanish outside it (missing text on partial-repaint frames).
#[test]
fn stencil_unmasked_batch_drained_under_active_mask_clears_first() {
    let sa = URect::new(0, 0, 40, 40);
    let mut buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: Some(sa),
                rounded_clips: Span::new(0, 1),
                quads: Span::new(0, 1),
            },
            // Plain anchor group, outside the damage rect.
            DrawGroup {
                scissor: Some(URect::new(50, 0, 40, 40)),
                rounded_clips: Span::default(),
                quads: Span::new(1, 1),
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
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    let damage = URect::new(0, 0, 45, 45);
    assert_eq!(
        collect(&buf, Some(damage), &mi, true),
        vec![
            RenderStep::SetScissor(damage),
            RenderStep::PreClear,
            // Group 0 stamps its mask; group 1 is damage-skipped.
            RenderStep::SetScissor(sa),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::Quads {
                range: Span::new(0, 1),
            },
            // Trailing drain: the unmasked batch clears group 0's
            // stamp (under the stamp-time scissor, which the walk still
            // holds) before drawing at ref 0 under its own scissor.
            // Stencil is clean at walk end — no tail clear.
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            RenderStep::SetScissor(URect::new(0, 0, 45, 40)),
            RenderStep::Text { batch: 0 },
        ],
    );
}

/// Pin: the staging dedups against every chain seen this frame, not
/// against the previous group. Groups 0 and 2 carry value-equal chains
/// in different spans with a foreign chain between them, so the three
/// groups stage two mask quads and group 2 reuses group 0's run. The
/// walk still brackets group 2 — group 1's chain displaced the stamp —
/// so what a neighbour-only dedup costs here is one uploaded quad.
#[test]
fn stencil_dedups_a_chain_seen_before_the_previous_group() {
    let e = URect::new(0, 0, 100, 100);
    let outer = rounded(100.0, 100.0, 8.0);
    let inner = rounded(50.0, 50.0, 4.0);
    let group = |chain, q| DrawGroup {
        scissor: Some(e),
        rounded_clips: chain,
        quads: Span::new(q, 1),
    };
    let mut buf = buf_with(vec![
        group(Span::new(0, 1), 0),
        group(Span::new(1, 1), 1),
        group(Span::new(2, 1), 2),
    ]);
    buf.rounded_clips = vec![outer, inner, outer];
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    assert_eq!(
        mi.groups,
        vec![Span::new(0, 1), Span::new(1, 1), Span::new(0, 1)]
    );
    assert_eq!(masks.len(), 2, "the repeated chain staged a second copy");

    let steps = collect(&buf, None, &mi, true);
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

/// Pin: a group the walk skips costs the group after it nothing. Group
/// 1's scissor misses the damage rect, so the walk emits no step for it
/// and group 0's chain is still stamped when group 2 arrives. Group 2
/// carries the same chain in a *different* source span, and draws under
/// the live stamp — one `MaskWrite` for the whole walk.
///
/// This is what the frame-wide dedup buys. Staging group 2's chain
/// separately would make the spans differ, and the walk reads the span
/// as the chain — so it would clear a mask that was already correct and
/// stamp an identical one back.
#[test]
fn stencil_keeps_a_chain_stamped_across_a_skipped_group() {
    let e = URect::new(0, 0, 100, 100);
    let outer = rounded(100.0, 100.0, 8.0);
    let inner = rounded(50.0, 50.0, 4.0);
    let mut buf = buf_with(vec![
        DrawGroup {
            scissor: Some(e),
            rounded_clips: Span::new(0, 1),
            quads: Span::new(0, 1),
        },
        // Entirely outside the damage rect below, so the walk skips it.
        DrawGroup {
            scissor: Some(URect::new(200, 200, 10, 10)),
            rounded_clips: Span::new(1, 1),
            quads: Span::new(1, 1),
        },
        DrawGroup {
            scissor: Some(e),
            rounded_clips: Span::new(2, 1),
            quads: Span::new(2, 1),
        },
    ]);
    buf.rounded_clips = vec![outer, inner, outer];
    let mut masks = Vec::new();
    let mi = mask_ix(&buf, &mut masks);
    assert_eq!(
        mi.groups,
        vec![Span::new(0, 1), Span::new(1, 1), Span::new(0, 1)]
    );

    let steps = collect(&buf, Some(e), &mi, true);
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

/// Run the real mask staging (CPU half) over `buf`, returning the
/// per-group / per-batch mask spans; `masks` receives the deduped
/// mask-quad instances.
fn mask_ix(buf: &RenderBuffer, masks: &mut Vec<Quad>) -> MaskPlan {
    let mut mi = MaskPlan::default();
    build_mask_plan(buf, &mut mi, masks);
    mi
}

/// A mask draw plus the scissor rect the pass held when it ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MaskUnderScissor {
    step: RenderStep,
    scissor: URect,
}

/// Replay `steps`, pairing every mask draw with the scissor in force at
/// that point. `SetScissor` is a deduplicated transition, so the rect a
/// mask runs under is the last distinct one emitted before it — the
/// "clear replays under its stamp-time scissor" invariant can't be read
/// off the immediately preceding step.
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
