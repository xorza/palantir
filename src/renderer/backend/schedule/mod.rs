//! Per-frame render schedule — the ordered sequence of conceptual GPU
//! operations that paints every group in a `RenderBuffer`.
//!
//! Both production (`WgpuBackend::render_groups`) and unit tests
//! consume this same step stream via [`for_each_step`], so the order
//! asserted in tests can't drift from the order actually issued to
//! wgpu. Pure data — no GPU calls live here.

#[cfg(feature = "bench")]
pub(crate) mod bench;

use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::primitives::{color::Color, color::ColorF16};
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::batch::{GroupBatch, PaintTier, TextBatch};

/// Per-group and per-text-batch spans into the staged mask-quad buffer.
#[derive(Debug, Default)]
pub(super) struct MaskPlan {
    pub(super) groups: Vec<Span>,
    pub(super) batches: Vec<Span>,
}

/// Build the schedule's mask spans and deduplicated mask-quad instances.
pub(super) fn build_mask_plan(buffer: &RenderBuffer, plan: &mut MaskPlan, masks: &mut Vec<Quad>) {
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
        debug_assert!(
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
pub(super) enum RenderStep {
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
    /// from `RenderBuffer::batches(Mesh)[batch].items` (then via
    /// `RenderBuffer.meshes`). One `MeshBatch { batch }` step → one
    /// pipeline+buffer bind → N `draw_indexed` calls.
    MeshBatch { batch: usize },
    /// Bind the image pipeline + issue one `draw` per `ImageDraw` in
    /// the referenced batch. Consumer pulls per-draw handles from
    /// `RenderBuffer::batches(Image)[batch].items` (then via
    /// `RenderBuffer.images.draws`). The pipeline switches the per-image
    /// bind group between draws.
    ImageBatch { batch: usize },
    /// Bind the icon pipeline + issue one instanced draw covering every icon
    /// in the referenced batch. Every icon shares one atlas bind group, so a
    /// run of them is a single draw whatever mix of icons it holds.
    IconBatch { batch: usize },
    /// Bind the stroke pipeline + issue a single indexed instanced draw
    /// covering every `CurveInstance` in the referenced batch. One
    /// `CurveBatch { batch }` step → one bind → one `draw_indexed`. This
    /// is the "one draw call per scissor group" the architecture targets
    /// for native GPU strokes.
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
/// 2. Each group narrows the scissor to its `effective` rect before
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
/// 6. `SetScissor` and `SetStencilRef` are *transitions*, not
///    announcements: [`PassState`] emits one only when the requested
///    value differs from what the walk has already established, so the
///    rect a draw runs under is the last distinct one emitted before
///    it, not necessarily the step immediately preceding. The first
///    scissor of each walk always emits. Invariant 3's "clear under the
///    stamp-time scissor" therefore reads as *no intervening
///    `SetScissor`* between a `MaskClear` and the stamp's rect.
///
/// [`PreClear`]: RenderStep::PreClear
pub(super) fn for_each_step(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    masks: &MaskPlan,
    use_stencil: bool,
    mut emit: impl FnMut(RenderStep),
) {
    let full_viewport = URect::new(0, 0, buffer.viewport_phys.x, buffer.viewport_phys.y);
    let mut state = PassState {
        emit: &mut emit,
        use_stencil,
        cur_scissor: None,
        cur_ref: 0,
        active: None,
    };

    if let Some(scissor) = damage_scissor {
        state.scissor(scissor);
        state.push(RenderStep::PreClear);
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

    for (i, g) in buffer.groups.iter().enumerate() {
        // Silently drop mesh/image/curve batches that anchored in
        // earlier damage-skipped groups — they had no visible scissor
        // so their draws don't paint.
        for tier in PaintTier::ALL {
            advance_past_skipped(buffer.batches(tier), &mut cursors.higher[tier.idx()], i);
        }

        let group_scissor = g.scissor.unwrap_or(full_viewport);
        let effective = match damage_scissor {
            Some(d) => match group_scissor.intersect(d) {
                Some(r) => r,
                None => continue,
            },
            None => group_scissor,
        };
        if effective.is_paint_empty() {
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
            &mut state,
        );

        // A group can be content-less at walk time — its only text
        // coalesced into a batch draining at a later group. Skip the
        // scissor / chain establish entirely then: a scissor with no
        // draws is a dead command, and on the stencil path the
        // establish would stamp a whole mask chain for nothing (the
        // next consumer establishes its own state regardless).
        let has_content = g.quads.len != 0
            || pending_at(&buffer.text_batches, cursors.text, i)
            || PaintTier::ALL
                .iter()
                .any(|&t| pending_at(buffer.batches(t), cursors.higher[t.idx()], i));
        if has_content {
            state.narrow(&masks.groups, i, effective);
            emit_group_body(
                buffer,
                damage_scissor,
                i,
                effective,
                masks,
                &mut cursors,
                &mut state,
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
        &mut state,
    );
    // Tail clear: never let a stamped chain survive the walk. The pass
    // clears the stencil once, not per damage rect, and AA padding can
    // make nominally-disjoint rects' scissors overlap — residue here
    // would be read by the next rect's walk.
    state.clear_active();
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

/// The render-pass state one schedule walk has established, and the
/// single point every step is emitted from. Branch-specific code
/// *requests* the state its draws need ([`Self::narrow`],
/// [`Self::clear_active`]) without knowing what the previous branch
/// left behind; a request matching the tracked value emits nothing.
/// wgpu records every `set_scissor_rect` / `set_stencil_reference` as a
/// real command, so a group re-requesting the scissor it already holds
/// (its text drain never widened it) would pay for a no-op.
///
/// Deduplication is only sound because `SetScissor` / `SetStencilRef`
/// are the *only* steps that touch either piece of state — no draw arm
/// in `WgpuBackend::render_groups`, including the text backend's
/// `render_batch`, sets a scissor or stencil reference of its own.
///
/// Tracked per *walk*, not per pass: one pass runs a walk per damage
/// rect, so the first scissor request of every walk emits and no walk
/// inherits another rect's state. A walk always exits with no chain
/// stamped and ref 0 (`chain.len == 0` establishes reset the ref; the
/// tail [`Self::clear_active`] closes any stamped chain), which is what
/// lets those walks share a pass that clears the stencil once.
struct PassState<'a> {
    emit: &'a mut dyn FnMut(RenderStep),
    use_stencil: bool,
    cur_scissor: Option<URect>,
    cur_ref: u32,
    active: Option<ActiveMask>,
}

// Manual: `emit` is a `&mut dyn FnMut`, which has nothing to format.
impl std::fmt::Debug for PassState<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PassState")
            .field("use_stencil", &self.use_stencil)
            .field("cur_scissor", &self.cur_scissor)
            .field("cur_ref", &self.cur_ref)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl PassState<'_> {
    fn push(&mut self, step: RenderStep) {
        (self.emit)(step);
    }

    fn scissor(&mut self, rect: URect) {
        if self.cur_scissor != Some(rect) {
            self.push(RenderStep::SetScissor(rect));
            self.cur_scissor = Some(rect);
        }
    }

    fn stencil_ref(&mut self, v: u32) {
        if self.cur_ref != v {
            self.push(RenderStep::SetStencilRef(v));
            self.cur_ref = v;
        }
    }

    /// Bring the pass to "ready to draw the content of `chains[idx]`
    /// inside `scissor`". `chains` is indexed only on the stencil path —
    /// the non-stencil path runs with an empty [`MaskPlan`].
    fn narrow(&mut self, chains: &[Span], idx: usize, scissor: URect) {
        if self.use_stencil {
            self.establish(chains[idx], scissor);
        } else {
            self.scissor(scissor);
        }
    }

    /// Clear the stamped chain (if any) under its own stamp-time
    /// scissor: one draw of the outermost mask quad at ref 0.
    fn clear_active(&mut self) {
        if let Some(prev) = self.active.take() {
            self.scissor(prev.scissor);
            self.stencil_ref(0);
            self.push(RenderStep::MaskClear(prev.masks.start));
        }
    }

    /// Bring the stencil to "`chain` stamped under `scissor`, ref =
    /// depth" and narrow the pass scissor to `scissor`. Elides the
    /// clear + re-stamp when the same chain is already stamped and its
    /// stamp scissor covers `scissor` — a wider scissor exposes pixels
    /// the stamp never wrote, which would wrongly fail `Equal`.
    fn establish(&mut self, chain: Span, scissor: URect) {
        let keep = chain.len != 0
            && self.active.is_some_and(|prev| {
                prev.masks == chain && prev.scissor.intersect(scissor) == Some(scissor)
            });
        if keep {
            self.scissor(scissor);
            self.stencil_ref(chain.len);
            return;
        }
        self.clear_active();
        self.scissor(scissor);
        for level in 0..chain.len {
            self.stencil_ref(level);
            self.push(RenderStep::MaskStamp(chain.start + level));
        }
        self.stencil_ref(chain.len);
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
    /// One per [`PaintTier`], indexed by `PaintTier::idx`.
    higher: [usize; PaintTier::COUNT],
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
fn advance_past_skipped(batches: &[GroupBatch], cursor: &mut usize, before: usize) {
    while *cursor < batches.len() && batches[*cursor].last_group() < before {
        *cursor += 1;
    }
}

/// `true` if the batch at `cursor` anchors to group `group` — i.e. this
/// group has a pending batch of that kind to emit.
///
/// The one helper that is genuinely generic: text batches and
/// higher-kind batches are different types that share this anchoring
/// rule, which is what [`PerGroupBatch`] exists for.
fn pending_at<B: PerGroupBatch>(batches: &[B], cursor: usize, group: usize) -> bool {
    cursor < batches.len() && batches[cursor].last_group() == group
}

/// Drain every batch anchored to group `group`, emitting `step(idx)`
/// for the batch's render step. The caller has already narrowed the
/// scissor (and stencil state) back to the group's own. One call per
/// [`PaintTier`], so every tier's per-group emit shape is this one.
fn drain_group_batches(
    batches: &[GroupBatch],
    cursor: &mut usize,
    group: usize,
    mut step: impl FnMut(usize) -> RenderStep,
    state: &mut PassState,
) {
    while pending_at(batches, *cursor, group) {
        state.push(step(*cursor));
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
fn drain_text_batches(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    target: usize,
    cursor: &mut usize,
    masks: &MaskPlan,
    state: &mut PassState,
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
        if !s.is_paint_empty() {
            state.narrow(&masks.batches, *cursor, s);
            state.push(RenderStep::Text { batch: *cursor });
        }
        *cursor += 1;
    }
}

/// The draws every non-skipped group emits, identical under both the
/// stencil and non-stencil paths: the group's quads, then its text
/// batches (drained after the quads so a child quad occludes a label),
/// then its mesh / image / curve batches — after re-requesting the
/// group's own scissor + stencil state, since the text drain may have
/// widened the scissor or restamped a different chain. Shared by the
/// stencil and non-stencil paths so the two can't drift; the caller
/// gates it on the group having any content.
fn emit_group_body(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    i: usize,
    effective: URect,
    masks: &MaskPlan,
    cursors: &mut ScheduleCursors,
    state: &mut PassState,
) {
    let quads = buffer.groups[i].quads;
    if quads.len != 0 {
        state.push(RenderStep::Quads { range: quads });
    }
    drain_text_batches(
        buffer,
        damage_scissor,
        i + 1,
        &mut cursors.text,
        masks,
        state,
    );
    if !PaintTier::ALL
        .iter()
        .any(|&t| pending_at(buffer.batches(t), cursors.higher[t.idx()], i))
    {
        return;
    }
    // Restore the group's own state: the text drain above may have
    // widened the scissor or restamped a different chain. Both requests
    // collapse to nothing when it didn't — the common case, since most
    // groups with a higher-kind batch carry no text at all.
    state.narrow(&masks.groups, i, effective);
    // Paint order is `PaintTier::ALL`'s order, which is `Ord`'s — the
    // property the composer's flush arbitration rests on.
    for tier in PaintTier::ALL {
        drain_group_batches(
            buffer.batches(tier),
            &mut cursors.higher[tier.idx()],
            i,
            |batch| match tier {
                PaintTier::Mesh => RenderStep::MeshBatch { batch },
                PaintTier::Image => RenderStep::ImageBatch { batch },
                PaintTier::Icon => RenderStep::IconBatch { batch },
                PaintTier::Curve => RenderStep::CurveBatch { batch },
            },
            state,
        );
    }
}

// `bench` only, not `any(test, …)`: the sole consumer is the
// `schedule` benchmark, which that feature gates too.
#[cfg(feature = "bench")]
pub(crate) mod internals {
    use super::*;

    /// What one schedule walk emitted: the step total plus the two
    /// pass-state transition counts [`PassState`] deduplicates. Counts
    /// explain a benchmark result — they don't replace its wall time.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct WalkCounts {
        pub(crate) steps: usize,
        pub(crate) scissors: usize,
        pub(crate) stencil_refs: usize,
    }

    /// Schedule-walk harness for the `schedule` benchmark: stages the
    /// mask plan once up front so an iteration measures only
    /// [`for_each_step`].
    #[derive(Debug, Default)]
    pub(crate) struct Walk {
        plan: MaskPlan,
        masks: Vec<Quad>,
    }

    impl Walk {
        pub(crate) fn new(buffer: &RenderBuffer) -> Self {
            let mut walk = Self::default();
            build_mask_plan(buffer, &mut walk.plan, &mut walk.masks);
            walk
        }

        pub(crate) fn run(
            &self,
            buffer: &RenderBuffer,
            damage: Option<URect>,
            use_stencil: bool,
        ) -> WalkCounts {
            let mut counts = WalkCounts::default();
            for_each_step(buffer, damage, &self.plan, use_stencil, |step| {
                counts.steps += 1;
                match step {
                    RenderStep::SetScissor(_) => counts.scissors += 1,
                    RenderStep::SetStencilRef(_) => counts.stencil_refs += 1,
                    _ => {}
                }
            });
            counts
        }
    }
}
