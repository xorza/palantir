//! The opt-in frame-stats readout: the counters one frame publishes, and the
//! `Layer::Debug` widget that draws them.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::text::font_family::FontFamily;
use crate::text::font_weight::FontWeight;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;
use std::fmt;
use std::time::Duration;

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
    pub(crate) gpu: Option<Duration>,
}

/// The GPU-time segment of the readout, or nothing until timestamp readback
/// yields a value — the first-frame readout must not reserve a misleading
/// placeholder column.
///
/// A `Display` shim rather than a formatted `String`, so the whole readout
/// reaches the arena through one [`Ui::fmt`] and the overlay costs no
/// allocation per record pass.
#[derive(Debug)]
struct GpuSegment(Option<Duration>);

impl fmt::Display for GpuSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(pass) => write!(f, " · gpu {:>5.2} ms", pass.as_secs_f64() * 1e3),
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
        gpu,
    } = ui.frame_stats();
    let gpu = GpuSegment(gpu);
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
        font_size: 12.0,
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

#[cfg(test)]
mod tests {
    use crate::diagnostics::frame_stats::GpuSegment;
    use std::time::Duration;

    /// The GPU segment appends a separator and a two-decimal time padded
    /// to five columns, and nothing at all when the device publishes no
    /// pass time.
    #[test]
    fn the_gpu_segment_formats_or_vanishes() {
        let ms = |micros| Some(Duration::from_micros(micros));
        assert_eq!(GpuSegment(ms(3_456)).to_string(), " · gpu  3.46 ms");
        assert_eq!(GpuSegment(ms(12_000)).to_string(), " · gpu 12.00 ms");
        assert_eq!(GpuSegment(None).to_string(), "");
    }
}
