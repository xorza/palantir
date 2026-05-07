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
use crate::primitives::rect::Rect;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::renderer::render_buffer::RenderBuffer;
use crate::tree::Tree;
use crate::tree::widget_id::WidgetId;
use crate::ui::cascade::CascadeResult;
use crate::ui::damage::DamagePaint;

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
}

impl FrameOutput<'_> {
    /// `true` when this frame's damage diff produced no work — the
    /// backbuffer already holds the right pixels. Hosts can skip
    /// `surface.get_current_texture()` + `submit` + `present` entirely.
    ///
    /// Only safe to early-bail when *the previous frame's `submit`
    /// actually presented*. If a host called `end_frame` and then
    /// failed to present (Occluded surface, validation error, lost
    /// device), it must call [`Ui::invalidate_prev_frame`] before
    /// the next `end_frame`; otherwise this method will return `true`
    /// against an unpainted backbuffer and the window stays black.
    ///
    /// [`Ui::invalidate_prev_frame`]: crate::Ui::invalidate_prev_frame
    pub fn can_skip_rendering(&self) -> bool {
        self.damage == DamagePaint::Skip
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
        tree: &Tree,
        layout: &LayoutResult,
        cascades: &CascadeResult,
        damage_filter: Option<Rect>,
        display: &Display,
    ) -> &RenderBuffer {
        let cmds = self.encoder.encode(
            tree,
            layout,
            cascades,
            damage_filter,
            display.logical_rect(),
        );
        self.composer.compose(cmds, display)
    }

    /// Drop encoder + composer cache entries for `WidgetId`s that
    /// vanished this frame. Called from `Ui::end_frame` with the same
    /// `removed` slice that the measure cache and text reuse map
    /// consume; keeps every cross-frame cache eviction-locked.
    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        self.encoder.sweep_removed(removed);
        self.composer.sweep_removed(removed);
    }
}
