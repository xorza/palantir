//! Viewport: CPU damage-rect → physical scissor math, plus the
//! [`ViewportPush`] carrier every shader's shared `Immediates`
//! region reads as `imm.viewport` (offset 0). The whole quad / curve
//! / mesh / image / text family shares the same immediate layout
//! ([`super::IMMEDIATES_BYTES`]), so a single `set_immediates(0, ..)`
//! per pass covers all of them — no bind group, no uniform buffer.

use crate::primitives::rect::Rect;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::ui::damage::region::DAMAGE_RECT_CAP;
use crate::ui::frame_report::RenderPlan;
use encase::{ShaderSize, ShaderType, UniformBuffer};
use glam::Vec2;

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
#[profiling::function]
pub(super) fn build_damage_scissors(
    out: &mut tinyvec::ArrayVec<[URect; DAMAGE_RECT_CAP]>,
    plan: RenderPlan,
    buffer: &RenderBuffer,
) {
    out.clear();
    if let RenderPlan::Partial { region, .. } = plan {
        for r in region.iter_rects() {
            if let Some(s) = logical_rect_to_phys_scissor(r, buffer) {
                out.push(s);
            }
        }
    }
}

/// Viewport size as it appears in the shared immediate. 8 bytes;
/// occupies offset 0 of every pipeline's immediate region (see
/// `Immediates` in each shader). Encodes through `encase` to follow
/// WGSL alignment rules.
#[derive(Copy, Clone, Debug, ShaderType)]
pub(crate) struct ViewportPush {
    pub(crate) size: Vec2,
}

impl ViewportPush {
    pub(crate) const BYTES: usize = Self::SHADER_SIZE.get() as usize;
    /// Offset inside the per-pipeline immediate region. Locked at 0
    /// because every shader puts `viewport` first.
    pub(crate) const OFFSET: u32 = 0;

    pub(crate) fn encode(&self) -> [u8; Self::BYTES] {
        let mut out = [0u8; Self::BYTES];
        UniformBuffer::new(&mut out[..]).write(self).unwrap();
        out
    }

    /// Push this viewport into the active pipeline's immediate region.
    /// Caller must ensure a pipeline is already bound — wgpu's
    /// `set_immediates` validation rejects an unbound pipeline.
    pub(crate) fn push_into(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_immediates(Self::OFFSET, &self.encode());
    }
}
