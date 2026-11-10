//! Per-frame render schedule: the ordered GPU steps painting every group of a
//! `RenderBuffer`. Production and tests share this step stream via
//! [`Schedule::for_each_step`], so asserted order can't drift from issued order.

#[cfg(feature = "internals")]
pub(crate) mod bench;

use crate::common::span::Span;
use crate::gpu::frame::frame_passes::ColorLoad;
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use std::fmt;

/// One step of the schedule: what to do, not how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderStep {
    /// Pre-clear quad inside the damage scissor so AA fringes don't compound across
    /// frames. Only when `damage_scissor` is `Some` and the pass loads what the frame
    /// before left.
    PreClear,
    SetScissor(URect),
    /// Draw a group's quads, shadows among them: the backend splits the range where the
    /// pipeline changes.
    Quads {
        range: Span,
    },
    /// Render a coalesced text batch, right after the batch's last group draws its quads: one
    /// draw per stretch of unturned or turned glyphs.
    Text {
        batch: usize,
    },
    /// Replay batch `batch` of `tier` from `RenderBuffer::batches(tier)`. Per tier:
    /// **Mesh** one `draw_indexed` per `MeshDraw`; **Image** one `draw` per run of one form and
    /// texture; **Icon** one draw per stretch of unturned or turned icons; **Curve** one
    /// instanced `draw_indexed` over the batch.
    TierBatch {
        tier: PaintTier,
        batch: usize,
    },
}

/// The groups one pass of the main pass paints, `groups[from..barrier]`: a main pass breaks at
/// a capturing backdrop's barrier to copy what the groups before it painted, and resumes there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GroupSpan {
    pub(crate) from: u32,
    /// `None` paints every group from `from`.
    pub(crate) barrier: Option<u32>,
}

/// One walk of a frame's schedule: what [`Self::for_each_step`] reads.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Schedule<'a> {
    pub(crate) buffer: &'a RenderBuffer,
    /// The damage rect this walk repaints; `None` on a full repaint.
    pub(crate) damage_scissor: Option<URect>,
    pub(crate) groups: GroupSpan,
    /// What the pass does with the target before it paints: one that clears needs no
    /// [`RenderStep::PreClear`].
    pub(crate) load: ColorLoad,
}

impl Schedule<'_> {
    /// Walk `buffer.groups` and emit one [`RenderStep`] at a time via `emit`.
    ///
    /// Ordering invariants:
    ///
    /// 1. With `damage_scissor` `Some`, the first steps are
    ///    `SetScissor(damage_scissor)` then [`PreClear`], on a walk from the first group
    ///    of a pass that loads.
    /// 2. Each group narrows the scissor to its `effective` rect before its draws.
    /// 3. Text renders after its group's quads, each batch under its own scissor.
    /// 4. Groups with an empty effective scissor, or one missing `damage_scissor`,
    ///    emit nothing.
    /// 5. `SetScissor` is a transition: [`PassState`] emits one only when the rect
    ///    changes. The first scissor of each walk always emits.
    ///
    /// [`PreClear`]: RenderStep::PreClear
    pub(crate) fn for_each_step(
        self,
        // `&mut dyn`: [`PassState`] holds the callback for the whole walk, so a generic
        // would be erased there anyway.
        emit: &mut dyn FnMut(RenderStep),
    ) {
        let end = self
            .groups
            .barrier
            .map_or(self.buffer.groups.len(), |barrier| barrier as usize);
        debug_assert!(
            end <= self.buffer.groups.len(),
            "a barrier past the last group"
        );
        ScheduleWalk {
            buffer: self.buffer,
            from: self.groups.from as usize,
            end,
            damage_scissor: self.damage_scissor,
            load: self.load,
            cursors: ScheduleCursors::default(),
            state: PassState {
                emit,
                cur_scissor: None,
            },
        }
        .run();
    }
}

#[derive(Debug)]
struct ScheduleWalk<'a> {
    buffer: &'a RenderBuffer,
    /// The groups walked, `groups[from..end]`.
    from: usize,
    end: usize,
    damage_scissor: Option<URect>,
    load: ColorLoad,
    cursors: ScheduleCursors,
    state: PassState<'a>,
}

impl ScheduleWalk<'_> {
    fn run(mut self) {
        let buffer = self.buffer;
        let full_viewport = URect::new(0, 0, buffer.display.physical.x, buffer.display.physical.y);

        // Only the walk that starts at the first group clears: one resuming after a barrier paints
        // over what the walk before it left, and a pass that clears its target has nothing to
        // clear again.
        if let Some(scissor) = self.damage_scissor {
            self.state.scissor(scissor);
            if self.from == 0 && self.load == ColorLoad::Load {
                self.state.push(RenderStep::PreClear);
            }
        }

        // Text batches map to a group via `last_group`, monotonic across batches, so one
        // cursor per kind suffices.
        //
        // Damage-pass drain: a batch whose `last_group` is damage-skipped must still
        // render, since earlier groups in it may be in the damage rect. Before each
        // rendered group, batches with `last_group < i` are emitted (the composer's
        // overlap rule means no quad in `(last_group, i)` overlapped them). A trailing
        // drain covers tail-skipped groups.
        // A walk that resumes after a barrier skips what the walk before it drew: the composer
        // closes every batch at a barrier, so none straddles `from`.
        let from = self.from;
        self.cursors
            .text
            .skip_before(&buffer.text_batches, |b| b.last_group, from);
        for (i, g) in buffer.groups[..self.end].iter().enumerate().skip(from) {
            // Drop mesh/image/curve batches anchored in earlier damage-skipped groups.
            for tier in PaintTier::ALL {
                self.cursors.higher[tier.idx()].skip_before(
                    buffer.batches(tier),
                    |b| b.last_group,
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
            // Drain stuck batches first so later quads paint over the drained text.
            self.drain_text_batches(i);

            // A group can be content-less at walk time (its text drains later); its scissor
            // would be a dead step.
            let has_content = g.quads.len != 0
                || self
                    .cursors
                    .text
                    .anchored_at(&buffer.text_batches, |b| b.last_group, i)
                || self.has_tier_batch_at(i);
            if has_content {
                self.state.scissor(effective);
                self.emit_group_body(i, effective);
            }
        }
        // Trailing drain. A batch past the walk's last group renders in a walk that reaches it.
        self.drain_text_batches(self.end);
    }

    fn has_tier_batch_at(&self, group: usize) -> bool {
        PaintTier::ALL.iter().any(|&t| {
            self.cursors.higher[t.idx()].anchored_at(
                self.buffer.batches(t),
                |b| b.last_group,
                group,
            )
        })
    }

    /// Drain every text batch whose `last_group < target`, each with its own
    /// bounds-union scissor intersected with the damage region (the text backend has
    /// no per-fragment x-clip). `target = i` drains stuck batches before group `i`;
    /// `i + 1` the group's own after its quads; `end` the tail.
    fn drain_text_batches(&mut self, target: usize) {
        let batches = &self.buffer.text_batches;
        while let Some(batch) = self
            .cursors
            .text
            .anchored_before(batches, |b| b.last_group, target)
        {
            let s = match self.damage_scissor {
                Some(d) => batches[batch].scissor.intersect(d).unwrap_or_default(),
                None => batches[batch].scissor,
            };
            if !s.is_paint_empty() {
                self.state.scissor(s);
                self.state.push(RenderStep::Text { batch });
            }
            self.cursors.text.advance();
        }
    }

    /// The draws every non-skipped group emits: quads, text batches, then mesh /
    /// image / curve batches, after re-requesting the group's scissor (the text drain
    /// may have changed it). Caller gates it on content.
    fn emit_group_body(&mut self, i: usize, effective: URect) {
        let quads = self.buffer.groups[i].quads;
        if quads.len != 0 {
            self.state.push(RenderStep::Quads { range: quads });
        }
        self.drain_text_batches(i + 1);
        if !self.has_tier_batch_at(i) {
            return;
        }
        // Restore the group's own scissor; a no-op when the text drain left it untouched.
        self.state.scissor(effective);
        // Paint order is `PaintTier::ALL`'s, which is `Ord`'s; the composer's flush
        // arbitration rests on it.
        for tier in PaintTier::ALL {
            self.drain_tier_batches(tier, i);
        }
    }

    /// Emit every `tier` batch anchored to `group`.
    fn drain_tier_batches(&mut self, tier: PaintTier, group: usize) {
        let batches = self.buffer.batches(tier);
        let cursor = &mut self.cursors.higher[tier.idx()];
        while cursor.anchored_at(batches, |b| b.last_group, group) {
            self.state.push(RenderStep::TierBatch {
                tier,
                batch: cursor.idx(),
            });
            cursor.advance();
        }
    }
}

/// The render-pass state one walk has set, and the single point steps are emitted from. A
/// request for the scissor in force emits nothing, since wgpu records every scissor set as a real
/// command. Sound only because `SetScissor` is the only step touching it; tracked per walk (one
/// pass runs a walk per damage rect).
struct PassState<'a> {
    emit: &'a mut dyn FnMut(RenderStep),
    cur_scissor: Option<URect>,
}

impl fmt::Debug for PassState<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PassState")
            .field("cur_scissor", &self.cur_scissor)
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
}

/// Per-kind walk cursors: the next unconsumed batch of each kind.
#[derive(Debug, Default)]
struct ScheduleCursors {
    text: BatchCursor,
    higher: [BatchCursor; PaintTier::COUNT],
}

/// One batch kind's walk cursor: the next unconsumed batch. Each kind stages its batches against
/// the last group they reach into, their anchor, which a caller reads off its own batch type.
/// Refused: a trait whose one method read that field, beside two generic helpers over it.
#[derive(Clone, Copy, Debug, Default)]
struct BatchCursor(usize);

impl BatchCursor {
    const fn idx(self) -> usize {
        self.0
    }

    const fn advance(&mut self) {
        self.0 += 1;
    }

    /// The next batch, if `anchor` puts it before group `before`.
    fn anchored_before<B>(
        self,
        batches: &[B],
        anchor: impl Fn(&B) -> u32,
        before: usize,
    ) -> Option<usize> {
        batches
            .get(self.0)
            .is_some_and(|batch| (anchor(batch) as usize) < before)
            .then_some(self.0)
    }

    /// Whether `anchor` puts the next batch at group `group`.
    fn anchored_at<B>(self, batches: &[B], anchor: impl Fn(&B) -> u32, group: usize) -> bool {
        batches
            .get(self.0)
            .is_some_and(|batch| anchor(batch) as usize == group)
    }

    /// Move past the batches anchored before group `before`, in groups damage skipped.
    fn skip_before<B>(&mut self, batches: &[B], anchor: impl Fn(&B) -> u32, before: usize) {
        while self.anchored_before(batches, &anchor, before).is_some() {
            self.advance();
        }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::frame::frame_passes::ColorLoad;
    use crate::gpu::frame::schedule::{GroupSpan, RenderStep, Schedule};
    use crate::primitives::geometry::urect::URect;
    use crate::renderer::render_buffer::RenderBuffer;

    /// What one schedule walk emitted: the step total and the scissor transitions
    /// [`PassState`](crate::gpu::frame::schedule::PassState) deduplicates.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct WalkCounts {
        pub(crate) steps: usize,
        pub(crate) scissors: usize,
    }

    impl GroupSpan {
        /// Every group: a main pass no backdrop breaks.
        pub(crate) const ALL: Self = Self {
            from: 0,
            barrier: None,
        };
    }

    impl WalkCounts {
        /// What a walk of `buffer`'s `groups` under `damage` emits, as one pass of a main pass
        /// broken at a backdrop's barrier walks them.
        pub(crate) fn of(buffer: &RenderBuffer, damage: Option<URect>, groups: GroupSpan) -> Self {
            let mut counts = Self::default();
            let schedule = Schedule {
                buffer,
                damage_scissor: damage,
                groups,
                load: ColorLoad::Load,
            };
            schedule.for_each_step(&mut |step| {
                counts.steps += 1;
                if let RenderStep::SetScissor(_) = step {
                    counts.scissors += 1;
                }
            });
            counts
        }
    }
}

#[cfg(test)]
mod tests;
