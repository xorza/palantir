//! Per-frame render schedule — the ordered sequence of conceptual GPU
//! operations that paints every group in a `RenderBuffer`.
//!
//! Both production (`WgpuBackend::render_groups`) and unit tests
//! consume this same step stream via [`for_each_step`], so the order
//! asserted in tests can't drift from the order actually issued to
//! wgpu. Pure data — no GPU calls live here.

use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::primitives::{color::Color, color::ColorF16};
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::batch::{GroupBatch, TextBatch};

/// Per-group and per-text-batch spans into the staged mask-quad buffer.
#[derive(Debug, Default)]
pub(crate) struct MaskPlan {
    pub(crate) groups: Vec<Span>,
    pub(crate) batches: Vec<Span>,
}

/// Build the schedule's mask spans and deduplicated mask-quad instances.
pub(crate) fn build_mask_plan(buffer: &RenderBuffer, plan: &mut MaskPlan, masks: &mut Vec<Quad>) {
    plan.groups.clear();
    plan.batches.clear();
    masks.clear();
    let clips = &buffer.rounded_clips;
    let mut previous_chain = Span::default();
    let mut previous_masks = Span::default();
    for group in &buffer.groups {
        let chain = group.rounded_clips;
        let mask_span = if group.scissor.is_some() && chain.len != 0 {
            if clips[chain.range()] == clips[previous_chain.range()] {
                previous_masks
            } else {
                let start = masks.len() as u32;
                for clip in &clips[chain.range()] {
                    masks.push(Quad {
                        rect: clip.mask_rect,
                        fill: Color::default().into(),
                        corners: clip.corners,
                        stroke_color: ColorF16::TRANSPARENT,
                        stroke_width: 0.0,
                        ..Default::default()
                    });
                }
                Span::new(start, chain.len)
            }
        } else {
            Span::default()
        };
        previous_chain = if mask_span.len != 0 {
            chain
        } else {
            Span::default()
        };
        previous_masks = mask_span;
        plan.groups.push(mask_span);
    }
    for batch in &buffer.text_batches {
        let group = batch.last_group as usize;
        assert!(
            clips[batch.rounded_clips.range()] == clips[buffer.groups[group].rounded_clips.range()],
            "text batch chain decorrelated from its last_group's chain"
        );
        plan.batches.push(plan.groups[group]);
    }
}

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
    /// Set the stencil reference value (stencil-path frames only):
    /// the chain depth for content draws (`Equal(depth)` passes only
    /// inside every stamped mask), level `k` before stamping mask
    /// level `k`, and `0` before a mask clear (`Replace` writes the
    /// reference). Elided when the pass already holds the value.
    SetStencilRef(u32),
    /// Bind the mask-stamp pipeline (`Equal` + `IncrementClamp`) +
    /// draw the mask quad at this index: writes `ref + 1` where the
    /// SDF passes and the stencil already equals the reference — one
    /// nesting level per draw, so a chain stamps outer→inner with the
    /// reference stepping 0, 1, ….
    MaskStamp(u32),
    /// Bind the mask-clear pipeline (`Always` + `Replace`, at ref 0) +
    /// draw the mask quad at this index. One draw of a chain's
    /// *outermost* quad resets the whole chain — inner stamps only
    /// ever incremented inside the outer's SDF.
    MaskClear(u32),
    /// Bind the quad pipeline (stencil-test variant when stencil is
    /// active, plain otherwise) + draw the group's quad range.
    Quads { range: Span },
    /// Render a coalesced text batch via the text-renderer pool slot.
    /// Emitted once per batch, immediately after the last group in
    /// the batch has drawn its quads (any meshes in that group still
    /// follow). One `Text { batch }` step → one text-backend render →
    /// one wgpu draw call covering every run in the batch.
    Text { batch: usize },
    /// Bind the mesh pipeline + issue one `draw_indexed` per
    /// `MeshDraw` in the referenced batch. Consumer pulls per-draw spans
    /// from `RenderBuffer.mesh_batches[batch].items` (then via
    /// `RenderBuffer.meshes`). One `MeshBatch { batch }` step → one
    /// pipeline+buffer bind → N `draw_indexed` calls.
    MeshBatch { batch: usize },
    /// Bind the image pipeline + issue one `draw` per `ImageDraw` in
    /// the referenced batch. Consumer pulls per-draw handles from
    /// `RenderBuffer.image_batches[batch].items` (then via
    /// `RenderBuffer.images.draws`). The pipeline switches the per-image
    /// bind group between draws.
    ImageBatch { batch: usize },
    /// Bind the stroke pipeline + issue a single non-indexed instanced
    /// draw covering every `CurveInstance` in the referenced batch
    /// (the vertex shader expands each instance's quads from
    /// `vertex_index` — no index buffer). One `CurveBatch { batch }`
    /// step → one bind → one `draw`. This is the "one draw call per
    /// scissor group" the architecture targets for native GPU strokes.
    CurveBatch { batch: usize },
}

/// Walk `buffer.groups` and emit one [`RenderStep`] at a time via
/// `emit`. Pure logic — no GPU calls.
///
/// `masks` holds the per-group and per-text-batch mask-quad chains
/// (see [`MaskPlan`]), built during quad mask staging.
/// Ignored when `use_stencil` is `false`.
///
/// Per-frame ordering invariants pinned by the emitted sequence:
///
/// 1. When `damage_scissor` is `Some`, the very first emitted steps
///    are `SetScissor(damage_scissor)` then [`PreClear`] — before
///    any group draws. AA-fringe drift would otherwise accumulate.
/// 2. Each group narrows the scissor (`SetScissor(effective)`) before
///    issuing its own draws.
/// 3. Stencil-path groups establish their mask chain before their
///    draws: each chain level stamps at `stencil_ref = level`
///    (`Equal` + `IncrementClamp`, so level `k` writes `k + 1` only
///    inside its ancestors), then content draws at
///    `stencil_ref = depth`. A stale chain clears with ONE draw of
///    its outermost mask quad at ref 0, replayed under the
///    *stamp-time* scissor before the next `SetScissor` — a clear
///    under the next scissor would miss stamped pixels wherever the
///    two scissors differ. Groups sharing the still-stamped chain
///    (with a scissor inside the stamp's) elide the clear + re-stamp
///    pair. A walk never ends with a chain stamped: a tail clear runs
///    after the last group, because the pass clears the stencil once
///    (not per damage rect) and AA padding can make nominally-disjoint
///    rects' scissors overlap, so residue would leak into the next
///    rect's walk.
/// 4. Text always renders *after* its group's quads so a child quad
///    declared after a label correctly occludes that label. A batch
///    drained past damage-skipped groups first establishes *its own*
///    chain (same clear / stamp / elision rules as a group), so its
///    text can't stencil-test against a foreign mask; the group that
///    follows re-establishes its own state.
/// 5. Groups whose effective scissor is empty (or doesn't intersect
///    `damage_scissor`) emit no steps at all.
///
/// [`PreClear`]: RenderStep::PreClear
pub(crate) fn for_each_step(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    masks: &MaskPlan,
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
    // group's setup, drain any batches whose `last_group < i`: emit
    // them now (paint-safe — the composer's overlap rule guarantees
    // no quad in `(last_group, i)` overlapped them, and any of those
    // skipped groups' quads don't paint this pass). A trailing drain
    // after the loop catches batches anchored in tail-skipped groups.
    // Each drained batch establishes its own mask chain, so drained
    // text never stencil-tests against whatever chain the walk left
    // stamped.
    let mut cursors = ScheduleCursors::default();
    let mut stencil = StencilTracker::default();

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
        // Drained first so a batch sharing the still-stamped chain
        // elides its stamp; the group establish below then clears /
        // restamps as its own chain requires.
        drain_text_batches(
            buffer,
            damage_scissor,
            i,
            &mut cursors.text,
            masks,
            use_stencil,
            &mut stencil,
            &mut emit,
        );

        // A group can be content-less at walk time — its only text
        // coalesced into a batch draining at a later group. Skip the
        // scissor / chain establish entirely then: a scissor with no
        // draws is a dead command, and on the stencil path the
        // establish would stamp a whole mask chain for nothing (the
        // next consumer establishes its own state regardless).
        let has_content = g.quads.len != 0
            || pending_at(&buffer.text_batches, cursors.text, i)
            || pending_at(&buffer.mesh_batches, cursors.mesh, i)
            || pending_at(&buffer.image_batches, cursors.image, i)
            || pending_at(&buffer.curve_batches, cursors.curve, i);
        if has_content {
            if use_stencil {
                stencil.establish(masks.groups[i], effective, &mut emit);
            } else {
                emit(RenderStep::SetScissor(effective));
            }
            emit_group_body(
                buffer,
                damage_scissor,
                i,
                effective,
                masks,
                use_stencil,
                &mut cursors,
                &mut stencil,
                &mut emit,
            );
        }
    }
    // Trailing drain — batches anchored in tail-skipped groups. Runs
    // BEFORE the tail clear so a batch whose chain is still stamped
    // elides, and a foreign one establishes its own.
    drain_text_batches(
        buffer,
        damage_scissor,
        usize::MAX,
        &mut cursors.text,
        masks,
        use_stencil,
        &mut stencil,
        &mut emit,
    );
    // Tail clear: never let a stamped chain survive the walk. The pass
    // clears the stencil once, not per damage rect, and AA padding can
    // make nominally-disjoint rects' scissors overlap — residue here
    // would be read by the next rect's walk.
    stencil.clear_active(&mut emit);
}

/// A stamped stencil chain: the mask quads stamped (outer→inner — the
/// stencil holds `k + 1` inside chain level `k`) plus the scissor
/// active when it was stamped. The clear must replay under that same
/// scissor — a clear under any later scissor misses stamped pixels
/// wherever the two differ.
#[derive(Clone, Copy, Debug)]
struct ActiveMask {
    masks: Span,
    scissor: URect,
}

/// Stencil bookkeeping for one schedule walk: the stamped chain (if
/// any) plus the stencil reference last emitted. A walk always exits
/// clean (no chain stamped, ref 0), so consecutive per-damage-rect
/// walks within one pass — which clears the stencil once — each start
/// consistent with the true stencil contents.
#[derive(Debug, Default)]
struct StencilTracker {
    active: Option<ActiveMask>,
    cur_ref: u32,
}

impl StencilTracker {
    fn set_ref(&mut self, v: u32, emit: &mut dyn FnMut(RenderStep)) {
        if self.cur_ref != v {
            emit(RenderStep::SetStencilRef(v));
            self.cur_ref = v;
        }
    }

    /// Clear the stamped chain (if any) under its own stamp-time
    /// scissor: one draw of the outermost mask quad at ref 0.
    fn clear_active(&mut self, emit: &mut dyn FnMut(RenderStep)) {
        if let Some(prev) = self.active.take() {
            emit(RenderStep::SetScissor(prev.scissor));
            self.set_ref(0, emit);
            emit(RenderStep::MaskClear(prev.masks.start));
        }
    }

    /// Bring the stencil to "`chain` stamped under `scissor`, ref =
    /// depth" and narrow the pass scissor to `scissor`. Elides the
    /// clear + re-stamp when the same chain is already stamped and its
    /// stamp scissor covers `scissor` — a wider scissor exposes pixels
    /// the stamp never wrote, which would wrongly fail `Equal`.
    fn establish(&mut self, chain: Span, scissor: URect, emit: &mut dyn FnMut(RenderStep)) {
        let keep = chain.len != 0
            && self.active.is_some_and(|prev| {
                prev.masks == chain && prev.scissor.intersect(scissor) == Some(scissor)
            });
        if keep {
            emit(RenderStep::SetScissor(scissor));
            self.set_ref(chain.len, emit);
            return;
        }
        self.clear_active(emit);
        emit(RenderStep::SetScissor(scissor));
        for level in 0..chain.len {
            self.set_ref(level, emit);
            emit(RenderStep::MaskStamp(chain.start + level));
        }
        self.set_ref(chain.len, emit);
        if chain.len != 0 {
            self.active = Some(ActiveMask {
                masks: chain,
                scissor,
            });
        }
    }
}

/// Per-kind walk cursors for [`for_each_step`]. Each field is the index
/// of the next unconsumed batch of that kind; the cursors only advance
/// (batches are emitted in `last_group` order), so the whole walk is
/// linear in the batch count.
#[derive(Debug, Default)]
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
impl PerGroupBatch for GroupBatch {
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

/// Drain every batch anchored to group `group`, emitting `step(idx)`
/// for the batch's render step. The caller has already narrowed the
/// scissor (and stencil state) back to the group's own. Shared by the
/// mesh / image / curve drains so their per-group emit shape can't
/// drift.
fn drain_group_batches<B: PerGroupBatch>(
    batches: &[B],
    cursor: &mut usize,
    group: usize,
    mut step: impl FnMut(usize) -> RenderStep,
    emit: &mut dyn FnMut(RenderStep),
) {
    while pending_at(batches, *cursor, group) {
        emit(step(*cursor));
        *cursor += 1;
    }
}

/// Drain every text batch whose `last_group < target`, emitting each
/// with its own bounds-union scissor (intersected with the damage
/// region) so the text backend's missing per-fragment x-clip doesn't
/// leak glyphs past a clipped owner's scissor (e.g. into a scrollbar
/// gutter). On the stencil path each batch also establishes its own
/// mask chain first — same clear / stamp / elision rules as a group —
/// so text drained past damage-skipped groups never stencil-tests
/// against a foreign mask. `target = i` drains stuck batches before
/// group `i`'s emits; `target = i + 1` drains the in-flight group's
/// own batches after its quads; `target = usize::MAX` drains tail
/// batches anchored in skipped groups.
#[allow(clippy::too_many_arguments)]
fn drain_text_batches(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    target: usize,
    cursor: &mut usize,
    masks: &MaskPlan,
    use_stencil: bool,
    stencil: &mut StencilTracker,
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
            if use_stencil {
                stencil.establish(masks.batches[*cursor], s, emit);
            } else {
                emit(RenderStep::SetScissor(s));
            }
            emit(RenderStep::Text { batch: *cursor });
        }
        *cursor += 1;
    }
}

/// The draws every non-skipped group emits, identical under both the
/// stencil and non-stencil paths: the group's quads, then its text
/// batches (drained after the quads so a child quad occludes a label),
/// then its mesh / image / curve batches — after restoring the group's
/// own scissor + stencil state, since the text drain may have widened
/// the scissor or restamped a different chain. The stencil path wraps
/// this with the chain establish; the non-stencil path gates it on the
/// group having any content. Shared so the two can't drift.
#[allow(clippy::too_many_arguments)]
fn emit_group_body(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    i: usize,
    effective: URect,
    masks: &MaskPlan,
    use_stencil: bool,
    cursors: &mut ScheduleCursors,
    stencil: &mut StencilTracker,
    emit: &mut dyn FnMut(RenderStep),
) {
    let quads = buffer.groups[i].quads;
    if quads.len != 0 {
        emit(RenderStep::Quads { range: quads });
    }
    drain_text_batches(
        buffer,
        damage_scissor,
        i + 1,
        &mut cursors.text,
        masks,
        use_stencil,
        stencil,
        emit,
    );
    if !(pending_at(&buffer.mesh_batches, cursors.mesh, i)
        || pending_at(&buffer.image_batches, cursors.image, i)
        || pending_at(&buffer.curve_batches, cursors.curve, i))
    {
        return;
    }
    if use_stencil {
        stencil.establish(masks.groups[i], effective, emit);
    } else {
        emit(RenderStep::SetScissor(effective));
    }
    drain_group_batches(
        &buffer.mesh_batches,
        &mut cursors.mesh,
        i,
        |batch| RenderStep::MeshBatch { batch },
        emit,
    );
    drain_group_batches(
        &buffer.image_batches,
        &mut cursors.image,
        i,
        |batch| RenderStep::ImageBatch { batch },
        emit,
    );
    drain_group_batches(
        &buffer.curve_batches,
        &mut cursors.curve,
        i,
        |batch| RenderStep::CurveBatch { batch },
        emit,
    );
}
