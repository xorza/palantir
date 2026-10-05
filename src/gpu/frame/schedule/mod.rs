//! Per-frame render schedule — the ordered sequence of conceptual GPU
//! operations that paints every group in a `RenderBuffer`.
//!
//! Both production (`WgpuBackend::render_groups`) and unit tests
//! consume this same step stream via [`for_each_step`], so the order
//! asserted in tests can't drift from the order actually issued to
//! wgpu. Pure data — no GPU calls live here.

#[cfg(feature = "bench")]
pub(crate) mod bench;

use crate::common::span::Span;
use crate::primitives::geometry::urect::URect;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::group_batch::GroupBatch;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::per_group_batch::PerGroupBatch;
use std::fmt;

/// One clip chain staged into the mask-quad buffer: the source span it
/// was read from, and the run it was written to.
#[derive(Clone, Copy, Debug)]
struct StagedChain {
    chain: Span,
    masks: Span,
}

/// Per-group and per-text-batch spans, and the deduplicated mask quads
/// they index.
///
/// **Value-equal chains always share one span.** [`PassState::establish`]
/// decides whether a group can keep the chain already stamped by
/// comparing these spans, so a span is only a sound stand-in for the
/// chain behind it while the staging is deduplicated across the whole
/// frame rather than against a neighbour.
#[derive(Debug, Default)]
pub(crate) struct MaskPlan {
    pub(crate) groups: Vec<Span>,
    pub(crate) batches: Vec<Span>,
    /// Every distinct chain staged this frame. Retained so the sweep
    /// costs no allocation.
    staged: Vec<StagedChain>,
    /// See [`Self::quads`]. Retained like `staged`.
    quads: Vec<Quad>,
}

impl MaskPlan {
    /// Rebuild the plan for `buffer`: the mask spans and the deduplicated
    /// mask quads they index.
    ///
    /// The scan over already-staged chains is linear in the number of
    /// *distinct* chains, not in the group count: a chain exists only where
    /// authoring nested a rounded clip, so the list is a handful of entries
    /// on any real frame and one on most. A neighbour-only comparison would
    /// be O(1), but it breaks the span-per-chain invariant above the moment
    /// anything sits between two groups that share a chain — including a
    /// group the walk goes on to skip entirely, which leaves the chain
    /// stamped and then denies the elision that would have kept it.
    pub(crate) fn build(&mut self, buffer: &RenderBuffer) {
        self.groups.clear();
        self.batches.clear();
        self.staged.clear();
        self.quads.clear();
        for group in &buffer.groups {
            let chain = group.rounded_clips;
            let mask_span = if group.scissor.is_some() && chain.len != 0 {
                if let Some(staged) = self
                    .staged
                    .iter()
                    .find(|staged| buffer.chains_equal(staged.chain, chain))
                {
                    staged.masks
                } else {
                    let start = self.quads.len() as u32;
                    for clip in &buffer.rounded_clips[chain.range()] {
                        self.quads.push(Quad {
                            rect: clip.mask_rect,
                            corners: clip.corners,
                            ..Default::default()
                        });
                    }
                    let staged = Span::new(start, chain.len);
                    self.staged.push(StagedChain {
                        chain,
                        masks: staged,
                    });
                    staged
                }
            } else {
                Span::default()
            };
            self.groups.push(mask_span);
        }
        for batch in &buffer.text_batches {
            let group = batch.last_group as usize;
            debug_assert!(
                buffer.chains_equal(batch.rounded_clips, buffer.groups[group].rounded_clips),
                "text batch chain decorrelated from its last_group's chain"
            );
            self.batches.push(self.groups[group]);
        }
    }

    /// The mask quads the spans index, one per level of each distinct
    /// chain, in the order the spans name them.
    pub(crate) fn quads(&self) -> &[Quad] {
        &self.quads
    }
}

/// One conceptual step of the per-frame render schedule. Variants
/// describe *what* to do, not *how*; the consumer holds context
/// (whether the pass has a stencil, the actual `RenderPass`) to
/// translate each into wgpu calls.
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
    /// active, plain otherwise) + draw this run of a group's quads.
    Quads { range: Span },
    /// The same for a run of shadow quads, through the shadow pipeline.
    /// A group's quad range splits into `Quads` and `Shadows` runs, in
    /// paint order, wherever the kind changes.
    Shadows { range: Span },
    /// Render a coalesced text batch via the text-renderer pool slot.
    /// Emitted once per batch, immediately after the last group in
    /// the batch has drawn its quads (any meshes in that group still
    /// follow). One `Text { batch }` step → one text-backend render →
    /// one wgpu draw call covering every run in the batch.
    Text { batch: usize },
    /// Bind `tier`'s pipeline and replay batch `batch` of it, pulling
    /// the batch's items from `RenderBuffer::batches(tier)[batch]`.
    ///
    /// **One variant carrying the tier, not one variant per tier.** The
    /// step a tier maps to is not a decision — it is the same tier — so
    /// carrying it leaves nothing to state wrongly, and a new tier needs
    /// no variant here at all.
    ///
    /// What each tier does with the batch differs, which is why the
    /// backend still matches on it:
    ///
    /// - **Mesh** — one bind, then N `draw_indexed`, one per `MeshDraw`.
    /// - **Image** — one `draw` per row, switching the per-image bind
    ///   group between draws.
    /// - **Icon** — one instanced draw for the whole batch; every icon
    ///   shares one atlas bind group whatever mix the batch holds.
    /// - **Curve** — one bind, one `draw_indexed` covering every
    ///   `CurveInstance`. The "one draw call per scissor group" native
    ///   strokes target.
    TierBatch { tier: PaintTier, batch: usize },
}

/// Walk `buffer.groups` and emit one [`RenderStep`] at a time via
/// `emit`. Pure logic — no GPU calls.
///
/// `masks` is the frame's [`MaskPlan`] when the pass has a stencil
/// attachment, and `None` when it has none — a frame without one has
/// no chains to establish.
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
pub(crate) fn for_each_step(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    masks: Option<&MaskPlan>,
    // `&mut dyn` rather than `impl FnMut`: [`PassState`] is the single
    // emit point and holds the callback for the whole walk, so a generic
    // parameter would be erased there anyway — buying a monomorphisation
    // per caller and no devirtualised call.
    emit: &mut dyn FnMut(RenderStep),
) {
    ScheduleWalk {
        buffer,
        damage_scissor,
        cursors: ScheduleCursors::default(),
        state: PassState {
            emit,
            masks,
            cur_scissor: None,
            cur_ref: 0,
            active: None,
        },
    }
    .run();
}

/// One schedule walk: the frame it walks, the per-kind cursors, and the
/// pass state every step is emitted through. [`for_each_step`] builds one
/// and runs it to the end.
#[derive(Debug)]
struct ScheduleWalk<'a> {
    buffer: &'a RenderBuffer,
    damage_scissor: Option<URect>,
    cursors: ScheduleCursors,
    state: PassState<'a>,
}

impl ScheduleWalk<'_> {
    fn run(mut self) {
        let buffer = self.buffer;
        let full_viewport = URect::new(0, 0, buffer.display.physical.x, buffer.display.physical.y);

        if let Some(scissor) = self.damage_scissor {
            self.state.scissor(scissor);
            self.state.push(RenderStep::PreClear);
        }

        // Text batches map to a group via `last_group`; the schedule
        // emits `RenderStep::Text` when the walk reaches that group
        // (after its quads, before its meshes). `last_group` values are
        // monotonically increasing across batches (composer pushes in
        // order), so one cursor per kind suffices instead of a per-group
        // scan.
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
        for (i, g) in buffer.groups.iter().enumerate() {
            // Silently drop mesh/image/curve batches that anchored in
            // earlier damage-skipped groups — they had no visible scissor
            // so their draws don't paint.
            for tier in PaintTier::ALL {
                advance_past_skipped(
                    buffer.batches(tier),
                    &mut self.cursors.higher[tier.idx()],
                    i,
                );
            }

            let group_scissor = g.scissor.unwrap_or(full_viewport);
            let effective = match self.damage_scissor {
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
            self.drain_text_batches(i);

            // A group can be content-less at walk time — its only text
            // coalesced into a batch draining at a later group. Skip the
            // scissor / chain establish entirely then: a scissor with no
            // draws is a dead command, and on the stencil path the
            // establish would stamp a whole mask chain for nothing (the
            // next consumer establishes its own state regardless).
            let has_content = g.quads.len != 0
                || pending_at(&buffer.text_batches, self.cursors.text, i)
                || self.has_tier_batch_at(i);
            if has_content {
                self.state.narrow(|masks| masks.groups[i], effective);
                self.emit_group_body(i, effective);
            }
        }
        // Trailing drain — batches anchored in tail-skipped groups. Runs
        // BEFORE the tail clear so a batch whose chain is still stamped
        // elides, and a foreign one establishes its own.
        self.drain_text_batches(usize::MAX);
        // Tail clear: never let a stamped chain survive the walk. The pass
        // clears the stencil once, not per damage rect, and AA padding can
        // make nominally-disjoint rects' scissors overlap — residue here
        // would be read by the next rect's walk.
        self.state.clear_active();
    }

    /// Whether group `group` has a mesh, image, icon or curve batch still
    /// to emit.
    fn has_tier_batch_at(&self, group: usize) -> bool {
        PaintTier::ALL
            .iter()
            .any(|&t| pending_at(self.buffer.batches(t), self.cursors.higher[t.idx()], group))
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
    fn drain_text_batches(&mut self, target: usize) {
        let batches = &self.buffer.text_batches;
        while self.cursors.text < batches.len() && batches[self.cursors.text].last_group() < target
        {
            let batch = self.cursors.text;
            let s = match self.damage_scissor {
                Some(d) => batches[batch].scissor.intersect(d).unwrap_or_default(),
                None => batches[batch].scissor,
            };
            if !s.is_paint_empty() {
                self.state.narrow(|masks| masks.batches[batch], s);
                self.state.push(RenderStep::Text { batch });
            }
            self.cursors.text += 1;
        }
    }

    /// The group's quads as runs of one pipeline each, in paint order: a
    /// `Shadows` run wherever the shadow kind starts, a `Quads` run
    /// wherever it stops.
    fn emit_quad_runs(&mut self, quads: Span) {
        let range = quads.range();
        let mut start = range.start;
        let runs = self.buffer.quads[range]
            .chunk_by(|a, b| a.fill_kind.is_shadow() == b.fill_kind.is_shadow());
        for run in runs {
            let span = Span::from(start..start + run.len());
            self.state.push(if run[0].fill_kind.is_shadow() {
                RenderStep::Shadows { range: span }
            } else {
                RenderStep::Quads { range: span }
            });
            start += run.len();
        }
    }

    /// The draws every non-skipped group emits, identical under both the
    /// stencil and non-stencil paths: the group's quads, then its text
    /// batches (drained after the quads so a child quad occludes a
    /// label), then its mesh / image / curve batches — after
    /// re-requesting the group's own scissor + stencil state, since the
    /// text drain may have widened the scissor or restamped a different
    /// chain. Shared by the stencil and non-stencil paths so the two
    /// can't drift; the caller gates it on the group having any content.
    fn emit_group_body(&mut self, i: usize, effective: URect) {
        self.emit_quad_runs(self.buffer.groups[i].quads);
        self.drain_text_batches(i + 1);
        if !self.has_tier_batch_at(i) {
            return;
        }
        // Restore the group's own state: the text drain above may have
        // widened the scissor or restamped a different chain. Both
        // requests collapse to nothing when it didn't — the common case,
        // since most groups with a higher-kind batch carry no text at all.
        self.state.narrow(|masks| masks.groups[i], effective);
        // Paint order is `PaintTier::ALL`'s order, which is `Ord`'s — the
        // property the composer's flush arbitration rests on.
        for tier in PaintTier::ALL {
            drain_group_batches(
                self.buffer.batches(tier),
                &mut self.cursors.higher[tier.idx()],
                i,
                |batch| RenderStep::TierBatch { tier, batch },
                &mut self.state,
            );
        }
    }
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
    /// The frame's mask chains, `Some` exactly when the pass has a
    /// stencil attachment to stamp them into.
    masks: Option<&'a MaskPlan>,
    cur_scissor: Option<URect>,
    cur_ref: u32,
    active: Option<ActiveMask>,
}

// Manual: `emit` is a `&mut dyn FnMut`, which has nothing to format.
impl fmt::Debug for PassState<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PassState")
            .field("masks", &self.masks)
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

    /// Bring the pass to "ready to draw the content of the chain
    /// `chain` picks out of the plan, inside `scissor`". Without a
    /// stencil there is no plan, and only the scissor moves.
    fn narrow(&mut self, chain: impl FnOnce(&MaskPlan) -> Span, scissor: URect) {
        match self.masks {
            Some(masks) => self.establish(chain(masks), scissor),
            None => self.scissor(scissor),
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
    ///
    /// "The same chain" is read off the staged span, which stands in for
    /// the chain only because [`MaskPlan`] gives value-equal chains one
    /// span frame-wide. Two spans holding identical clips would elide
    /// nothing and pay a clear plus a full re-stamp for it.
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

/// Per-kind walk cursors for a [`ScheduleWalk`]. Each field is the index
/// of the next unconsumed batch of that kind; the cursors only advance
/// (batches are emitted in `last_group` order), so the whole walk is
/// linear in the batch count.
#[derive(Debug, Default)]
struct ScheduleCursors {
    text: usize,
    /// One per [`PaintTier`], indexed by `PaintTier::idx`.
    higher: [usize; PaintTier::COUNT],
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
/// **One generic helper, not a family.** This is the only one written
/// over [`PerGroupBatch`], because anchoring is the only rule the two
/// batch kinds share. [`advance_past_skipped`] is concrete because only
/// higher-kind cursors skip, and [`ScheduleWalk::drain_text_batches`]
/// drains on a *range* predicate rather than [`drain_group_batches`]'s equality one
/// because every text batch also needs its own bounds-union scissor, a
/// damage intersection, an empty-skip and its own mask chain. That is
/// different work, not a missed reuse.
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
    state: &mut PassState<'_>,
) {
    while pending_at(batches, *cursor, group) {
        state.push(step(*cursor));
        *cursor += 1;
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
    }

    impl Walk {
        pub(crate) fn new(buffer: &RenderBuffer) -> Self {
            let mut walk = Self::default();
            walk.plan.build(buffer);
            walk
        }

        pub(crate) fn run(
            &self,
            buffer: &RenderBuffer,
            damage: Option<URect>,
            use_stencil: bool,
        ) -> WalkCounts {
            let mut counts = WalkCounts::default();
            let masks = use_stencil.then_some(&self.plan);
            for_each_step(buffer, damage, masks, &mut |step| {
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
