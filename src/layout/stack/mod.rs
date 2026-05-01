use super::LayoutEngine;
use crate::element::NodeElement;
use crate::primitives::{Align, AxisAlign, Justify, Rect, Size, Sizes, Sizing};
use crate::tree::{NodeId, Tree};
use glam::Vec2;

/// Which axis a stack distributes children along. `X` = `HStack`, `Y` = `VStack`.
/// All stack math is written axis-symmetrically — the dispatcher just picks one.
#[derive(Copy, Clone, PartialEq)]
pub(super) enum Axis {
    X,
    Y,
}

impl Axis {
    fn main(self, s: Size) -> f32 {
        match self {
            Axis::X => s.w,
            Axis::Y => s.h,
        }
    }
    fn cross(self, s: Size) -> f32 {
        match self {
            Axis::X => s.h,
            Axis::Y => s.w,
        }
    }
    fn main_v(self, v: Vec2) -> f32 {
        match self {
            Axis::X => v.x,
            Axis::Y => v.y,
        }
    }
    fn cross_v(self, v: Vec2) -> f32 {
        match self {
            Axis::X => v.y,
            Axis::Y => v.x,
        }
    }
    fn main_sizing(self, s: Sizes) -> Sizing {
        match self {
            Axis::X => s.w,
            Axis::Y => s.h,
        }
    }
    fn cross_sizing(self, s: Sizes) -> Sizing {
        match self {
            Axis::X => s.h,
            Axis::Y => s.w,
        }
    }
    /// Cross-axis alignment of a child, with parent's `child_align` as
    /// fallback when the child's own align is `Auto`. Mapped through
    /// `AxisAlign` so the math is type-symmetric across axes.
    fn cross_align(self, child: &NodeElement, parent_child_align: Align) -> AxisAlign {
        let child_align = child.flags.align();
        match self {
            // HStack: cross = vertical
            Axis::X => child_align
                .valign()
                .or(parent_child_align.valign())
                .to_axis(),
            // VStack: cross = horizontal
            Axis::Y => child_align
                .halign()
                .or(parent_child_align.halign())
                .to_axis(),
        }
    }
    /// Build a `Size` from main- and cross-axis lengths.
    fn compose_size(self, main: f32, cross: f32) -> Size {
        match self {
            Axis::X => Size::new(main, cross),
            Axis::Y => Size::new(cross, main),
        }
    }
    /// Build a `Vec2` from main- and cross-axis positions.
    fn compose_point(self, main: f32, cross: f32) -> Vec2 {
        match self {
            Axis::X => Vec2::new(main, cross),
            Axis::Y => Vec2::new(cross, main),
        }
    }
    /// Build a `Rect` from main- and cross-axis positions and lengths.
    fn compose_rect(self, main_pos: f32, cross_pos: f32, main: f32, cross: f32) -> Rect {
        match self {
            Axis::X => Rect::new(main_pos, cross_pos, main, cross),
            Axis::Y => Rect::new(cross_pos, main_pos, cross, main),
        }
    }
}

pub(super) fn measure(
    layout: &mut LayoutEngine,
    tree: &mut Tree,
    node: NodeId,
    inner: Size,
    axis: Axis,
) -> Size {
    // Pass infinite size on the main axis (WPF trick): children report intrinsic.
    let child_avail = axis.compose_size(f32::INFINITY, axis.cross(inner));
    let gap = tree.read_extras(node).gap;

    let mut total_main = 0.0f32;
    let mut max_cross = 0.0f32;
    let mut count = 0usize;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        // Collapsed children still get measured (so `desired` is set to ZERO),
        // but don't contribute to the parent's content size or gap count.
        let collapsed = tree.node(c).is_collapsed();
        let d = layout.measure(tree, c, child_avail);
        if collapsed {
            continue;
        }
        total_main += axis.main(d);
        max_cross = max_cross.max(axis.cross(d));
        count += 1;
    }
    total_main += gap * count.saturating_sub(1) as f32;
    axis.compose_size(total_main, max_cross)
}

pub(super) fn arrange(
    layout: &mut LayoutEngine,
    tree: &mut Tree,
    node: NodeId,
    inner: Rect,
    axis: Axis,
) {
    let extras = tree.read_extras(node);
    let (gap, justify, parent_child_align) = (extras.gap, extras.justify, extras.child_align);

    // Sum desired along main axis for non-Fill children; collect Fill weights.
    // Fill siblings split the remaining space proportionally (WPF Star semantics)
    // independent of their intrinsic content size.
    let mut sum_main_desired = 0.0f32;
    let mut total_weight = 0.0f32;
    let mut count = 0usize;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let n = tree.node(c);
        if n.is_collapsed() {
            continue;
        }
        if let Sizing::Fill(weight) = axis.main_sizing(n.element.size) {
            total_weight += weight.max(0.0);
        } else {
            sum_main_desired += axis.main(n.desired);
        }
        count += 1;
    }
    let total_gap = gap * count.saturating_sub(1) as f32;

    let main_total = axis.main(inner.size);
    let cross = axis.cross(inner.size);
    let leftover = (main_total - sum_main_desired - total_gap).max(0.0);

    // `justify` distributes any unused main-axis space. With Fill children
    // present, leftover is consumed by Fill weights → justify is a no-op
    // (degrade to Start / original gap).
    let (start_offset, effective_gap) = if total_weight > 0.0 {
        (0.0, gap)
    } else {
        match justify {
            Justify::Start => (0.0, gap),
            Justify::Center => (leftover * 0.5, gap),
            Justify::End => (leftover, gap),
            Justify::SpaceBetween if count > 1 => (0.0, gap + leftover / (count - 1) as f32),
            Justify::SpaceAround if count > 0 => {
                let extra = leftover / count as f32;
                (extra * 0.5, gap + extra)
            }
            // Fewer than 2 / 1 children → fallback to Start.
            Justify::SpaceBetween | Justify::SpaceAround => (0.0, gap),
        }
    };

    let cross_min = axis.cross_v(inner.min);
    let mut cursor = axis.main_v(inner.min) + start_offset;
    let mut first = true;

    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let (s, d, collapsed) = {
            let n = tree.node(c);
            (n.element, n.desired, n.is_collapsed())
        };
        if collapsed {
            super::zero_subtree(tree, c, axis.compose_point(cursor, cross_min));
            continue;
        }
        if !first {
            cursor += effective_gap;
        }
        first = false;

        let main_sizing = axis.main_sizing(s.size);
        let main_size = match main_sizing {
            Sizing::Fill(weight) if total_weight > 0.0 => {
                leftover * (weight.max(0.0) / total_weight)
            }
            _ => axis.main(d),
        };

        let cross_align = axis.cross_align(&s, parent_child_align);
        let cross_sizing = axis.cross_sizing(s.size);
        let cross_desired = axis.cross(d);
        let (cross_size, cross_offset) =
            super::place_axis(cross_align, cross_sizing, cross_desired, cross, false);

        let child_rect = axis.compose_rect(cursor, cross_min + cross_offset, main_size, cross_size);
        layout.arrange(tree, c, child_rect);
        cursor += main_size;
    }
}

#[cfg(test)]
mod tests;
