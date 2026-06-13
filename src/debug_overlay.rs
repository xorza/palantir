//! Debug overlay configuration on `Ui`. Set fields on
//! [`DebugOverlayConfig`] via `ui.debug_overlay.field = …` to enable
//! per-frame visualizations. `damage_rect` and `dim_undamaged` draw
//! on top of the regular paint without changing the main pass's
//! `LoadOp`. `frame_stats` records a `Text` widget into
//! `Layer::Debug` at the top-right during `Ui::frame` (via
//! [`record_frame_stats`]); it goes through the regular paint pipeline
//! (dirties its own small rect every frame, Main scene damage is
//! untouched).

use crate::forest::Layer;
use crate::forest::element::Configure;
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::spacing::Spacing;
use crate::text::FontFamily;
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;

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

/// Debug-overlay FPS readout. Recorded each frame into `Layer::Debug`
/// when [`DebugOverlayConfig::frame_stats`] is on; pinned to the
/// top-right of the viewport in a semi-transparent black chrome so it
/// stays legible against any background. Records every frame even on
/// idle paths so the text changes and damage picks up the small rect —
/// keeps the readout ticking when the rest of the UI is steady.
pub(crate) fn record_frame_stats(ui: &mut Ui) {
    // GPU pass time column: omitted entirely on adapters / first
    // frames where the timestamp-query readback hasn't yielded a
    // value yet, rather than printing "n/a" — the leading layout of
    // the readout stays clean and stable.
    let gpu = ui
        .ctx
        .pass_stats
        .last_pass_ms()
        .map(|ms| format!(" · gpu {ms:>5.2} ms"))
        .unwrap_or_default();
    let label = format!("f {} · {:>4.0} fps{}", ui.frame_id, ui.fps_ema, gpu);
    let style = TextStyle {
        family: FontFamily::Mono,
        color: Color::rgb(1.0, 0.2, 0.2),
        font_size_px: 12.0,
        ..ui.theme.text
    };
    let chrome = Background {
        fill: Color::linear_rgba(0.0, 0.0, 0.0, 0.75).into(),
        ..Default::default()
    };
    ui.layer(Layer::Debug, glam::Vec2::ZERO, None, |ui| {
        Panel::hstack()
            .size((Sizing::FILL, Sizing::Hug))
            .justify(Justify::End)
            .show(ui, |ui| {
                Panel::hstack()
                    .background(chrome)
                    .size((Sizing::Hug, Sizing::Hug))
                    .padding(Spacing::xy(4.0, 2.0))
                    .show(ui, |ui| {
                        Text::new(label).style(style).show(ui);
                    });
            });
    });
}
