//! Viewport: CPU damage-rect → physical scissor math, plus the
//! [`ViewportPush`] carrier every shader's shared `Immediates`
//! region reads as `imm.viewport` (offset 0). The whole quad / curve
//! / mesh / image / text family shares the same immediate layout
//! ([`crate::renderer::backend::IMMEDIATES_BYTES`]), so a single `set_immediates(0, ..)`
//! per pass covers all of them — no bind group, no uniform buffer.

use crate::primitives::rect::Rect;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_plan::{RenderKind, RenderPlan};
use crate::scene::damage::region::DAMAGE_RECT_CAP;
use glam::Vec2;
use tinyvec::ArrayVec;

#[derive(Debug)]
pub(super) enum RepaintScissors {
    Full,
    Partial(PartialScissors),
}

/// The non-empty scissor list a `Partial` repaint walks, one pass walk
/// per rect.
///
/// Non-emptiness is a real invariant — a `Partial` plan with nothing to
/// scissor would load the backbuffer and draw nothing — but it is carried
/// by the constructor's `assert!` rather than the field layout. Splitting
/// a `first` off the array would restate the same guarantee while costing
/// an O(n) shift to build and a chained iterator to read.
#[derive(Debug)]
pub(super) struct PartialScissors {
    rects: ArrayVec<[URect; DAMAGE_RECT_CAP]>,
}

impl PartialScissors {
    fn new(rects: ArrayVec<[URect; DAMAGE_RECT_CAP]>) -> Self {
        assert!(
            !rects.is_empty(),
            "Partial plan produced no damage scissors"
        );
        Self { rects }
    }

    pub(super) fn len(&self) -> usize {
        self.rects.len()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = URect> + '_ {
        self.rects.iter().copied()
    }
}

/// Convert a logical-px damage rect to a physical-px scissor, padded
/// by [`RenderPlan::AA_PADDING`] on every side and clamped to the viewport.
/// Returns `None` if the result clamps to zero area — callers degrade
/// that case to "loaded but not drawn" inside the pass.
fn logical_rect_to_phys_scissor(r: Rect, buffer: &RenderBuffer) -> Option<URect> {
    let phys = r.scaled_by(buffer.scale, true);
    let pad = RenderPlan::AA_PADDING as f32;
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

/// Build the physical-px repaint shape for this frame. `Full` stays
/// distinct from `Partial`, which carries one or more scissors after
/// physical-px scaling, AA padding, and viewport clamping. Region rects
/// arrive surface-clipped and non-empty
/// (`DamageRegion::collapse_from`) and the AA padding keeps their
/// scissors nonzero. An empty result means the plan and composed draw
/// list disagree; it must not degrade to a full clear.
pub(super) fn build_repaint_scissors(
    render_kind: RenderKind,
    buffer: &RenderBuffer,
) -> RepaintScissors {
    match render_kind {
        RenderKind::Full => RepaintScissors::Full,
        RenderKind::Partial { damage } => {
            let mut rects = ArrayVec::new();
            for r in damage.region.iter_rects() {
                if let Some(s) = logical_rect_to_phys_scissor(r, buffer) {
                    rects.push(s);
                }
            }
            RepaintScissors::Partial(PartialScissors::new(rects))
        }
    }
}

/// Viewport size as it appears in the shared immediate. 8 bytes;
/// occupies offset 0 of every pipeline's immediate region (see
/// `Immediates` in each shader).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ViewportPush {
    pub(crate) size: Vec2,
}

impl ViewportPush {
    pub(super) const BYTES: usize = size_of::<Self>();
    /// Offset inside the per-pipeline immediate region. Locked at 0
    /// because every shader puts `viewport` first.
    pub(super) const OFFSET: u32 = 0;

    pub(super) fn encode(&self) -> [u8; Self::BYTES] {
        bytemuck::cast(*self)
    }

    /// Push this viewport into the active pipeline's immediate region.
    /// Caller must ensure a pipeline is already bound — wgpu's
    /// `set_immediates` validation rejects an unbound pipeline.
    pub(super) fn push_into(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_immediates(Self::OFFSET, &self.encode());
    }
}

const _: () = assert!(
    ViewportPush::BYTES == 2 * size_of::<f32>(),
    "ViewportPush must match the shader's vec2<f32> viewport layout",
);

#[cfg(test)]
mod tests {
    use crate::display::Display;
    use crate::primitives::rect::Rect;
    use crate::primitives::urect::URect;
    use crate::renderer::backend::viewport::{
        RepaintScissors, ViewportPush, build_repaint_scissors,
    };
    use crate::renderer::render_buffer::RenderBuffer;
    use crate::renderer::render_plan::RenderKind;
    use crate::scene::damage::region::DamageRegion;
    use glam::{UVec2, Vec2};
    use std::time::Duration;

    fn buffer() -> RenderBuffer {
        let mut buffer = RenderBuffer::new();
        buffer.start_frame(
            Display::from_physical(UVec2::new(100, 100), 2.0),
            Duration::ZERO,
        );
        buffer
    }

    #[test]
    fn full_repaint_has_no_partial_scissors() {
        let repaint = build_repaint_scissors(RenderKind::Full, &buffer());
        assert!(matches!(repaint, RepaintScissors::Full));
    }

    #[test]
    fn partial_repaint_preserves_padded_physical_scissors() {
        let damage = DamageRegion::collapse_from(
            &[
                Rect::new(5.0, 5.0, 5.0, 5.0),
                Rect::new(30.0, 20.0, 10.0, 5.0),
            ],
            0.0,
            Rect::new(0.0, 0.0, 50.0, 50.0),
        );
        let repaint = build_repaint_scissors(RenderKind::Partial { damage }, &buffer());
        let RepaintScissors::Partial(rects) = repaint else {
            panic!("partial plan produced a full repaint");
        };
        // At 2x, the rects are (10,10)-(20,20) and (60,40)-(80,50).
        // Extending each edge by the 2px AA pad gives these exact scissors.
        assert_eq!(
            rects.iter().collect::<Vec<_>>(),
            [URect::new(8, 8, 14, 14), URect::new(58, 38, 24, 14),]
        );
    }

    #[test]
    #[should_panic(expected = "Partial plan produced no damage scissors")]
    fn partial_repaint_rejects_scissors_clamped_outside_viewport() {
        build_repaint_scissors(
            RenderKind::Partial {
                damage: DamageRegion::from(Rect::new(200.0, 200.0, 10.0, 10.0)).unmeasured(),
            },
            &buffer(),
        );
    }

    #[test]
    fn viewport_immediate_is_two_native_endian_floats() {
        let encoded = ViewportPush {
            size: Vec2::new(1.5, -2.25),
        }
        .encode();
        let mut expected = [0; ViewportPush::BYTES];
        expected[..4].copy_from_slice(&1.5_f32.to_ne_bytes());
        expected[4..].copy_from_slice(&(-2.25_f32).to_ne_bytes());
        assert_eq!(encoded, expected);
    }
}
