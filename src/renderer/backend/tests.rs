//! Pin per-frame render schedule against `for_each_step`'s actual
//! emit order — same module the production renderer
//! ([`super::WgpuBackend::render_groups`]) consumes, so the asserted
//! sequence can't drift from the real wgpu dispatch.

use super::schedule::{RenderStep, for_each_step};
use crate::layout::types::span::Span;
use crate::primitives::color::{Color, ColorF16, ColorU8};
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::urect::URect;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::{
    DrawGroup, MeshScene, RenderBuffer, RoundedClip, TextBatch, TextRun,
};
use crate::text::TextCacheKey;
use glam::{UVec2, Vec2};

/// "Simplified" view of the render schedule — strips bookkeeping
/// (`SetScissor`, `SetStencilRef`) that the tests don't care to pin
/// directly, and folds the dual-use `MaskQuad` step into the
/// distinguishing variants `MaskWrite` / `MaskClear` based on the
/// stencil reference at emit time. Stencil tests assert on this view;
/// raw [`RenderStep`] is also tested by `setscissor_steps_present`
/// for fidelity that scissor narrowing actually happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DrawOp {
    PreClear,
    MaskWrite(u32),
    MaskClear(u32),
    Quads(usize),
    Text(usize),
    Meshes(usize),
}

fn collect(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    mask_indices: &[Option<u32>],
    use_stencil: bool,
) -> Vec<RenderStep> {
    let mut steps = Vec::new();
    for_each_step(buffer, damage_scissor, mask_indices, use_stencil, |s| {
        steps.push(s);
    });
    steps
}

fn simplify(steps: &[RenderStep]) -> Vec<DrawOp> {
    let mut current_ref: u32 = 0;
    let mut out = Vec::new();
    for s in steps {
        match s {
            RenderStep::PreClear => out.push(DrawOp::PreClear),
            RenderStep::SetScissor(_) => {}
            RenderStep::SetStencilRef(v) => current_ref = *v,
            RenderStep::MaskQuad(mi) => {
                if current_ref == 0 {
                    out.push(DrawOp::MaskClear(*mi));
                } else {
                    out.push(DrawOp::MaskWrite(*mi));
                }
            }
            RenderStep::Quads { group, .. } => out.push(DrawOp::Quads(*group)),
            RenderStep::Text { batch } => out.push(DrawOp::Text(*batch)),
            RenderStep::Meshes { group, .. } => out.push(DrawOp::Meshes(*group)),
        }
    }
    out
}

fn dummy_quad() -> Quad {
    Quad {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        fill: Color::WHITE.into(),
        radius: Corners::ZERO,
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

/// Builds a 100×100 buffer with the given groups. Quads/texts pools
/// have one slot each so any non-empty span is valid. Each group
/// with text gets its own one-group text batch (mimicking the
/// pre-coalesce behavior); the batch's `last_group` is the group's
/// index in the input.
fn buf_with(groups: Vec<DrawGroup>) -> RenderBuffer {
    let mut text_batches = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        if g.texts.len != 0 {
            text_batches.push(TextBatch {
                texts: g.texts,
                last_group: i as u32,
            });
        }
    }
    RenderBuffer {
        quads: vec![dummy_quad(); 4],
        texts: vec![dummy_text(); 4],
        meshes: MeshScene::default(),
        groups,
        text_batches,
        viewport_phys: UVec2::new(100, 100),
        viewport_phys_f: Vec2::new(100.0, 100.0),
        scale: 1.0,
        ..RenderBuffer::default()
    }
}

// ---------- High-level ordering (was `render_schedule_*`) -----------

/// Pin: text in group 0 renders *between* group 0's quads and group 1's
/// quads, so a child quad declared after a label can occlude it. The
/// per-group z-order contract — the showcase tab `text z-order`
/// demonstrates the visual outcome.
#[test]
fn text_interleaves_per_group() {
    let buf = buf_with(vec![
        // Group 0: 2 quads + 1 text
        DrawGroup {
            scissor: None,
            rounded_clip: None,
            quads: Span::new(0, 2),
            texts: Span::new(0, 1),
            meshes: Span::default(),
        },
        // Group 1: 1 quad, no text
        DrawGroup {
            scissor: None,
            rounded_clip: None,
            quads: Span::new(2, 1),
            texts: Span::new(1, 0),
            meshes: Span::default(),
        },
    ]);
    assert_eq!(
        simplify(&collect(&buf, None, &[], false)),
        vec![DrawOp::Quads(0), DrawOp::Text(0), DrawOp::Quads(1)],
    );
}

/// Edge case: a group with text but no quads (e.g. a Hug parent whose
/// only paint is its label). Schedule must still emit `Text(i)`.
#[test]
fn text_emits_for_quadless_group() {
    let buf = buf_with(vec![
        DrawGroup {
            scissor: None,
            rounded_clip: None,
            quads: Span::new(0, 1),
            texts: Span::new(0, 0),
            meshes: Span::default(),
        },
        DrawGroup {
            scissor: None,
            rounded_clip: None,
            quads: Span::new(1, 0),
            texts: Span::new(0, 2),
            meshes: Span::default(),
        },
    ]);
    assert_eq!(
        simplify(&collect(&buf, None, &[], false)),
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
    let buf = buf_with(vec![DrawGroup {
        scissor: None,
        rounded_clip: None,
        quads: Span::new(0, 1),
        texts: Span::new(0, 1),
        meshes: Span::default(),
    }]);
    let damage = Some(URect::new(0, 0, 50, 50));
    assert_eq!(
        simplify(&collect(&buf, damage, &[], false)),
        vec![DrawOp::PreClear, DrawOp::Quads(0), DrawOp::Text(0),],
    );
    assert_eq!(
        simplify(&collect(&buf, None, &[], false)),
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
            rounded_clip: None,
            quads: Span::new(0, 1),
            texts: Span::new(0, 0),
            meshes: Span::default(),
        },
        DrawGroup {
            scissor: Some(URect::new(50, 0, 50, 100)),
            rounded_clip: None,
            quads: Span::new(1, 1),
            texts: Span::new(0, 0),
            meshes: Span::default(),
        },
    ]);
    // DamageEngine rect A covers only group 0; rect B covers only group 1.
    let pass_a = collect(&buf, Some(URect::new(0, 0, 50, 100)), &[], false);
    let pass_b = collect(&buf, Some(URect::new(50, 0, 50, 100)), &[], false);
    let mut combined = pass_a;
    combined.extend(pass_b);
    assert_eq!(
        simplify(&combined),
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

// ---------- Stencil-path coverage --------------------------------

/// Pin: a stencil-clipped group emits `MaskWrite` before its draws so
/// fragments inside the rounded SDF pass `Equal(1)`. The trailing
/// clear is elided here — the pass's `StoreOp::Discard` drops the
/// stencil contents, so leaving a mask stamped at end-of-pass is
/// correctness-neutral. The clear only appears when a *following*
/// group needs a clean stencil (different mask, or `None`).
#[test]
fn stencil_group_brackets_draws_with_mask_write() {
    let buf = buf_with(vec![DrawGroup {
        scissor: Some(URect::new(0, 0, 100, 100)),
        rounded_clip: Some(RoundedClip {
            mask_rect: Rect {
                min: Vec2::ZERO,
                size: Size::new(100.0, 100.0),
            },
            radius: Corners::all(8.0),
        }),
        quads: Span::new(0, 2),
        texts: Span::new(0, 1),
        meshes: Span::default(),
    }]);
    let mask_indices = &[Some(0u32)];
    assert_eq!(
        simplify(&collect(&buf, None, mask_indices, true)),
        vec![DrawOp::MaskWrite(0), DrawOp::Quads(0), DrawOp::Text(0)],
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
    let buf = buf_with(vec![
        // Group 0: rounded clip
        DrawGroup {
            scissor: Some(URect::new(0, 0, 100, 100)),
            rounded_clip: Some(RoundedClip {
                mask_rect: Rect {
                    min: Vec2::ZERO,
                    size: Size::new(100.0, 100.0),
                },
                radius: Corners::all(8.0),
            }),
            quads: Span::new(0, 1),
            texts: Span::new(0, 0),
            meshes: Span::default(),
        },
        // Group 1: plain (no rounded clip)
        DrawGroup {
            scissor: Some(URect::new(0, 0, 100, 100)),
            rounded_clip: None,
            quads: Span::new(1, 1),
            texts: Span::new(0, 1),
            meshes: Span::default(),
        },
    ]);
    let mask_indices = &[Some(0u32), None];
    assert_eq!(
        simplify(&collect(&buf, None, mask_indices, true)),
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

/// Pin: consecutive groups sharing a `rounded_clip` (same `mask_idx`)
/// elide the prior group's tail clear and the next group's prologue
/// write — the mask stays stamped, both groups draw under ref=1.
/// Saves two mask-quad draws and two stencil-ref sets per shared run.
/// A third group with a different mask still triggers the full
/// clear-then-write transition.
#[test]
fn stencil_consecutive_same_mask_groups_dedup_writes() {
    let buf = buf_with(vec![
        // Groups 0 and 1: share mask 0 (e.g. a panel with multiple
        // children all bound to the same rounded-clip ancestor).
        DrawGroup {
            scissor: Some(URect::new(0, 0, 100, 100)),
            rounded_clip: Some(RoundedClip {
                mask_rect: Rect {
                    min: Vec2::ZERO,
                    size: Size::new(100.0, 100.0),
                },
                radius: Corners::all(8.0),
            }),
            quads: Span::new(0, 1),
            texts: Span::new(0, 0),
            meshes: Span::default(),
        },
        DrawGroup {
            scissor: Some(URect::new(0, 0, 100, 100)),
            rounded_clip: Some(RoundedClip {
                mask_rect: Rect {
                    min: Vec2::ZERO,
                    size: Size::new(100.0, 100.0),
                },
                radius: Corners::all(8.0),
            }),
            quads: Span::new(1, 1),
            texts: Span::new(0, 0),
            meshes: Span::default(),
        },
        // Group 2: different mask — full transition required.
        DrawGroup {
            scissor: Some(URect::new(0, 0, 100, 100)),
            rounded_clip: Some(RoundedClip {
                mask_rect: Rect {
                    min: Vec2::ZERO,
                    size: Size::new(50.0, 50.0),
                },
                radius: Corners::all(4.0),
            }),
            quads: Span::new(2, 1),
            texts: Span::new(0, 0),
            meshes: Span::default(),
        },
    ]);
    let mask_indices = &[Some(0u32), Some(0u32), Some(1u32)];
    assert_eq!(
        simplify(&collect(&buf, None, mask_indices, true)),
        vec![
            // Group 0: stamp mask 0.
            DrawOp::MaskWrite(0),
            DrawOp::Quads(0),
            // Group 1: same mask — no bracket, just draw.
            DrawOp::Quads(1),
            // Group 2: clear 0, stamp 1.
            DrawOp::MaskClear(0),
            DrawOp::MaskWrite(1),
            DrawOp::Quads(2),
        ],
    );
}

/// Pin: a stencil-pass group with text but no quads still emits the
/// mask write. Without it, the text would render unstenciled —
/// rounded clip would silently leak past the mask boundary.
#[test]
fn stencil_text_only_group_still_writes_mask() {
    let buf = buf_with(vec![DrawGroup {
        scissor: Some(URect::new(0, 0, 100, 100)),
        rounded_clip: Some(RoundedClip {
            mask_rect: Rect {
                min: Vec2::ZERO,
                size: Size::new(100.0, 100.0),
            },
            radius: Corners::all(8.0),
        }),
        quads: Span::new(0, 0),
        texts: Span::new(0, 1),
        meshes: Span::default(),
    }]);
    let mask_indices = &[Some(0u32)];
    assert_eq!(
        simplify(&collect(&buf, None, mask_indices, true)),
        vec![DrawOp::MaskWrite(0), DrawOp::Text(0)],
    );
}

// ---------- Fidelity over the granular RenderStep sequence ---------

/// Pin: under partial damage, the very first emitted step is
/// `SetScissor(damage_scissor)`, and the per-group `SetScissor`
/// narrows further. Confirms the schedule actually emits the scissor
/// transitions production code relies on.
#[test]
fn setscissor_steps_present_under_partial_damage() {
    let buf = buf_with(vec![DrawGroup {
        scissor: Some(URect::new(10, 10, 50, 50)),
        rounded_clip: None,
        quads: Span::new(0, 1),
        texts: Span::new(0, 0),
        meshes: Span::default(),
    }]);
    let damage = URect::new(0, 0, 80, 80);
    let steps = collect(&buf, Some(damage), &[], false);
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
            rounded_clip: None,
            quads: Span::new(0, 1),
            texts: Span::new(0, 0),
            meshes: Span::default(),
        },
        // Group 1: outside damage
        DrawGroup {
            scissor: Some(URect::new(60, 60, 30, 30)),
            rounded_clip: None,
            quads: Span::new(1, 1),
            texts: Span::new(0, 0),
            meshes: Span::default(),
        },
    ]);
    let damage = URect::new(0, 0, 40, 40);
    assert_eq!(
        simplify(&collect(&buf, Some(damage), &[], false)),
        vec![DrawOp::PreClear, DrawOp::Quads(0)],
    );
}

// ---------- Text-batch coalescing schedule emit positions ---------

/// Constructs a buffer with explicit `text_batches`, bypassing
/// `buf_with`'s one-batch-per-group default. Used by the batch-emit
/// tests to assert on coalesced batches the composer would have
/// produced.
fn buf_with_batches(groups: Vec<DrawGroup>, text_batches: Vec<TextBatch>) -> RenderBuffer {
    RenderBuffer {
        quads: vec![dummy_quad(); 4],
        texts: vec![dummy_text(); 4],
        meshes: MeshScene::default(),
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
                rounded_clip: None,
                quads: Span::new(0, 1),
                texts: Span::new(0, 1),
                meshes: Span::default(),
            },
            DrawGroup {
                scissor: None,
                rounded_clip: None,
                quads: Span::new(1, 1),
                texts: Span::new(1, 1),
                meshes: Span::default(),
            },
        ],
        vec![TextBatch {
            texts: Span::new(0, 2),
            last_group: 1,
        }],
    );
    assert_eq!(
        simplify(&collect(&buf, None, &[], false)),
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
                rounded_clip: None,
                quads: Span::new(0, 1),
                texts: Span::new(0, 1),
                meshes: Span::default(),
            },
            // Group 1: trailing quad-only group (different batch state).
            DrawGroup {
                scissor: None,
                rounded_clip: None,
                quads: Span::new(1, 1),
                texts: Span::new(0, 0),
                meshes: Span::default(),
            },
        ],
        vec![TextBatch {
            texts: Span::new(0, 1),
            last_group: 0,
        }],
    );
    assert_eq!(
        simplify(&collect(&buf, None, &[], false)),
        vec![DrawOp::Quads(0), DrawOp::Text(0), DrawOp::Quads(1)],
    );
}

/// Pin: a batch whose `last_group` falls in a damage-skipped group
/// must still render — earlier groups in the same batch may sit
/// inside the damage rect, and dropping the whole batch silently
/// removes their text. Glyphon clips per-`TextArea.bounds` + the
/// active scissor, so emitting late is paint-safe.
#[test]
fn text_batch_anchored_in_damage_skipped_group_still_emits() {
    // Two groups in distinct scissors. Both contribute text to one
    // batch (last_group = 1). Damage rect covers group 0's scissor
    // only, so group 1 is filtered out by the damage intersect.
    let buf = buf_with_batches(
        vec![
            DrawGroup {
                scissor: Some(URect::new(0, 0, 50, 50)),
                rounded_clip: None,
                quads: Span::new(0, 1),
                texts: Span::new(0, 1),
                meshes: Span::default(),
            },
            DrawGroup {
                scissor: Some(URect::new(60, 0, 40, 50)),
                rounded_clip: None,
                quads: Span::new(1, 1),
                texts: Span::new(1, 1),
                meshes: Span::default(),
            },
        ],
        vec![TextBatch {
            texts: Span::new(0, 2),
            last_group: 1,
        }],
    );
    // Damage rect: covers only group 0.
    let damage = URect::new(0, 0, 50, 50);
    let steps = simplify(&collect(&buf, Some(damage), &[], false));
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
                rounded_clip: None,
                quads: Span::new(0, 1),
                texts: Span::new(0, 1),
                meshes: Span::default(),
            },
            DrawGroup {
                // Final group, outside damage.
                scissor: Some(URect::new(60, 0, 40, 50)),
                rounded_clip: None,
                quads: Span::new(1, 1),
                texts: Span::new(1, 1),
                meshes: Span::default(),
            },
        ],
        vec![TextBatch {
            texts: Span::new(0, 2),
            last_group: 1,
        }],
    );
    let damage = URect::new(0, 0, 50, 50);
    let steps = simplify(&collect(&buf, Some(damage), &[], false));
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
                rounded_clip: None,
                quads: Span::new(0, 1),
                texts: Span::new(0, 1),
                meshes: Span::default(),
            },
            DrawGroup {
                scissor: None,
                rounded_clip: None,
                quads: Span::new(1, 1),
                texts: Span::new(1, 1),
                meshes: Span::default(),
            },
        ],
        vec![
            TextBatch {
                texts: Span::new(0, 1),
                last_group: 0,
            },
            TextBatch {
                texts: Span::new(1, 1),
                last_group: 1,
            },
        ],
    );
    assert_eq!(
        simplify(&collect(&buf, None, &[], false)),
        vec![
            DrawOp::Quads(0),
            DrawOp::Text(0),
            DrawOp::Quads(1),
            DrawOp::Text(1),
        ],
    );
}
