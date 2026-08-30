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
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::corners::Corners;
use crate::primitives::stroke::Stroke;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::scene::forest::Forest;

/// Magenta — distinct from the opt-in red damage-rect overlay. Painted
/// unclipped at the end of `encode`, after every layer's regular paint.
const STROKE: Stroke = Stroke::solid(Color::rgb(1.0, 0.0, 1.0), 3.0);

/// Final pass: emit a magenta outline for each explicit-id collision
/// recorded this frame. Painted after the regular per-layer walk so
/// it sits on top of everything; emitted with no scissor push so it
/// ignores any clip context the colliding widgets sit under (scroll
/// viewports, clipped popups). Both `NodeId`s are precomputed at
/// recording time (`SeenIds.curr` hashmap lookup) — no tree scan.
///
/// Development-only. A duplicate explicit `WidgetId` is a caller bug,
/// not a rendering property to inspect the way the `DebugOverlayConfig`
/// overlays are — the developer wants the outline while building, and a
/// shipped app wants neither it over its UI nor the branch. The
/// `tracing::error!` in `Forest::report_explicit_collision` is what
/// survives into release, and it carries the diagnosis anyway.
pub(super) fn emit(forest: &Forest, layout: &Layout, out: &mut dyn PaintSink) {
    for record in &forest.collisions {
        for ep in [record.first, record.second] {
            let rects = &layout[ep.layer].rect;
            // Both endpoints carry the `NodeId` `Tree::open_node`
            // assigned, so arrange produced a rect for each — the index
            // cannot exceed `rects`. The assert is the whole guard: this
            // module is `#[cfg(debug_assertions)]`, so there is no build
            // that compiles it with the assert compiled out.
            debug_assert!(
                ep.node.idx() < rects.len(),
                "collision endpoint {:?} out of bounds for layer rects len {}",
                ep.node,
                rects.len(),
            );
            out.draw_quad(DrawQuadPayload::rect(
                rects[ep.node.idx()],
                Corners::ZERO,
                BrushSource::Solid(ColorF16::TRANSPARENT),
                STROKE.into(),
            ));
        }
    }
}
