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
pub(crate) mod gradient_atlas;

use crate::forest::Forest;
use crate::layout::result::LayoutResult;
use crate::layout::types::display::Display;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::renderer::frontend::gradient_atlas::GradientCpuAtlas;
use crate::renderer::render_buffer::RenderBuffer;
use crate::ui::cascade::CascadeResult;
use crate::ui::damage::DamagePaint;
use crate::ui::damage::region::DamageRegion;
use crate::ui::debug_overlay::DebugOverlayConfig;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Submission status of the most recently produced [`FrameOutput`].
/// Held by both [`crate::Ui`] and `FrameOutput` (via `Arc`); written
/// by `Ui::end_frame` (→ `Pending`) and `WgpuBackend::submit` on the
/// success path (→ `Submitted`). Read by `Ui::begin_frame`, which
/// auto-rewinds `damage.prev_surface` whenever the last frame's
/// state isn't `Submitted` (host dropped the `FrameOutput`, surface
/// acquire failed, or it's the very first frame from
/// `FrameState::default()` — which leaves the underlying byte at
/// `Initial`). Turns "host dropped a `FrameOutput`" into "next frame
/// is `Full`" — wasteful but correct, instead of silent damage smear.
///
/// `AtomicU8` is overkill for the single-threaded renderer path, but
/// cheap and lets `Ui` / `FrameOutput` stay `Send`/`Sync` compatible
/// without further constraints.
#[derive(Clone, Debug, Default)]
pub(crate) struct FrameState(Arc<AtomicU8>);

// FrameState::default() leaves the inner byte at 0, which doesn't
// match SUBMITTED below — so the first `was_last_submitted` returns
// false and the first `Ui::begin_frame` rewinds, exactly as wanted.
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
    /// Cross-frame gradient LUT atlas, borrowed mutably for the
    /// duration of this `FrameOutput`. The backend drains the dirty
    /// bytes once during `submit` (no-op when nothing changed) and
    /// uploads them to the GPU texture before the render pass. Split
    /// borrow off `Frontend` — `&buffer` (Frontend.composer.buffer)
    /// and `&mut gradient_atlas` are disjoint fields, so the
    /// borrow checker accepts both lifetimes simultaneously.
    pub(crate) gradient_atlas: &'a mut GradientCpuAtlas,
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
    /// Cross-frame gradient atlas — composer registers gradients into
    /// it during compose, backend uploads dirty rows during submit.
    /// Persistent: rows stay baked across frames so repeated authoring
    /// of the same gradient is O(1) hash lookup.
    pub(crate) gradient_atlas: gradient_atlas::GradientCpuAtlas,
}

impl Frontend {
    /// Encode the tree into commands, compose them into the buffer.
    /// Disabled-dim and other paint-time theme constants are
    /// pre-resolved into `cascades` (`Cascade::rgb_mul`), so this stage
    /// reads everything it needs from the inputs without per-call
    /// theme threading.
    ///
    /// Returns `()` — the buffer + gradient atlas live on `Frontend`
    /// and are accessed via split borrows by the caller (so it can
    /// hold `&buffer` and `&mut gradient_atlas` simultaneously when
    /// constructing `FrameOutput`).
    pub(crate) fn build(
        &mut self,
        forest: &Forest,
        results: &LayoutResult,
        cascades: &CascadeResult,
        damage_filter: Option<&DamageRegion>,
        display: &Display,
    ) {
        let cmds = self.encoder.encode(
            forest,
            results,
            cascades,
            damage_filter,
            display.logical_rect(),
        );
        self.composer
            .compose(cmds, display, &mut self.gradient_atlas);
    }
}
