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

/// The GPU-time segment of the readout, or nothing until timestamp readback
/// yields a value — the first-frame readout must not reserve a misleading
/// placeholder column.
///
/// A `Display` shim rather than a formatted `String`, so the whole readout
/// reaches the arena through one [`Ui::fmt`] and the overlay costs no
/// allocation per record pass.
#[derive(Debug)]
struct GpuSegment(Option<f32>);

impl std::fmt::Display for GpuSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(ms) => write!(f, " · gpu {ms:>5.2} ms"),
            None => Ok(()),
        }
    }
}

/// Record the opt-in FPS readout into the top-right of `Layer::Debug`.
pub(super) fn record(ui: &mut Ui) {
    let gpu = GpuSegment(ui.resources.diagnostics.gpu_pass_stats.last_pass_ms());
    // Settling second record passes, over full-record frames. Read the
    // *delta* across a gesture: a sustained drag that still double-records
    // advances both halves in lockstep, one that doesn't advances only the
    // right. Counted through the previous frame — this runs mid-pass, so
    // the current frame's outcome isn't known yet.
    let render_frame_id = ui.frame_runtime.render_frame_id;
    let fps = ui.frame_runtime.fps_ema;
    let settle_frames = ui.frame_runtime.settle_frames;
    let frame_id = ui.frame_runtime.frame_id;
    let label = ui.fmt(format_args!(
        "f {render_frame_id} · {fps:>4.0} fps · settle {settle_frames}/{frame_id}{gpu}"
    ));
    let style = TextStyle {
        family: FontFamily::Mono,
        weight: FontWeight::Regular,
        color: Color::rgb(1.0, 0.2, 0.2),
        font_size_px: 12.0,
        ..ui.theme().text.clone()
    };
    let chrome = Background::fill(Color::linear_rgba(0.0, 0.0, 0.0, 0.75));
    ui.layer(Layer::Debug).show(|ui| {
        Panel::hstack()
            .size((Sizing::FILL, Sizing::HUG))
            .justify(Justify::End)
            .show(ui, |ui| {
                Panel::hstack()
                    .background(chrome)
                    .size((Sizing::HUG, Sizing::HUG))
                    .padding(Spacing::xy(4.0, 2.0))
                    .show(ui, |ui| {
                        Text::new(label).style(&style).show(ui);
                    });
            });
    });
}
