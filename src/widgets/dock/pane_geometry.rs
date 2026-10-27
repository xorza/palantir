//! One pane's last-frame geometry, and the drop it classifies a pointer into.

use glam::Vec2;

use crate::primitives::geometry::rect::Rect;
use crate::widgets::dock::allowed_splits::AllowedSplits;
use crate::widgets::dock::dock_operation::DockDrop;
use crate::widgets::dock::split_side::SplitSide;
use crate::widgets::dock::tab_group::TabGroupId;

/// One pane's last-frame geometry for drop classification; a struct because `pane` and `strip` are both
/// `Rect`, so transposed arguments would type-check. Panes hold still during a tab drag.
#[derive(Clone, Copy, Debug)]
pub(super) struct PaneGeometry {
    pub(super) group: TabGroupId,
    pub(super) pane: Rect,
    pub(super) strip: Rect,
    /// The strip's drop slot under the pointer, counted over its chips' uncut extents.
    pub(super) strip_slot: usize,
    /// How many tabs the strip holds; a join appends after them.
    pub(super) tabs: usize,
    /// Whether this pane may still split (nesting cap); if not, edge wedges degrade to a join.
    pub(super) can_split: bool,
    pub(super) allowed: AllowedSplits,
    /// Reach of the split wedges in from each edge, as a fraction of the content rect.
    pub(super) edge_fraction: f32,
}

/// Where a drop over one pane would land, and what shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum DropTarget {
    /// Into the pane's strip at a slot, which the strip marks with its caret.
    Slot { group: TabGroupId, idx: usize },
    /// A join or split over the pane's content, washing the region the tab would take.
    Region { drop: DockDrop, wash: Rect },
}

impl DropTarget {
    pub(super) const fn drop(self) -> DockDrop {
        match self {
            Self::Slot { group, idx } => DockDrop::Into { group, idx },
            Self::Region { drop, .. } => drop,
        }
    }

    /// The caret slot `group`'s strip shows for this drop, if it lands in that strip.
    pub(super) fn caret_in(self, group: TabGroupId) -> Option<usize> {
        match self {
            Self::Slot { group: g, idx } if g == group => Some(idx),
            _ => None,
        }
    }
}

impl PaneGeometry {
    /// Classifies pointer `p` (known to be over this pane): strip band inserts, inner box joins, outer band
    /// splits toward the nearest offered edge; at the nesting cap or without an offered edge it joins.
    pub(super) fn classify(&self, p: Vec2) -> DropTarget {
        let Self {
            group,
            pane,
            strip,
            strip_slot,
            tabs,
            can_split,
            allowed,
            edge_fraction,
        } = *self;
        if strip.contains(p) {
            return DropTarget::Slot {
                group,
                idx: strip_slot,
            };
        }

        let content = Rect::new(
            pane.min.x,
            strip.max().y,
            pane.size.w,
            (pane.max().y - strip.max().y).max(0.0),
        );
        let join = DropTarget::Region {
            drop: DockDrop::Into { group, idx: tabs },
            wash: content,
        };
        if !can_split || center_box(content, edge_fraction).contains(p) {
            return join;
        }

        // Outer band: nearest offered edge by normalized distance, so a wide pane does not bias top/bottom.
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
            Some((side, _)) => DropTarget::Region {
                drop: DockDrop::Split { group, side },
                wash: half_rect(content, side),
            },
            None => join,
        }
    }
}

fn center_box(content: Rect, f: f32) -> Rect {
    Rect::new(
        content.min.x + content.size.w * f,
        content.min.y + content.size.h * f,
        content.size.w * (1.0 - 2.0 * f),
        content.size.h * (1.0 - 2.0 * f),
    )
}

fn half_rect(content: Rect, side: SplitSide) -> Rect {
    let Rect { min, size } = content;
    match side {
        SplitSide::Left => Rect::new(min.x, min.y, size.w * 0.5, size.h),
        SplitSide::Right => Rect::new(min.x + size.w * 0.5, min.y, size.w * 0.5, size.h),
        SplitSide::Top => Rect::new(min.x, min.y, size.w, size.h * 0.5),
        SplitSide::Bottom => Rect::new(min.x, min.y + size.h * 0.5, size.w, size.h * 0.5),
    }
}
