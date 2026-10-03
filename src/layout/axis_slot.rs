//! The per-axis inputs the measure pass resolves an outer extent from.

use crate::layout::measured::Measured;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::primitives::spacing::Sums;
use crate::scene::node::layout_core::LayoutCore;

/// What a parent grants one axis of one node, in the six numbers plus
/// `Sizing` that decide the axis' extent.
///
/// One slot per axis, built once and read twice: [`Self::inner_avail`]
/// derives what the driver measures against, and [`Self::resolve`] folds
/// the driver's answer back into the node's own extent. Two lanes of one
/// rule, rather than one rule written per lane.
///
/// `available` and what [`Self::resolve`] returns are margin-inclusive;
/// everything in between is margin-exclusive.
#[derive(Clone, Copy, Debug)]
pub(super) struct AxisSlot {
    pub(super) sizing: Sizing,
    pub(super) available: f32,
    pub(super) intrinsic_min: f32,
    pub(super) margin: f32,
    pub(super) min: f32,
    pub(super) max: f32,
}

impl AxisSlot {
    /// The extent this node's own box takes before its padding — a Fixed
    /// axis' own value, otherwise what is left of the parent's grant past
    /// the margin — clamped to `[min, max]`.
    ///
    /// The clamp matches [`Self::resolve`]'s, so a child's `available`
    /// tracks the parent's eventual arranged extent: a `max_size`-capped
    /// parent must not grant children more room than it can later
    /// arrange.
    #[inline]
    const fn outer(self, dispatch_avail: f32) -> f32 {
        match self.sizing.fixed_value() {
            Some(value) => value,
            None => (dispatch_avail - self.margin).max(0.0),
        }
        .clamp(self.min, self.max)
    }

    /// What the driver measures its children against on this axis: the
    /// outer extent above, less `padding`.
    ///
    /// `available` is floored by `intrinsic_min` first, so children
    /// measure against the parent's actual outer size. Without it a Hug
    /// grid inside a FILL panel whose own `intrinsic_min` is pinned by a
    /// long sibling would shape children against the smaller surface
    /// width. INFINITY on a Hug axis survives (`INF.max(x) == INF`); a
    /// Fixed axis reads neither input.
    #[inline]
    const fn inner_avail(self, padding: f32) -> f32 {
        (self.outer(self.available.max(self.intrinsic_min)) - padding).max(0.0)
    }
    /// **Contains-content rule:** Hug aims for content size, Fill aims
    /// for `available`. Fill floors at `max(content, intrinsic_min)`.
    /// Hug, under a finite `available`, caps its content at what is
    /// available and floors at `max(floor, intrinsic_min)` — so a Hug
    /// rect shrinks what can give way, a scroll on its panned axis, but
    /// never below what its content takes at the constraints it was
    /// measured under. If the floor exceeds `available`, the node
    /// overflows its parent rather than its content overflowing the
    /// node's rect. Downstream (cascade/composer/backend) tolerates
    /// overflow, same as the root-vs-surface case.
    ///
    /// `content` and `floor` here are the post-dispatch measured content
    /// and its floor (margin-exclusive). Both already reflect wrapping
    /// under the constrained available width, so on the block axis of a
    /// wrapping text leaf the floor is the multi-line height — unlike
    /// `intrinsic_min`, which is computed pure-subtree at `available =
    /// INFINITY` and only captures the single-line case. `intrinsic_min`
    /// still floors both sizings for what the measure cannot see, rigid
    /// descendants of a `Fill` track among them.
    ///
    /// The two cases where desired exceeds `available`:
    /// `max(floor, intrinsic_min) > available` (rigid descendant or
    /// post-wrap content doesn't fit) or `Sizing::fixed(v)`. An explicit
    /// `min_size` floor applies on top of all three branches via the
    /// trailing `clamp`.
    ///
    /// `Fill` on an unconstrained axis (intrinsic queries with
    /// `available = INFINITY`) collapses to its content size — matches
    /// CSS Grid's `1fr` track in an auto-context parent.
    pub(super) const fn resolve(self, content_plus_padding: f32, floor_plus_padding: f32) -> f32 {
        let rendered = if let Some(value) = self.sizing.fixed_value() {
            value
        } else if self.sizing.is_hug() {
            if self.available.is_finite() {
                content_plus_padding
                    .min(self.available - self.margin)
                    .max(floor_plus_padding)
                    .max(self.intrinsic_min - self.margin)
            } else {
                content_plus_padding
            }
        } else {
            // WPF Stretch: Fill returns content at measure-time. The
            // "fill the slot" expansion happens at *arrange* — driver
            // arrange code redistributes leftover to Fill children
            // proportionally. Returning `available` here would balloon
            // any Hug ancestor to its grandparent's allocation (CSS auto-
            // sizing's classic Hug+Fill bug).
            content_plus_padding.max(self.intrinsic_min - self.margin)
        };
        rendered.max(0.0).clamp(self.min, self.max) + self.margin
    }

    /// The node's own floor on this axis, margin-inclusive: the floor of
    /// its content with padding, or its fixed value, under the same
    /// clamps as [`Self::resolve`] — and never more than `resolved`, what
    /// that answered.
    const fn floor(self, floor_plus_padding: f32, resolved: f32) -> f32 {
        let rendered = match self.sizing.fixed_value() {
            Some(value) => value,
            None => floor_plus_padding.max(self.intrinsic_min - self.margin),
        };
        (rendered.max(0.0).clamp(self.min, self.max) + self.margin).min(resolved)
    }

    /// The least finite `available` from which this axis' measure holds
    /// unchanged — see [`Measured`] — given the driver's own
    /// `inner_stable_from` and the content and floor it answered,
    /// padding-inclusive.
    ///
    /// A Fixed axis reads no offer. Otherwise two things must hold. The
    /// node's own fold: a Fill axis does not read `available`, and a Hug
    /// axis reads it only through its cap — free once the offer is past
    /// the content, while a content at or below its floors resolves to
    /// the floors under every finite offer. And the driver's: what this node
    /// hands its children must not fall below `inner_stable_from`, which
    /// the `intrinsic_min` and `min` floors on that hand-down may already
    /// guarantee whatever the offer.
    ///
    /// Finite offers only: an unbounded Hug axis skips its floors, so an
    /// offer of `INFINITY` is served by the cache only where it was
    /// measured.
    const fn stable_from(
        self,
        content_plus_padding: f32,
        floor_plus_padding: f32,
        inner_stable_from: f32,
        padding: f32,
    ) -> f32 {
        if self.sizing.fixed_value().is_some() {
            return 0.0;
        }
        let own = if self.sizing.is_hug() {
            let floors = floor_plus_padding.max(self.intrinsic_min - self.margin);
            if content_plus_padding <= floors {
                if self.available.is_finite() || content_plus_padding == floors {
                    0.0
                } else {
                    Measured::AT_OFFER_ONLY
                }
            } else if content_plus_padding <= self.available - self.margin {
                content_plus_padding + self.margin
            } else {
                Measured::AT_OFFER_ONLY
            }
        } else {
            0.0
        };
        let handed = inner_stable_from + padding;
        let handed_floor = (self.intrinsic_min - self.margin)
            .max(0.0)
            .clamp(self.min, self.max);
        let inner = if inner_stable_from <= 0.0 || handed_floor >= handed {
            0.0
        } else {
            handed + self.margin
        };
        own.max(inner)
    }

    /// Full per-node sizing pipeline: build a slot per axis, hand the
    /// driver what each says its children measure against, and fold the
    /// driver's raw content, floor and range back through the pair into a
    /// margin-inclusive `desired`, floor and range.
    ///
    /// Per-node padding/margin sums are unpacked once and threaded
    /// through both halves, which is only possible because the dispatch
    /// is single-shot.
    ///
    /// Single dispatch: when `desired` exceeds `available` on a non-Fixed
    /// axis it is because a rigid descendant pinned the floor; a
    /// re-dispatch against the grown outer would converge to the same
    /// value, because every driver's content size is monotone in
    /// `available` and pass 1 already saturated at the floor. Pinned by
    /// `cross_driver_tests::convergence`.
    #[inline]
    pub(super) fn resolve_node(
        layout: LayoutCore,
        available: Size,
        intrinsic_min: Size,
        min_size: Size,
        max_size: Size,
        dispatch: impl FnOnce(Size) -> Measured,
    ) -> Measured {
        let Sums {
            horizontal: p_horiz,
            vertical: p_vert,
        } = layout.padding.sums();
        let Sums {
            horizontal: m_horiz,
            vertical: m_vert,
        } = layout.margin.sums();

        let w = Self {
            sizing: layout.size.w(),
            available: available.w,
            intrinsic_min: intrinsic_min.w,
            margin: m_horiz,
            min: min_size.w,
            max: max_size.w,
        };
        let h = Self {
            sizing: layout.size.h(),
            available: available.h,
            intrinsic_min: intrinsic_min.h,
            margin: m_vert,
            min: min_size.h,
            max: max_size.h,
        };

        let inner_avail = Size::new(w.inner_avail(p_horiz), h.inner_avail(p_vert));
        let Measured {
            size,
            floor,
            stable_from,
        } = dispatch(inner_avail);
        debug_assert!(
            floor.w <= size.w && floor.h <= size.h,
            "a driver's floor {floor:?} exceeds its content {size:?}",
        );
        debug_assert!(
            (stable_from.w <= inner_avail.w || stable_from.w == Measured::AT_OFFER_ONLY)
                && (stable_from.h <= inner_avail.h || stable_from.h == Measured::AT_OFFER_ONLY),
            "a driver holds from {stable_from:?}, past the {inner_avail:?} it measured at",
        );

        // Margin is added once at the end, inside `resolve`, so the fold
        // works in margin-exclusive space.
        let desired = Size::new(
            w.resolve(size.w + p_horiz, floor.w + p_horiz),
            h.resolve(size.h + p_vert, floor.h + p_vert),
        );
        let stable_from = Size::new(
            w.stable_from(size.w + p_horiz, floor.w + p_horiz, stable_from.w, p_horiz),
            h.stable_from(size.h + p_vert, floor.h + p_vert, stable_from.h, p_vert),
        );
        debug_assert!(
            (stable_from.w <= available.w || stable_from.w == Measured::AT_OFFER_ONLY)
                && (stable_from.h <= available.h || stable_from.h == Measured::AT_OFFER_ONLY),
            "a node holds from {stable_from:?}, past the {available:?} it measured at",
        );
        Measured {
            size: desired,
            floor: Size::new(
                w.floor(floor.w + p_horiz, desired.w),
                h.floor(floor.h + p_vert, desired.h),
            ),
            stable_from,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: f32 = Measured::AT_OFFER_ONLY;

    /// A slot with a 2 px margin and no `min`/`max` clamp.
    const fn slot(sizing: Sizing, available: f32, intrinsic_min: f32) -> AxisSlot {
        AxisSlot {
            sizing,
            available,
            intrinsic_min,
            margin: 2.0,
            min: 0.0,
            max: f32::INFINITY,
        }
    }

    /// Every branch of [`AxisSlot::stable_from`], with a 2 px margin and
    /// 4 px padding: `content` and `floor` are padding-inclusive, as the
    /// pipeline hands them over.
    #[test]
    fn stable_from_covers_each_way_an_axis_reads_its_offer() {
        let min_40 = AxisSlot {
            min: 40.0,
            ..slot(Sizing::FILL, 100.0, 0.0)
        };
        // (label, slot, content, floor, inner_stable_from, expected)
        let cases = [
            // Fixed reads no offer, whatever its driver read.
            (
                "fixed",
                slot(Sizing::fixed(50.0), 100.0, 0.0),
                60.0,
                20.0,
                AT,
                0.0,
            ),
            (
                "fill, driver holds anywhere",
                slot(Sizing::FILL, 100.0, 0.0),
                60.0,
                20.0,
                0.0,
                0.0,
            ),
            // The children need 30 inside, so the node needs 30 + 4 + 2.
            (
                "fill, driver holds from 30",
                slot(Sizing::FILL, 100.0, 0.0),
                60.0,
                20.0,
                30.0,
                36.0,
            ),
            // 50 − 2 = 48 is always handed down, past the 34 needed.
            (
                "fill, intrinsic floor covers it",
                slot(Sizing::FILL, 100.0, 50.0),
                60.0,
                20.0,
                30.0,
                0.0,
            ),
            ("fill, min_size covers it", min_40, 60.0, 20.0, 30.0, 0.0),
            (
                "fill, driver holds at its offer",
                slot(Sizing::FILL, 100.0, 0.0),
                60.0,
                20.0,
                AT,
                AT,
            ),
            // Past 60 + 2 the cap does not bind.
            (
                "hug under its offer",
                slot(Sizing::HUG, 100.0, 0.0),
                60.0,
                20.0,
                0.0,
                62.0,
            ),
            (
                "hug capped by its offer",
                slot(Sizing::HUG, 50.0, 0.0),
                60.0,
                20.0,
                0.0,
                AT,
            ),
            // max(min(60, A − 2), 60) is 60 at every finite A.
            (
                "hug at its floor",
                slot(Sizing::HUG, 50.0, 0.0),
                60.0,
                60.0,
                0.0,
                0.0,
            ),
            (
                "hug unbounded",
                slot(Sizing::HUG, f32::INFINITY, 0.0),
                60.0,
                20.0,
                0.0,
                62.0,
            ),
            // Unbounded it is 10; any finite offer floors it at 30 − 2.
            (
                "hug unbounded below its floor",
                slot(Sizing::HUG, f32::INFINITY, 30.0),
                10.0,
                0.0,
                0.0,
                AT,
            ),
            (
                "hug unbounded at its floor",
                slot(Sizing::HUG, f32::INFINITY, 30.0),
                28.0,
                0.0,
                0.0,
                0.0,
            ),
            // Its own 62, and its children's 70 + 4 + 2.
            (
                "hug and its driver",
                slot(Sizing::HUG, 100.0, 0.0),
                60.0,
                20.0,
                70.0,
                76.0,
            ),
        ];
        for (label, slot, content, floor, inner, expected) in cases {
            assert_eq!(
                slot.stable_from(content, floor, inner, 4.0),
                expected,
                "{label}"
            );
        }
    }
}
