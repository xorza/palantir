//! The explicit-`WidgetId` collision overlay. Development-only: a duplicate id is a caller bug, and `Forest::report_explicit_collision`'s `tracing::error!` survives into release. Gated at the `mod` declaration so its imports leave the release build too.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::frontend::FrameScene;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_quad_payload::{DrawQuadPayload, RectQuad};
use crate::shape::rect::RectKind;

/// Magenta, distinct from the red damage-rect overlay. Painted unclipped after every layer.
const STROKE: Stroke = Stroke::new(RgbaF32::srgb(1.0, 0.0, 1.0), 3.0);

/// Final pass: a magenta outline per explicit-id collision recorded this frame, over everything. Emitted with no scissor or transform, so each outline is the node's unclipped screen rect. `NodeId`s are precomputed at recording time (`SeenIds.curr`).
pub(super) fn emit(scene: &FrameScene<'_>, out: &mut impl PaintSink) {
    for record in &scene.forest.collisions {
        for ep in [record.first, record.second] {
            let rect = scene.layout[ep.layer].rect[ep.node.idx()];
            let screen = scene.cascade.entry_at(ep).transform.apply_rect(rect);
            out.draw_quad(
                DrawQuadPayload::rect(RectQuad {
                    kind: RectKind::Rounded,
                    rect: screen,
                    corners: Corners::ZERO,
                    fill: BrushSource::Solid(RgbaF16::TRANSPARENT),
                    stroke: STROKE.into(),
                }),
                1.0,
            );
        }
    }
}
