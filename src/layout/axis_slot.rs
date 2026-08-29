//! The per-axis inputs the measure pass resolves an outer extent from.

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
    fn outer(self, dispatch_avail: f32) -> f32 {
        self.sizing
            .fixed_value()
            .unwrap_or_else(|| (dispatch_avail - self.margin).max(0.0))
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
    fn inner_avail(self, padding: f32) -> f32 {
        (self.outer(self.available.max(self.intrinsic_min)) - padding).max(0.0)
    }
    /// **Contains-content rule:** Hug aims for content size, Fill aims
    /// for `available`. Both floor at `max(content, intrinsic_min)` — a
    /// node's rect always contains what's inside it. If the rigid floor
    /// exceeds `available`, the node overflows its parent rather than its
    /// content overflowing the node's rect. Downstream
    /// (cascade/composer/backend) tolerates overflow, same as the
    /// root-vs-surface case.
    ///
    /// `content` here is the post-dispatch measured content size
    /// (margin-exclusive). It already reflects wrapping/shrink under the
    /// constrained available width, so on the cross axis of a wrapping
    /// text leaf it's the correct multi-line height — unlike
    /// `intrinsic_min`, which is computed pure-subtree at `available =
    /// INFINITY` and only captures the single-line case. Hug needed both
    /// (content already reflects wrapping; intrinsic_min catches rigid X
    /// descendants like long unbreakable words). Fill needs both for the
    /// same reason: `content` keeps the rect ≥ its measured content,
    /// `intrinsic_min` keeps it ≥ rigid descendants the pure-subtree
    /// query identified.
    ///
    /// The two cases where desired exceeds `available`:
    /// `max(content, intrinsic_min) > available` (rigid descendant or
    /// post-wrap content doesn't fit) or `Sizing::fixed(v)`. An explicit
    /// `min_size` floor applies on top of all three branches via the
    /// trailing `clamp`.
    ///
    /// `Fill` on an unconstrained axis (intrinsic queries with
    /// `available = INFINITY`) collapses to its content size — matches
    /// CSS Grid's `1fr` track in an auto-context parent.
    pub(super) fn resolve(self, content_plus_padding: f32) -> f32 {
        let rendered = if let Some(value) = self.sizing.fixed_value() {
            value
        } else if self.sizing.is_hug() {
            if self.available.is_finite() {
                content_plus_padding
                    .min(self.available - self.margin)
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

    /// Full per-node sizing pipeline: build a slot per axis, hand the
    /// driver what each says its children measure against, and fold the
    /// driver's raw `content` back through the pair into a
    /// margin-inclusive `desired`.
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
        dispatch: impl FnOnce(Size) -> Size,
    ) -> Size {
        let Sums {
            horiz: p_horiz,
            vert: p_vert,
        } = layout.padding.sums();
        let Sums {
            horiz: m_horiz,
            vert: m_vert,
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

        let content = dispatch(Size::new(w.inner_avail(p_horiz), h.inner_avail(p_vert)));

        // Margin is added once at the end, inside `resolve`, so the fold
        // works in margin-exclusive space.
        Size::new(
            w.resolve(content.w + p_horiz),
            h.resolve(content.h + p_vert),
        )
    }
}
