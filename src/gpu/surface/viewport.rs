//! Viewport: CPU damage-rect → physical scissor math, and the [`ViewportPush`] carrier for `imm.viewport_size` (offset 0 of the shared immediates, [`crate::gpu::pipeline::IMMEDIATES_BYTES`]).

use crate::damage::Damage;
use crate::damage::region::DAMAGE_RECT_CAP;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_plan::RenderPlan;
use glam::Vec2;
use tinyvec::ArrayVec;

#[derive(Debug)]
pub(crate) enum RepaintScissors {
    Full,
    Partial(PartialScissors),
}

/// The non-empty scissor list a `Partial` repaint walks. Non-emptiness is the constructor's `debug_assert!`; splitting off a `first` would cost an O(n) shift to build.
#[derive(Debug)]
pub(crate) struct PartialScissors {
    rects: ArrayVec<[URect; DAMAGE_RECT_CAP]>,
}

impl PartialScissors {
    fn new(rects: ArrayVec<[URect; DAMAGE_RECT_CAP]>) -> Self {
        debug_assert!(
            !rects.is_empty(),
            "Partial plan produced no damage scissors"
        );
        Self { rects }
    }

    pub(crate) fn len(&self) -> usize {
        self.rects.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = URect> + '_ {
        self.rects.iter().copied()
    }
}

/// Convert a logical-px damage rect to a physical-px scissor, padded by [`RenderPlan::AA_PADDING`] and clamped to the viewport; `None` if it clamps to zero area.
fn logical_rect_to_phys_scissor(r: Rect, buffer: &RenderBuffer) -> Option<URect> {
    let phys = r.scaled_by(buffer.display.scale_factor(), true);
    let padded = phys.inflated(RenderPlan::AA_PADDING as f32);
    let physical = buffer.display.physical;
    URect::covering(padded).intersect(URect::new(0, 0, physical.x, physical.y))
}

/// Build the physical-px repaint shape: `Full`, or `Partial` with scissors after scaling, AA padding and clamping. Regions arrive non-empty and the padding keeps scissors nonzero; an empty result means the plan and draw list disagree and must not degrade to a full clear.
pub(crate) fn build_repaint_scissors(damage: Damage, buffer: &RenderBuffer) -> RepaintScissors {
    match damage {
        Damage::Full => RepaintScissors::Full,
        Damage::Partial(damage) => {
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

/// Viewport size as it appears in the shared immediate (8 bytes, offset 0; see `Immediates` in `prelude.wgsl`).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ViewportPush {
    pub(crate) size: Vec2,
}

impl ViewportPush {
    pub(crate) const BYTES: usize = size_of::<Self>();
    /// Offset 0: the shared prelude puts `viewport_size` first.
    pub(super) const OFFSET: u32 = 0;

    /// The viewport every pass of one frame pushes, so main and overlay passes agree.
    pub(crate) fn for_buffer(buffer: &RenderBuffer) -> Self {
        Self {
            size: buffer.display.physical.as_vec2(),
        }
    }

    pub(super) fn encode(self) -> [u8; Self::BYTES] {
        bytemuck::cast(self)
    }

    /// Push this viewport into the active pipeline's immediate region; a pipeline must be bound.
    pub(crate) fn push_into(self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_immediates(Self::OFFSET, &self.encode());
    }
}

const _: () = assert!(
    ViewportPush::BYTES == 2 * size_of::<f32>(),
    "ViewportPush must match the shader's vec2<f32> viewport layout",
);

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::surface::viewport::{PartialScissors, RepaintScissors};
    use crate::primitives::geometry::urect::URect;
    use tinyvec::ArrayVec;

    impl RepaintScissors {
        /// A partial repaint inside `rects`, as `build_repaint_scissors`
        /// would make one.
        pub(crate) fn partial(rects: &[URect]) -> Self {
            Self::Partial(PartialScissors::new(
                rects.iter().copied().collect::<ArrayVec<_>>(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::damage::Damage;
    use crate::damage::region::DamageRegion;
    use crate::display::Display;
    use crate::gpu::surface::viewport::{RepaintScissors, ViewportPush, build_repaint_scissors};
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::geometry::urect::URect;
    use crate::renderer::render_buffer::RenderBuffer;
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
        let repaint = build_repaint_scissors(Damage::Full, &buffer());
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
        let repaint = build_repaint_scissors(Damage::Partial(damage), &buffer());
        let RepaintScissors::Partial(rects) = repaint else {
            panic!("partial plan produced a full repaint");
        };
        // At 2x the rects are (10,10)-(20,20) and (60,40)-(80,50); the 2px AA pad extends each edge.
        assert_eq!(
            rects.iter().collect::<Vec<_>>(),
            [URect::new(8, 8, 14, 14), URect::new(58, 38, 24, 14),]
        );
    }

    #[test]
    #[should_panic(expected = "Partial plan produced no damage scissors")]
    fn partial_repaint_rejects_scissors_clamped_outside_viewport() {
        build_repaint_scissors(
            Damage::Partial(DamageRegion::from(Rect::new(200.0, 200.0, 10.0, 10.0)).unmeasured()),
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
