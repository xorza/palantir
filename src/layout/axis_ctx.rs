//! The per-axis inputs the measure pass resolves an outer extent from.

use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::primitives::spacing::Sums;
use crate::scene::node::layout_core::LayoutCore;

/// Per-axis inputs for [`Self::resolve`]. Bundles the seven
/// numbers + `Sizing` into one struct so the call site reads as
/// "given this axis context, resolve the outer size" rather than a
/// 7-arg parameter cliff. `content_plus_padding` is the
/// margin-exclusive hug size (`content + padding`); `available` and
/// the returned value are margin-inclusive.
#[derive(Debug)]
pub(super) struct AxisCtx {
    pub(super) sizing: Sizing,
    pub(super) content_plus_padding: f32,
    pub(super) available: f32,
    pub(super) intrinsic_min: f32,
    pub(super) margin: f32,
    pub(super) min: f32,
    pub(super) max: f32,
}

impl AxisCtx {
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
    pub(super) fn resolve(self) -> f32 {
        let rendered = if let Some(value) = self.sizing.fixed_value() {
            value
        } else if self.sizing.is_hug() {
            if self.available.is_finite() {
                self.content_plus_padding
                    .min(self.available - self.margin)
                    .max(self.intrinsic_min - self.margin)
            } else {
                self.content_plus_padding
            }
        } else {
            // WPF Stretch: Fill returns content at measure-time. The
            // "fill the slot" expansion happens at *arrange* — driver
            // arrange code redistributes leftover to Fill children
            // proportionally. Returning `available` here would balloon
            // any Hug ancestor to its grandparent's allocation (CSS auto-
            // sizing's classic Hug+Fill bug).
            self.content_plus_padding
                .max(self.intrinsic_min - self.margin)
        };
        rendered.max(0.0).clamp(self.min, self.max) + self.margin
    }

    /// Full per-node sizing pipeline: derive `inner_avail` from the parent-
    /// supplied `available` + `layout` + clamps, hand it to the driver via
    /// `dispatch`, fold the driver's raw `content` into a margin-inclusive
    /// `desired`. Returns `desired`.
    ///
    /// Per-node padding/margin sums are unpacked once and threaded through
    /// both halves of the pipeline, which is only possible because the
    /// dispatch is single-shot — see the single-dispatch note below.
    ///
    /// `available` is the parent-supplied slot (margin-inclusive).
    /// `intrinsic_min` floors `available` so children measure against the
    /// parent's actual outer size (`max(available, intrinsic_min)` per
    /// [`Self::resolve`]) — without this, a Hug grid inside a FILL panel
    /// whose own intrinsic_min is pinned by a long sibling would shape
    /// children against the smaller surface width. INFINITY-on-Hug-axis
    /// preserved (`INF.max(x) == INF`); Fixed axes ignore both inputs in
    /// [`Self::resolve`].
    ///
    /// Single dispatch: when `desired` exceeds `available` on a non-Fixed
    /// axis it's because a rigid descendant pinned the floor; a re-dispatch
    /// against the grown outer would converge to the same value because
    /// every driver's content size is monotone in `available` and pass-1
    /// already saturated at the floor. Pinned by
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

        let dispatch_avail = Size::new(
            available.w.max(intrinsic_min.w),
            available.h.max(intrinsic_min.h),
        );

        // `inner_avail`: outer = `Fixed(v)` on Fixed axes else
        // `dispatch_avail - margin`; clamp outer to `[min_size, max_size]`
        // so a `max_size`-capped parent doesn't grant children more room
        // than it can later arrange; deflate by padding. The clamp matches
        // `AxisCtx::resolve` below so children's `available` tracks the
        // parent's eventual arranged width.
        let outer_w = layout
            .size
            .w()
            .fixed_value()
            .unwrap_or_else(|| (dispatch_avail.w - m_horiz).max(0.0))
            .clamp(min_size.w, max_size.w);
        let outer_h = layout
            .size
            .h()
            .fixed_value()
            .unwrap_or_else(|| (dispatch_avail.h - m_vert).max(0.0))
            .clamp(min_size.h, max_size.h);
        let inner_avail = Size::new((outer_w - p_horiz).max(0.0), (outer_h - p_vert).max(0.0));

        let content = dispatch(inner_avail);

        // Fold content into margin-inclusive desired. Margin is added once
        // at the end inside `AxisCtx::resolve`; this function works in
        // margin-exclusive space (`content_plus_padding = content + p_*`).
        Size::new(
            AxisCtx {
                sizing: layout.size.w(),
                content_plus_padding: content.w + p_horiz,
                available: available.w,
                intrinsic_min: intrinsic_min.w,
                margin: m_horiz,
                min: min_size.w,
                max: max_size.w,
            }
            .resolve(),
            AxisCtx {
                sizing: layout.size.h(),
                content_plus_padding: content.h + p_vert,
                available: available.h,
                intrinsic_min: intrinsic_min.h,
                margin: m_vert,
                min: min_size.h,
                max: max_size.h,
            }
            .resolve(),
        )
    }
}
