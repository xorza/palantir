//! The per-axis inputs the measure pass resolves an outer extent from.

use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::measured::Measured;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::layout_core::LayoutCore;

/// What a parent grants one axis of one node: `Sizing` plus the numbers that
/// decide the axis' extent.
///
/// One slot per axis, built once and read twice: [`Self::inner_avail`] gives
/// what the driver measures against, [`Self::resolve`] folds its answer back
/// into the node's extent. `available` and `resolve`'s result are
/// margin-inclusive; everything between is margin-exclusive.
#[derive(Clone, Copy, Debug)]
pub(super) struct AxisSlot {
    pub(super) sizing: Sizing,
    pub(super) available: f32,
    pub(super) intrinsic_min: f32,
    pub(super) margin: f32,
    pub(super) padding: f32,
    pub(super) min: f32,
    pub(super) max: f32,
}

impl AxisSlot {
    /// `axis` of `layout`'s box under an unbounded offer, the one an intrinsic query asks about.
    pub(super) fn unbounded(layout: &LayoutCore, bounds: &BoundsExtras, axis: Axis) -> Self {
        Self {
            sizing: axis.main_sizing(layout.size),
            available: f32::INFINITY,
            intrinsic_min: 0.0,
            margin: axis.spacing(layout.margin),
            padding: axis.spacing(layout.padding),
            min: axis.main(bounds.min_size),
            max: axis.main(bounds.max_size),
        }
    }

    /// Wrap the raw content range a walk answered in this axis' box: padding,
    /// the `Sizing` override, margin, and the `min_size` / `max_size` clamps, on
    /// the halves `query` asks for.
    ///
    /// Padding is added unconditionally: a Fixed axis arrives with a zero content
    /// range, and [`Self::resolve`] returns the declared value without reading
    /// either, so the add cannot reach the result.
    pub(super) fn outer(
        self,
        query: IntrinsicQuery,
        mut content: IntrinsicRange,
    ) -> IntrinsicRange {
        for (_, value) in content.requested(query) {
            *value = self.resolve(*value + self.padding, 0.0);
        }
        content
    }

    /// The extent of this node's own box before padding (a Fixed axis' value,
    /// else the parent's grant less margin), clamped to `[min, max]`. The clamp
    /// matches [`Self::resolve`]'s so a `max_size`-capped parent does not grant
    /// children more room than it can arrange.
    #[inline]
    const fn outer_extent(self, dispatch_avail: f32) -> f32 {
        match self.sizing.fixed_value() {
            Some(value) => value,
            None => (dispatch_avail - self.margin).max(0.0),
        }
        .clamp(self.min, self.max)
    }

    /// What the driver measures its children against on this axis: the outer
    /// extent less padding. `available` is floored by `intrinsic_min` first,
    /// so children measure against the parent's actual outer size (a Hug grid
    /// in a FILL panel pinned by a long sibling would otherwise shape against
    /// the smaller surface width). INFINITY on a Hug axis survives; a Fixed axis
    /// reads neither input.
    #[inline]
    const fn inner_avail(self) -> f32 {
        (self.outer_extent(self.available.max(self.intrinsic_min)) - self.padding).max(0.0)
    }
    /// **Contains-content rule:** Hug aims for content size, Fill for
    /// `available`. Fill floors at `max(content, intrinsic_min)`. Hug under a
    /// finite `available` caps content at what is available and floors at
    /// `max(floor, intrinsic_min)`, so a Hug rect shrinks what can give way
    /// (a scroll on its panned axis) but never below what its content takes
    /// at the constraints it was measured under. If the floor exceeds
    /// `available` the node overflows its parent, which downstream tolerates.
    ///
    /// `content` and `floor` are post-dispatch and margin-exclusive, already
    /// reflecting wrapping under the constrained width (so a wrapping text
    /// leaf's block-axis floor is the multi-line height), unlike
    /// `intrinsic_min`, computed at `available = INFINITY`. `intrinsic_min`
    /// still floors both sizings for what measure cannot see, such as rigid
    /// descendants of a `Fill` track.
    ///
    /// Desired exceeds `available` when `max(floor, intrinsic_min) > available`
    /// or `Sizing::fixed(v)`. An explicit `min_size` applies on top via the
    /// trailing `clamp`.
    ///
    /// `Fill` on an unconstrained axis collapses to its content size, like
    /// CSS Grid's `1fr` in an auto-context parent.
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
            // WPF Stretch: Fill returns content at measure time; arrange
            // redistributes leftover to Fill children. Returning `available`
            // would balloon any Hug ancestor to its grandparent's allocation.
            content_plus_padding.max(self.intrinsic_min - self.margin)
        };
        rendered.max(0.0).clamp(self.min, self.max) + self.margin
    }

    /// The node's own floor on this axis, margin-inclusive: the content floor
    /// with padding, or the fixed value, under [`Self::resolve`]'s clamps and
    /// never more than `resolved`.
    const fn floor(self, floor_plus_padding: f32, resolved: f32) -> f32 {
        let rendered = match self.sizing.fixed_value() {
            Some(value) => value,
            None => floor_plus_padding.max(self.intrinsic_min - self.margin),
        };
        (rendered.max(0.0).clamp(self.min, self.max) + self.margin).min(resolved)
    }

    /// The least finite `available` from which this axis' measure holds
    /// unchanged (see [`Measured`]), given the driver's `inner_stable_from`
    /// and the padding-inclusive content and floor it answered.
    ///
    /// A Fixed axis reads no offer. Otherwise two things must hold. The node's
    /// fold: Fill does not read `available`; Hug reads it only through its cap,
    /// free once the offer is past the content, while content at or below its
    /// floors resolves to the floors under every finite offer. And the
    /// driver's: what the node hands its children must not fall below
    /// `inner_stable_from`, which the `intrinsic_min` and `min` floors may
    /// already guarantee.
    ///
    /// Finite offers only: an unbounded Hug axis skips its floors, so an
    /// `INFINITY` offer is served only where it was measured.
    const fn stable_from(
        self,
        content_plus_padding: f32,
        floor_plus_padding: f32,
        inner_stable_from: f32,
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
        let handed = inner_stable_from + self.padding;
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
}

/// What a parent grants one node on both axes: its box, the offer, and the
/// floor its rigid descendants set. [`Self::resolve`] runs the sizing pipeline.
#[derive(Clone, Copy, Debug)]
pub(super) struct NodeSlot<'a> {
    pub(super) layout: LayoutCore,
    pub(super) bounds: &'a BoundsExtras,
    pub(super) available: Size,
    pub(super) intrinsic_min: Size,
}

impl NodeSlot<'_> {
    /// Full per-node sizing pipeline: build an [`AxisSlot`] per axis, hand the
    /// driver what each says its children measure against, and fold its raw
    /// content, floor and range back into a margin-inclusive `desired`, floor
    /// and range.
    ///
    /// Single dispatch: when `desired` exceeds `available` on a non-Fixed axis
    /// a rigid descendant pinned the floor, and a re-dispatch would converge
    /// to the same value since every driver's content size is monotone in
    /// `available`. Pinned by `layout::tests::convergence`.
    #[inline]
    pub(super) fn resolve(self, dispatch: impl FnOnce(Size) -> Measured) -> Measured {
        let Self {
            layout,
            bounds,
            available,
            intrinsic_min,
        } = self;
        let Size {
            w: p_horiz,
            h: p_vert,
        } = layout.padding.sums();
        let Size {
            w: m_horiz,
            h: m_vert,
        } = layout.margin.sums();

        let w = AxisSlot {
            sizing: layout.size.w(),
            available: available.w,
            intrinsic_min: intrinsic_min.w,
            margin: m_horiz,
            padding: p_horiz,
            min: bounds.min_size.w,
            max: bounds.max_size.w,
        };
        let h = AxisSlot {
            sizing: layout.size.h(),
            available: available.h,
            intrinsic_min: intrinsic_min.h,
            margin: m_vert,
            padding: p_vert,
            min: bounds.min_size.h,
            max: bounds.max_size.h,
        };

        let inner_avail = Size::new(w.inner_avail(), h.inner_avail());
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

        // Margin is added once at the end, inside `resolve`.
        let desired = Size::new(
            w.resolve(size.w + p_horiz, floor.w + p_horiz),
            h.resolve(size.h + p_vert, floor.h + p_vert),
        );
        let stable_from = Size::new(
            w.stable_from(size.w + p_horiz, floor.w + p_horiz, stable_from.w),
            h.stable_from(size.h + p_vert, floor.h + p_vert, stable_from.h),
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

    /// A slot with a 2 px margin, 4 px padding and no `min`/`max` clamp.
    const fn slot(sizing: Sizing, available: f32, intrinsic_min: f32) -> AxisSlot {
        AxisSlot {
            sizing,
            available,
            intrinsic_min,
            margin: 2.0,
            padding: 4.0,
            min: 0.0,
            max: f32::INFINITY,
        }
    }

    /// Every branch of [`AxisSlot::stable_from`]: `content` and `floor` are
    /// padding-inclusive, as the pipeline hands them over.
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
            assert_eq!(slot.stable_from(content, floor, inner), expected, "{label}");
        }
    }
}
