//! Debug overlay configuration on `Host`. Set fields on
//! [`DebugOverlayConfig`] and assign via
//! `host.debug_overlay = Some(cfg)` to enable per-frame visualizations.
//! Each flag draws on top of the regular paint without changing the
//! main pass's `LoadOp`: [`DebugOverlayConfig::damage_rect`] strokes
//! the damaged rects on the swapchain after the backbuffer→surface
//! copy; [`DebugOverlayConfig::dim_undamaged`] paints a translucent
//! quad onto the backbuffer in a separate `LoadOp::Load` pre-pass
//! before the partial damage passes. Neither flag mutates the
//! `RenderBuffer` or the schedule.

/// Per-overlay flags. Each `bool` toggles one visualization.
/// Default is all-off; flip the flags you want individually.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DebugOverlayConfig {
    /// Draws a 2px red stroke around the damaged region of each
    /// frame. `Skip` frames draw nothing; `Full` outlines the whole
    /// surface; `Partial(rect)` outlines the damage rect.
    pub damage_rect: bool,
    /// Visualize damage on `Partial` frames: before each frame's
    /// damage passes the backend paints a single full-viewport
    /// 40%-translucent black quad over the backbuffer (`LoadOp::Load`,
    /// no scissor) — undamaged pixels fade by 40% per frame; damaged
    /// pixels get dimmed but are then overwritten by the frame's
    /// regular draws, so they stay at full brightness. Across many
    /// frames static regions decay toward black while moving content
    /// stays current. Non-destructive: `Full` frames and frames with
    /// no partial damage skip the dim entirely (one full-screen clear
    /// resets the trail).
    pub dim_undamaged: bool,
}
