//! Per-frame render schedule — the ordered sequence of conceptual GPU
//! operations that paints every group in a `RenderBuffer`.
//!
//! Both production (`WgpuBackend::render_groups`) and unit tests
//! consume this same step stream via [`for_each_step`], so the order
//! asserted in tests can't drift from the order actually issued to
//! wgpu. Pure data — no GPU calls live here.

use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::{CurveBatch, ImageBatch, MeshBatch, RenderBuffer, TextBatch};

/// One conceptual step of the per-frame render schedule. Variants
/// describe *what* to do, not *how*; the consumer holds context
/// (`use_stencil`, the actual `RenderPass`) to translate each into
/// wgpu calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderStep {
    /// Pre-clear quad inside the damage scissor: paints the clear
    /// color (alpha 1) over last frame's pixels so AA fringes don't
    /// compound across animation frames. Emitted only when
    /// `damage_scissor` is `Some`.
    PreClear,
    /// Narrow the render-pass scissor to this physical-px rect.
    /// Emitted both for per-group narrowing and for text-scissor
    /// expansion mid-group.
    SetScissor(URect),
    /// Set the stencil reference value (stencil-path frames only).
    /// `1` for masked-region writes, `0` for non-rounded groups and
    /// the post-draw mask clear.
    SetStencilRef(u32),
    /// Bind the mask-write pipeline + draw the mask quad at this
    /// index. Used both for the pre-draw mask stamp (at ref `1`) and
    /// the post-draw mask clear (at ref `0`) — same GPU op, different
    /// surrounding stencil refs.
    MaskQuad(u32),
    /// Bind the quad pipeline (stencil-test variant when stencil is
    /// active, plain otherwise) + draw the group's quad range.
    Quads { group: usize, range: Span },
    /// Render a coalesced text batch via the text-renderer pool slot.
    /// Emitted once per batch, immediately after the last group in
    /// the batch has drawn its quads (any meshes in that group still
    /// follow). One `Text { batch }` step → one text-backend render →
    /// one wgpu draw call covering every run in the batch.
    Text { batch: usize },
    /// Bind the mesh pipeline + issue one `draw_indexed` per
    /// `MeshDraw` in the referenced batch. Consumer pulls per-draw spans
    /// from `RenderBuffer.mesh_batches[batch].meshes` (then via
    /// `RenderBuffer.meshes`). One `MeshBatch { batch }` step → one
    /// pipeline+buffer bind → N `draw_indexed` calls.
    MeshBatch { batch: usize },
    /// Bind the image pipeline + issue one `draw` per `ImageDraw` in
    /// the referenced batch. Consumer pulls per-draw handles from
    /// `RenderBuffer.image_batches[batch].images` (then via
    /// `RenderBuffer.images.draws`). The pipeline switches the per-image
    /// bind group between draws.
    ImageBatch { batch: usize },
    /// Bind the curve pipeline + issue a single indexed-instanced draw
    /// covering every `CurveInstance` in the referenced batch. One
    /// `CurveBatch { batch }` step → one bind → one `draw_indexed`.
    /// This is the "one draw call per scissor group" the architecture
    /// targets for native GPU curves.
    CurveBatch { batch: usize },
}

/// Walk `buffer.groups` and emit one [`RenderStep`] at a time via
/// `emit`. Pure logic — no GPU calls.
///
/// `mask_indices` parallels `buffer.groups`; index `i`'s `Some(j)`
/// says group `i`'s rounded-clip mask is mask quad `j` in the upload
/// buffer. Ignored when `use_stencil` is `false`.
///
/// Per-frame ordering invariants pinned by the emitted sequence:
///
/// 1. When `damage_scissor` is `Some`, the very first emitted steps
///    are `SetScissor(damage_scissor)` then [`PreClear`] — before
///    any group draws. AA-fringe drift would otherwise accumulate.
/// 2. Each group narrows the scissor (`SetScissor(effective)`) before
///    issuing its own draws.
/// 3. Stencil-path groups bracket their draws with mask write at
///    `stencil_ref = 1` and mask clear at `stencil_ref = 0`, so each
///    group sees a clean stencil regardless of clip ordering — except
///    when consecutive groups share the same mask, where the prior
///    group's tail clear and the new group's prologue write cancel
///    out and both are elided. The pass-final stencil is dropped
///    (`StoreOp::Discard`), so leaving a mask stamped at end of run
///    is correctness-neutral.
/// 4. Text always renders *after* its group's quads so a child quad
///    declared after a label correctly occludes that label.
/// 5. Groups whose effective scissor is empty (or doesn't intersect
///    `damage_scissor`) emit no steps at all.
///
/// [`PreClear`]: RenderStep::PreClear
pub(crate) fn for_each_step(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    mask_indices: &[Option<u32>],
    use_stencil: bool,
    mut emit: impl FnMut(RenderStep),
) {
    let full_viewport = URect::new(0, 0, buffer.viewport_phys.x, buffer.viewport_phys.y);

    if let Some(scissor) = damage_scissor {
        emit(RenderStep::SetScissor(scissor));
        emit(RenderStep::PreClear);
    }

    // Per-kind walk cursors (see [`ScheduleCursors`]). Text batches map
    // to a group via `last_group`; the schedule emits `RenderStep::Text`
    // when the walk reaches that group (after its quads, before its
    // meshes). `last_group` values are monotonically increasing across
    // batches (composer pushes in order), so one cursor per kind
    // suffices instead of a per-group scan.
    //
    // **Damage-pass drain.** A batch whose `last_group` falls in a
    // damage-skipped group must still render — earlier groups in the
    // batch may sit inside the damage rect, and dropping the whole
    // batch would silently erase their text. So before each rendered
    // group's quads, drain any batches whose `last_group < i`: emit
    // them now (paint-safe — the composer's overlap rule guarantees
    // no quad in `(last_group, i)` overlapped them, and any of those
    // skipped groups' quads don't paint this pass). A trailing drain
    // after the loop catches batches anchored in tail-skipped groups.
    // Stencil limitation: under rounded clip the drained batch's mask
    // may differ from the active mask at the drain point — the text
    // will stencil-clip against the wrong mask. Accepted: rare combo.
    let mut cursors = ScheduleCursors::default();

    // `Some(mi)` means the stencil currently has mask `mi` stamped
    // (ref=1 inside the SDF, 0 outside). `None` means stencil is
    // clean and ref=0. Updated only when a group actually emits;
    // groups skipped for zero area / no damage intersect leave it
    // alone, so dedup spans across them.
    let mut active_mask: Option<u32> = None;

    for (i, g) in buffer.groups.iter().enumerate() {
        // Silently drop mesh/image/curve batches that anchored in
        // earlier damage-skipped groups — they had no visible scissor
        // so their draws don't paint.
        advance_past_skipped(&buffer.mesh_batches, &mut cursors.mesh, i);
        advance_past_skipped(&buffer.image_batches, &mut cursors.image, i);
        advance_past_skipped(&buffer.curve_batches, &mut cursors.curve, i);

        let group_scissor = g.scissor.unwrap_or(full_viewport);
        let effective = match damage_scissor {
            Some(d) => match group_scissor.intersect(d) {
                Some(r) => r,
                None => continue,
            },
            None => group_scissor,
        };
        if effective.w == 0 || effective.h == 0 {
            continue;
        }
        // Drain batches stuck behind earlier damage-skipped groups
        // BEFORE this group's own setup, so the next quad/meshes
        // emitted (in this group) can paint over the drained text.
        drain_text_batches(buffer, damage_scissor, i, &mut cursors.text, &mut emit);
        emit(RenderStep::SetScissor(effective));

        if use_stencil {
            let mask_idx = mask_indices[i];
            match (active_mask, mask_idx) {
                // Same mask still stamped from a prior group: skip
                // both its tail clear and this prologue write. Ref
                // is still 1 from that write.
                (Some(prev), Some(curr)) if prev == curr => {}
                // Stencil dirty from a prior mask: clear it. If this
                // group has its own mask, stamp that next.
                (Some(prev), _) => {
                    emit(RenderStep::SetStencilRef(0));
                    emit(RenderStep::MaskQuad(prev));
                    if let Some(curr) = mask_idx {
                        emit(RenderStep::SetStencilRef(1));
                        emit(RenderStep::MaskQuad(curr));
                    }
                }
                (None, Some(curr)) => {
                    emit(RenderStep::SetStencilRef(1));
                    emit(RenderStep::MaskQuad(curr));
                }
                (None, None) => {
                    emit(RenderStep::SetStencilRef(0));
                }
            }
            emit_group_body(
                buffer,
                damage_scissor,
                i,
                effective,
                g.quads,
                &mut cursors,
                &mut emit,
            );
            active_mask = mask_idx;
        } else if g.quads.len != 0
            || pending_at(&buffer.text_batches, cursors.text, i)
            || pending_at(&buffer.mesh_batches, cursors.mesh, i)
            || pending_at(&buffer.image_batches, cursors.image, i)
            || pending_at(&buffer.curve_batches, cursors.curve, i)
        {
            emit_group_body(
                buffer,
                damage_scissor,
                i,
                effective,
                g.quads,
                &mut cursors,
                &mut emit,
            );
        }
    }
    // Trailing drain — batches anchored in tail-skipped groups.
    drain_text_batches(
        buffer,
        damage_scissor,
        usize::MAX,
        &mut cursors.text,
        &mut emit,
    );
}

/// Per-kind walk cursors for [`for_each_step`]. Each field is the index
/// of the next unconsumed batch of that kind; the cursors only advance
/// (batches are emitted in `last_group` order), so the whole walk is
/// linear in the batch count.
#[derive(Default)]
struct ScheduleCursors {
    text: usize,
    mesh: usize,
    image: usize,
    curve: usize,
}

/// A batch that anchors to a single draw group via its `last_group`
/// index. Lets the advance / drain / pending helpers operate uniformly
/// over the four batch kinds.
trait PerGroupBatch {
    fn last_group(&self) -> usize;
}

impl PerGroupBatch for TextBatch {
    fn last_group(&self) -> usize {
        self.last_group as usize
    }
}
impl PerGroupBatch for MeshBatch {
    fn last_group(&self) -> usize {
        self.last_group as usize
    }
}
impl PerGroupBatch for ImageBatch {
    fn last_group(&self) -> usize {
        self.last_group as usize
    }
}
impl PerGroupBatch for CurveBatch {
    fn last_group(&self) -> usize {
        self.last_group as usize
    }
}

/// Advance `cursor` past every batch whose `last_group` falls before
/// group `before` — they anchored in damage-skipped groups and don't
/// paint this pass.
fn advance_past_skipped<B: PerGroupBatch>(batches: &[B], cursor: &mut usize, before: usize) {
    while *cursor < batches.len() && batches[*cursor].last_group() < before {
        *cursor += 1;
    }
}

/// `true` if the batch at `cursor` anchors to group `group` — i.e. this
/// group has a pending batch of that kind to emit.
fn pending_at<B: PerGroupBatch>(batches: &[B], cursor: usize, group: usize) -> bool {
    cursor < batches.len() && batches[cursor].last_group() == group
}

/// Drain every batch anchored to group `group`, re-narrowing the scissor
/// to `effective` before each (the text drain may have widened it) and
/// emitting `step(idx)` for the batch's render step. Shared by the
/// mesh / image / curve drains so their per-group emit shape can't drift.
fn drain_group_batches<B: PerGroupBatch>(
    batches: &[B],
    cursor: &mut usize,
    group: usize,
    effective: URect,
    mut step: impl FnMut(usize) -> RenderStep,
    emit: &mut dyn FnMut(RenderStep),
) {
    while pending_at(batches, *cursor, group) {
        emit(RenderStep::SetScissor(effective));
        emit(step(*cursor));
        *cursor += 1;
    }
}

/// Drain every text batch whose `last_group < target`, emitting each
/// with its own bounds-union scissor (intersected with the damage
/// region) so the text backend's missing per-fragment x-clip doesn't
/// leak glyphs past a clipped owner's scissor (e.g. into a scrollbar
/// gutter). `target = i` drains stuck batches before group `i`'s emits;
/// `target = i + 1` drains the in-flight group's own batches after its
/// quads; `target = usize::MAX` drains tail batches anchored in skipped
/// groups.
fn drain_text_batches(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    target: usize,
    cursor: &mut usize,
    emit: &mut dyn FnMut(RenderStep),
) {
    while *cursor < buffer.text_batches.len() && buffer.text_batches[*cursor].last_group() < target
    {
        let s = match damage_scissor {
            Some(d) => buffer.text_batches[*cursor]
                .scissor
                .intersect(d)
                .unwrap_or_default(),
            None => buffer.text_batches[*cursor].scissor,
        };
        if s.w != 0 && s.h != 0 {
            emit(RenderStep::SetScissor(s));
            emit(RenderStep::Text { batch: *cursor });
        }
        *cursor += 1;
    }
}

/// The draws every non-skipped group emits, identical under both the
/// stencil and non-stencil paths: the group's quads, then its text
/// batches (drained after the quads so a child quad occludes a label),
/// then its mesh / image / curve batches (each restoring the group's own
/// scissor in case the text drain widened it). The stencil path wraps
/// this with the mask bracket; the non-stencil path gates it on the group
/// having any content. Shared so the two can't drift.
fn emit_group_body(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    i: usize,
    effective: URect,
    quads: Span,
    cursors: &mut ScheduleCursors,
    emit: &mut dyn FnMut(RenderStep),
) {
    if quads.len != 0 {
        emit(RenderStep::Quads {
            group: i,
            range: quads,
        });
    }
    drain_text_batches(buffer, damage_scissor, i + 1, &mut cursors.text, emit);
    drain_group_batches(
        &buffer.mesh_batches,
        &mut cursors.mesh,
        i,
        effective,
        |batch| RenderStep::MeshBatch { batch },
        emit,
    );
    drain_group_batches(
        &buffer.image_batches,
        &mut cursors.image,
        i,
        effective,
        |batch| RenderStep::ImageBatch { batch },
        emit,
    );
    drain_group_batches(
        &buffer.curve_batches,
        &mut cursors.curve,
        i,
        effective,
        |batch| RenderStep::CurveBatch { batch },
        emit,
    );
}
