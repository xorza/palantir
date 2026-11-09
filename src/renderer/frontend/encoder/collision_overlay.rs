//! The explicit-`WidgetId` collision overlay, in every build: a duplicate id shares a widget's state between two call sites, and a bug that shows beats one that only logs (`Forest::report_explicit_collision`).

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::frontend::FrameScene;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_quad_payload::{DrawQuadPayload, RectQuad};
use crate::shape::paint::lowered_border::LoweredBorder;

/// Magenta, distinct from the red damage-rect overlay. Painted unclipped after every layer.
const BORDER: Border = Border::new(RgbaF32::srgb(1.0, 0.0, 1.0), 3.0);

/// Final pass: a magenta outline per explicit-id collision recorded this frame, over everything. Emitted with no scissor or transform, so each outline is the node's unclipped screen rect. `NodeId`s are precomputed at recording time (`SeenIds.cur`).
pub(super) fn emit(scene: &FrameScene<'_>, out: &mut impl PaintSink) {
    for record in &scene.forest.collisions {
        for ep in [record.first, record.second] {
            let rect = scene.layout[ep.plane].rect[ep.node.idx()];
            let entry = scene.cascade.entry(scene.cascade.location(ep));
            let screen = entry.transform.apply_rect(rect);
            out.draw(
                DrawQuadPayload::rect(RectQuad {
                    coverage: Coverage::Inside,
                    rect: screen,
                    corners: Corners::ZERO,
                    fill: BrushSource::Solid(RgbaF16::TRANSPARENT),
                    border: LoweredBorder::lower(BORDER),
                }),
                1.0,
            );
        }
    }
}
