//! Viewport: CPU damage-rect → physical scissor math, and the [`ViewportPush`] carrier for `imm.viewport_size` (offset 0 of the shared immediates, [`crate::gpu::pipeline::IMMEDIATES_BYTES`]).

use crate::damage::Damage;
use crate::damage::region::DAMAGE_RECT_CAP;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::geometry::urect::URect;
use crate::renderer::pane_captures::PaneCaptures;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::PaneSource;
use crate::renderer::render_plan::RenderPlan;
use glam::Vec2;
use std::{iter, slice};
use tinyvec::ArrayVec;

#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "built once a frame on the stack; boxing it would allocate per frame"
)]
pub(crate) enum RepaintScissors {
    Full,
    Partial(PartialScissors),
}

impl RepaintScissors {
    /// The physical-px repaint shape of `damage`: `Full`, or `Partial` with the footprint of each pane that captures, as the target there still holds the last frame's pane, then the damage scissors after scaling, AA padding and clamping. Footprints go first, as a pane's damage, its own rect and what changed under it, lies inside its footprint and then adds no piece. Regions arrive non-empty and the padding keeps scissors nonzero; an empty result means the plan and draw list disagree and must not degrade to a full clear.
    ///
    /// The scissors are disjoint: each keeps only the pixels the ones before it left, split into
    /// at most four rects. The main pass replays the groups once per scissor, and after a pane's
    /// barrier it replays them without a clear, so a pixel two scissors share would composite
    /// every translucent draw past the barrier twice, as a padded damage rect inside a footprint
    /// did. `None` when the pieces outgrow [`SCISSOR_CAP`]: the frame then paints whole.
    pub(crate) fn new(damage: Damage, buffer: &RenderBuffer) -> Option<Self> {
        match damage {
            Damage::Full => Some(Self::Full),
            Damage::Partial(damage) => {
                let mut rects: ArrayVec<[URect; SCISSOR_CAP]> = ArrayVec::new();
                let damage_rects = damage
                    .region
                    .iter_rects()
                    .filter_map(|r| logical_rect_to_physical_scissor(r, buffer));
                let footprints = buffer
                    .backdrops
                    .iter()
                    .filter(|row| row.source == PaneSource::Capture)
                    .map(|row| row.footprint);
                for scissor in footprints.chain(damage_rects) {
                    let mut pieces: ArrayVec<[URect; SCISSOR_CAP]> = ArrayVec::new();
                    pieces.push(scissor);
                    for &kept in &rects {
                        let mut left: ArrayVec<[URect; SCISSOR_CAP]> = ArrayVec::new();
                        for piece in pieces {
                            for rest in piece.minus(kept) {
                                // `try_push` hands back what did not fit.
                                if left.try_push(rest).is_some() {
                                    return None;
                                }
                            }
                        }
                        pieces = left;
                    }
                    for piece in pieces {
                        if rects.try_push(piece).is_some() {
                            return None;
                        }
                    }
                }
                debug_assert!(
                    !rects.is_empty(),
                    "Partial plan produced no damage scissors"
                );
                Some(Self::Partial(PartialScissors { rects }))
            }
        }
    }
}

/// The non-empty, pairwise disjoint scissor list a `Partial` repaint walks. Non-emptiness is [`RepaintScissors::new`]'s `debug_assert!`; splitting off a `first` would cost an O(n) shift to build.
#[derive(Debug)]
pub(crate) struct PartialScissors {
    rects: ArrayVec<[URect; SCISSOR_CAP]>,
}

/// The damage rects, and a footprint for each pane a partial frame captures, each footprint room
/// for the four pieces a damage rect inside it leaves.
const SCISSOR_CAP: usize = DAMAGE_RECT_CAP + 4 * PaneCaptures::PARTIAL_CAP;

impl PartialScissors {
    pub(crate) fn len(&self) -> usize {
        self.rects.len()
    }

    pub(crate) fn iter(&self) -> iter::Copied<slice::Iter<'_, URect>> {
        self.rects.iter().copied()
    }
}

impl<'a> IntoIterator for &'a PartialScissors {
    type Item = URect;
    type IntoIter = iter::Copied<slice::Iter<'a, URect>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Convert a logical-px damage rect to a physical-px scissor, padded by [`RenderPlan::AA_PADDING`] and clamped to the viewport; `None` if it clamps to zero area.
fn logical_rect_to_physical_scissor(r: Rect, buffer: &RenderBuffer) -> Option<URect> {
    let scaled = r.scaled_by(buffer.display.scale_factor(), EdgeRounding::WholePixels);
    let padded = scaled.inflated(RenderPlan::AA_PADDING as f32);
    let physical = buffer.display.physical;
    URect::covering(padded).intersect(URect::new(0, 0, physical.x, physical.y))
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
        /// A partial repaint inside `rects`, as [`RepaintScissors::new`]
        /// would make one: they must be disjoint, as its are.
        pub(crate) fn partial(rects: &[URect]) -> Self {
            debug_assert!(
                rects
                    .iter()
                    .enumerate()
                    .all(|(i, a)| rects[i + 1..].iter().all(|b| !a.intersects(*b))),
                "repaint scissors are disjoint"
            );
            Self::Partial(PartialScissors {
                rects: rects.iter().copied().collect::<ArrayVec<_>>(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::damage::Damage;
    use crate::damage::region::DamageRegion;
    use crate::display::Display;
    use crate::display::frame_stamp::FrameStamp;
    use crate::gpu::surface::viewport::{RepaintScissors, ViewportPush};
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::geometry::urect::URect;
    use crate::primitives::identity::pane_key::PaneKey;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::renderer::render_buffer::RenderBuffer;
    use crate::renderer::render_buffer::backdrop_row::{BackdropRow, PaneSource};
    use crate::shape::paint::blur_plan::BlurPlan;
    use glam::{UVec2, Vec2};
    use std::time::Duration;

    fn buffer() -> RenderBuffer {
        let mut buffer = RenderBuffer::new();
        buffer.start_frame(FrameStamp::new(
            Display::from_physical(UVec2::new(100, 100), 2.0),
            Duration::ZERO,
        ));
        buffer
    }

    #[test]
    fn full_repaint_has_no_partial_scissors() {
        let repaint = RepaintScissors::new(Damage::Full, &buffer());
        assert!(matches!(repaint, Some(RepaintScissors::Full)));
    }

    /// Eight capturing footprints, bands across eight damage strips, which they split into eight
    /// pieces apiece: 8 + 8 × 8 = 72 scissors, past the cap of 40, so no partial repaint fits.
    #[test]
    fn scissors_past_the_cap_fit_no_partial_repaint() {
        let strips: Vec<Rect> = (0..8)
            .map(|i| Rect::new(1.0 + 6.0 * i as f32, 0.0, 1.0, 50.0))
            .collect();
        let damage = DamageRegion::collapse_from(&strips, 0.0, Rect::new(0.0, 0.0, 50.0, 50.0));
        assert_eq!(damage.region.iter_rects().count(), 8);
        let mut buffer = buffer();
        buffer.backdrops.extend((0..8).map(|j| BackdropRow {
            key: PaneKey {
                owner: WidgetId::from_hash("pane"),
                shape: j,
            },
            footprint: URect::new(0, 12 * j, 100, 6),
            plan: BlurPlan::new(2.0),
            barrier: j + 1,
            lens: None,
            max_luminance: 65504.0,
            source: PaneSource::Capture,
        }));
        assert!(RepaintScissors::new(Damage::Partial(damage), &buffer).is_none());
    }

    /// A partial repaint takes the footprint of each pane that captures, as the target there holds
    /// the last frame's pane, then each damage rect, scaled, padded and clamped, less what the
    /// footprints already cover: no pixel lies under two scissors, and damage inside a footprint
    /// adds no piece. A pane that keeps its result or shows the clear reads no pixel of the target,
    /// so its footprint adds none.
    #[test]
    fn partial_repaint_preserves_padded_physical_scissors() {
        let damage = DamageRegion::collapse_from(
            &[
                Rect::new(5.0, 5.0, 5.0, 5.0),
                Rect::new(30.0, 20.0, 10.0, 5.0),
                Rect::new(20.0, 18.0, 4.0, 4.0),
            ],
            0.0,
            Rect::new(0.0, 0.0, 50.0, 50.0),
        );
        // Three panes, one capturing: its footprint, already physical, joins the scissors first.
        let mut buffer = buffer();
        let pane = |shape, footprint, source| BackdropRow {
            key: PaneKey {
                owner: WidgetId::from_hash("pane"),
                shape,
            },
            footprint,
            plan: BlurPlan::new(2.0),
            barrier: shape,
            lens: None,
            max_luminance: 65504.0,
            source,
        };
        buffer.backdrops.extend([
            pane(0, URect::new(0, 80, 20, 20), PaneSource::Clear),
            pane(1, URect::new(30, 30, 40, 20), PaneSource::Capture),
            pane(2, URect::new(0, 60, 20, 20), PaneSource::Held),
        ]);
        let repaint = RepaintScissors::new(Damage::Partial(damage), &buffer);
        let Some(RepaintScissors::Partial(rects)) = repaint else {
            panic!("partial plan produced a full repaint");
        };
        // At 2x the rects are (10,10)-(20,20), (60,40)-(80,50) and (40,36)-(48,44); the 2px AA pad
        // extends each edge. The footprint, (30,30)-(70,50), holds the third whole, and of the
        // second leaves the band below it, (58,50)-(82,52), and the part right of it,
        // (70,38)-(82,50).
        assert_eq!(
            rects.iter().collect::<Vec<_>>(),
            [
                URect::new(30, 30, 40, 20),
                URect::new(8, 8, 14, 14),
                URect::new(58, 50, 24, 2),
                URect::new(70, 38, 12, 12),
            ]
        );
        assert!(matches!(
            RepaintScissors::new(Damage::Full, &buffer),
            Some(RepaintScissors::Full)
        ));
    }

    #[test]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "probes a debug_assert!, which release compiles out"
    )]
    #[should_panic(expected = "Partial plan produced no damage scissors")]
    fn partial_repaint_rejects_scissors_clamped_outside_viewport() {
        RepaintScissors::new(
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
