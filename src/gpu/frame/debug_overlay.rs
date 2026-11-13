//! Debug-overlay quads: the full-viewport dim quad (before partial passes, when
//! `DebugOverlayConfig::dim_undamaged`) and the damage-rect outline quads (after the
//! backbuffer-to-surface copy, when `damage_rect`). Both ride the frame's quad stream and the
//! quad pipeline's solid pipeline and the frame group, passed in as an [`OverlayDraw`].

use crate::damage::Damage;
use crate::damage::region::DAMAGE_RECT_CAP;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
use crate::gpu::pipeline::viewport_push::ViewportPush;
use crate::gpu::resource::pipeline_recipe::Pipeline;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::wgsl::FRAME_GROUP;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::quad_draw_row::QuadInstance;
use glam::Vec2;
use std::ops::Range;
use tinyvec::ArrayVec;

/// Damage-rect outline border color: bright opaque red, not theme-driven.
const DAMAGE_OVERLAY_COLOR: RgbaF32 = RgbaF32::srgb(1.0, 0.0, 0.0);

/// Damage-rect outline border width in logical px, scaled by
/// `Display::scale_factor()` at submit.
const DAMAGE_OVERLAY_BORDER_WIDTH: f32 = 2.0;

/// Gap between outline and damage edge in logical px. `Partial` rects outset by
/// it so thin damage (a 1px caret) stays visible; the full-viewport outline
/// insets by it to stay on-screen.
const DAMAGE_OVERLAY_GAP: f32 = 1.0;

/// Linear-space alpha of the `dim_undamaged` fill. With premultiplied blending
/// 40% alpha leaves 60% of the underlying pixel per Partial frame.
const DIM_ALPHA: f32 = 0.4;

/// What every debug-overlay draw binds: the solid quad pipeline with the frame group, and the
/// viewport.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OverlayDraw<'a> {
    pub(crate) quad: &'a QuadPipeline,
    pub(crate) pipeline: &'a Pipeline,
    pub(crate) frame_group: &'a wgpu::BindGroup,
    pub(crate) viewport: ViewportPush,
}

impl<'a> OverlayDraw<'a> {
    /// Draw `instances` of the frame's quad stream in `pass`, which has bound nothing.
    pub(crate) fn quads(self, pass: wgpu::RenderPass<'a>, instances: Range<u32>) {
        let mut rec = PassRecorder::new(pass, self.viewport);
        rec.group(FRAME_GROUP, self.frame_group);
        self.quad.draw_boxes(&mut rec, self.pipeline, instances);
    }
}

/// The translucent-black full-viewport quad ([`DIM_ALPHA`]), drawn with `LoadOp::Load`
/// before partial passes so undamaged regions fade across frames while repainted damage
/// stays bright.
pub(crate) fn dim_quad(viewport: Vec2) -> QuadInstance {
    QuadInstance {
        fill: RgbaF32::new(0.0, 0.0, 0.0, DIM_ALPHA).to_f16(),
        ..QuadInstance::of_rect(Rect::new(0.0, 0.0, viewport.x, viewport.y))
    }
}

/// The overlay's quads, which ride the frame's quad stream
/// ([`QuadExtras`](crate::gpu::pipeline::quad_pipeline::QuadExtras)).
#[derive(Debug, Default)]
pub(crate) struct DebugOverlay {
    /// Damage-rect outline quads (transparent fill, red border), drawn on the
    /// swapchain texture after the backbuffer-to-surface copy, so no ghosts.
    /// Bounded by [`DAMAGE_RECT_CAP`], so steady-state frames are alloc-free.
    outlines: ArrayVec<[QuadInstance; DAMAGE_RECT_CAP]>,
}

impl DebugOverlay {
    /// Build this frame's damage-rect outline quads: `Partial` gives one per region rect,
    /// `Full` one full-viewport outline.
    pub(crate) fn build_outlines(&mut self, damage: Damage, buffer: &RenderBuffer) {
        let scale = buffer.display.scale_factor();
        let gap_px = (DAMAGE_OVERLAY_GAP * scale).max(1.0);
        let border_color = DAMAGE_OVERLAY_COLOR.to_f16();
        let border_width = DAMAGE_OVERLAY_BORDER_WIDTH * scale;
        let outline = |rect: Rect| QuadInstance {
            fill: RgbaF16::TRANSPARENT,
            border_color,
            border_width,
            ..QuadInstance::of_rect(rect)
        };
        self.outlines.clear();
        match damage {
            Damage::Partial(damage) => {
                // Outset, not inset: damage thinner than `2 * gap_px` (a 1px caret) would
                // collapse to zero area. The pass is unscissored and the surface clips the
                // spill.
                for r in damage.region.iter_rects() {
                    self.outlines.push(outline(
                        r.scaled_by(scale, EdgeRounding::WholePixels)
                            .inflated(gap_px),
                    ));
                }
            }
            // The full-viewport outline insets instead: outsetting would push it off-screen.
            Damage::Full => {
                let physical = buffer.display.physical.as_vec2();
                self.outlines.push(outline(
                    Rect::new(0.0, 0.0, physical.x, physical.y).deflated(gap_px),
                ));
            }
        }
    }

    /// Forget the last outlines, for a frame that draws none.
    pub(crate) fn clear_outlines(&mut self) {
        self.outlines.clear();
    }

    /// This frame's damage-rect outlines.
    pub(crate) fn outlines(&self) -> &[QuadInstance] {
        &self.outlines
    }
}
