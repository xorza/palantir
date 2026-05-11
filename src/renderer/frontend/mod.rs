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

use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::renderer::render_buffer::RenderBuffer;
use crate::ui::Ui;
use crate::ui::damage::Damage;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Submission status of the most recently produced [`FrameReport`].
/// Held by both [`crate::Ui`] and `FrameReport` (via `Arc`); written
/// by `Ui::run_frame` (→ `Pending`) and the renderer on the success
/// path (→ `Submitted`). Read by `Ui::pre_record`, which auto-rewinds
/// `damage.prev_surface` whenever the last frame's state isn't
/// `Submitted` (host dropped the `FrameReport`, surface acquire
/// failed, or it's the very first frame from `FrameState::default()` —
/// which leaves the underlying byte at `Initial`). Turns "host dropped
/// a `FrameReport`" into "next frame is `Full`" — wasteful but
/// correct, instead of silent damage smear.
///
/// `AtomicU8` is overkill for the single-threaded renderer path, but
/// cheap and lets `Ui` / `FrameReport` stay `Send`/`Sync` compatible
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

/// One frame's plain-data report from [`Ui::frame`]: the post-record
/// signals the host needs to act on. All frame-shaped state (forest,
/// layout, cascades, display, damage) stays on [`Ui`] itself —
/// [`Frontend::build`] reads it directly via a `&Ui` borrow, so this
/// struct doesn't carry borrows and has no lifetime.
///
/// [`Ui`]: crate::ui::Ui
pub struct FrameReport {
    pub(crate) repaint_requested: bool,
    /// Shared with `Ui::frame_state`. Set to `Pending` by `Ui::frame`
    /// and (on success) to `Submitted` by the renderer. The next
    /// `Ui::pre_record` auto-rewinds damage if it doesn't see
    /// `Submitted`.
    pub(crate) frame_state: FrameState,
    pub(crate) skip_render: bool,
}

impl FrameReport {
    /// `true` when an animation tick during this frame hasn't
    /// settled (set by `Ui::animate`). Hosts honor by calling
    /// `window.request_redraw()` (or equivalent) after present, so
    /// the next frame runs even when input is idle.
    pub fn repaint_requested(&self) -> bool {
        self.repaint_requested
    }

    pub fn skip_render(&self) -> bool {
        self.skip_render
    }

    pub fn confirm_submitted(&self) {
        self.frame_state.mark_submitted();
    }
}

/// CPU paint stage: tree → encoded commands → composed buffer. Owns
/// every persistent allocation (the encoder's
/// [`RenderCmdBuffer`](cmd_buffer::RenderCmdBuffer), the output
/// `RenderBuffer` — which carries the gradient atlas as a field —
/// and the [`Composer`] with its scratch). No GPU handles.
///
/// Owned by [`Renderer`](crate::renderer::Renderer) alongside the
/// backend; the renderer drives `Frontend::build` and hands the
/// returned `&mut RenderBuffer` straight to the backend.
#[derive(Default)]
pub(crate) struct Frontend {
    pub(crate) encoder: Encoder,
    pub(crate) composer: Composer,
    pub(crate) buffer: RenderBuffer,
}

impl Frontend {
    /// Encode the tree into commands, compose them into the owned
    /// buffer, and return a borrow of the composed result.
    /// Disabled-dim and other paint-time theme constants are
    /// pre-resolved into `cascades` (`Cascade::rgb_mul`), so this
    /// stage reads everything it needs from the inputs without
    /// per-call theme threading.
    pub(crate) fn build(&mut self, ui: &Ui) -> &mut RenderBuffer {
    
        let cmds = self.encoder.encode(ui);
        self.composer.compose(cmds, ui.display, &mut self.buffer);
        &mut self.buffer
    }
}
