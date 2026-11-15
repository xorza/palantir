//! Text drawn under a zoom: where its glyphs land against the reach its run was culled and
//! batched by.

use glam::{UVec2, Vec2};

use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::HarnessDepth;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;

/// A run a zoom shows whole draws every glyph inside the bounds its reach gives, which the
/// encoder asserts of a run it draws with no clip: a glyph's raster lies on the pixel grid, its
/// pen and its box rounded out, so it reaches past the run's exact ink, which a descender at a
/// small zoom crosses into the next pixel row. A label with descenders at 14 px, swept over the
/// zooms of a node graph zoomed out, 0.300 to 0.400 in steps of 0.005, each at four offsets a
/// quarter pixel apart, draws whole each time.
#[test]
fn a_zoomed_runs_glyphs_lie_inside_its_reach() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(200, 100))
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    for step in 60..=80u16 {
        let scale = f32::from(step) * 0.005;
        for quarter in 0..4u8 {
            let offset = Vec2::new(10.0, 10.0 + f32::from(quarter) * 0.25);
            h.frame(|ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("zoomed"))
                    .transform(TranslateScale::new(offset, scale))
                    .show(ui, |ui| {
                        Text::new("Frequency gyqp").font_size(14.0).show(ui);
                    });
            });
        }
    }
}
