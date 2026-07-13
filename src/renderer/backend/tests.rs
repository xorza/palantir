//! Pin per-frame render schedule against `for_each_step`'s actual
//! emit order — same module the production renderer
//! ([`crate::renderer::backend::WgpuBackend::render_groups`]) consumes, so the asserted
//! sequence can't drift from the real wgpu dispatch.

use crate::primitives::color::{Color, ColorF16, ColorU8};
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::renderer::backend::quad_pipeline::{MaskIndices, build_mask_indices};
use crate::renderer::backend::schedule::{RenderStep, for_each_step};
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::{
    DrawGroup, MeshBatch, RenderBuffer, RoundedClip, TextBatch, TextRun,
};
use crate::text::TextCacheKey;
use glam::{UVec2, Vec2};

/// "Simplified" view of the render schedule — strips bookkeeping
/// (`SetScissor`, `SetStencilRef`) that the tests don't care to pin
/// directly; `MaskStamp` / `MaskClear` map to `MaskWrite` /
/// `MaskClear`. Stencil tests assert on this view; raw [`RenderStep`]
/// is also tested (e.g. `setscissor_steps_present`) for fidelity that
/// scissor narrowing and stencil-ref stepping actually happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DrawOp {
    PreClear,
    MaskWrite(u32),
    MaskClear(u32),
    Quads(usize),
    Text(usize),
    Meshes(usize),
    Images(usize),
    Curves(usize),
}

fn collect(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    masks: &MaskIndices,
    use_stencil: bool,
) -> Vec<RenderStep> {
    let mut steps = Vec::new();
    for_each_step(buffer, damage_scissor, masks, use_stencil, |s| {
        steps.push(s);
    });
    steps
}

/// Run the real mask staging (CPU half) over `buf`, returning the
/// per-group / per-batch mask spans; `masks` receives the deduped
/// mask-quad instances.
fn mask_ix(buf: &RenderBuffer, masks: &mut Vec<Quad>) -> MaskIndices {
    let mut mi = MaskIndices::default();
    build_mask_indices(buf, &mut mi, masks);
    mi
}

fn simplify(buffer: &RenderBuffer, steps: &[RenderStep]) -> Vec<DrawOp> {
    let mut out = Vec::new();
    for s in steps {
        match s {
            RenderStep::PreClear => out.push(DrawOp::PreClear),
            RenderStep::SetScissor(_) | RenderStep::SetStencilRef(_) => {}
            RenderStep::MaskStamp(mi) => out.push(DrawOp::MaskWrite(*mi)),
            RenderStep::MaskClear(mi) => out.push(DrawOp::MaskClear(*mi)),
            RenderStep::Quads { group, .. } => out.push(DrawOp::Quads(*group)),
            RenderStep::Text { batch } => out.push(DrawOp::Text(*batch)),
            RenderStep::MeshBatch { batch } => out.push(DrawOp::Meshes(
                buffer.mesh_batches[*batch].last_group as usize,
            )),
            RenderStep::ImageBatch { batch } => out.push(DrawOp::Images(
                buffer.image_batches[*batch].last_group as usize,
            )),
            RenderStep::CurveBatch { batch } => out.push(DrawOp::Curves(
                buffer.curve_batches[*batch].last_group as usize,
            )),
        }
    }
    out
}

fn dummy_quad() -> Quad {
    Quad {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        fill: Color::WHITE.into(),
        corners: Corners::ZERO,
        stroke_color: ColorF16::TRANSPARENT,
        stroke_width: 0.0,
        ..Default::default()
    }
}

fn dummy_text() -> TextRun {
    TextRun {
        origin: glam::Vec2::ZERO,
        bounds: URect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        },
        color: ColorU8::WHITE,
        key: TextCacheKey::INVALID,
        scale: 1.0,
    }
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

/// Builds a 100×100 buffer with the given groups and no text batches.
/// Quads/texts pools have four slots each so any small span is valid.
fn buf_with(groups: Vec<DrawGroup>) -> RenderBuffer {
    buf_with_batches(groups, Vec::new())
}

/// A `TextBatch` with the full-viewport sentinel scissor and no mask
/// chain — schedule tests don't drive shader-level clipping, so the
/// scissor only needs to survive the damage intersect. Text batches
/// are constructed explicitly (mirroring what the composer emits)
/// rather than derived from groups: `DrawGroup` carries no per-group
/// text span, and a fixture that synthesized batches from groups
/// would mask composer/batch decorrelation bugs. Batches anchored at
/// *rounded* groups build their `TextBatch` inline instead: they need
/// a chain matching their `last_group`'s and a realistic bounds-union
/// scissor (the composer clamps it to the clip, so it never exceeds
/// the stamp scissor the way this sentinel would).
fn text_batch(texts: Span, last_group: u32) -> TextBatch {
    TextBatch {
        texts,
        last_group,
        scissor: URect::new(0, 0, u32::MAX, u32::MAX),
        rounded_clips: Span::default(),
    }
}

/// Adds one `MeshBatch` per entry in `anchors`, each anchored at the
/// group index listed. Span values are stub indices into a parallel
/// `meshes.draws` vec — the schedule only reads `last_group`, so the
/// span content doesn't matter for these tests.
fn buf_with_mesh_anchors(groups: Vec<DrawGroup>, anchors: &[u32]) -> RenderBuffer {
    let mut buf = buf_with(groups);
    for (i, &g) in anchors.iter().enumerate() {
        buf.mesh_batches.push(MeshBatch {
            meshes: Span::new(i as u32, 1),
            last_group: g,
        });
    }
    buf
}

/// Same shape as [`buf_with_mesh_anchors`] but for image batches.
fn buf_with_image_anchors(groups: Vec<DrawGroup>, anchors: &[u32]) -> RenderBuffer {
    use crate::renderer::render_buffer::ImageBatch;
    let mut buf = buf_with(groups);
    for (i, &g) in anchors.iter().enumerate() {
        buf.image_batches.push(ImageBatch {
            images: Span::new(i as u32, 1),
            last_group: g,
        });
    }
    buf
}

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
        simplify(&buf, &collect(&buf, None, &MaskIndices::default(), false)),
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
        simplify(&buf, &collect(&buf, None, &MaskIndices::default(), false)),
        // Group 0 has no text → not part of a batch. Group 1's text
        // is the only batch (idx 0), emitted after group 1's quads
        // (it has none) → immediately.
        vec![DrawOp::Quads(0), DrawOp::Text(0)],
    );
}

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
        simplify(&buf, &collect(&buf, damage, &MaskIndices::default(), false)),
        vec![DrawOp::PreClear, DrawOp::Quads(0), DrawOp::Text(0),],
    );
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &MaskIndices::default(), false)),
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
        &MaskIndices::default(),
        false,
    );
    let pass_b = collect(
        &buf,
        Some(URect::new(50, 0, 50, 100)),
        &MaskIndices::default(),
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

/// Pin: a stencil-clipped group stamps its mask before its draws so
/// fragments inside the rounded SDF pass `Equal(1)`, and the walk ends
/// with a tail `MaskClear` — the pass clears the stencil once (not per
/// damage rect) and padded damage scissors can overlap, so a stamped
/// mask must never survive a walk. Raw steps additionally pin the
/// depth-1 grammar: the stamp draws at ref 0 (no `SetStencilRef`
/// before it — the pass opens at 0), content follows at ref 1.
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
                group: 0,
                range: Span::new(0, 2),
            },
            // Batch drain: same chain, batch scissor inside the stamp's
            // — elided, text draws under the still-stamped mask at ref 1.
            RenderStep::SetScissor(s),
            RenderStep::Text { batch: 0 },
            // Tail clear under the stamp-time scissor.
            RenderStep::SetScissor(s),
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
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

/// End-to-end pin of the same-mask elision: `build_mask_indices` (the
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
    // Elision at raw-step level: nothing between the sharing groups'
    // quads but group 1's scissor set — no SetStencilRef, no mask quad.
    let q0 = steps
        .iter()
        .position(|s| matches!(s, RenderStep::Quads { group: 0, .. }))
        .unwrap();
    let q1 = steps
        .iter()
        .position(|s| matches!(s, RenderStep::Quads { group: 1, .. }))
        .unwrap();
    assert!(
        steps[q0 + 1..q1]
            .iter()
            .all(|s| matches!(s, RenderStep::SetScissor(_))),
        "no stencil traffic between same-mask groups; got {:?}",
        &steps[q0 + 1..q1],
    );
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
    assert_eq!(
        collect(&buf, None, &mi, true),
        vec![
            // Group A: narrow to SA, stamp mask 0 at ref 0 (pass opens
            // at 0), content at ref 1.
            RenderStep::SetScissor(sa),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::Quads {
                group: 0,
                range: Span::new(0, 1),
            },
            // A→B transition: clear A's mask under SA — the scissor
            // the stamp ran under — BEFORE SetScissor(SB). SA ∩ SB is
            // empty, so a clear inside SB (the old order) would touch
            // none of the stamped pixels.
            RenderStep::SetScissor(sa),
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            // Group B: narrow to SB, stamp mask 1 (ref still 0 after
            // the clear), draw at ref 1.
            RenderStep::SetScissor(sb),
            RenderStep::MaskStamp(1),
            RenderStep::SetStencilRef(1),
            RenderStep::Quads {
                group: 1,
                range: Span::new(1, 1),
            },
            // B→C transition: clear B's mask under SB; the clear left
            // ref at 0, which is what unmasked group C needs.
            RenderStep::SetScissor(sb),
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(1),
            RenderStep::SetScissor(sc),
            RenderStep::Quads {
                group: 2,
                range: Span::new(2, 1),
            },
            // C is unmasked: stencil already clean, no tail clear.
        ],
    );

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
        &steps[steps.len() - 3..],
        &[
            RenderStep::SetScissor(sb),
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(1),
        ],
    );
}

/// Depth-2 chain grammar, hand-derived end to end. Group 0 nests two
/// rounded clips (outer mask 0, inner mask 1): the stamp ladder runs
/// outer at ref 0 → stencil 1, inner at ref 1 → stencil 2 (only
/// inside the outer), content at ref 2. Group 1 carries a value-equal
/// chain in a *different* span (pop/re-push of identical clips) —
/// `build_mask_indices` dedups by value, so the schedule elides and
/// nothing but a scissor set separates the two groups' quads. Group 2
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
    assert_eq!(
        collect(&buf, None, &mi, true),
        vec![
            // Group 0: ladder — outer at ref 0, inner at ref 1,
            // content at ref 2.
            RenderStep::SetScissor(e),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::MaskStamp(1),
            RenderStep::SetStencilRef(2),
            RenderStep::Quads {
                group: 0,
                range: Span::new(0, 1),
            },
            // Group 1: identical chain, contained scissor — elided.
            RenderStep::SetScissor(e),
            RenderStep::Quads {
                group: 1,
                range: Span::new(1, 1),
            },
            // Group 2: one clear of the OUTERMOST quad resets both
            // levels; content at ref 0.
            RenderStep::SetScissor(e),
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            RenderStep::SetScissor(e),
            RenderStep::Quads {
                group: 2,
                range: Span::new(2, 1),
            },
        ],
    );

    // Walk ending at depth 2: tail clear is still the single
    // outermost-quad draw under the stamp-time scissor.
    let mut buf = buf_with(vec![group(Span::new(0, 2), 0), group(Span::new(2, 2), 1)]);
    buf.rounded_clips = vec![outer, inner, outer, inner];
    let mi = mask_ix(&buf, &mut masks);
    let steps = collect(&buf, None, &mi, true);
    assert_eq!(
        &steps[steps.len() - 3..],
        &[
            RenderStep::SetScissor(e),
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
        ],
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
    // Batch scissor ∩ damage = (60,0,30,40).
    let s = URect::new(60, 0, 30, 40);
    assert_eq!(
        collect(&buf, Some(damage), &mi, true),
        vec![
            RenderStep::SetScissor(damage),
            RenderStep::PreClear,
            // Trailing drain: the batch establishes its own chain
            // (stamp at ref 0, text at ref 1) under its own scissor.
            RenderStep::SetScissor(s),
            RenderStep::MaskStamp(0),
            RenderStep::SetStencilRef(1),
            RenderStep::Text { batch: 0 },
            // Tail clear of the batch's stamp.
            RenderStep::SetScissor(s),
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
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
                group: 0,
                range: Span::new(0, 1),
            },
            // Group 1 skipped; its batch drains before group 2: same
            // chain, contained scissor — elided, text at ref 1 under
            // the still-stamped mask.
            RenderStep::SetScissor(sa),
            RenderStep::Text { batch: 0 },
            // Group 2 (unmasked) restores: clear under the stamp-time
            // scissor, then its own scissor + quads at ref 0.
            RenderStep::SetScissor(sa),
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            RenderStep::SetScissor(URect::new(45, 0, 50, 40)),
            RenderStep::Quads {
                group: 2,
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
                group: 0,
                range: Span::new(0, 1),
            },
            // Trailing drain: the unmasked batch clears group 0's
            // stamp (under the stamp-time scissor) before drawing at
            // ref 0 under its own scissor. Stencil is clean at walk
            // end — no tail clear.
            RenderStep::SetScissor(sa),
            RenderStep::SetStencilRef(0),
            RenderStep::MaskClear(0),
            RenderStep::SetScissor(URect::new(0, 0, 45, 40)),
            RenderStep::Text { batch: 0 },
        ],
    );
}

/// Pin: under partial damage, the very first emitted step is
/// `SetScissor(damage_scissor)`, and the per-group `SetScissor`
/// narrows further. Confirms the schedule actually emits the scissor
/// transitions production code relies on.
#[test]
fn setscissor_steps_present_under_partial_damage() {
    let buf = buf_with(vec![DrawGroup {
        scissor: Some(URect::new(10, 10, 50, 50)),
        rounded_clips: Span::default(),
        quads: Span::new(0, 1),
    }]);
    let damage = URect::new(0, 0, 80, 80);
    let steps = collect(&buf, Some(damage), &MaskIndices::default(), false);
    // First two: scissor to damage, then PreClear.
    assert_eq!(steps[0], RenderStep::SetScissor(damage));
    assert_eq!(steps[1], RenderStep::PreClear);
    // Group 0's effective scissor is intersection (10,10,50,50) ∩ damage = (10,10,50,50).
    assert_eq!(steps[2], RenderStep::SetScissor(URect::new(10, 10, 50, 50)));
    // Then quads.
    assert!(matches!(steps[3], RenderStep::Quads { group: 0, .. }));
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
            &collect(&buf, Some(damage), &MaskIndices::default(), false)
        ),
        vec![DrawOp::PreClear, DrawOp::Quads(0)],
    );
}

/// Constructs a 100×100 buffer with the given groups and explicit
/// `text_batches` (built the way the composer would emit them — see
/// [`text_batch`]). Quads/texts pools have four slots each so any
/// small span is valid.
fn buf_with_batches(groups: Vec<DrawGroup>, text_batches: Vec<TextBatch>) -> RenderBuffer {
    RenderBuffer {
        quads: vec![dummy_quad(); 4],
        texts: vec![dummy_text(); 4],
        groups,
        text_batches,
        viewport_phys: UVec2::new(100, 100),
        viewport_phys_f: Vec2::new(100.0, 100.0),
        scale: 1.0,
        ..RenderBuffer::default()
    }
}

/// Pin: two groups sharing one text batch emit `Text` ONCE, after the
/// last group's quads. Without coalescing the schedule would emit two
/// text steps (and the backend two glyphon prepares/renders).
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
        simplify(&buf, &collect(&buf, None, &MaskIndices::default(), false)),
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
        simplify(&buf, &collect(&buf, None, &MaskIndices::default(), false)),
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
        &collect(&buf, Some(damage), &MaskIndices::default(), false),
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
        &collect(&buf, Some(damage), &MaskIndices::default(), false),
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
        simplify(&buf, &collect(&buf, None, &MaskIndices::default(), false)),
        vec![
            DrawOp::Quads(0),
            DrawOp::Text(0),
            DrawOp::Quads(1),
            DrawOp::Text(1),
        ],
    );
}

/// Pin: each mesh-emitting group contributes its own `MeshBatch`,
/// drained at the group iteration anchored by `last_group`. Two
/// adjacent mesh groups → two emit steps, in order.
#[test]
fn mesh_batches_emit_per_group_in_order() {
    let buf = buf_with_mesh_anchors(
        vec![
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::default(),
            },
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::default(),
            },
        ],
        &[0, 1],
    );
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &MaskIndices::default(), false)),
        vec![DrawOp::Meshes(0), DrawOp::Meshes(1)],
    );
}

/// Pin: a mesh batch anchored in a damage-skipped group is silently
/// dropped — the stale-cursor advance at the top of each schedule
/// iteration moves past it, so no `MeshBatch` step is emitted for
/// invisible meshes. Counter-pin: the visible group still drains
/// its own batch.
#[test]
fn mesh_batch_in_damage_skipped_group_drops_silently() {
    let buf = buf_with_mesh_anchors(
        vec![
            DrawGroup {
                scissor: Some(URect::new(0, 0, 50, 100)),
                rounded_clips: Span::default(),
                quads: Span::default(),
            },
            DrawGroup {
                scissor: Some(URect::new(50, 0, 50, 100)),
                rounded_clips: Span::default(),
                quads: Span::default(),
            },
        ],
        &[0, 1],
    );
    let damage = Some(URect::new(50, 0, 50, 100));
    assert_eq!(
        simplify(&buf, &collect(&buf, damage, &MaskIndices::default(), false)),
        vec![DrawOp::PreClear, DrawOp::Meshes(1)],
    );
}

/// Pin: an image batch anchored at group `j` emits `ImageBatch` after
/// that group's quads and meshes (image sits at mesh tier in the
/// kind order). Counter-pin to ensure the new `next_image_batch`
/// cursor wires through both stencil and non-stencil paths.
#[test]
fn image_batch_emits_after_group_quads_in_non_stencil_path() {
    let buf = buf_with_image_anchors(
        vec![
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::default(),
            },
            DrawGroup {
                scissor: None,
                rounded_clips: Span::default(),
                quads: Span::default(),
            },
        ],
        &[0, 1],
    );
    assert_eq!(
        simplify(&buf, &collect(&buf, None, &MaskIndices::default(), false)),
        vec![DrawOp::Images(0), DrawOp::Images(1)],
    );
}

/// Pin: image batch in a damage-skipped group is silently dropped.
#[test]
fn image_batch_in_damage_skipped_group_drops_silently() {
    let buf = buf_with_image_anchors(
        vec![
            DrawGroup {
                scissor: Some(URect::new(0, 0, 50, 100)),
                rounded_clips: Span::default(),
                quads: Span::default(),
            },
            DrawGroup {
                scissor: Some(URect::new(50, 0, 50, 100)),
                rounded_clips: Span::default(),
                quads: Span::default(),
            },
        ],
        &[0, 1],
    );
    let damage = Some(URect::new(50, 0, 50, 100));
    assert_eq!(
        simplify(&buf, &collect(&buf, damage, &MaskIndices::default(), false)),
        vec![DrawOp::PreClear, DrawOp::Images(1)],
    );
}
