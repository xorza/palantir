//! Per-frame render schedule: the ordered GPU steps painting every group of a
//! `RenderBuffer`. Production and tests share this step stream via
//! [`for_each_step`], so asserted order can't drift from issued order.

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

/// One clip chain staged into the mask-quad buffer: its source span and run.
#[derive(Clone, Copy, Debug)]
struct StagedChain {
    chain: Span,
    masks: Span,
}

/// Per-group and per-text-batch spans plus the deduplicated mask quads they index.
/// Value-equal chains share one span: [`PassState::establish`] compares spans to
/// decide whether a group keeps the stamped chain.
#[derive(Debug, Default)]
pub(crate) struct MaskPlan {
    pub(crate) groups: Vec<Span>,
    pub(crate) batches: Vec<Span>,
    staged: Vec<StagedChain>,
    quads: Vec<Quad>,
}

impl MaskPlan {
    /// Rebuild the plan for `buffer`. The scan over staged chains is linear in
    /// distinct chains (rare: nested rounded clips). A neighbour-only comparison
    /// would break the span-per-chain invariant when anything, even a skipped group,
    /// sits between two groups sharing a chain.
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

    pub(crate) fn quads(&self) -> &[Quad] {
        &self.quads
    }
}

/// One step of the schedule: what to do, not how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderStep {
    /// Pre-clear quad inside the damage scissor so AA fringes don't compound across
    /// frames. Only when `damage_scissor` is `Some`.
    PreClear,
    SetScissor(URect),
    /// Set the stencil reference (stencil-path frames only): chain depth for content,
    /// level `k` before stamping level `k`, `0` before a clear. Elided when held.
    SetStencilRef(u32),
    /// Bind the mask-stamp pipeline (`Equal` + `IncrementClamp`) and draw the mask
    /// quad at this index, writing `ref + 1`.
    MaskStamp(u32),
    /// Bind the mask-clear pipeline (`Always` + `Replace`, ref 0) and draw the mask
    /// quad at this index; a chain's outermost quad resets the whole chain.
    MaskClear(u32),
    Quads {
        range: Span,
    },
    /// The same for shadow quads. A group's quad range splits into runs wherever the
    /// kind changes.
    Shadows {
        range: Span,
    },
    /// Render a coalesced text batch in one draw, right after the batch's last group
    /// draws its quads.
    Text {
        batch: usize,
    },
    /// Replay batch `batch` of `tier` from `RenderBuffer::batches(tier)`. Per tier:
    /// **Mesh** one `draw_indexed` per `MeshDraw`; **Image** one `draw` per row;
    /// **Icon** one instanced draw; **Curve** one `draw_indexed` for every
    /// `CurveInstance`.
    TierBatch {
        tier: PaintTier,
        batch: usize,
    },
}

/// Walk `buffer.groups` and emit one [`RenderStep`] at a time via `emit`.
/// `masks` is the frame's [`MaskPlan`] when the pass has a stencil, else `None`.
///
/// Ordering invariants:
///
/// 1. With `damage_scissor` `Some`, the first steps are
///    `SetScissor(damage_scissor)` then [`PreClear`].
/// 2. Each group narrows the scissor to its `effective` rect before its draws.
/// 3. Stencil-path groups establish their mask chain first: level `k` stamps at
///    `stencil_ref = k`, content draws at `depth`. A stale chain clears with one
///    draw of its outermost quad at ref 0 under the stamp-time scissor (a clear
///    under another scissor misses stamped pixels). Groups sharing the stamped
///    chain elide the clear and re-stamp. A walk never ends with a chain stamped:
///    the pass clears the stencil once, and AA padding can make disjoint damage
///    scissors overlap, so residue would leak into the next walk.
/// 4. Text renders after its group's quads. A batch drained past damage-skipped
///    groups establishes its own chain.
/// 5. Groups with an empty effective scissor, or one missing `damage_scissor`,
///    emit nothing.
/// 6. `SetScissor` and `SetStencilRef` are transitions: [`PassState`] emits one
///    only when the value changes. The first scissor of each walk always emits.
///
/// [`PreClear`]: RenderStep::PreClear
pub(crate) fn for_each_step(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    masks: Option<&MaskPlan>,
    // `&mut dyn`: [`PassState`] holds the callback for the whole walk, so a generic
    // would be erased there anyway.
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

        // Text batches map to a group via `last_group`, monotonic across batches, so one
        // cursor per kind suffices.
        //
        // Damage-pass drain: a batch whose `last_group` is damage-skipped must still
        // render, since earlier groups in it may be in the damage rect. Before each
        // rendered group, batches with `last_group < i` are emitted (the composer's
        // overlap rule means no quad in `(last_group, i)` overlapped them). A trailing
        // drain covers tail-skipped groups.
        for (i, g) in buffer.groups.iter().enumerate() {
            // Drop mesh/image/curve batches anchored in earlier damage-skipped groups.
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
            // Drain stuck batches first so later quads paint over the drained text and a
            // batch sharing the stamped chain elides its stamp.
            self.drain_text_batches(i);

            // A group can be content-less at walk time (its text drains later); establishing
            // would emit a dead scissor and stamp a mask chain for nothing.
            let has_content = g.quads.len != 0
                || pending_at(&buffer.text_batches, self.cursors.text, i)
                || self.has_tier_batch_at(i);
            if has_content {
                self.state.narrow(|masks| masks.groups[i], effective);
                self.emit_group_body(i, effective);
            }
        }
        // Trailing drain, before the tail clear so a still-stamped chain elides.
        self.drain_text_batches(usize::MAX);
        self.state.clear_active();
    }

    fn has_tier_batch_at(&self, group: usize) -> bool {
        PaintTier::ALL
            .iter()
            .any(|&t| pending_at(self.buffer.batches(t), self.cursors.higher[t.idx()], group))
    }

    /// Drain every text batch whose `last_group < target`, each with its own
    /// bounds-union scissor intersected with the damage region (the text backend has
    /// no per-fragment x-clip). `target = i` drains stuck batches before group `i`;
    /// `i + 1` the group's own after its quads; `usize::MAX` the tail.
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

    /// The draws every non-skipped group emits: quads, text batches, then mesh /
    /// image / curve batches, after re-requesting the group's scissor and stencil
    /// (the text drain may have changed them). Caller gates it on content.
    fn emit_group_body(&mut self, i: usize, effective: URect) {
        self.emit_quad_runs(self.buffer.groups[i].quads);
        self.drain_text_batches(i + 1);
        if !self.has_tier_batch_at(i) {
            return;
        }
        // Restore the group's own state; a no-op when the text drain left it untouched.
        self.state.narrow(|masks| masks.groups[i], effective);
        // Paint order is `PaintTier::ALL`'s, which is `Ord`'s; the composer's flush
        // arbitration rests on it.
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

/// A stamped stencil chain: mask quads (outer to inner) and the stamp-time
/// scissor, which the clear must replay.
#[derive(Clone, Copy, Debug)]
struct ActiveMask {
    masks: Span,
    scissor: URect,
}

/// The render-pass state one walk has established, and the single point steps are
/// emitted from. Branches request state ([`Self::narrow`], [`Self::clear_active`]);
/// a matching request emits nothing, since wgpu records every scissor and stencil
/// set as a real command.
///
/// Dedup is sound only because `SetScissor` / `SetStencilRef` are the only steps
/// touching that state. Tracked per walk (one pass runs a walk per damage rect);
/// a walk exits with no chain stamped and ref 0.
struct PassState<'a> {
    emit: &'a mut dyn FnMut(RenderStep),
    /// The frame's mask chains; `Some` exactly when the pass has a stencil.
    masks: Option<&'a MaskPlan>,
    cur_scissor: Option<URect>,
    cur_ref: u32,
    active: Option<ActiveMask>,
}

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

    /// Bring the pass to "ready to draw the content of `chain` inside `scissor`".
    /// Without a stencil only the scissor moves.
    fn narrow(&mut self, chain: impl FnOnce(&MaskPlan) -> Span, scissor: URect) {
        match self.masks {
            Some(masks) => self.establish(chain(masks), scissor),
            None => self.scissor(scissor),
        }
    }

    /// Clear the stamped chain under its stamp-time scissor.
    fn clear_active(&mut self) {
        if let Some(prev) = self.active.take() {
            self.scissor(prev.scissor);
            self.stencil_ref(0);
            self.push(RenderStep::MaskClear(prev.masks.start));
        }
    }

    /// Bring the stencil to "`chain` stamped under `scissor`, ref = depth" and
    /// narrow the scissor. Elides the clear and re-stamp when the same chain is
    /// stamped and its stamp scissor covers `scissor` (a wider one would expose
    /// unwritten pixels and fail `Equal`). "Same chain" is the staged span, unique
    /// per value in [`MaskPlan`].
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

/// Per-kind walk cursors: the next unconsumed batch of each kind.
#[derive(Debug, Default)]
struct ScheduleCursors {
    text: usize,
    higher: [usize; PaintTier::COUNT],
}

/// Advance `cursor` past batches anchored in damage-skipped groups.
fn advance_past_skipped(batches: &[GroupBatch], cursor: &mut usize, before: usize) {
    while *cursor < batches.len() && batches[*cursor].last_group() < before {
        *cursor += 1;
    }
}

/// `true` if the batch at `cursor` anchors to group `group`. Generic over
/// [`PerGroupBatch`] because anchoring is all the batch kinds share.
fn pending_at<B: PerGroupBatch>(batches: &[B], cursor: usize, group: usize) -> bool {
    cursor < batches.len() && batches[cursor].last_group() == group
}

/// Drain every batch anchored to group `group`, emitting `step(idx)`.
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

#[cfg(feature = "bench")]
pub(crate) mod internals {
    use super::*;

    /// What one schedule walk emitted: the step total and the transition counts
    /// [`PassState`] deduplicates.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct WalkCounts {
        pub(crate) steps: usize,
        pub(crate) scissors: usize,
        pub(crate) stencil_refs: usize,
    }

    /// Schedule-walk harness for the `schedule` benchmark.
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
