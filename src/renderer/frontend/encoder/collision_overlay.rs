//! The explicit-`WidgetId` collision overlay. Development-only: a duplicate id is a caller bug, and `Forest::report_explicit_collision`'s `tracing::error!` survives into release. Gated at the `mod` declaration so its imports leave the release build too.

use crate::cascade::Cascade;
use crate::layout::Layout;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::scene::forest::Forest;

/// Magenta, distinct from the red damage-rect overlay. Painted unclipped after every layer.
const STROKE: Stroke = Stroke::new(RgbaF32::srgb(1.0, 0.0, 1.0), 3.0);

/// Final pass: a magenta outline per explicit-id collision recorded this frame, over everything. Emitted with no scissor or transform, so each outline is the node's unclipped screen rect. `NodeId`s are precomputed at recording time (`SeenIds.curr`).
pub(super) fn emit(forest: &Forest, layout: &Layout, cascade: &Cascade, out: &mut impl PaintSink) {
    for record in &forest.collisions {
        for ep in [record.first, record.second] {
            let rect = layout[ep.layer].rect[ep.node.idx()];
            let screen = cascade.entry_at(ep).transform.apply_rect(rect);
            out.draw_quad(
                DrawQuadPayload::rect(
                    screen,
                    Corners::ZERO,
                    BrushSource::Solid(RgbaF16::TRANSPARENT),
                    STROKE.into(),
                ),
                1.0,
            );
        }
    }
}
