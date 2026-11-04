//! The physical scissors a frame repaints inside: its damage, and the footprint of each pane
//! that captures.

use crate::damage::Damage;
use crate::damage::region::{CollapsedDamage, DAMAGE_RECT_CAP};
use crate::display::Display;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::geometry::urect::URect;
use crate::renderer::pane_captures::PaneCaptures;
use crate::renderer::render_plan::RenderPlan;
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
    /// Whether the frame repaints inside rects, keeping the pixels around them.
    pub(crate) const fn is_partial(&self) -> bool {
        matches!(self, Self::Partial(_))
    }

    /// The physical-px repaint shape of `damage` on `display`: `Full`, or `Partial` with each of
    /// `footprints`, the physical footprint of each pane the frame may capture, as the target
    /// there still holds the last frame's pane, then the damage scissors after scaling, AA
    /// padding and clamping. Footprints go first, as a pane's damage, its own rect and what
    /// changed under it, lies inside its footprint and then adds no piece. Regions arrive
    /// non-empty and the padding keeps scissors nonzero; an empty result means the plan and draw
    /// list disagree and must not degrade to a full clear.
    ///
    /// Decided from the ledger's tickets before the frame builds, so a frame past the cap builds
    /// once, whole. A ticket that captures may end as a pane that holds, clears or hides; its
    /// footprint then repaints what the encoder kept under it, overdraw and no more. Refused:
    /// the footprints of the built frame's capturing rows, which a frame past the cap learned
    /// only by building twice.
    ///
    /// The scissors are disjoint: each keeps only the pixels the ones before it left, split into
    /// at most four rects. The main pass replays the groups once per scissor, and after a pane's
    /// barrier it replays them without a clear, so a pixel two scissors share would composite
    /// every translucent draw past the barrier twice, as a padded damage rect inside a footprint
    /// did. `None` when the pieces outgrow [`SCISSOR_CAP`]: the frame then paints whole.
    pub(crate) fn new(
        damage: Damage,
        display: &Display,
        footprints: impl IntoIterator<Item = URect>,
    ) -> Option<Self> {
        let Damage::Partial(damage) = damage else {
            return Some(Self::Full);
        };
        let mut held: ArrayVec<[URect; SCISSOR_CAP]> = ArrayVec::new();
        for footprint in footprints {
            if held.try_push(footprint).is_some() {
                return None;
            }
        }
        Self::partial_of(damage, display, &held)
    }

    /// [`Self::new`]'s partial shape, over `footprints` that fit the cap.
    fn partial_of(
        damage: CollapsedDamage,
        display: &Display,
        footprints: &[URect],
    ) -> Option<Self> {
        let mut rects: ArrayVec<[URect; SCISSOR_CAP]> = ArrayVec::new();
        let damage_rects = damage
            .region
            .iter_rects()
            .filter_map(|r| logical_rect_to_physical_scissor(r, display));
        for scissor in footprints.iter().copied().chain(damage_rects) {
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

/// The non-empty, pairwise disjoint scissor list a `Partial` repaint walks. Non-emptiness is [`RepaintScissors::new`]'s `debug_assert!`; splitting off a `first` would cost an O(n) shift to build.
#[derive(Debug)]
pub(crate) struct PartialScissors {
    rects: ArrayVec<[URect; SCISSOR_CAP]>,
}

/// The damage rects, and a footprint for each pane a partial frame captures, each footprint room
/// for the four pieces a damage rect inside it leaves.
const SCISSOR_CAP: usize = DAMAGE_RECT_CAP + 4 * PaneCaptures::PARTIAL_CAP;

impl PartialScissors {
    pub(crate) fn iter(&self) -> iter::Copied<slice::Iter<'_, URect>> {
        self.rects.iter().copied()
    }

    /// The least rect that holds every scissor.
    pub(crate) fn bounds(&self) -> URect {
        self.iter().fold(URect::ZERO, URect::union)
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
fn logical_rect_to_physical_scissor(r: Rect, display: &Display) -> Option<URect> {
    let scaled = r.scaled_by(display.scale_factor(), EdgeRounding::WholePixels);
    let padded = scaled.inflated(RenderPlan::AA_PADDING as f32);
    let physical = display.physical;
    URect::covering(padded).intersect(URect::new(0, 0, physical.x, physical.y))
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::geometry::urect::URect;
    use crate::renderer::repaint_scissors::{PartialScissors, RepaintScissors};
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
mod tests;
