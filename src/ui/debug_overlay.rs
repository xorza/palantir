//! Debug overlay configuration on `Ui`. Set fields on
//! [`DebugOverlayConfig`] and assign via
//! `ui.debug_overlay = Some(cfg)` to enable per-frame visualizations.
//! Most are drawn onto the swapchain texture *after* the
//! backbuffer→surface copy in the wgpu backend, so they never land on
//! the backbuffer and produce no ghost pixels across frames. The
//! exception is [`DebugOverlayConfig::clear_damage`], which alters
//! the main pass's `LoadOp` directly.

/// Per-overlay flags. Each `bool` toggles one visualization.
/// Default is all-off; flip the flags you want individually. Future
/// flags (frame-time HUD, hit-rect viz, layer-boundary outlines, …)
/// land here as additional fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DebugOverlayConfig {
    /// Draws a 2px red stroke around the damaged region of each
    /// frame. `Skip` frames draw nothing; `Full` outlines the whole
    /// surface; `Partial(rect)` outlines the damage rect.
    pub damage_rect: bool,
    /// Force `LoadOp::Clear` on `Partial`-damage frames so the
    /// undamaged region flashes the clear color instead of preserving
    /// last frame's pixels. The damage scissor still narrows draws to
    /// the dirty rect — surrounding pixels show the clear color, which
    /// makes the painted region visually pop. Useful for verifying
    /// damage tracking by eye and for damage-fixture readbacks.
    pub clear_damage: bool,
}
