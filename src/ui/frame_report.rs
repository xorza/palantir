//! One frame's plain-data report from [`Ui::frame`]: the post-record
//! signals the host needs to act on. All frame-shaped state (forest,
//! layout, cascades, display) stays on [`Ui`] itself — `Frontend::build`
//! reads it directly via a `&Ui` borrow, plus the per-frame [`Damage`]
//! this report carries.
//!
//! [`Ui`]: crate::ui::Ui
//! [`Ui::frame`]: crate::ui::Ui::frame

use crate::ui::damage::Damage;

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
