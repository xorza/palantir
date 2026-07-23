use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::spacing::Spacing;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::text::{FontFamily, FontWeight};
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;

/// Record the opt-in FPS readout into the top-right of `Layer::Debug`.
pub(crate) fn record(ui: &mut Ui) {
    // Omit GPU time until timestamp readback yields a value so the first-frame
    // readout does not reserve a misleading placeholder column.
    let gpu = ui
        .resources
        .diagnostics
        .gpu_pass_stats
        .last_pass_ms()
        .map(|ms| format!(" · gpu {ms:>5.2} ms"))
        .unwrap_or_default();
    let label = format!(
        "f {} · {:>4.0} fps{}",
        ui.frame_runtime.frame_id, ui.frame_runtime.fps_ema, gpu
    );
    let style = TextStyle {
        family: FontFamily::Mono,
        weight: FontWeight::Regular,
        color: Color::rgb(1.0, 0.2, 0.2),
        font_size_px: 12.0,
        ..ui.theme.text.clone()
    };
    let chrome = Background::fill(Color::linear_rgba(0.0, 0.0, 0.0, 0.75));
    ui.layer(Layer::Debug, glam::Vec2::ZERO, None, |ui| {
        Panel::hstack()
            .size((Sizing::FILL, Sizing::HUG))
            .justify(Justify::End)
            .show(ui, |ui| {
                Panel::hstack()
                    .background(chrome)
                    .size((Sizing::HUG, Sizing::HUG))
                    .padding(Spacing::xy(4.0, 2.0))
                    .show(ui, |ui| {
                        let label = ui.intern(&label);
                        Text::new(label).style(&style).show(ui);
                    });
            });
    });
}
