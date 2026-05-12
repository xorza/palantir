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

/// One frame's plain-data report from [`Ui::frame`]: the post-record
/// signals the host needs to act on. All frame-shaped state (forest,
/// layout, cascades, display) stays on [`Ui`] itself —
/// [`Frontend::build`] reads it directly via a `&Ui` borrow, plus the
/// per-frame [`Damage`] this report carries.
///
/// [`Ui`]: crate::ui::Ui
pub struct FrameReport {
    pub(crate) repaint_requested: bool,
    pub(crate) skip_render: bool,
    /// Per-frame paint plan produced by `Ui::finalize_frame`. `None`
    /// ⇒ skip path (nothing changed; backbuffer is correct).
    /// `Some(Full | Partial)` ⇒ work for the renderer.
    pub(crate) damage: Option<Damage>,
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
    pub(crate) fn build(&mut self, ui: &Ui, damage: Damage) -> &RenderBuffer {
        let cmds = self.encoder.encode(ui, damage);
        self.composer.compose(cmds, ui.display, &mut self.buffer);
        &self.buffer
    }
}
