//! Debug-overlay FPS readout. Recorded each frame into `Layer::Debug`
//! by [`record_frame_stats`] when `DebugOverlayConfig::frame_stats` is
//! on; pinned to the top-right of the viewport in a semi-transparent
//! black chrome so it stays legible against any background.
//!
//! Records every frame even on idle paths so the text changes and
//! damage picks up the small rect — keeps the readout ticking when
//! the rest of the UI is steady.

use crate::forest::element::Configure;
use crate::forest::tree::Layer;
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

pub(crate) fn record_frame_stats(ui: &mut Ui) {
    let label = format!("f {} · {:>4.0} fps", ui.frame_id, ui.fps_ema);
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
