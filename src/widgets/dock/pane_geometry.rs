//! One pane's last-frame geometry, and the drop it classifies a pointer
//! into.

use glam::Vec2;

use crate::primitives::rect::Rect;
use crate::widgets::dock::allowed_splits::AllowedSplits;
use crate::widgets::dock::dock_op::DockDrop;
use crate::widgets::dock::split_side::SplitSide;
use crate::widgets::dock::tab_group::TabGroupId;
use crate::widgets::tabs::tab_strip::TabStrip;

/// One pane's last-frame geometry, as the drop classification needs it.
///
/// A struct rather than a parameter list: `pane` and `strip` are both
/// `Rect`, so transposing them type-checks and yields a plausible but
/// wrong classification.
///
/// Last frame's rects are the right ones to measure against. Panes hold
/// still while a tab is dragged, so the picture the user drops onto is
/// the picture the arithmetic runs against.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PaneGeometry<'a> {
    pub(crate) group: TabGroupId,
    /// The whole pane — strip row and content together.
    pub(crate) pane: Rect,
    /// The strip row alone, along the pane's top edge.
    pub(crate) strip: Rect,
    /// The strip's chip rects, in tab order.
    pub(crate) chips: &'a [Rect],
    /// Whether this pane may still split — the nesting cap. When it
    /// cannot, every edge wedge degrades to a join.
    pub(crate) can_split: bool,
    /// Which split directions the dock offers.
    pub(crate) allowed: AllowedSplits,
    /// How far in from each edge the split wedges reach, as a fraction
    /// of the content rect.
    pub(crate) edge_fraction: f32,
    /// Breadth of the insertion caret drawn between two chips.
    pub(crate) caret_width: f32,
}

/// Where a drop over one pane would land, plus the region to highlight
/// while the pointer hovers it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DropTarget {
    pub(crate) drop: DockDrop,
    pub(crate) highlight: Rect,
}

impl PaneGeometry<'_> {
    /// Classify pointer `p` against this pane — the caller has already
    /// established that `p` is over it.
    ///
    /// The strip band yields an insertion slot between chips, the
    /// content's inner box joins the group, and the outer band splits
    /// toward the nearest edge. A pane at the nesting cap, or one whose
    /// nearest edge the dock does not offer, degrades to a join.
    pub(crate) fn classify(&self, p: Vec2) -> DropTarget {
        let Self {
            group,
            pane,
            strip,
            chips,
            can_split,
            allowed,
            edge_fraction,
            caret_width,
        } = *self;
        if strip.contains(p) {
            let index = TabStrip::insertion_slot(chips.iter().copied(), p.x);
            return DropTarget {
                drop: DockDrop::Into { group, index },
                highlight: caret_rect(strip, chips, index, caret_width),
            };
        }

        let content = Rect::new(
            pane.min.x,
            strip.max().y,
            pane.size.w,
            (pane.max().y - strip.max().y).max(0.0),
        );
        let join = DropTarget {
            drop: DockDrop::Into {
                group,
                index: chips.len(),
            },
            highlight: content,
        };
        if !can_split || center_box(content, edge_fraction).contains(p) {
            return join;
        }

        // Outer band: split toward the nearest offered edge, compared on
        // normalised distance so a wide pane does not bias toward top and
        // bottom.
        let w = content.size.w.max(1.0);
        let h = content.size.h.max(1.0);
        let edges = [
            (SplitSide::Left, (p.x - content.min.x) / w),
            (SplitSide::Right, (content.max().x - p.x) / w),
            (SplitSide::Top, (p.y - content.min.y) / h),
            (SplitSide::Bottom, (content.max().y - p.y) / h),
        ];
        let nearest = edges
            .into_iter()
            .filter(|(side, _)| allowed.allows(*side))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match nearest {
            Some((side, _)) => DropTarget {
                drop: DockDrop::Split { group, side },
                highlight: half_rect(content, side),
            },
            None => join,
        }
    }
}

/// The inner box of `content` the join zone occupies — `fraction` in
/// from each edge on both axes.
fn center_box(content: Rect, fraction: f32) -> Rect {
    let f = fraction.clamp(0.0, 0.5);
    Rect::new(
        content.min.x + content.size.w * f,
        content.min.y + content.size.h * f,
        content.size.w * (1.0 - 2.0 * f),
        content.size.h * (1.0 - 2.0 * f),
    )
}

/// The half of `content` a split on `side` would give the dragged tab.
fn half_rect(content: Rect, side: SplitSide) -> Rect {
    let Rect { min, size } = content;
    match side {
        SplitSide::Left => Rect::new(min.x, min.y, size.w * 0.5, size.h),
        SplitSide::Right => Rect::new(min.x + size.w * 0.5, min.y, size.w * 0.5, size.h),
        SplitSide::Top => Rect::new(min.x, min.y, size.w, size.h * 0.5),
        SplitSide::Bottom => Rect::new(min.x, min.y + size.h * 0.5, size.w, size.h * 0.5),
    }
}

/// The insertion caret between the strip's chips: on the boundary of
/// slot `index` — before `chips[index]`, or after the last chip for an
/// append. An empty strip cannot happen, but degrades to the strip's
/// left inset if it ever does.
fn caret_rect(strip: Rect, chips: &[Rect], index: usize, width: f32) -> Rect {
    let x = match (chips.get(index), chips.last()) {
        (Some(next), _) => next.min.x - 1.5,
        (None, Some(last)) => last.max().x + 1.5,
        (None, None) => strip.min.x + 6.0,
    };
    Rect::new(
        x - width * 0.5,
        strip.min.y + 2.0,
        width,
        (strip.size.h - 2.0).max(0.0),
    )
}
