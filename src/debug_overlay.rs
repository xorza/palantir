//! Debug overlay configuration on `Ui`. Set fields on
//! [`DebugOverlayConfig`] via `ui.debug_overlay.field = …` to enable
//! per-frame visualizations. `damage_rect` and `dim_undamaged` draw
//! on top of the regular paint without changing the main pass's
//! `LoadOp`. `frame_stats` records a `Text` widget into
//! `Layer::Debug` at the top-left during `Ui::frame`; it goes through
//! the regular paint pipeline (dirties its own small rect every frame,
//! Main scene damage is untouched).

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
    /// Show a frame counter + EMA FPS readout in the top-left,
    /// recorded into `Layer::Debug` by `Ui::frame` after the user's
    /// record callback. Because the text changes every frame, this
    /// forces a `Partial(small rect)` damage even when the rest of
    /// the scene is idle — the readout's rect is unioned into the
    /// damage region; the Main scene's dirty-rect calculation is
    /// unaffected.
    pub frame_stats: bool,
}
