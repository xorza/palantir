//! Frontend (CPU) rendering pipeline.
//!
//! 1. [`encode`] — `&Tree` → [`RenderCmdBuffer`](cmd_buffer::RenderCmdBuffer)
//!    (logical-px). Pure free fn.
//! 2. [`Composer`] — `&RenderCmdBuffer` → `RenderBuffer` (physical-px
//!    quads + scissor groups). Owns the output + scratch; no GPU handles.
//! 3. [`Frontend`] (this struct) — orchestrates (1) + (2) and owns every
//!    persistent per-frame allocation. The owning [`Renderer`] calls
//!    [`Frontend::build`] once per frame and feeds the composed buffer
//!    plus gradient atlas into the backend.
//!
//! Output crosses into the backend as `&RenderBuffer` (defined one
//! level up so it sits at the frontend↔backend contract line).
//!
//! [`Renderer`]: crate::renderer::Renderer

pub(crate) mod cmd_buffer;
pub(crate) mod composer;
pub(crate) mod encoder;
pub(crate) mod gradient_atlas;

use crate::forest::Forest;
use crate::layout::Layout;
use crate::layout::types::display::Display;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::ui::cascade::CascadeResult;
use crate::ui::damage::DamagePaint;
use crate::ui::damage::region::DamageRegion;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Submission status of the most recently produced [`RecordedFrame`].
/// Held by both [`crate::Ui`] and `RecordedFrame` (via `Arc`); written
/// by `Ui::run_frame` (→ `Pending`) and the renderer on the success
/// path (→ `Submitted`). Read by `Ui::pre_record`, which auto-rewinds
/// `damage.prev_surface` whenever the last frame's state isn't
/// `Submitted` (host dropped the `RecordedFrame`, surface acquire
/// failed, or it's the very first frame from `FrameState::default()` —
/// which leaves the underlying byte at `Initial`). Turns "host dropped
/// a `RecordedFrame`" into "next frame is `Full`" — wasteful but
/// correct, instead of silent damage smear.
///
/// `AtomicU8` is overkill for the single-threaded renderer path, but
/// cheap and lets `Ui` / `RecordedFrame` stay `Send`/`Sync` compatible
/// without further constraints.
#[derive(Clone, Debug, Default)]
pub(crate) struct FrameState(Arc<AtomicU8>);

// FrameState::default() leaves the inner byte at 0, which doesn't
// match SUBMITTED below — so the first `was_last_submitted` returns
// false and the first `Ui::pre_record` rewinds, exactly as wanted.
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

/// One frame's recorded result: a borrowed view into the [`Ui`]'s
/// per-frame data plus the damage decision. Returned from
/// [`Ui::run_frame`], consumed by [`Renderer::render`]. The three
/// [`DamagePaint`] variants — `Full`, `Partial`, `Skip` — let the
/// no-changes case opt out of the GPU pass entirely instead of being
/// forced through a full clear+repaint.
///
/// Pure CPU data; no GPU handles. Backends consume it through the
/// owning [`Renderer`], which also holds the [`Frontend`] that turns
/// `forest`/`results`/`cascades` into a composed `RenderBuffer`.
///
/// [`Ui`]: crate::ui::Ui
/// [`Ui::run_frame`]: crate::ui::Ui::run_frame
/// [`Renderer`]: crate::renderer::Renderer
/// [`Renderer::render`]: crate::renderer::Renderer::render
pub struct RecordedFrame<'a> {
    pub(crate) forest: &'a Forest,
    pub(crate) layout: &'a Layout,
    pub(crate) cascades: &'a CascadeResult,
    pub(crate) display: Display,
    pub(crate) damage: DamagePaint,
    pub(crate) repaint_requested: bool,
    /// Shared with `Ui::frame_state`. Set to `Pending` by
    /// `Ui::run_frame` and (on success) to `Submitted` by
    /// `Renderer::render`. The next `Ui::pre_record` auto-rewinds
    /// damage if it doesn't see `Submitted`.
    pub(crate) frame_state: FrameState,
}

impl RecordedFrame<'_> {
    /// `true` when this frame's damage diff produced no work — the
    /// backbuffer already holds the right pixels. Hosts can skip
    /// `surface.get_current_texture()` + render + `present` entirely.
    ///
    /// Safe by construction: if the previous frame's render didn't
    /// run (host dropped the `RecordedFrame`, surface acquire failed,
    /// etc.), the framework's auto-rewind in `Ui::pre_record`
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

    pub(crate) fn damage_filter(&self) -> Option<&DamageRegion> {
        match &self.damage {
            DamagePaint::Partial(region) => Some(region),
            DamagePaint::Full | DamagePaint::Skip => None,
        }
    }
}

/// CPU paint stage: tree → encoded commands → composed buffer. Owns
/// every persistent allocation (the encoder's
/// [`RenderCmdBuffer`](cmd_buffer::RenderCmdBuffer), the output
/// `RenderBuffer`, the [`Composer`] with its scratch). No GPU
/// handles — the composed buffer is fed into any backend
/// (`WgpuBackend`, future software/Vello/etc.).
///
/// Owned by [`Renderer`](crate::renderer::Renderer) alongside the
/// backend; the renderer drives `Frontend::build` then hands the
/// composed buffer + gradient atlas to the backend.
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
        results: &Layout,
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
