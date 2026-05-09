//! Frontend (CPU) rendering pipeline.
//!
//! 1. [`encode`] — `&Tree` → [`RenderCmdBuffer`](cmd_buffer::RenderCmdBuffer)
//!    (logical-px). Pure free fn.
//! 2. [`Composer`] — `&RenderCmdBuffer` → `RenderBuffer` (physical-px
//!    quads + scissor groups). Owns the output + scratch; no GPU handles.
//! 3. [`Frontend`] (this struct) — orchestrates (1) + (2) and owns every
//!    persistent per-frame allocation. `Ui::end_frame` calls
//!    [`Frontend::build`] once and pulls the painted output via
//!    [`FrameOutput`].
//!
//! Output crosses into the backend as `&RenderBuffer` (defined one
//! level up so it sits at the frontend↔backend contract line).

pub(crate) mod cmd_buffer;
pub(crate) mod composer;
pub(crate) mod encoder;

use crate::layout::result::LayoutResult;
use crate::layout::types::display::Display;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::renderer::render_buffer::RenderBuffer;
use crate::tree::forest::Forest;
use crate::ui::cascade::CascadeResult;
use crate::ui::damage::DamagePaint;
use crate::ui::damage::region::DamageRegion;
use crate::ui::debug_overlay::DebugOverlayConfig;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Shared state tracking whether the most recently produced
/// [`FrameOutput`] actually reached the GPU. Held by both
/// [`crate::Ui`] and `FrameOutput` (via `Arc`); written by
/// `Ui::end_frame` (→ `Pending`) and `WgpuBackend::submit` on a
/// successful submit path (→ `Submitted`). Read by `Ui::begin_frame`,
/// which auto-rewinds `damage.prev_surface` when the last frame's
/// state is anything other than `Submitted`. The "host dropped a
/// `FrameOutput` without submitting it" bug class becomes
/// "next frame is `Full`" — wasteful but correct, instead of silent
/// damage smear.
///
/// `AtomicU8` is overkill for the single-threaded path the renderer
/// actually runs on, but cheap and lets `Ui` / `FrameOutput` stay
/// `Send`/`Sync` compatible without further constraints.
#[derive(Clone, Debug, Default)]
pub(crate) struct FrameState(Arc<AtomicU8>);

const FRAME_STATE_IDLE: u8 = 0;
const FRAME_STATE_PENDING: u8 = 1;
const FRAME_STATE_SUBMITTED: u8 = 2;

impl FrameState {
    pub(crate) fn mark_pending(&self) {
        self.0.store(FRAME_STATE_PENDING, Ordering::Relaxed);
    }
    pub(crate) fn mark_submitted(&self) {
        self.0.store(FRAME_STATE_SUBMITTED, Ordering::Relaxed);
    }
    pub(crate) fn was_last_submitted(&self) -> bool {
        self.0.load(Ordering::Relaxed) == FRAME_STATE_SUBMITTED
    }
    pub(crate) fn reset_to_idle(&self) {
        self.0.store(FRAME_STATE_IDLE, Ordering::Relaxed);
    }
}

/// One frame's CPU output: the composed render buffer and what the
/// GPU should do with it. Returned from [`Ui::end_frame`], consumed
/// by [`WgpuBackend::submit`]. The three [`DamagePaint`] variants —
/// `Full`, `Partial`, `Skip` — replace the old `Option<Rect>` so the
/// no-changes case can opt out of the GPU pass entirely instead of
/// being forced through a full clear+repaint.
///
/// [`Ui::end_frame`]: crate::ui::Ui::end_frame
/// [`WgpuBackend::submit`]: crate::renderer::WgpuBackend::submit
pub struct FrameOutput<'a> {
    pub(crate) buffer: &'a RenderBuffer,
    pub(crate) damage: DamagePaint,
    pub(crate) repaint_requested: bool,
    /// Snapshot of [`crate::Ui::debug_overlay`] at end-of-frame. Read
    /// by the wgpu backend to draw the requested visualizations onto
    /// the swapchain texture after the backbuffer→surface copy.
    pub(crate) debug_overlay: Option<DebugOverlayConfig>,
    /// Shared with `Ui::frame_state`. Set to `Pending` by
    /// `Ui::end_frame` and (on success) to `Submitted` by
    /// `WgpuBackend::submit`. The next `Ui::begin_frame` auto-rewinds
    /// damage if it doesn't see `Submitted`.
    pub(crate) frame_state: FrameState,
}

impl FrameOutput<'_> {
    /// `true` when this frame's damage diff produced no work — the
    /// backbuffer already holds the right pixels. Hosts can skip
    /// `surface.get_current_texture()` + `submit` + `present` entirely.
    ///
    /// Safe by construction: if the previous frame's `submit` didn't
    /// run (host dropped the `FrameOutput`, surface acquire failed,
    /// etc.), the framework's auto-rewind in `Ui::begin_frame`
    /// forced this frame to `Full`, so this method returns `false`
    /// and the host paints. No "invalidate" call needed in
    /// surface-error paths.
    pub fn can_skip_rendering(&self) -> bool {
        self.damage == DamagePaint::Skip
    }

    /// `true` when an animation tick during this frame hasn't
    /// settled (set by `Ui::animate`). Hosts honor by calling
    /// `window.request_redraw()` (or equivalent) after present, so
    /// the next frame runs even when input is idle.
    pub fn repaint_requested(&self) -> bool {
        self.repaint_requested
    }
}

/// CPU paint stage: tree → encoded commands → composed buffer. Owns
/// every persistent allocation (the encoder's
/// [`RenderCmdBuffer`](cmd_buffer::RenderCmdBuffer), the output
/// `RenderBuffer`, the [`Composer`] with its scratch). No GPU
/// handles — `buffer()` is fed into any backend (`WgpuBackend`, future
/// software/Vello/etc.).
///
/// Lives inside [`Ui`](crate::ui::Ui) so a host gets the entire CPU
/// frame state (UI logic + paint output) from one
/// [`Ui::end_frame`](crate::ui::Ui::end_frame) call.
#[derive(Default)]
pub(crate) struct Frontend {
    pub(crate) encoder: Encoder,
    pub(crate) composer: Composer,
}

impl Frontend {
    /// Encode the tree into commands, compose them into the buffer.
    /// Disabled-dim and other paint-time theme constants are
    /// pre-resolved into `cascades` (`Cascade::rgb_mul`), so this stage
    /// reads everything it needs from the inputs without per-call
    /// theme threading.
    pub(crate) fn build(
        &mut self,
        forest: &Forest,
        results: &LayoutResult,
        cascades: &CascadeResult,
        damage_filter: Option<&DamageRegion>,
        display: &Display,
    ) -> &RenderBuffer {
        let cmds = self.encoder.encode(
            forest,
            results,
            cascades,
            damage_filter,
            display.logical_rect(),
        );
        self.composer.compose(cmds, display)
    }
}
