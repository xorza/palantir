//! The opt-in frame-stats readout: the `Layer::Debug` subtree drawing the counters one frame
//! publishes. It records nodes and a text shape directly, so the frame cycle that draws it needs
//! no widget, and no theme's panel chrome reaches it.

use crate::diagnostics::frame_stats::FrameStats;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::node::ident::Ident;
use crate::scene::node::node_mode::NodeMode;
use crate::shape::Shape;
use crate::text::font_family::FontFamily;
use crate::text::font_weight::FontWeight;
use crate::theme::text_style::TextStyle;
use crate::ui::Ui;
use std::fmt;
use std::panic::Location;
use std::time::Duration;

/// The GPU-time segment: empty until timestamp readback yields a value (no placeholder column on frame 1). A `Display` shim so the readout reaches the arena through one [`Ui::fmt`], allocation-free per pass.
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
    // `settle/frame` reads as a ratio across a gesture: a double-recording drag advances both halves, one that stops advances only the right.
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
    let row = Node {
        size: Some(SizeSpec::new(Sizing::FILL, Sizing::HUG)),
        justify: Justify::End,
        ..Node::new(NodeMode::Resolved(LayoutMode::Stack(Axis::X)))
    };
    let chip = Node {
        size: Some(SizeSpec::new(Sizing::HUG, Sizing::HUG)),
        padding: Some(Spacing::xy(4.0, 2.0)),
        ..Node::new(NodeMode::Resolved(LayoutMode::Stack(Axis::X)))
    };
    let leaf = Node::new(NodeMode::Resolved(LayoutMode::Leaf));
    ui.layer(Layer::Debug).show(|ui| {
        record_node(ui, &row, None, |ui| {
            record_node(ui, &chip, Some(&chrome), |ui| {
                record_node(ui, &leaf, None, |ui| {
                    ui.add_shape(Shape::text(label, style.font()).color(style.color));
                });
            });
        });
    });
}

/// Records `node` around `body` under the id of this call site, as `Widget::record` does for a
/// widget's `#[track_caller]` constructor.
#[track_caller]
fn record_node(ui: &mut Ui, node: &Node, chrome: Option<&Background>, body: impl FnOnce(&mut Ui)) {
    let resolved = ui.resolve_ident(Ident::Auto(Location::caller()));
    ui.open_node(resolved, node, chrome);
    body(ui);
    ui.close_node();
}

#[cfg(test)]
mod tests {
    use crate::ui::frame_stats_readout::GpuSegment;
    use std::time::Duration;

    /// The GPU segment appends a separator and a two-decimal time padded to five columns, or nothing without a pass time.
    #[test]
    fn the_gpu_segment_formats_or_vanishes() {
        let ms = |micros| Some(Duration::from_micros(micros));
        assert_eq!(GpuSegment(ms(3_456)).to_string(), " · gpu  3.46 ms");
        assert_eq!(GpuSegment(ms(12_000)).to_string(), " · gpu 12.00 ms");
        assert_eq!(GpuSegment(None).to_string(), "");
    }
}
