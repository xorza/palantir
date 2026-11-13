//! Per-frame render schedule: the ordered GPU steps painting the runs of a `RenderBuffer`.
//! Production and tests share this step stream via [`Schedule::for_each_step`], so asserted order
//! can't drift from issued order.

#[cfg(feature = "internals")]
pub(crate) mod bench;

use crate::gpu::frame::frame_passes::ColorLoad;
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::draw_run::DrawRun;
use std::fmt;

/// One step of the schedule: what to do, not how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderStep {
    /// Pre-clear quad inside the damage scissor so AA fringes don't compound across
    /// frames. Only when `damage_scissor` is `Some` and the pass loads what the frame
    /// before left.
    PreClear,
    SetScissor(URect),
    /// Draw a run's rows, as its lane draws them: quads split where the pipeline changes; text
    /// and icons one draw per stretch of unturned or turned rasters; a mesh one `draw_indexed` a
    /// row; images one `draw` per stretch of one form and texture; curves one instanced
    /// `draw_indexed`; effects one draw per stretch of one key.
    Draw(DrawRun),
}

/// The runs one pass of the main pass paints, `runs[from..barrier]`: a main pass breaks at a
/// capturing backdrop's barrier to copy what the runs before it painted, and resumes there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RunSpan {
    pub(crate) from: u32,
    /// `None` paints every run from `from`.
    pub(crate) barrier: Option<u32>,
}

/// One walk of a frame's schedule: what [`Self::for_each_step`] reads.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Schedule<'a> {
    pub(crate) buffer: &'a RenderBuffer,
    /// The damage rect this walk repaints; `None` on a full repaint.
    pub(crate) damage_scissor: Option<URect>,
    pub(crate) runs: RunSpan,
    /// What the pass does with the target before it paints: one that clears needs no
    /// [`RenderStep::PreClear`].
    pub(crate) load: ColorLoad,
}

impl Schedule<'_> {
    /// Walk `buffer.runs[from..barrier]` in order and emit one [`RenderStep`] at a time via
    /// `emit`.
    ///
    /// Ordering invariants:
    ///
    /// 1. With `damage_scissor` `Some`, the first steps are `SetScissor(damage_scissor)` then
    ///    [`PreClear`], on a walk from the first run of a pass that loads.
    /// 2. A run whose pixels miss the damage emits nothing.
    /// 3. A scissored run draws under its scissor cut by the damage; an unscissored one under the
    ///    scissor in force where that holds its pixels in the damage, else under the damage, or
    ///    the viewport on a full walk. Every scissor set lies inside the damage, so nothing
    ///    paints past it.
    /// 4. `SetScissor` is a transition: [`PassState`] emits one only when the rect changes. The
    ///    first scissor of each walk always emits.
    ///
    /// [`PreClear`]: RenderStep::PreClear
    pub(crate) fn for_each_step(
        self,
        // `&mut dyn`: [`PassState`] holds the callback for the whole walk, so a generic
        // would be erased there anyway.
        emit: &mut dyn FnMut(RenderStep),
    ) {
        let runs = &self.buffer.runs;
        let end = self
            .runs
            .barrier
            .map_or(runs.len(), |barrier| barrier as usize);
        debug_assert!(end <= runs.len(), "a barrier past the last run");
        let from = self.runs.from as usize;
        let mut state = PassState {
            emit,
            cur_scissor: None,
        };
        // Only the walk that starts at the first run clears: one resuming after a barrier paints
        // over what the walk before it left, and a pass that clears its target has nothing to
        // clear again.
        if let Some(damage) = self.damage_scissor {
            state.scissor(damage);
            if from == 0 && self.load == ColorLoad::Load {
                state.push(RenderStep::PreClear);
            }
        }
        let physical = self.buffer.display.physical;
        let area = self
            .damage_scissor
            .unwrap_or_else(|| URect::new(0, 0, physical.x, physical.y));
        for &run in &runs[from..end] {
            let Some(pixels) = run
                .bounds
                .intersect(area)
                .filter(|pixels| !pixels.is_paint_empty())
            else {
                continue;
            };
            match run.scissor {
                Some(scissor) => {
                    let Some(cut) = scissor.intersect(area) else {
                        continue;
                    };
                    state.scissor(cut);
                }
                None => {
                    if !state
                        .cur_scissor
                        .is_some_and(|cur| pixels.clamp_to(cur) == pixels)
                    {
                        state.scissor(area);
                    }
                }
            }
            state.push(RenderStep::Draw(run));
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

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::frame::frame_passes::ColorLoad;
    use crate::gpu::frame::schedule::{RenderStep, RunSpan, Schedule};
    use crate::primitives::geometry::urect::URect;
    use crate::renderer::render_buffer::RenderBuffer;

    /// What one schedule walk emitted: the step total and the scissor transitions
    /// [`PassState`](crate::gpu::frame::schedule::PassState) deduplicates.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct WalkCounts {
        pub(crate) steps: usize,
        pub(crate) scissors: usize,
    }

    impl RunSpan {
        /// Every run: a main pass no backdrop breaks.
        pub(crate) const ALL: Self = Self {
            from: 0,
            barrier: None,
        };
    }

    impl WalkCounts {
        /// What a walk of `buffer`'s `runs` under `damage` emits, as one pass of a main pass
        /// broken at a backdrop's barrier walks them.
        pub(crate) fn of(buffer: &RenderBuffer, damage: Option<URect>, runs: RunSpan) -> Self {
            let mut counts = Self::default();
            let schedule = Schedule {
                buffer,
                damage_scissor: damage,
                runs,
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
