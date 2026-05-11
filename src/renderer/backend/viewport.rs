//! Logical→physical viewport conversions for the wgpu backend.
//! Pure math; no GPU handles.

use crate::primitives::rect::Rect;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::ui::damage::Damage;
use crate::ui::damage::region::DAMAGE_RECT_CAP;

/// Pad the damage scissor by this many physical pixels on every
/// side. Quads and glyphs may anti-alias slightly outside their
/// nominal rect (SDF rounded-rect AA, italic descenders); without
/// padding the scissor would clip the AA fringe and leave a
/// 1-px-hard edge along the damage boundary.
const DAMAGE_AA_PADDING: u32 = 2;

/// Convert a logical-px damage rect to a physical-px scissor, padded
/// by [`DAMAGE_AA_PADDING`] on every side and clamped to the viewport.
/// Returns `None` if the result clamps to zero area — callers degrade
/// that case to "loaded but not drawn" inside the pass.
pub(super) fn logical_rect_to_phys_scissor(r: Rect, buffer: &RenderBuffer) -> Option<URect> {
    let phys = r.scaled_by(buffer.scale, true);
    let pad = DAMAGE_AA_PADDING as f32;
    let mins_x = (phys.min.x - pad).max(0.0) as u32;
    let mins_y = (phys.min.y - pad).max(0.0) as u32;
    let maxs_x = ((phys.min.x + phys.size.w + pad).max(0.0) as u32).min(buffer.viewport_phys.x);
    let maxs_y = ((phys.min.y + phys.size.h + pad).max(0.0) as u32).min(buffer.viewport_phys.y);
    if maxs_x > mins_x && maxs_y > mins_y {
        Some(URect::new(mins_x, mins_y, maxs_x - mins_x, maxs_y - mins_y))
    } else {
        None
    }
}

/// Fill `out` with the per-rect physical-px scissors for this frame.
/// `Full` and `Skip` leave it empty; `Partial(region)` produces one
/// entry per rect after physical-px scaling, AA padding, and viewport
/// clamping — rects that clamp to zero area are filtered out. If every
/// rect clamps to zero, the list ends up empty and the caller degrades
/// the frame to a Full repaint (correct, just wasteful — won't happen
/// in practice unless damage lies entirely outside the surface).
pub(super) fn build_damage_scissors(
    out: &mut tinyvec::ArrayVec<[URect; DAMAGE_RECT_CAP]>,
    damage: Damage,
    buffer: &RenderBuffer,
) {
    out.clear();
    if let Damage::Partial(region) = damage {
        for r in region.iter_rects() {
            if let Some(s) = logical_rect_to_phys_scissor(r, buffer) {
                out.push(s);
            }
        }
    }
}
