//! The explicit-`WidgetId` collision overlay.
//!
//! A development-only module: a duplicate explicit id is a caller bug,
//! not a rendering property to inspect the way the `DebugOverlayConfig`
//! overlays are. The developer wants the outline while building; a
//! shipped app wants neither it over its UI nor the branch that tests
//! for it. `Forest::report_explicit_collision`'s `tracing::error!` is
//! what survives into release, and it carries the diagnosis anyway.
//!
//! Gated at the `mod` declaration rather than per item, so the imports
//! this needs — and the magenta stroke — leave the release build with
//! it instead of becoming dead weight behind `#[cfg]`s.

use crate::layout::Layout;
use crate::primitives::color::RgbaF32;
use crate::primitives::color::rgba_f16::RgbaF16;
use crate::primitives::corners::Corners;
use crate::primitives::stroke::Stroke;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::scene::cascade::Cascade;
use crate::scene::forest::Forest;

/// Magenta — distinct from the opt-in red damage-rect overlay. Painted
/// unclipped at the end of `encode`, after every layer's regular paint.
const STROKE: Stroke = Stroke::new(RgbaF32::srgb(1.0, 0.0, 1.0), 3.0);

/// Final pass: emit a magenta outline for each explicit-id collision
/// recorded this frame. Painted after the regular per-layer walk so
/// it sits on top of everything; emitted with no scissor push and no
/// transform, so each outline is the node's screen rect — its layout
/// rect through every ancestor transform, unclipped — and ignores any
/// clip context the colliding widgets sit under (scroll viewports,
/// clipped popups). Both `NodeId`s are precomputed at recording time
/// (`SeenIds.curr` hashmap lookup) — no tree scan.
///
/// Development-only. A duplicate explicit `WidgetId` is a caller bug,
/// not a rendering property to inspect the way the `DebugOverlayConfig`
/// overlays are — the developer wants the outline while building, and a
/// shipped app wants neither it over its UI nor the branch. The
/// `tracing::error!` in `Forest::report_explicit_collision` is what
/// survives into release, and it carries the diagnosis anyway.
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
