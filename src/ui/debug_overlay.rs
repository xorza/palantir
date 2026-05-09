//! Debug overlay configuration on `Ui`. Set
//! `ui.debug_overlay = Some(DebugOverlayConfig { damage_rect: true })`
//! to enable per-frame visualizations drawn onto the swapchain
//! texture *after* the backbuffer→surface copy in the wgpu backend
//! (so the overlay never lands on the backbuffer and produces no
//! ghost pixels across frames).

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
}
