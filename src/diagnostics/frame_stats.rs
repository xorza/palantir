//! The opt-in frame-stats readout: the counters one frame publishes, and the
//! `Layer::Debug` widget that draws them.

use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::spacing::Spacing;
use crate::scene::layer::Layer;
use crate::text::font_family::FontFamily;
use crate::text::font_weight::FontWeight;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;

/// One frame's diagnostic counters, as [`Ui::frame_stats`] snapshots them.
///
/// A snapshot rather than a borrow of the clock behind it: the readout
/// records through `&mut Ui`, and no borrow taken off that `Ui` survives the
/// widget calls that draw the label.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameStats {
    pub(crate) frame_id: u64,
    pub(crate) render_frame_id: u64,
    pub(crate) fps: f32,
    pub(crate) settle_frames: u32,
    /// Whole-pass GPU time of the last frame that read a timestamp back.
    pub(crate) gpu_ms: Option<f32>,
}

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
pub(crate) fn record(ui: &mut Ui) {
    let FrameStats {
        frame_id,
        render_frame_id,
        fps,
        settle_frames,
        gpu_ms,
    } = ui.frame_stats();
    let gpu = GpuSegment(gpu_ms);
    // `settle/frame` reads as a ratio across a gesture: a sustained drag that
    // still double-records advances both halves in lockstep, one that stops
    // advances only the right.
    let label = ui.fmt(format_args!(
        "f {render_frame_id} · {fps:>4.0} fps · settle {settle_frames}/{frame_id}{gpu}"
    ));
    let style = TextStyle {
        family: FontFamily::MONO,
        weight: FontWeight::REGULAR,
        color: RgbaF32::srgb(1.0, 0.2, 0.2),
        font_size_px: 12.0,
        ..ui.theme().text
    };
    let chrome = Background::fill(RgbaF32::new(0.0, 0.0, 0.0, 0.75));
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
