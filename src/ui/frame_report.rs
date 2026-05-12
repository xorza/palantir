//! One frame's plain-data report from [`Ui::frame`]: the post-record
//! signals the host needs to act on. All frame-shaped state (forest,
//! layout, cascades, display) stays on [`Ui`] itself — `Frontend::build`
//! reads it directly via a `&Ui` borrow, plus the per-frame [`Damage`]
//! and clear color this report carries.
//!
//! [`Ui`]: crate::ui::Ui
//! [`Ui::frame`]: crate::ui::Ui::frame

use crate::primitives::color::Color;
use crate::ui::damage::Damage;

pub struct FrameReport {
    pub(crate) repaint_requested: bool,
    pub(crate) skip_render: bool,
    /// Per-frame paint plan produced by `Ui::finalize_frame`. `None`
    /// ⇒ skip path (nothing changed; backbuffer is correct).
    /// `Some(Full | Partial)` ⇒ work for the renderer.
    pub(crate) damage: Option<Damage>,
    /// Snapshot of `Ui.theme.window_clear` at frame time. Threaded
    /// through so `Host::render` doesn't need a separate `clear` arg
    /// and so a theme change mid-frame doesn't desync the paint.
    pub(crate) clear_color: Color,
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
