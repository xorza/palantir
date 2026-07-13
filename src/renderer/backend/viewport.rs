//! Viewport: CPU damage-rect → physical scissor math, plus the
//! [`ViewportPush`] carrier every shader's shared `Immediates`
//! region reads as `imm.viewport` (offset 0). The whole quad / curve
//! / mesh / image / text family shares the same immediate layout
//! ([`crate::renderer::backend::IMMEDIATES_BYTES`]), so a single `set_immediates(0, ..)`
//! per pass covers all of them — no bind group, no uniform buffer.

use crate::primitives::rect::Rect;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::ui::damage::region::DAMAGE_RECT_CAP;
use crate::ui::frame_report::{RenderKind, RenderPlan};
use encase::{ShaderSize, ShaderType, UniformBuffer};
use glam::Vec2;

/// Pad the damage scissor by this many physical pixels on every
/// side. Quads and glyphs may anti-alias slightly outside their
/// nominal rect (SDF rounded-rect AA, italic descenders); without
/// padding the scissor would clip the AA fringe and leave a
/// 1-px-hard edge along the damage boundary.
///
/// The backend's PreClear wipes this *padded* region, so the encoder's
/// damage subtree-cull must inflate its intersection test to match —
/// see [`damage_cull_margin`].
const DAMAGE_AA_PADDING: u32 = 2;

/// Convert a logical-px damage rect to a physical-px scissor, padded
/// by [`DAMAGE_AA_PADDING`] on every side and clamped to the viewport.
/// Returns `None` if the result clamps to zero area — callers degrade
/// that case to "loaded but not drawn" inside the pass.
fn logical_rect_to_phys_scissor(r: Rect, buffer: &RenderBuffer) -> Option<URect> {
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

/// Logical-px slack the encoder's damage subtree-cull inflates each
/// node's paint rect by, so the cull covers the *padded* region the
/// backend actually PreClears. The scissor is
/// `round(edge · scale) ± DAMAGE_AA_PADDING` physical px
/// ([`logical_rect_to_phys_scissor`]); the round can push each edge out
/// by ½ px, so the cleared region reaches `(DAMAGE_AA_PADDING + 0.5) /
/// scale` logical px past the raw damage rect. One extra px on top so a
/// strict `Rect::intersects` (touching doesn't count) can't drop a node
/// sitting exactly on that boundary — a node in the pad ring would be
/// cleared but culled from repaint, leaving a hard cut at the damage
/// edge. Owning the derivation here, next to the scissor math, keeps
/// the two from drifting.
pub(crate) fn damage_cull_margin(scale: f32) -> f32 {
    (DAMAGE_AA_PADDING as f32 + 1.0) / scale
}

/// Fill `out` with the per-rect physical-px scissors for this frame.
/// `Full` and `Skip` leave it empty; `Partial(region)` produces one
/// entry per rect after physical-px scaling, AA padding, and viewport
/// clamping. Region rects arrive surface-clipped and non-empty
/// (`DamageRegion::collapse_from`) and the AA padding keeps their
/// scissors nonzero, so `Partial` always yields at least one entry —
/// `WgpuBackend::submit` asserts that rather than degrade.
#[profiling::function]
pub(crate) fn build_damage_scissors(
    out: &mut tinyvec::ArrayVec<[URect; DAMAGE_RECT_CAP]>,
    plan: RenderPlan,
    buffer: &RenderBuffer,
) {
    out.clear();
    if let RenderKind::Partial { region } = plan.kind {
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

#[cfg(test)]
mod tests {
    use super::damage_cull_margin;

    #[test]
    fn damage_cull_margin_scales_inversely() {
        // (DAMAGE_AA_PADDING + 1) / scale: (2 + 1) / 1 = 3 logical px
        // at 1×, (2 + 1) / 2 = 1.5 at 2× (the physical pad is fixed,
        // so its logical equivalent shrinks as scale grows).
        assert_eq!(damage_cull_margin(1.0), 3.0);
        assert_eq!(damage_cull_margin(2.0), 1.5);
    }
}
